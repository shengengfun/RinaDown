//! FluxDown auto-updater helper binary.
//!
//! Spawned by the main FluxDown process right before it exits.  Waits for the
//! parent process to terminate, then performs the requested update action and
//! restarts (or lets the installer restart) the application.
//!
//! ## CLI
//!
//! ### Windows – portable ZIP
//!   fluxdown_updater --pid PID --zip PATH --dir PATH --exe NAME
//!
//! ### Windows – NSIS installer
//!   fluxdown_updater --pid PID --installer PATH
//!
//! ### Linux – AppImage replacement
//!   fluxdown_updater --pid PID --appimage-src NEW --appimage-dst CURRENT
//!
//! ### Linux – portable tar.gz
//!   fluxdown_updater --pid PID --tarball PATH --dir PATH --exe NAME
//!
//! ### Linux – deb package (pkexec)
//!   fluxdown_updater --pid PID --package-deb PATH
//!
//! ### Linux – Arch package (pkexec)
//!   fluxdown_updater --pid PID --package-arch PATH
//!
//! ### macOS – .app bundle replacement
//!   fluxdown_updater --pid PID --tarball PATH --app-bundle NAME --install-dir PATH

// Don't open a console window when launched on Windows.
// If the parent attached a console, stdout/stderr are still inherited.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;

use thiserror::Error;

// ─── Error ────────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
enum UpdaterError {
    #[error("missing required argument: {0}")]
    MissingArg(&'static str),
    #[error("invalid argument: {0}")]
    InvalidArg(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("path traversal detected in archive entry: {0}")]
    PathTraversal(String),
    #[error("{0}")]
    Other(String),
}

// ─── Action ───────────────────────────────────────────────────────────────────

enum Action {
    /// Extract a ZIP archive and copy files into `dir`, then restart `exe`.
    PortableZip {
        zip: PathBuf,
        dir: PathBuf,
        exe: String,
    },
    /// Run a downloaded NSIS installer silently (Windows only).
    Setup { installer: PathBuf },
    /// Replace the running AppImage with a new one (Linux only).
    AppImage { src: PathBuf, dst: PathBuf },
    /// Extract a .tar.gz and copy files into `dir`, then restart `exe` (Linux only).
    PortableTarball {
        tarball: PathBuf,
        dir: PathBuf,
        exe: String,
    },
    /// Install a .deb package via pkexec + dpkg (Linux only).
    PackageDeb { package: PathBuf },
    /// Install an Arch package via pkexec + pacman (Linux only).
    PackageArch { package: PathBuf },
    /// Replace the running .app bundle with a new one from a tar.gz (macOS only).
    MacosApp {
        tarball: PathBuf,
        /// Bundle name, e.g. "FluxDown.app"
        app_bundle: String,
        /// Directory that contains the .app, e.g. "/Applications"
        install_dir: PathBuf,
    },
}

// ─── Argument parsing ─────────────────────────────────────────────────────────

struct Args {
    pid: u32,
    action: Action,
}

impl Args {
    fn parse() -> Result<Self, UpdaterError> {
        let raw: Vec<String> = std::env::args().collect();

        let mut pid: Option<u32> = None;
        let mut zip: Option<PathBuf> = None;
        let mut dir: Option<PathBuf> = None;
        let mut exe: Option<String> = None;
        let mut installer: Option<PathBuf> = None;
        let mut appimage_src: Option<PathBuf> = None;
        let mut appimage_dst: Option<PathBuf> = None;
        let mut tarball: Option<PathBuf> = None;
        let mut package_deb: Option<PathBuf> = None;
        let mut package_arch: Option<PathBuf> = None;
        let mut app_bundle: Option<String> = None;
        let mut install_dir: Option<PathBuf> = None;

        let mut i = 1usize;
        while i < raw.len() {
            let key = raw[i].as_str();
            let next = raw.get(i + 1).map(String::as_str);

            match (key, next) {
                ("--pid", Some(v)) => {
                    pid = Some(
                        v.parse::<u32>()
                            .map_err(|e| UpdaterError::InvalidArg(format!("--pid: {e}")))?,
                    );
                    i += 2;
                }
                ("--zip", Some(v)) => {
                    zip = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--dir", Some(v)) => {
                    dir = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--exe", Some(v)) => {
                    exe = Some(v.to_string());
                    i += 2;
                }
                ("--installer", Some(v)) => {
                    installer = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--appimage-src", Some(v)) => {
                    appimage_src = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--appimage-dst", Some(v)) => {
                    appimage_dst = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--tarball", Some(v)) => {
                    tarball = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--package-deb", Some(v)) => {
                    package_deb = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--package-arch", Some(v)) => {
                    package_arch = Some(PathBuf::from(v));
                    i += 2;
                }
                ("--app-bundle", Some(v)) => {
                    app_bundle = Some(v.to_string());
                    i += 2;
                }
                ("--install-dir", Some(v)) => {
                    install_dir = Some(PathBuf::from(v));
                    i += 2;
                }
                _ => {
                    i += 1;
                }
            }
        }

        let pid = pid.ok_or(UpdaterError::MissingArg("--pid"))?;

        let action = if let Some(installer) = installer {
            Action::Setup { installer }
        } else if let Some(src) = appimage_src {
            Action::AppImage {
                src,
                dst: appimage_dst.ok_or(UpdaterError::MissingArg("--appimage-dst"))?,
            }
        } else if let Some(tarball) = tarball {
            if let Some(bundle) = app_bundle {
                Action::MacosApp {
                    tarball,
                    app_bundle: bundle,
                    install_dir: install_dir.ok_or(UpdaterError::MissingArg("--install-dir"))?,
                }
            } else {
                Action::PortableTarball {
                    tarball,
                    dir: dir.ok_or(UpdaterError::MissingArg("--dir"))?,
                    exe: exe.ok_or(UpdaterError::MissingArg("--exe"))?,
                }
            }
        } else if let Some(pkg) = package_deb {
            Action::PackageDeb { package: pkg }
        } else if let Some(pkg) = package_arch {
            Action::PackageArch { package: pkg }
        } else {
            Action::PortableZip {
                zip: zip.ok_or(UpdaterError::MissingArg("--zip"))?,
                dir: dir.ok_or(UpdaterError::MissingArg("--dir"))?,
                exe: exe.ok_or(UpdaterError::MissingArg("--exe"))?,
            }
        };

        Ok(Args { pid, action })
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    if let Err(e) = run() {
        let msg = format!("fluxdown_updater error: {e}");
        let _ = log_msg(&msg);
        eprintln!("{msg}");
        process::exit(1);
    }
}

fn run() -> Result<(), UpdaterError> {
    let args = Args::parse()?;
    log_msg(&format!("started, waiting for pid={}", args.pid)).ok();

    // Wait for the main application to fully terminate before touching any files.
    wait_for_pid(args.pid);
    log_msg("main process exited, proceeding with update").ok();

    match args.action {
        Action::PortableZip { zip, dir, exe } => do_portable_zip(&zip, &dir, &exe),
        Action::Setup { installer } => do_setup(&installer),
        Action::AppImage { src, dst } => do_appimage(&src, &dst),
        Action::PortableTarball { tarball, dir, exe } => do_tarball(&tarball, &dir, &exe),
        Action::PackageDeb { package } => do_pkg_deb(&package),
        Action::PackageArch { package } => do_pkg_arch(&package),
        Action::MacosApp {
            tarball,
            app_bundle,
            install_dir,
        } => do_macos_app(&tarball, &app_bundle, &install_dir),
    }
}

// ─── Actions ─────────────────────────────────────────────────────────────────

/// Windows portable: extract ZIP → copy files over install dir → restart.
fn do_portable_zip(zip: &Path, dir: &Path, exe: &str) -> Result<(), UpdaterError> {
    log_msg(&format!(
        "portable-zip: zip={} dir={} exe={exe}",
        zip.display(),
        dir.display()
    ))
    .ok();

    // On Windows, rename ourselves aside so the ZIP can overwrite
    // fluxdown_updater.exe even though we are currently running.
    // A running .exe on Windows cannot be deleted but CAN be renamed.
    #[cfg(target_os = "windows")]
    let old_self = rename_self_aside();

    // The NMH relay (fluxdown_nmh.exe) lives in the same directory but is
    // spawned and owned by the browser, NOT by the main app — so it may still
    // be running after the app exits and will hold a lock on its own .exe,
    // causing the copy step below to fail with ERROR_SHARING_VIOLATION.
    // Terminate any residual NMH processes and give the OS a moment to release
    // the file handle before we attempt the overwrite.
    #[cfg(target_os = "windows")]
    {
        terminate_residual_processes(dir);
    }

    // Extract ZIP to a private temp directory.
    let tmp = std::env::temp_dir().join(format!("fluxdown_upd_{}", process::id()));
    extract_zip(zip, &tmp)?;
    log_msg(&format!("extracted to {}", tmp.display())).ok();

    // Unwrap single-folder ZIPs (typical GitHub release layout).
    let src = single_dir_or_root(&tmp)?;
    log_msg(&format!("copy source: {}", src.display())).ok();

    // Copy with exponential-backoff retry (handles transient antivirus locks).
    // On failure, the main process has already exited and cannot surface the
    // error in the UI — so we drop a failure marker that the app reads on its
    // next launch, and open the install directory so the user can replace the
    // files manually from the (still present) extracted ZIP.
    if let Err(e) = copy_dir_retry(&src, dir) {
        log_msg(&format!("copy failed: {e}")).ok();
        write_failure_marker(
            dir,
            &format!(
                "Automatic update could not replace the program files \
                 (the folder may be read-only, in a protected location such as \
                 \"Program Files\", or a file was locked by another program).\n\n\
                 The new version has been extracted to:\n{}\n\n\
                 Please copy its contents over your FluxDown folder manually, \
                 or download the latest version from https://fluxdown.zerx.dev",
                src.display()
            ),
        );
        // Keep the extracted files so the user can copy them by hand.
        #[cfg(target_os = "windows")]
        open_in_explorer(&src);
        // Do NOT delete tmp/zip here — the user needs them for manual recovery.
        // Restart the (old) app so the user is not left with nothing running;
        // the app will then show the failure marker on startup.
        let exe_path = dir.join(exe);
        let _ = spawn_no_elevation(&exe_path, &[]);
        return Err(e);
    }
    log_msg("files copied").ok();

    // Delete the stale self we renamed aside earlier.
    #[cfg(target_os = "windows")]
    if let Some(old) = old_self {
        let _ = fs::remove_file(old);
    }

    // Cleanup temp dir and original archive.
    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_file(zip);

    // Restart the application.
    let exe_path = dir.join(exe);
    log_msg(&format!("restarting: {}", exe_path.display())).ok();
    spawn_no_elevation(&exe_path, &[]).map_err(UpdaterError::Io)?;

    Ok(())
}

/// Windows setup: remove Mark-of-the-Web, run NSIS installer silently.
fn do_setup(installer: &Path) -> Result<(), UpdaterError> {
    log_msg(&format!("setup: installer={}", installer.display())).ok();

    #[cfg(target_os = "windows")]
    {
        // Remove the Zone.Identifier ADS (Mark of the Web) from the downloaded
        // installer.  On Windows 11 with Smart App Control enabled this is
        // essential: our updater binary is already installed and trusted, so it
        // is allowed to unblock files on behalf of the user.
        remove_zone_identifier(installer);

        spawn_no_elevation(
            installer,
            &["/SILENT", "/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"],
        )
        .map_err(UpdaterError::Io)?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    Err(UpdaterError::Other(
        "setup mode is only supported on Windows".to_string(),
    ))
}

/// Linux AppImage: replace current AppImage with new one and relaunch.
#[allow(unused_variables)]
fn do_appimage(src: &Path, dst: &Path) -> Result<(), UpdaterError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;

        log_msg(&format!(
            "appimage: src={} dst={}",
            src.display(),
            dst.display()
        ))
        .ok();

        fs::set_permissions(src, fs::Permissions::from_mode(0o755)).map_err(UpdaterError::Io)?;
        fs::rename(src, dst).map_err(UpdaterError::Io)?;

        log_msg(&format!("restarting: {}", dst.display())).ok();
        process::Command::new(dst)
            .spawn()
            .map_err(UpdaterError::Io)?;

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    Err(UpdaterError::Other(
        "appimage mode is only supported on Linux".to_string(),
    ))
}

/// Linux tar.gz portable: extract → copy files into install dir → restart.
#[allow(unused_variables)]
fn do_tarball(tarball: &Path, dir: &Path, exe: &str) -> Result<(), UpdaterError> {
    #[cfg(target_os = "linux")]
    {
        log_msg(&format!(
            "tarball: tar={} dir={} exe={exe}",
            tarball.display(),
            dir.display()
        ))
        .ok();

        let tmp = std::env::temp_dir().join(format!("fluxdown_upd_{}", process::id()));
        extract_tarball(tarball, &tmp)?;
        log_msg(&format!("extracted to {}", tmp.display())).ok();

        let src = single_dir_or_root(&tmp)?;
        if let Err(e) = copy_dir_retry(&src, dir) {
            log_msg(&format!("copy failed: {e}")).ok();
            write_failure_marker(
                dir,
                &format!(
                    "Automatic update could not replace the program files \
                     (the folder may be read-only or in a protected location).\n\n\
                     The new version has been extracted to:\n{}\n\n\
                     Please copy its contents over your FluxDown folder manually, \
                     or download the latest version from https://fluxdown.zerx.dev",
                    src.display()
                ),
            );
            // Keep tmp/tarball for manual recovery; restart the old app so the
            // failure marker can be shown on startup.
            let exe_path = dir.join(exe);
            let _ = process::Command::new(&exe_path).spawn();
            return Err(e);
        }
        log_msg("files copied").ok();

        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_file(tarball);

        let exe_path = dir.join(exe);
        log_msg(&format!("restarting: {}", exe_path.display())).ok();
        process::Command::new(&exe_path)
            .spawn()
            .map_err(UpdaterError::Io)?;

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    Err(UpdaterError::Other(
        "tarball mode is only supported on Linux".to_string(),
    ))
}

/// Linux deb: install via pkexec + dpkg, then restart.
#[allow(unused_variables)]
fn do_pkg_deb(package: &Path) -> Result<(), UpdaterError> {
    #[cfg(target_os = "linux")]
    {
        log_msg(&format!("deb: {}", package.display())).ok();
        pkexec_install(&["dpkg", "-i", &package.to_string_lossy()])?;
        restart_current_exe()
    }

    #[cfg(not(target_os = "linux"))]
    Err(UpdaterError::Other(
        "deb mode is only supported on Linux".to_string(),
    ))
}

/// Linux Arch: install via pkexec + pacman, then restart.
#[allow(unused_variables)]
fn do_pkg_arch(package: &Path) -> Result<(), UpdaterError> {
    #[cfg(target_os = "linux")]
    {
        log_msg(&format!("arch: {}", package.display())).ok();
        pkexec_install(&["pacman", "-U", "--noconfirm", &package.to_string_lossy()])?;
        restart_current_exe()
    }

    #[cfg(not(target_os = "linux"))]
    Err(UpdaterError::Other(
        "arch mode is only supported on Linux".to_string(),
    ))
}

// ─── Process spawning ─────────────────────────────────────────────────────────

/// Spawn a process, retrying once with the `RunAsInvoker` compatibility layer
/// when Windows reports `ERROR_ELEVATION_REQUIRED` (os error 740).
///
/// 740 appears when the target executable carries a "Run as administrator"
/// compatibility flag — set manually via the file-properties checkbox, or
/// automatically by the Program Compatibility Assistant. Nothing we launch
/// needs elevation (the Inno installer is `PrivilegesRequired=lowest`; the
/// app itself is asInvoker), so forcing the layer is always safe here.
///
/// Also strips an inherited `__COMPAT_LAYER` on the first attempt: the main
/// app may have launched *us* with `RunAsInvoker` forced, and the layer must
/// not silently leak into long-lived children like the restarted app.
fn spawn_no_elevation(program: &Path, args: &[&str]) -> io::Result<process::Child> {
    let mut cmd = process::Command::new(program);
    cmd.args(args);
    cmd.env_remove("__COMPAT_LAYER");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        #[cfg(target_os = "windows")]
        Err(e) if e.raw_os_error() == Some(740) => {
            log_msg(&format!(
                "spawn requires elevation (740); retrying with RunAsInvoker: {}",
                program.display()
            ))
            .ok();
            let mut retry = process::Command::new(program);
            retry.args(args).env("__COMPAT_LAYER", "RunAsInvoker");
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                retry.creation_flags(CREATE_NO_WINDOW);
            }
            retry.spawn()
        }
        other => other,
    }
}

/// `CREATE_NO_WINDOW`: don't flash a console window for spawned children.
/// GUI-subsystem executables are unaffected by this flag.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ─── Process waiting ──────────────────────────────────────────────────────────

fn wait_for_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };

        // SAFETY: FFI call with valid constant flags and a user-supplied PID.
        // OpenProcess returns NULL when the process does not exist.
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if !handle.is_null() {
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
                return;
            }
        }
        // OpenProcess returned null → the process has already exited.
        // Poll with short sleeps to give the OS time to fully release all file
        // handles that were held by the process (antivirus, shell extensions,
        // and the loader may briefly retain handles after process exit).
        for ms in [50u64, 100, 200, 400, 800] {
            thread::sleep(Duration::from_millis(ms));
            // Re-check: if OpenProcess now succeeds, the process object still
            // exists but is exiting; wait on it properly.
            // SAFETY: same as above – valid flags, user-supplied PID.
            unsafe {
                let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
                if !handle.is_null() {
                    WaitForSingleObject(handle, INFINITE);
                    CloseHandle(handle);
                    break;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // /proc/<pid> exists as long as the process is alive on Linux.
        let proc = format!("/proc/{pid}");
        while Path::new(&proc).exists() {
            thread::sleep(Duration::from_millis(250));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // kill(pid, 0) returns 0 while the process exists, -1/ESRCH when it has gone.
        // SAFETY: kill() with signal 0 is a no-op; we only check the return value.
        while unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
            thread::sleep(Duration::from_millis(250));
        }
        return;
    }

    // Fallback for other Unix-like systems.
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        thread::sleep(Duration::from_secs(2));
    }
}

// ─── Windows platform helpers ─────────────────────────────────────────────────

/// Rename the currently running updater binary to a `.old` sibling so the
/// ZIP extraction can overwrite `fluxdown_updater.exe`.  On Windows a running
/// executable cannot be replaced in-place but can be renamed; the file-backed
/// memory mapping keeps the old image alive for the duration of this process.
#[cfg(target_os = "windows")]
fn rename_self_aside() -> Option<PathBuf> {
    let self_path = std::env::current_exe().ok()?;
    let old_path = self_path.with_extension("old");
    // Remove a stale .old from a previous update run, if any.
    let _ = fs::remove_file(&old_path);
    fs::rename(&self_path, &old_path).ok()?;
    Some(old_path)
}

/// Delete the `Zone.Identifier` alternate data stream (Mark of the Web) from
/// a downloaded file.  This is the same operation as "Unblock" in Windows
/// Explorer or `Unblock-File` in PowerShell.
#[cfg(target_os = "windows")]
fn remove_zone_identifier(path: &Path) {
    let ads = format!("{}:Zone.Identifier", path.display());
    // Ignore failure – the stream simply may not exist.
    let _ = fs::remove_file(ads);
}

/// Terminate residual sidecar processes that live in the install directory and
/// would otherwise lock their own `.exe`, blocking the file overwrite.
///
/// `fluxdown_nmh.exe` is the Native Messaging relay: it is spawned and owned by
/// the **browser**, not by the FluxDown app, so it can still be running after
/// the app exits. We forcibly terminate any instances whose image path is
/// inside `install_dir`, then wait briefly for the OS to release the handles.
///
/// This is best-effort: failure to terminate is non-fatal because
/// `copy_file_retry` still retries, and a genuine lock will surface as a clear
/// failure marker rather than a silent hang.
#[cfg(target_os = "windows")]
fn terminate_residual_processes(install_dir: &Path) {
    const SIDECARS: &[&str] = &["fluxdown_nmh.exe"];

    let dir_lower = install_dir.to_string_lossy().to_lowercase();
    let mut killed_any = false;

    for name in SIDECARS {
        for pid in pids_for_image(name, &dir_lower) {
            if terminate_pid(pid) {
                log_msg(&format!("terminated residual {name} pid={pid}")).ok();
                killed_any = true;
            }
        }
    }

    if killed_any {
        // Give the OS time to fully release the file handles held by the
        // terminated processes before we try to overwrite their .exe.
        thread::sleep(Duration::from_millis(500));
    }
}

/// Enumerate PIDs of running processes whose executable is `image_name` AND
/// whose full image path is located under `dir_lower` (a lowercased directory
/// path). Uses the Toolhelp snapshot + per-process image path query so we never
/// kill an unrelated FluxDown install elsewhere on the machine.
#[cfg(target_os = "windows")]
fn pids_for_image(image_name: &str, dir_lower: &str) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, MAX_PATH};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    let mut out: Vec<u32> = Vec::new();
    let image_lower = image_name.to_lowercase();
    let self_pid = process::id();

    // SAFETY: standard Toolhelp snapshot enumeration with checked return values.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        // CreateToolhelp32Snapshot returns INVALID_HANDLE_VALUE (-1), not NULL,
        // on failure — check both to be safe.
        if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
            return out;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let exe_name = wide_to_string(&entry.szExeFile);
                if exe_name.to_lowercase() == image_lower && entry.th32ProcessID != self_pid {
                    let pid = entry.th32ProcessID;
                    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                    if !handle.is_null() {
                        let mut buf = [0u16; MAX_PATH as usize];
                        let mut size = buf.len() as u32;
                        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
                        CloseHandle(handle);
                        if ok != 0 {
                            let full = String::from_utf16_lossy(&buf[..size as usize]);
                            if full.to_lowercase().starts_with(dir_lower) {
                                out.push(pid);
                            }
                        }
                    }
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }

    out
}

/// Forcibly terminate a process by PID. Returns true on success.
#[cfg(target_os = "windows")]
fn terminate_pid(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    // SAFETY: OpenProcess/TerminateProcess with a validated PID; NULL handle is
    // checked before use.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let ok = TerminateProcess(handle, 1) != 0;
        CloseHandle(handle);
        ok
    }
}

/// Convert a NUL-terminated UTF-16 buffer (Win32 wide string) to a Rust String.
#[cfg(target_os = "windows")]
fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Open a folder in Windows Explorer so the user can recover manually.
#[cfg(target_os = "windows")]
fn open_in_explorer(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = process::Command::new("explorer.exe")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

// ─── Linux platform helpers ───────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn pkexec_install(args: &[&str]) -> Result<(), UpdaterError> {
    let status = process::Command::new("pkexec")
        .args(args)
        .status()
        .map_err(UpdaterError::Io)?;
    if !status.success() {
        return Err(UpdaterError::Other(format!(
            "pkexec exited with code {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn restart_current_exe() -> Result<(), UpdaterError> {
    let exe = std::env::current_exe().map_err(UpdaterError::Io)?;
    process::Command::new(exe)
        .spawn()
        .map_err(UpdaterError::Io)?;
    Ok(())
}

// ─── ZIP extraction ───────────────────────────────────────────────────────────

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), UpdaterError> {
    fs::create_dir_all(dest).map_err(UpdaterError::Io)?;

    let file = fs::File::open(zip_path).map_err(UpdaterError::Io)?;
    let mut archive = zip::ZipArchive::new(file).map_err(UpdaterError::Zip)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(UpdaterError::Zip)?;
        let rel = safe_archive_path(entry.name())?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&rel);

        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(UpdaterError::Io)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(UpdaterError::Io)?;
            }
            let mut f = fs::File::create(&out).map_err(UpdaterError::Io)?;
            io::copy(&mut entry, &mut f).map_err(UpdaterError::Io)?;
        }
    }

    Ok(())
}

// ─── tar.gz extraction (Linux / macOS) ───────────────────────────────────────

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn extract_tarball(tarball: &Path, dest: &Path) -> Result<(), UpdaterError> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    fs::create_dir_all(dest).map_err(UpdaterError::Io)?;

    let file = fs::File::open(tarball).map_err(UpdaterError::Io)?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);
    archive.unpack(dest).map_err(UpdaterError::Io)?;

    Ok(())
}

// ─── Path helpers ─────────────────────────────────────────────────────────────

/// Strip absolute roots and `..` components from an archive entry name to
/// prevent zip-slip / path-traversal attacks.
fn safe_archive_path(name: &str) -> Result<PathBuf, UpdaterError> {
    let mut out = PathBuf::new();
    for comp in Path::new(name).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(UpdaterError::PathTraversal(name.to_string()));
            }
        }
    }
    Ok(out)
}

/// macOS: extract tar.gz → replace .app bundle → relaunch via `open`.
#[allow(unused_variables)]
fn do_macos_app(tarball: &Path, app_bundle: &str, install_dir: &Path) -> Result<(), UpdaterError> {
    #[cfg(target_os = "macos")]
    {
        log_msg(&format!(
            "macos-app: tar={} bundle={app_bundle} dir={}",
            tarball.display(),
            install_dir.display()
        ))
        .ok();

        // Extract to a private temp dir.
        let tmp = std::env::temp_dir().join(format!("fluxdown_upd_{}", process::id()));
        extract_tarball(tarball, &tmp)?;
        log_msg(&format!("extracted to {}", tmp.display())).ok();

        // Unwrap single-folder archives (common GitHub release layout).
        let src_root = single_dir_or_root(&tmp)?;

        // Locate the .app inside the extracted content.
        let new_app = src_root.join(app_bundle);
        if !new_app.exists() {
            return Err(UpdaterError::Other(format!(
                "{app_bundle} not found in extracted archive (looked in {})",
                src_root.display()
            )));
        }

        // Remove Gatekeeper quarantine from the new bundle.
        // The updater itself is already a trusted binary inside the original
        // signed bundle, so macOS allows it to strip the quarantine attribute.
        let _ = process::Command::new("xattr")
            .args(["-dr", "com.apple.quarantine", &new_app.to_string_lossy()])
            .status();

        // Replace the existing .app bundle.
        let dest_app = install_dir.join(app_bundle);
        if dest_app.exists() {
            fs::remove_dir_all(&dest_app).map_err(UpdaterError::Io)?;
        }
        // Try atomic rename (same filesystem) first; fall back to `cp -a`
        // which properly preserves symlinks and extended attributes.
        if fs::rename(&new_app, &dest_app).is_err() {
            let status = process::Command::new("cp")
                .args([
                    "-a",
                    new_app.to_string_lossy().as_ref(),
                    dest_app.to_string_lossy().as_ref(),
                ])
                .status()
                .map_err(UpdaterError::Io)?;
            if !status.success() {
                return Err(UpdaterError::Other(format!(
                    "cp -a failed to copy {app_bundle}"
                )));
            }
            let _ = fs::remove_dir_all(&new_app);
        }
        log_msg("app bundle replaced").ok();

        // Cleanup.
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_file(tarball);

        // Relaunch the updated bundle using the macOS `open` command.
        process::Command::new("open")
            .arg(&dest_app)
            .spawn()
            .map_err(UpdaterError::Io)?;
        log_msg(&format!("reopened: {}", dest_app.display())).ok();

        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    Err(UpdaterError::Other(
        "macos-app mode is only supported on macOS".to_string(),
    ))
}

/// If `dir` contains exactly one child directory (common GitHub release layout
/// such as `FluxDown-v1.2.3-windows/`), return that child as the real source
/// so files are not nested one level too deep in the destination.
fn single_dir_or_root(dir: &Path) -> Result<PathBuf, UpdaterError> {
    let children: Vec<_> = fs::read_dir(dir)
        .map_err(UpdaterError::Io)?
        .filter_map(|e| e.ok())
        .collect();
    if children.len() == 1 {
        let entry = &children[0];
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            return Ok(entry.path());
        }
    }
    Ok(dir.to_path_buf())
}

// ─── File copy with retry ─────────────────────────────────────────────────────

/// Recursively copy everything in `src` into `dst`.  Individual files are
/// retried up to `MAX_FILE_RETRIES` times with exponential back-off to
/// tolerate transient locks from antivirus scanners or the OS loader.
fn copy_dir_retry(src: &Path, dst: &Path) -> Result<(), UpdaterError> {
    fs::create_dir_all(dst).map_err(UpdaterError::Io)?;
    for entry in fs::read_dir(src).map_err(UpdaterError::Io)? {
        let entry = entry.map_err(UpdaterError::Io)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_retry(&src_path, &dst_path)?;
        } else {
            copy_file_retry(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn copy_file_retry(src: &Path, dst: &Path) -> Result<(), UpdaterError> {
    const MAX_RETRIES: u32 = 3;
    for attempt in 0..=MAX_RETRIES {
        match fs::copy(src, dst) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if attempt == MAX_RETRIES {
                    return Err(UpdaterError::Io(e));
                }
                // Exponential back-off: 500 ms → 1 s → 2 s
                thread::sleep(Duration::from_millis(500u64 << attempt));
            }
        }
    }
    // Logically unreachable: every iteration either returns early above or is
    // guarded by `attempt == MAX_RETRIES` which returns on the last iteration.
    Err(UpdaterError::Other("copy failed after retries".to_string()))
}

// ─── Failure marker ───────────────────────────────────────────────────────────

/// Write a failure marker file that the main app reads on its next launch to
/// inform the user that the automatic update failed.
///
/// The marker is written to the install directory (so the app can find it
/// relative to its own executable). If the install directory itself is not
/// writable (the very condition that often causes the update to fail), we fall
/// back to the OS temp directory, which the app also checks on startup.
///
/// Format: first line is a short machine-readable reason tag, the rest is a
/// human-readable message shown verbatim in the UI.
fn write_failure_marker(install_dir: &Path, message: &str) {
    const MARKER_NAME: &str = "update_failed.marker";
    let body = format!("portable-copy-failed\n{message}");

    // Primary: install directory (preferred — app looks here first).
    let primary = install_dir.join(MARKER_NAME);
    if fs::write(&primary, &body).is_ok() {
        log_msg(&format!("wrote failure marker: {}", primary.display())).ok();
        return;
    }

    // Fallback: temp directory (always writable).
    let fallback = std::env::temp_dir().join(MARKER_NAME);
    if fs::write(&fallback, &body).is_ok() {
        log_msg(&format!(
            "wrote failure marker (temp): {}",
            fallback.display()
        ))
        .ok();
    } else {
        log_msg("failed to write failure marker anywhere").ok();
    }
}

// ─── Logging ─────────────────────────────────────────────────────────────────

fn log_msg(msg: &str) -> io::Result<()> {
    use io::Write;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let path = std::env::temp_dir().join("fluxdown_updater.log");
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "[{ts}] {msg}")
}
