//! URL protocol (scheme) handler registration for the app's own `fluxdown://`
//! deep links and for `ed2k://` eDonkey2000 links.
//!
//! Windows — HKCU registry:
//! ```text
//! HKCU\Software\Classes\<scheme>                             → "URL:<desc>"
//! HKCU\Software\Classes\<scheme>  "URL Protocol"             → ""
//! HKCU\Software\Classes\<scheme>\DefaultIcon                 → "\"<exe>\",0"
//! HKCU\Software\Classes\<scheme>\shell\open\command          → "\"<exe>\" \"%1\""
//! ```
//! All operations target `HKEY_CURRENT_USER` — no admin elevation required.
//!
//! Linux — the XDG `x-scheme-handler/<scheme>` MIME type, set via `xdg-mime`
//! (the `.desktop` file must declare the handler; see `linux/com.fluxdown.app.desktop`).
//!
//! macOS — Launch Services (`LSSetDefaultHandlerForURLScheme`); the scheme must
//! be declared in `CFBundleURLTypes` (`macos/Runner/Info.plist`).
//!
//! `register`/`unregister` above run outside the Windows installer's
//! tracking (written directly via winreg at runtime), so
//! `installer/windows/setup.iss` removes any leftover `fluxdown`/`ed2k`/
//! `magnet` Windows registry keys explicitly on uninstall — keep both in sync.

/// A URL scheme this app can claim as the system default handler for.
#[derive(Clone, Copy)]
pub struct UrlScheme {
    /// Scheme token without `://` (e.g. `fluxdown`), lowercase.
    pub scheme: &'static str,
    /// Windows shell description stored as the class key's default value.
    /// Unused on Linux/macOS, where the handler is named by the packaged
    /// `.desktop` entry / bundle id instead.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub desc: &'static str,
}

/// The app's own deep-link scheme. Auto-registered on startup (Windows only,
/// see `download_actor`); packaged declarations cover Linux/macOS.
pub const FLUXDOWN: UrlScheme = UrlScheme {
    scheme: "fluxdown",
    desc: "URL:FluxDown Protocol",
};

/// eDonkey2000 links (`ed2k://|file|…`). Opt-in from settings only — other
/// clients (eMule, aMule) legitimately compete for this scheme.
pub const ED2K: UrlScheme = UrlScheme {
    scheme: "ed2k",
    desc: "URL:ed2k Protocol",
};

/// BitTorrent magnet links (`magnet:?xt=urn:btih:…`). Default-on but
/// non-preemptive: startup auto-registration (Windows, see `download_actor`)
/// skips the scheme when another client (qBittorrent, …) already claims it;
/// only the explicit settings toggle takes it over. Linux/macOS candidate
/// declarations live in the packaged `.desktop` / `Info.plist`.
pub const MAGNET: UrlScheme = UrlScheme {
    scheme: "magnet",
    desc: "URL:Magnet Link",
};

/// Resolve a wire scheme name (as sent by Dart) to a known [`UrlScheme`].
///
/// Returns `None` for anything not in the allow-list above: the registration
/// primitives write to the shell/registry, so the scheme must never be an
/// arbitrary caller-supplied string.
pub fn from_name(name: &str) -> Option<UrlScheme> {
    match name {
        "fluxdown" => Some(FLUXDOWN),
        "ed2k" => Some(ED2K),
        "magnet" => Some(MAGNET),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
mod inner {
    use super::UrlScheme;
    use crate::logger::log_info;
    use std::io;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};

    /// Get the canonical path of the current running executable.
    ///
    /// Uses `std::fs::canonicalize` to resolve symlinks and `\\?\` prefixes,
    /// then strips the `\\?\` prefix (if any) for clean comparison with
    /// registry values written by `register()`.
    fn exe_path() -> Result<String, io::Error> {
        let path = std::env::current_exe()?;
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        let s = canonical.to_string_lossy().into_owned();
        Ok(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
    }

    /// Check whether `proto` is currently registered **to this running
    /// executable**.
    ///
    /// Returns `true` only if `HKCU\Software\Classes\<scheme>` exists, has a
    /// `URL Protocol` value (which identifies it as a protocol handler), **and**
    /// its `shell\open\command` default value points to the current exe.
    ///
    /// The exe-path check is what prevents a stale registration from being kept
    /// after the app is moved, upgraded, or run as a different portable build:
    /// without it, the presence of the (now-wrong) key would make startup skip
    /// re-registration and leave the scheme pointing at a dead/old exe. This
    /// mirrors `nmh_registry::needs_update`'s exe-drift detection. If the exe
    /// path cannot be determined, the value check alone decides the result so we
    /// do not spuriously force re-registration on a transient I/O error.
    ///
    /// For contended schemes (`ed2k://`) the same check doubles as ownership
    /// detection: another client's registration fails the exe comparison, so we
    /// report "not registered" instead of claiming someone else's handler.
    pub fn is_registered(proto: UrlScheme) -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let scheme = proto.scheme;

        let key =
            match hkcu.open_subkey_with_flags(format!("Software\\Classes\\{scheme}"), KEY_READ) {
                Ok(k) => k,
                Err(_) => return false,
            };

        // The presence of "URL Protocol" value is what makes this a protocol handler.
        let url_protocol: Result<String, _> = key.get_value("URL Protocol");
        if url_protocol.is_err() {
            return false;
        }

        // Determine the current exe path; if it cannot be resolved, fall back to
        // the legacy "value exists" semantics rather than forcing re-registration.
        let current_exe = match exe_path() {
            Ok(p) => p,
            Err(_) => return true,
        };

        // Read shell\open\command's default value and extract the exe it points
        // to. The stored form is `"<exe>" "%1"`; we extract the first
        // double-quoted token (the exe path).
        let registered_exe = match read_command_exe(&hkcu, scheme) {
            Some(exe) => exe,
            // Command subkey/value missing: registration is incomplete → treat as
            // not registered to this exe so startup rewrites it.
            None => return false,
        };

        paths_equivalent(&registered_exe, &current_exe)
    }

    /// Read `Software\Classes\<scheme>\shell\open\command`'s default value and
    /// extract the executable path (the first double-quoted token).
    ///
    /// Returns `None` if the key/value is missing or no quoted token is present.
    fn read_command_exe(hkcu: &RegKey, scheme: &str) -> Option<String> {
        let cmd_key = hkcu
            .open_subkey_with_flags(
                format!("Software\\Classes\\{scheme}\\shell\\open\\command"),
                KEY_READ,
            )
            .ok()?;
        let command: String = cmd_key.get_value("").ok()?;

        // Extract the substring between the first pair of double quotes.
        let after_first = command.find('"')? + 1;
        let rest = &command[after_first..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    }

    /// Compare two Windows exe paths for equivalence.
    ///
    /// Canonicalizes both (resolving symlinks / `\\?\` prefixes) when possible
    /// and compares case-insensitively, matching Windows' case-insensitive file
    /// system semantics. Falls back to a case-insensitive comparison of the raw
    /// strings if canonicalization fails (e.g. the registered exe no longer
    /// exists), which still correctly reports a stale/moved registration.
    fn paths_equivalent(a: &str, b: &str) -> bool {
        let norm = |s: &str| -> String {
            let canonical = std::fs::canonicalize(s)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| s.to_string());
            canonical
                .strip_prefix(r"\\?\")
                .unwrap_or(&canonical)
                .to_ascii_lowercase()
        };
        norm(a) == norm(b)
    }

    /// Whether `proto` appears to be actively claimed by another application.
    ///
    /// Used by startup auto-registration to stay non-preemptive on contended
    /// schemes (`magnet://`): FluxDown never silently steals a handler that a
    /// competing client (qBittorrent, …) or the user has already established.
    /// The explicit settings toggle bypasses this check — `register()` always
    /// writes.
    ///
    /// Signals checked, mirroring Windows' own resolution order:
    /// 1. `UserChoice` pin (`HKCU\…\Shell\Associations\UrlAssociations\<scheme>`)
    ///    — a per-user default explicitly chosen in Windows Settings / the
    ///    "open with" prompt. FluxDown never creates one for itself, so its
    ///    presence (while we are not the registered handler) means someone
    ///    else won the scheme.
    /// 2. A foreign exe in `HKCU\Software\Classes\<scheme>\shell\open\command`.
    /// 3. A foreign exe in `HKLM\Software\Classes\<scheme>\shell\open\command`
    ///    — machine-wide direct registrations. A bare HKLM class marker
    ///    without a command (qBittorrent's capability-style declaration, often
    ///    orphaned by its uninstaller) is deliberately NOT treated as a claim:
    ///    it cannot open links by itself, only feed the "choose an app" list.
    pub fn is_claimed_by_other(proto: UrlScheme) -> bool {
        if is_registered(proto) {
            return false;
        }
        let scheme = proto.scheme;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // 1. UserChoice pin.
        let user_choice_path = format!(
            "Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\{scheme}\\UserChoice"
        );
        if hkcu
            .open_subkey_with_flags(user_choice_path, KEY_READ)
            .ok()
            .is_some_and(|k| k.get_value::<String, _>("ProgId").is_ok())
        {
            return true;
        }

        // 2./3. Direct scheme command pointing at a foreign exe. `current_exe`
        // failing to resolve is treated as foreign — better to skip
        // auto-registration than to overwrite an unidentified handler.
        let current_exe = exe_path().ok();
        let foreign_command = |hive: &RegKey| -> bool {
            match read_command_exe(hive, scheme) {
                Some(exe) => !current_exe
                    .as_deref()
                    .is_some_and(|cur| paths_equivalent(&exe, cur)),
                None => false,
            }
        };
        foreign_command(&hkcu) || foreign_command(&RegKey::predef(HKEY_LOCAL_MACHINE))
    }

    /// Register `proto` as handled by this executable.
    pub fn register(proto: UrlScheme) -> Result<(), io::Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let exe = exe_path()?;
        let scheme = proto.scheme;

        // 1. <scheme> → "URL:<desc>"
        let (proto_key, _) =
            hkcu.create_subkey_with_flags(format!("Software\\Classes\\{scheme}"), KEY_WRITE)?;
        proto_key.set_value("", &proto.desc)?;
        // The empty "URL Protocol" value is required to mark this as a URL protocol handler.
        proto_key.set_value("URL Protocol", &"")?;

        // 2. DefaultIcon
        let (icon_key, _) = hkcu.create_subkey_with_flags(
            format!("Software\\Classes\\{scheme}\\DefaultIcon"),
            KEY_WRITE,
        )?;
        icon_key.set_value("", &format!("\"{exe}\",0"))?;

        // 3. shell\open\command
        let (cmd_key, _) = hkcu.create_subkey_with_flags(
            format!("Software\\Classes\\{scheme}\\shell\\open\\command"),
            KEY_WRITE,
        )?;
        cmd_key.set_value("", &format!("\"{exe}\" \"%1\""))?;

        // Notify the shell about the change
        notify_shell();

        log_info!("[protocol_registry] registered {scheme}:// protocol (exe={exe})");
        Ok(())
    }

    /// Remove this app's registration of `proto`.
    pub fn unregister(proto: UrlScheme) -> Result<(), io::Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let scheme = proto.scheme;

        // Only remove if currently registered (don't break other app's registration)
        if !is_registered(proto) {
            log_info!(
                "[protocol_registry] {scheme}:// not registered to FluxDown, skipping removal"
            );
            return Ok(());
        }

        // Remove the per-user scheme tree; an HKLM registration by another
        // client (if any) becomes effective again.
        let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_WRITE)?;
        let _ = classes.delete_subkey_all(scheme);

        // Notify the shell about the change
        notify_shell();

        log_info!("[protocol_registry] removed {scheme}:// protocol registration");
        Ok(())
    }

    /// Call SHChangeNotify to inform Explorer about association changes.
    fn notify_shell() {
        // SHCNE_ASSOCCHANGED = 0x08000000, SHCNF_IDLIST = 0x0000
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SHChangeNotify(
                wEventId: i32,
                uFlags: u32,
                dwItem1: *const std::ffi::c_void,
                dwItem2: *const std::ffi::c_void,
            );
        }
        unsafe {
            SHChangeNotify(0x08000000, 0, std::ptr::null(), std::ptr::null());
        }
    }
}

// Linux implementation — the XDG `x-scheme-handler/<scheme>` MIME type,
// mirroring `file_association`'s handling of `application/x-bittorrent`.
#[cfg(target_os = "linux")]
mod inner {
    use super::UrlScheme;
    use std::io;

    const DESKTOP_ENTRY: &str = "com.fluxdown.app.desktop";

    fn mime_type(proto: UrlScheme) -> String {
        format!("x-scheme-handler/{}", proto.scheme)
    }

    /// Check whether FluxDown is the default handler for `proto`.
    ///
    /// Queries `xdg-mime query default x-scheme-handler/<scheme>` and checks
    /// whether the returned .desktop name contains "fluxdown".
    pub fn is_registered(proto: UrlScheme) -> bool {
        let Ok(output) = std::process::Command::new("xdg-mime")
            .args(["query", "default", &mime_type(proto)])
            .output()
        else {
            return false;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.to_lowercase().contains("fluxdown")
    }

    /// Register FluxDown as the default handler for `proto`.
    ///
    /// Requires that `com.fluxdown.app.desktop` is installed in an XDG
    /// applications directory and declares the scheme handler in `MimeType`.
    pub fn register(proto: UrlScheme) -> Result<(), io::Error> {
        std::process::Command::new("xdg-mime")
            .args(["default", DESKTOP_ENTRY, &mime_type(proto)])
            .status()
            .map(|_| ())
    }

    /// Hand `proto` back to the system default by dropping the user override.
    ///
    /// xdg-mime has no "unset" command, so we edit `mimeapps.list` directly:
    /// remove the `x-scheme-handler/<scheme>=com.fluxdown.app.desktop` line
    /// from the `[Default Applications]` section.
    pub fn unregister(proto: UrlScheme) -> Result<(), io::Error> {
        use std::io::{BufRead, Write};

        // Locate ~/.config/mimeapps.list (XDG spec default).
        let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.config")
        });
        let path = std::path::PathBuf::from(&config_dir).join("mimeapps.list");

        if !path.exists() {
            return Ok(());
        }

        let file = std::fs::File::open(&path)?;
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()?;

        let prefix = format!("{}=", mime_type(proto));
        let filtered: Vec<&str> = lines
            .iter()
            .filter(|l| {
                let lower = l.to_lowercase();
                !(lower.starts_with(&prefix) && lower.contains("fluxdown"))
            })
            .map(|l| l.as_str())
            .collect();

        let mut out = std::fs::File::create(&path)?;
        for line in filtered {
            writeln!(out, "{line}")?;
        }
        Ok(())
    }
}

// macOS implementation — Launch Services default handler for a URL scheme.
// The scheme must be declared in `CFBundleURLTypes` (macos/Runner/Info.plist)
// for Launch Services to accept this bundle as a candidate handler.
#[cfg(target_os = "macos")]
mod inner {
    use super::UrlScheme;
    use crate::logger::log_info;
    use crate::macos_cf::{CFStringRef, CfOwned, cf_string, cf_to_string, main_bundle_id};
    use std::io;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyDefaultHandlerForURLScheme(url_scheme: CFStringRef) -> CFStringRef;
        fn LSSetDefaultHandlerForURLScheme(
            url_scheme: CFStringRef,
            handler_bundle_id: CFStringRef,
        ) -> i32;
    }

    /// Check whether this bundle is the default handler for `proto`.
    pub fn is_registered(proto: UrlScheme) -> bool {
        let Ok(scheme) = cf_string(proto.scheme) else {
            return false;
        };
        // SAFETY: `scheme.raw()` is a valid CFStringRef; the returned handler
        // ref is owned by us and released via the `CfOwned` guard below.
        let handler = CfOwned::new(unsafe { LSCopyDefaultHandlerForURLScheme(scheme.raw()) });
        let Some(handler_id) = cf_to_string(handler.raw()) else {
            return false;
        };
        match main_bundle_id() {
            Some(mine) => handler_id.eq_ignore_ascii_case(&mine),
            None => false,
        }
    }

    /// Make this bundle the default handler for `proto`.
    pub fn register(proto: UrlScheme) -> Result<(), io::Error> {
        let bundle_id =
            main_bundle_id().ok_or_else(|| io::Error::other("main bundle id unavailable"))?;
        let scheme = cf_string(proto.scheme)?;
        let id = cf_string(&bundle_id)?;
        // SAFETY: both CFStringRefs are alive for the duration of the call; the
        // function does not take ownership of them.
        let status = unsafe { LSSetDefaultHandlerForURLScheme(scheme.raw(), id.raw()) };
        if status != 0 {
            return Err(io::Error::other(format!(
                "LSSetDefaultHandlerForURLScheme failed (OSStatus={status})"
            )));
        }
        log_info!(
            "[protocol_registry] registered {}:// protocol (bundle={bundle_id})",
            proto.scheme
        );
        Ok(())
    }

    /// Hand `proto` back to the system default.
    ///
    /// Launch Services has no "unset" primitive; setting the handler to an empty
    /// bundle id releases the scheme. Only acts if we currently own it (don't
    /// clobber another client's choice).
    pub fn unregister(proto: UrlScheme) -> Result<(), io::Error> {
        if !is_registered(proto) {
            log_info!(
                "[protocol_registry] {}:// not registered to FluxDown, skipping removal",
                proto.scheme
            );
            return Ok(());
        }
        let scheme = cf_string(proto.scheme)?;
        let empty = cf_string("")?;
        // SAFETY: both CFStringRefs are alive for the duration of the call.
        let status = unsafe { LSSetDefaultHandlerForURLScheme(scheme.raw(), empty.raw()) };
        if status != 0 {
            return Err(io::Error::other(format!(
                "LSSetDefaultHandlerForURLScheme (clear) failed (OSStatus={status})"
            )));
        }
        log_info!(
            "[protocol_registry] removed {}:// protocol registration",
            proto.scheme
        );
        Ok(())
    }
}

// Fallback stubs for platforms without a native implementation.
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod inner {
    use super::UrlScheme;
    use std::io;

    pub fn is_registered(_proto: UrlScheme) -> bool {
        false
    }

    pub fn register(_proto: UrlScheme) -> Result<(), io::Error> {
        Ok(())
    }

    pub fn unregister(_proto: UrlScheme) -> Result<(), io::Error> {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use inner::is_claimed_by_other;
pub use inner::{is_registered, register, unregister};

#[cfg(all(test, target_os = "windows"))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

    /// A scheme nobody ships, so the round-trip below cannot disturb a real
    /// handler (not in [`from_name`]'s allow-list either — tests reach the
    /// primitives directly).
    const SCRATCH: UrlScheme = UrlScheme {
        scheme: "fluxdown-selftest",
        desc: "URL:FluxDown Selftest Protocol",
    };

    /// Drop the scratch key regardless of which exe last claimed it, so a
    /// previously aborted run cannot poison this one.
    fn purge_scratch() {
        let classes = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\Classes", KEY_WRITE)
            .unwrap();
        let _ = classes.delete_subkey_all(SCRATCH.scheme);
    }

    #[test]
    fn registry_roundtrip_is_observable_and_reversible() {
        purge_scratch();
        assert!(!is_registered(SCRATCH));

        register(SCRATCH).unwrap();
        assert!(
            is_registered(SCRATCH),
            "a scheme we just claimed must read back as ours"
        );

        unregister(SCRATCH).unwrap();
        assert!(
            !is_registered(SCRATCH),
            "unregister must hand the scheme back"
        );
        purge_scratch();
    }

    #[test]
    fn from_name_only_resolves_known_schemes() {
        assert_eq!(from_name("ed2k").unwrap().scheme, "ed2k");
        assert_eq!(from_name("fluxdown").unwrap().scheme, "fluxdown");
        assert_eq!(from_name("magnet").unwrap().scheme, "magnet");
        // Anything else must never reach the registry writers.
        assert!(from_name("file").is_none());
        assert!(from_name("ED2K").is_none());
        assert!(from_name("").is_none());
    }

    /// A second scratch scheme so this test cannot race
    /// `registry_roundtrip_is_observable_and_reversible` (tests run in
    /// parallel within the binary).
    const SCRATCH_CLAIM: UrlScheme = UrlScheme {
        scheme: "fluxdown-selftest-claim",
        desc: "URL:FluxDown Selftest Claim Protocol",
    };

    fn purge(scheme: &str) {
        let classes = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\Classes", KEY_WRITE)
            .unwrap();
        let _ = classes.delete_subkey_all(scheme);
    }

    /// Non-preemptive startup contract: a scheme registered by another exe is
    /// reported as claimed (so auto-register skips it) but never as ours,
    /// while our own registration is ours and not "claimed by other".
    #[test]
    fn foreign_registration_is_claimed_but_not_ours() {
        purge(SCRATCH_CLAIM.scheme);
        assert!(!is_claimed_by_other(SCRATCH_CLAIM), "unclaimed scheme");

        // Simulate a competing client's registration pointing at another exe.
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let scheme = SCRATCH_CLAIM.scheme;
        let (key, _) = hkcu
            .create_subkey(format!("Software\\Classes\\{scheme}"))
            .unwrap();
        key.set_value("URL Protocol", &"").unwrap();
        let (cmd, _) = hkcu
            .create_subkey(format!("Software\\Classes\\{scheme}\\shell\\open\\command"))
            .unwrap();
        cmd.set_value("", &"\"C:\\nonexistent\\other-client.exe\" \"%1\"")
            .unwrap();

        assert!(!is_registered(SCRATCH_CLAIM), "foreign handler is not ours");
        assert!(
            is_claimed_by_other(SCRATCH_CLAIM),
            "foreign handler must block auto-registration"
        );

        // Explicit takeover (the settings toggle path) overwrites it.
        register(SCRATCH_CLAIM).unwrap();
        assert!(is_registered(SCRATCH_CLAIM));
        assert!(!is_claimed_by_other(SCRATCH_CLAIM));

        purge(SCRATCH_CLAIM.scheme);
    }
}
