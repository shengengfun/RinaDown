//! Rewrites the `IconLocation` of Windows Explorer shortcuts (`.lnk`) that
//! target this app's executable, so desktop / Start Menu / taskbar-pinned
//! shortcuts follow the user's chosen app icon.
//!
//! # Why this exists
//!
//! `installer/windows/setup.iss`'s `[Icons]` section creates shortcuts with
//! `Filename: "{app}\flux_down.exe"` and no explicit `IconFilename` — Inno
//! Setup then defaults `IconLocation` to the target exe itself (icon index
//! 0), which is the icon compiled into the exe's PE resources
//! (`windows/runner/Runner.rc` → `app_icon.ico`). That reference is written
//! once at install time and never changes afterwards.
//!
//! `AppIconService` (`lib/src/services/app_icon_service.dart`) switches the
//! *running* window/taskbar-button/Alt-Tab/tray icon via `WM_SETICON`
//! (`lib/src/services/win32_window_icon.dart`), which only affects the
//! current process's own
//! window handle — it cannot touch a `.lnk` file's static `IconLocation`,
//! nor the exe's own on-disk PE resources. Without this module, changing
//! the app icon in Settings never propagates to the desktop icon, the Start
//! Menu icon, or an icon the user has pinned to the taskbar.
//!
//! # Approach
//!
//! `windows-sys` ships no COM interface bindings (only raw functions and
//! constants), so `IShellLinkW`/`IPersistFile` are hand-rolled here as
//! `#[repr(C)]` vtables matching the stable, decades-old Win32 ABI
//! (`shobjidl_core.h` / `objidl.h`). For each shortcut found:
//! `CoCreateInstance(CLSID_ShellLink)` → `QueryInterface(IPersistFile)` →
//! `Load` the existing `.lnk` → verify its target resolves to this app's
//! exe (guards the taskbar pin folder, which holds pins for every app, not
//! just FluxDown) → `SetIconLocation` → `Save` → `SHChangeNotify` to make
//! Explorer refresh that item's icon immediately.
//!
//! The target check fails **closed**: shortcuts whose target cannot be
//! positively resolved to this exe are never rewritten. That matters for
//! PIDL-only pins — the taskbar's own "File Explorer" pin and UWP app pins
//! have no filesystem target at all (`IShellLinkW::GetPath` returns
//! `S_FALSE` with an empty buffer). A previous version failed *open* here
//! and stamped the FluxDown icon onto the Explorer pin; `listen` therefore
//! also runs a startup repair sweep that clears any foreign shortcut whose
//! `IconLocation` still points at a FluxDown-owned icon file.
//!
//! Linux/macOS handle the equivalent problem differently (see
//! `AppIconService._applyIcon`): Linux overwrites the user's XDG icon-theme
//! override, macOS uses `NSWorkspace.setIcon` via the native
//! `com.fluxdown/window` channel. Neither needs a Rust-side component, so
//! this module is Windows-only.

#[cfg(target_os = "windows")]
mod inner {
    use crate::logger::log_info;
    use crate::signals::UpdateShortcutIcons;
    use rinf::DartSignal;
    use std::ffi::{OsStr, c_void};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_QuickLaunch, SHChangeNotify,
        SHGetKnownFolderPath, ShellLink as CLSID_SHELL_LINK,
    };
    use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};

    /// `{000214F9-0000-0000-C000-000000000046}` — `IID_IShellLinkW`. Not
    /// exported by `windows-sys` (no COM interface bindings ship in this
    /// crate); value from `shobjidl_core.h`, stable since Windows 95.
    const IID_ISHELLLINKW: GUID = GUID::from_u128(0x000214F9_0000_0000_C000_000000000046);
    /// `{0000010B-0000-0000-C000-000000000046}` — `IID_IPersistFile`, from
    /// `objidl.h`.
    const IID_IPERSISTFILE: GUID = GUID::from_u128(0x0000010B_0000_0000_C000_000000000046);
    /// `{886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}` — `IID_IPropertyStore`, from
    /// `propsys.h`. `ShellLink` implements it; used to read a pin's
    /// AppUserModelID.
    const IID_IPROPERTYSTORE: GUID = GUID::from_u128(0x886D8EEB_8CF2_4446_8D02_CDBA1DBDCF99);

    /// `STGM_READWRITE`, from `objidl.h`'s `STGM` enum.
    const STGM_READWRITE: u32 = 0x0000_0002;
    /// `SHCNE_ASSOCCHANGED` — global association change. Fired once after a
    /// sweep that modified any `.lnk`: per-item `SHCNE_UPDATEITEM` alone does
    /// not make the taskbar repaint an already-displayed pin (its icon is
    /// served from the shell icon cache); ASSOCCHANGED flushes that cache.
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    /// `SHCNF_IDLIST` — notification payload is a PIDL (null here).
    const SHCNF_IDLIST: u32 = 0;
    /// Win32 `MAX_PATH`.
    const MAX_PATH: usize = 260;
    /// `COINIT_APARTMENTTHREADED`.
    const COINIT_APARTMENTTHREADED: u32 = 2;
    /// `SHCNE_UPDATEITEM` — a single shell item's attributes/icon changed.
    const SHCNE_UPDATEITEM: i32 = 0x0000_2000;
    /// `SHCNF_PATHW` — the notification's item is a null-terminated wide path.
    const SHCNF_PATHW: u32 = 0x0005;
    /// `PKEY_AppUserModel_ID` (`{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}`, 5)
    /// — a shortcut's AppUserModelID, from `propkey.h`.
    const PKEY_APPUSERMODEL_ID: PropertyKey = PropertyKey {
        fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
        pid: 5,
    };
    /// `VT_LPWSTR`, from `wtypes.h`.
    const VT_LPWSTR: u16 = 31;
    /// AppUserModelID of the taskbar's own "File Explorer" pin — a stable,
    /// locale-independent identity (the `.lnk` file name is localized).
    const EXPLORER_AUMID: &str = "Microsoft.Windows.Explorer";

    // ---- hand-rolled COM vtables ------------------------------------------------
    // windows-sys 0.59 only generates plain FFI (functions/constants/plain
    // structs), not COM interface bindings. Layout matches the standard,
    // unchanged-since-NT4 Win32 ABI: a COM object pointer is `*mut *const Vtbl`
    // (first field of the object is a pointer to its vtable).

    #[repr(C)]
    struct IUnknownVtbl {
        query_interface: unsafe extern "system" fn(
            this: *mut c_void,
            riid: *const GUID,
            ppv: *mut *mut c_void,
        ) -> HRESULT,
        add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
        release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    }

    #[repr(C)]
    struct IPersistVtbl {
        unknown: IUnknownVtbl,
        get_class_id: unsafe extern "system" fn(this: *mut c_void, class_id: *mut GUID) -> HRESULT,
    }

    #[repr(C)]
    struct IPersistFileVtbl {
        persist: IPersistVtbl,
        is_dirty: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
        load: unsafe extern "system" fn(this: *mut c_void, file_name: PCWSTR, mode: u32) -> HRESULT,
        save: unsafe extern "system" fn(
            this: *mut c_void,
            file_name: PCWSTR,
            remember: i32,
        ) -> HRESULT,
        save_completed: unsafe extern "system" fn(this: *mut c_void, file_name: PCWSTR) -> HRESULT,
        get_cur_file:
            unsafe extern "system" fn(this: *mut c_void, file_name: *mut PWSTR) -> HRESULT,
    }

    #[repr(C)]
    struct IShellLinkWVtbl {
        unknown: IUnknownVtbl,
        get_path: unsafe extern "system" fn(
            this: *mut c_void,
            file: PWSTR,
            cch_max_path: i32,
            find_data: *mut WIN32_FIND_DATAW,
            flags: u32,
        ) -> HRESULT,
        get_id_list:
            unsafe extern "system" fn(this: *mut c_void, ppidl: *mut *mut c_void) -> HRESULT,
        set_id_list: unsafe extern "system" fn(this: *mut c_void, pidl: *const c_void) -> HRESULT,
        get_description:
            unsafe extern "system" fn(this: *mut c_void, name: PWSTR, cch_max_name: i32) -> HRESULT,
        set_description: unsafe extern "system" fn(this: *mut c_void, name: PCWSTR) -> HRESULT,
        get_working_directory:
            unsafe extern "system" fn(this: *mut c_void, dir: PWSTR, cch_max_path: i32) -> HRESULT,
        set_working_directory: unsafe extern "system" fn(this: *mut c_void, dir: PCWSTR) -> HRESULT,
        get_arguments:
            unsafe extern "system" fn(this: *mut c_void, args: PWSTR, cch_max_path: i32) -> HRESULT,
        set_arguments: unsafe extern "system" fn(this: *mut c_void, args: PCWSTR) -> HRESULT,
        get_hotkey: unsafe extern "system" fn(this: *mut c_void, hotkey: *mut u16) -> HRESULT,
        set_hotkey: unsafe extern "system" fn(this: *mut c_void, hotkey: u16) -> HRESULT,
        get_show_cmd: unsafe extern "system" fn(this: *mut c_void, show_cmd: *mut i32) -> HRESULT,
        set_show_cmd: unsafe extern "system" fn(this: *mut c_void, show_cmd: i32) -> HRESULT,
        get_icon_location: unsafe extern "system" fn(
            this: *mut c_void,
            icon_path: PWSTR,
            cch_icon_path: i32,
            icon_index: *mut i32,
        ) -> HRESULT,
        set_icon_location: unsafe extern "system" fn(
            this: *mut c_void,
            icon_path: PCWSTR,
            icon_index: i32,
        ) -> HRESULT,
        set_relative_path: unsafe extern "system" fn(
            this: *mut c_void,
            path_rel: PCWSTR,
            reserved: u32,
        ) -> HRESULT,
        resolve:
            unsafe extern "system" fn(this: *mut c_void, hwnd: *mut c_void, flags: u32) -> HRESULT,
        set_path: unsafe extern "system" fn(this: *mut c_void, file: PCWSTR) -> HRESULT,
    }

    /// `PROPERTYKEY`, from `wtypes.h`.
    #[repr(C)]
    struct PropertyKey {
        fmtid: GUID,
        pid: u32,
    }

    /// `PROPVARIANT` (64-bit layout): 8-byte header, then the value union
    /// (`pwszVal` for `VT_LPWSTR` sits at offset 8).
    #[repr(C)]
    struct PropVariant {
        vt: u16,
        w_reserved1: u16,
        w_reserved2: u16,
        w_reserved3: u16,
        data: [usize; 2],
    }

    #[repr(C)]
    struct IPropertyStoreVtbl {
        unknown: IUnknownVtbl,
        get_count: unsafe extern "system" fn(this: *mut c_void, props: *mut u32) -> HRESULT,
        get_at: unsafe extern "system" fn(
            this: *mut c_void,
            index: u32,
            key: *mut PropertyKey,
        ) -> HRESULT,
        get_value: unsafe extern "system" fn(
            this: *mut c_void,
            key: *const PropertyKey,
            value: *mut PropVariant,
        ) -> HRESULT,
        set_value: unsafe extern "system" fn(
            this: *mut c_void,
            key: *const PropertyKey,
            value: *const PropVariant,
        ) -> HRESULT,
        commit: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn PropVariantClear(pvar: *mut PropVariant) -> HRESULT;
    }

    unsafe fn vtbl<T>(obj: *mut c_void) -> *const T {
        unsafe { *(obj as *mut *const T) }
    }

    unsafe fn unknown_query_interface(obj: *mut c_void, iid: &GUID) -> Option<*mut c_void> {
        unsafe {
            let v = vtbl::<IUnknownVtbl>(obj);
            let mut out: *mut c_void = std::ptr::null_mut();
            let hr = ((*v).query_interface)(obj, iid as *const GUID, &mut out as *mut *mut c_void);
            if hr >= 0 && !out.is_null() {
                Some(out)
            } else {
                None
            }
        }
    }

    unsafe fn unknown_release(obj: *mut c_void) {
        unsafe {
            let v = vtbl::<IUnknownVtbl>(obj);
            ((*v).release)(obj);
        }
    }

    unsafe fn shelllink_get_path(obj: *mut c_void, buf: &mut [u16]) -> HRESULT {
        unsafe {
            let v = vtbl::<IShellLinkWVtbl>(obj);
            let len = i32::try_from(buf.len()).unwrap_or(i32::MAX);
            ((*v).get_path)(obj, buf.as_mut_ptr(), len, std::ptr::null_mut(), 0)
        }
    }

    unsafe fn shelllink_set_icon_location(obj: *mut c_void, icon_path: PCWSTR) -> HRESULT {
        unsafe {
            let v = vtbl::<IShellLinkWVtbl>(obj);
            ((*v).set_icon_location)(obj, icon_path, 0)
        }
    }

    unsafe fn shelllink_get_icon_location(obj: *mut c_void, buf: &mut [u16]) -> HRESULT {
        unsafe {
            let v = vtbl::<IShellLinkWVtbl>(obj);
            let len = i32::try_from(buf.len()).unwrap_or(i32::MAX);
            let mut index = 0i32;
            ((*v).get_icon_location)(obj, buf.as_mut_ptr(), len, &mut index)
        }
    }

    unsafe fn persistfile_load(obj: *mut c_void, path: PCWSTR, mode: u32) -> HRESULT {
        unsafe {
            let v = vtbl::<IPersistFileVtbl>(obj);
            ((*v).load)(obj, path, mode)
        }
    }

    unsafe fn persistfile_save(obj: *mut c_void, path: PCWSTR, remember: bool) -> HRESULT {
        unsafe {
            let v = vtbl::<IPersistFileVtbl>(obj);
            ((*v).save)(obj, path, i32::from(remember))
        }
    }

    /// Reads a shortcut's AppUserModelID (`None` when absent). PIDL-only
    /// taskbar pins carry no filesystem target, but system/UWP pins do carry
    /// an AUMID — the only locale-independent way to identify e.g. the
    /// "File Explorer" pin.
    unsafe fn shortcut_aumid(shell_link: *mut c_void) -> Option<String> {
        unsafe {
            let store = unknown_query_interface(shell_link, &IID_IPROPERTYSTORE)?;
            let v = vtbl::<IPropertyStoreVtbl>(store);
            let mut pv = std::mem::zeroed::<PropVariant>();
            let hr = ((*v).get_value)(store, &PKEY_APPUSERMODEL_ID, &mut pv);
            let out = if hr >= 0 && pv.vt == VT_LPWSTR && pv.data[0] != 0 {
                let ptr = pv.data[0] as *const u16;
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )))
            } else {
                None
            };
            PropVariantClear(&mut pv);
            unknown_release(store);
            out
        }
    }

    // ---- helpers -----------------------------------------------------------------

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn wide_to_string(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    /// Canonical path of the running executable (resolves symlinks, strips
    /// the `\\?\` extended-length prefix for clean comparison).
    fn exe_path() -> Result<PathBuf, io::Error> {
        let path = std::env::current_exe()?;
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        let s = canonical.to_string_lossy();
        Ok(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s)))
    }

    /// Case-insensitive path comparison after best-effort canonicalization
    /// (resolves 8.3 short names / casing differences between the
    /// installer-written target and the live exe path).
    fn paths_match(a: &Path, b: &Path) -> bool {
        let ca = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
        let cb = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
        ca.to_string_lossy()
            .eq_ignore_ascii_case(&cb.to_string_lossy())
    }

    unsafe fn known_folder_dir(folder_id: &GUID) -> Option<PathBuf> {
        unsafe {
            let mut raw: PWSTR = std::ptr::null_mut();
            let hr = SHGetKnownFolderPath(
                folder_id as *const GUID,
                0,
                std::ptr::null_mut(),
                &mut raw as *mut PWSTR,
            );
            if hr < 0 || raw.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *raw.add(len) != 0 {
                len += 1;
            }
            let path = PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(
                raw, len,
            )));
            CoTaskMemFree(raw as *const c_void);
            Some(path)
        }
    }

    unsafe fn notify_shell_item(path: &Path) {
        unsafe {
            let wide = to_wide(&path.to_string_lossy());
            SHChangeNotify(
                SHCNE_UPDATEITEM,
                SHCNF_PATHW,
                wide.as_ptr() as *const c_void,
                std::ptr::null(),
            );
        }
    }

    /// Flush the shell icon cache so changed pins repaint immediately.
    unsafe fn notify_assoc_changed() {
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }

    /// What `update_one` did with a shortcut.
    enum Outcome {
        /// FluxDown-owned shortcut — `IconLocation` rewritten to the new icon.
        Updated,
        /// Foreign shortcut whose `IconLocation` a past FluxDown version
        /// hijacked (old fail-open guard) — icon reference cleared so the
        /// shell derives the icon from the target again.
        Repaired,
        /// Foreign shortcut, untouched.
        Skipped,
    }

    /// Per-sweep context shared across all candidate shortcuts.
    struct Context {
        /// Canonical path of this app's exe.
        exe: PathBuf,
        /// New icon to stamp onto FluxDown-owned shortcuts; `None` for a
        /// repair-only sweep (startup).
        icon_wide: Option<Vec<u16>>,
        /// `<data_dir>/icons` — where bolt/custom `.ico` files live.
        owned_icons_dir: Option<PathBuf>,
        /// `<exe_dir>/app_icon.ico` — the packaged default icon.
        default_ico: PathBuf,
    }

    impl Context {
        /// Whether `icon` is a FluxDown-owned icon file — i.e. something
        /// only this app would have written into a shortcut.
        ///
        /// Exact-path checks cover the running instance's own locations, but
        /// hijacked pins may reference a *different* FluxDown location than
        /// the one currently executing (e.g. the installed copy under
        /// `…\Programs\FluxDown\app_icon.ico` while a dev/portable build runs
        /// the sweep). Location-independent heuristics cover those:
        /// - `app_icon.ico` sitting next to a `flux_down.exe`, or in a
        ///   directory literally named `FluxDown` (uninstalled leftovers);
        /// - `bolt_icon.ico`/`custom_icon.ico` inside a `FluxDown\icons`
        ///   data directory.
        ///
        /// Both anchor on FluxDown-specific names, so another app's
        /// (Flutter-default) `app_icon.ico` never matches.
        fn is_owned_icon(&self, icon: &Path) -> bool {
            if paths_match(icon, &self.default_ico) {
                return true;
            }
            let parent = icon.parent();
            if let (Some(dir), Some(parent)) = (&self.owned_icons_dir, parent)
                && paths_match(parent, dir)
            {
                return true;
            }

            let file = icon
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or_default();
            let dir_named = |p: Option<&Path>, name: &str| {
                p.and_then(Path::file_name)
                    .and_then(|f| f.to_str())
                    .is_some_and(|f| f.eq_ignore_ascii_case(name))
            };
            if file.eq_ignore_ascii_case("app_icon.ico") {
                let beside_our_exe = parent.is_some_and(|p| p.join("flux_down.exe").is_file());
                return beside_our_exe || dir_named(parent, "FluxDown");
            }
            if file.eq_ignore_ascii_case("bolt_icon.ico")
                || file.eq_ignore_ascii_case("custom_icon.ico")
            {
                return dir_named(parent, "icons")
                    && dir_named(parent.and_then(Path::parent), "FluxDown");
            }
            false
        }
    }

    /// Visits one `.lnk`: rewrites its `IconLocation` if its resolved target
    /// matches our exe, repairs it if it is foreign but still carries a
    /// FluxDown-owned icon, otherwise leaves it alone.
    unsafe fn update_one(lnk_path: &Path, ctx: &Context) -> Result<Outcome, String> {
        unsafe {
            let lnk_wide = to_wide(&lnk_path.to_string_lossy());

            let mut shell_link: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_SHELL_LINK as *const GUID,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ISHELLLINKW as *const GUID,
                &mut shell_link as *mut *mut c_void,
            );
            if hr < 0 || shell_link.is_null() {
                return Err(format!("CoCreateInstance(CLSID_ShellLink) failed: {hr:#x}"));
            }

            let persist_file = match unknown_query_interface(shell_link, &IID_IPERSISTFILE) {
                Some(p) => p,
                None => {
                    unknown_release(shell_link);
                    return Err("QueryInterface(IID_IPersistFile) failed".to_string());
                }
            };

            let result = (|| -> Result<Outcome, String> {
                let hr = persistfile_load(persist_file, lnk_wide.as_ptr(), STGM_READWRITE);
                if hr < 0 {
                    return Err(format!("IPersistFile::Load failed: {hr:#x}"));
                }

                // Resolve the shortcut's filesystem target. PIDL-only pins
                // (Explorer's own taskbar pin, UWP app pins) have none:
                // GetPath returns S_FALSE with an empty buffer. Anything not
                // positively matched to our exe is foreign — fail closed.
                let mut path_buf = [0u16; MAX_PATH];
                let hr = shelllink_get_path(shell_link, &mut path_buf);
                let target = if hr >= 0 {
                    wide_to_string(&path_buf)
                } else {
                    String::new()
                };
                let is_ours = !target.is_empty() && paths_match(Path::new(&target), &ctx.exe);

                if is_ours {
                    let Some(icon_wide) = ctx.icon_wide.as_deref() else {
                        return Ok(Outcome::Skipped);
                    };
                    let hr = shelllink_set_icon_location(shell_link, icon_wide.as_ptr());
                    if hr < 0 {
                        return Err(format!("SetIconLocation failed: {hr:#x}"));
                    }
                    let hr = persistfile_save(persist_file, lnk_wide.as_ptr(), true);
                    if hr < 0 {
                        return Err(format!("IPersistFile::Save failed: {hr:#x}"));
                    }
                    return Ok(Outcome::Updated);
                }

                // Foreign shortcut: undo damage from the old fail-open guard.
                // Only touch it when its IconLocation clearly points at a
                // FluxDown-owned icon file — with one extra case: the "File
                // Explorer" pin (identified by AUMID; its file name is
                // localized) ships with an explicit `%windir%\explorer.exe,0`
                // icon, so an *empty* IconLocation there is also our damage
                // (an earlier repair cleared instead of restoring, leaving a
                // generic folder glyph).
                let mut icon_buf = [0u16; MAX_PATH];
                if shelllink_get_icon_location(shell_link, &mut icon_buf) < 0 {
                    return Ok(Outcome::Skipped);
                }
                let icon = wide_to_string(&icon_buf);
                let hijacked = !icon.is_empty() && ctx.is_owned_icon(Path::new(&icon));
                let is_explorer_pin =
                    shortcut_aumid(shell_link).is_some_and(|a| a == EXPLORER_AUMID);
                if !hijacked && !(is_explorer_pin && icon.is_empty()) {
                    return Ok(Outcome::Skipped);
                }

                // Explorer pin gets its factory icon back; everything else
                // (UWP pins ship with an empty IconLocation) is cleared so
                // the shell derives the icon from the target again.
                let restored = if is_explorer_pin {
                    let windir = std::env::var("windir").unwrap_or_else(|_| r"C:\Windows".into());
                    to_wide(&format!(r"{windir}\explorer.exe"))
                } else {
                    vec![0u16]
                };
                let hr = shelllink_set_icon_location(shell_link, restored.as_ptr());
                if hr < 0 {
                    return Err(format!("SetIconLocation(restore) failed: {hr:#x}"));
                }
                let hr = persistfile_save(persist_file, lnk_wide.as_ptr(), true);
                if hr < 0 {
                    return Err(format!("IPersistFile::Save failed: {hr:#x}"));
                }
                Ok(Outcome::Repaired)
            })();

            unknown_release(persist_file);
            unknown_release(shell_link);
            result
        }
    }

    /// Candidate `.lnk` paths: the fixed Desktop/Start Menu shortcuts this
    /// app's own installer creates, plus every `.lnk` in the taskbar's
    /// pinned-items folder (name not controlled by us — Windows names it
    /// after whatever the user pinned from, so all entries are scanned and
    /// filtered by target inside `update_one`).
    unsafe fn candidate_shortcuts() -> Vec<PathBuf> {
        unsafe {
            let mut targets = Vec::new();
            if let Some(desktop) = known_folder_dir(&FOLDERID_Desktop) {
                targets.push(desktop.join("FluxDown.lnk"));
            }
            if let Some(programs) = known_folder_dir(&FOLDERID_Programs) {
                targets.push(programs.join("FluxDown").join("FluxDown.lnk"));
            }
            if let Some(quick_launch) = known_folder_dir(&FOLDERID_QuickLaunch) {
                let taskbar_dir = quick_launch.join("User Pinned").join("TaskBar");
                if let Ok(entries) = std::fs::read_dir(&taskbar_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let is_lnk = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("lnk"))
                            .unwrap_or(false);
                        if is_lnk {
                            targets.push(path);
                        }
                    }
                }
            }
            targets.retain(|p| p.is_file());
            targets
        }
    }

    /// Sweeps all candidate shortcuts. `icon_path = Some(..)` applies that
    /// icon to FluxDown-owned shortcuts; `None` is a repair-only pass. Both
    /// modes clear FluxDown icons hijacked onto foreign shortcuts.
    fn update_all(icon_path: Option<&str>) {
        let exe = match exe_path() {
            Ok(p) => p,
            Err(e) => {
                log_info!("[shortcut_icon] cannot resolve exe path: {e}");
                return;
            }
        };
        let ctx = Context {
            icon_wide: icon_path.map(to_wide),
            owned_icons_dir: fluxdown_engine::data_dir::resolve_data_dir(None)
                .ok()
                .map(|d| d.join("icons")),
            default_ico: exe
                .parent()
                .map(|d| d.join("app_icon.ico"))
                .unwrap_or_default(),
            exe,
        };

        // SAFETY: `candidate_shortcuts`/`update_one`/`notify_shell_item` only
        // call well-formed, correctly-typed Win32/COM APIs with valid
        // pointers built just above; COM is initialized on this thread for
        // the duration of the block and torn down before returning.
        unsafe {
            let targets = candidate_shortcuts();
            if targets.is_empty() {
                return;
            }

            let hr = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED);
            if hr < 0 {
                log_info!("[shortcut_icon] CoInitializeEx failed: {hr:#x}");
                return;
            }

            let mut changed = false;
            for lnk in &targets {
                match update_one(lnk, &ctx) {
                    Ok(Outcome::Updated) => {
                        changed = true;
                        notify_shell_item(lnk);
                        log_info!("[shortcut_icon] updated {}", lnk.display());
                    }
                    Ok(Outcome::Repaired) => {
                        changed = true;
                        notify_shell_item(lnk);
                        log_info!(
                            "[shortcut_icon] repaired hijacked icon on {}",
                            lnk.display()
                        );
                    }
                    Ok(Outcome::Skipped) => {}
                    Err(e) => {
                        log_info!("[shortcut_icon] failed to update {}: {e}", lnk.display());
                    }
                }
            }
            if changed {
                notify_assoc_changed();
            }

            CoUninitialize();
        }
    }

    /// Listens for `UpdateShortcutIcons` signals from Dart (sent by
    /// `AppIconService._applyIcon`/`useDefault` alongside the runtime
    /// `WM_SETICON` call) and rewrites matching shortcuts on a blocking
    /// worker thread — COM calls and file IO must not run on the
    /// single-threaded async runtime (see `lib.rs`'s runtime-constraint
    /// note).
    ///
    /// Before entering the loop it runs one repair-only sweep: users hit by
    /// the old fail-open guard may since have switched back to the default
    /// icon, in which case no `UpdateShortcutIcons` signal ever fires again
    /// — the sweep is the only chance to un-hijack their Explorer/UWP pins.
    pub async fn listen() {
        let _ = tokio::task::spawn_blocking(|| update_all(None)).await;
        let recv = UpdateShortcutIcons::get_dart_signal_receiver();
        while let Some(pack) = recv.recv().await {
            let icon_path = pack.message.icon_path;
            let _ = tokio::task::spawn_blocking(move || update_all(Some(&icon_path))).await;
        }
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::*;

        /// Per-test unique temp dir (crate has no tempfile dev-dep; same
        /// pid-suffixed pattern as the engine's bt_downloader tests).
        fn test_dir(tag: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "fluxdown_shortcut_icon_{tag}_{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// COM apartment for the current test thread.
        struct ComGuard;
        impl ComGuard {
            fn init() -> Self {
                // S_FALSE (already initialized on this thread) is fine.
                let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED) };
                assert!(hr >= 0, "CoInitializeEx failed: {hr:#x}");
                ComGuard
            }
        }
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }

        unsafe fn shelllink_set_path(obj: *mut c_void, path: PCWSTR) -> HRESULT {
            unsafe {
                let v = vtbl::<IShellLinkWVtbl>(obj);
                ((*v).set_path)(obj, path)
            }
        }

        /// Creates a `.lnk` on disk. `target: None` produces a link with no
        /// filesystem path — the same shape as PIDL-only pins (Explorer's
        /// taskbar pin, UWP pins), whose `GetPath` yields an empty buffer.
        fn create_lnk(lnk: &Path, target: Option<&Path>, icon: Option<&Path>) {
            unsafe {
                let mut shell_link: *mut c_void = std::ptr::null_mut();
                let hr = CoCreateInstance(
                    &CLSID_SHELL_LINK as *const GUID,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_ISHELLLINKW as *const GUID,
                    &mut shell_link as *mut *mut c_void,
                );
                assert!(hr >= 0 && !shell_link.is_null());
                if let Some(target) = target {
                    let wide = to_wide(&target.to_string_lossy());
                    assert!(shelllink_set_path(shell_link, wide.as_ptr()) >= 0);
                }
                if let Some(icon) = icon {
                    let wide = to_wide(&icon.to_string_lossy());
                    assert!(shelllink_set_icon_location(shell_link, wide.as_ptr()) >= 0);
                }
                let persist_file = unknown_query_interface(shell_link, &IID_IPERSISTFILE).unwrap();
                let lnk_wide = to_wide(&lnk.to_string_lossy());
                assert!(persistfile_save(persist_file, lnk_wide.as_ptr(), true) >= 0);
                unknown_release(persist_file);
                unknown_release(shell_link);
            }
        }

        /// Creates a PIDL-pin-shaped `.lnk` (no filesystem target) carrying
        /// an AppUserModelID — the shape of the taskbar's system/UWP pins.
        fn create_pin(lnk: &Path, icon: Option<&Path>, aumid: &str) {
            unsafe {
                let mut shell_link: *mut c_void = std::ptr::null_mut();
                let hr = CoCreateInstance(
                    &CLSID_SHELL_LINK as *const GUID,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_ISHELLLINKW as *const GUID,
                    &mut shell_link as *mut *mut c_void,
                );
                assert!(hr >= 0 && !shell_link.is_null());
                if let Some(icon) = icon {
                    let wide = to_wide(&icon.to_string_lossy());
                    assert!(shelllink_set_icon_location(shell_link, wide.as_ptr()) >= 0);
                }
                let store = unknown_query_interface(shell_link, &IID_IPROPERTYSTORE).unwrap();
                let v = vtbl::<IPropertyStoreVtbl>(store);
                let wide = to_wide(aumid);
                let pv = PropVariant {
                    vt: VT_LPWSTR,
                    w_reserved1: 0,
                    w_reserved2: 0,
                    w_reserved3: 0,
                    data: [wide.as_ptr() as usize, 0],
                };
                assert!(((*v).set_value)(store, &PKEY_APPUSERMODEL_ID, &pv) >= 0);
                assert!(((*v).commit)(store) >= 0);
                unknown_release(store);
                let persist_file = unknown_query_interface(shell_link, &IID_IPERSISTFILE).unwrap();
                let lnk_wide = to_wide(&lnk.to_string_lossy());
                assert!(persistfile_save(persist_file, lnk_wide.as_ptr(), true) >= 0);
                unknown_release(persist_file);
                unknown_release(shell_link);
            }
        }

        /// 期望恢复出厂 Explorer 图标路径：`<windir>\explorer.exe`。
        fn explorer_icon() -> String {
            let windir = std::env::var("windir").unwrap_or_else(|_| r"C:\Windows".into());
            format!(r"{windir}\explorer.exe")
        }

        /// Reads back a saved `.lnk`'s `IconLocation` path.
        fn read_icon(lnk: &Path) -> String {
            unsafe {
                let mut shell_link: *mut c_void = std::ptr::null_mut();
                let hr = CoCreateInstance(
                    &CLSID_SHELL_LINK as *const GUID,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_ISHELLLINKW as *const GUID,
                    &mut shell_link as *mut *mut c_void,
                );
                assert!(hr >= 0 && !shell_link.is_null());
                let persist_file = unknown_query_interface(shell_link, &IID_IPERSISTFILE).unwrap();
                let lnk_wide = to_wide(&lnk.to_string_lossy());
                assert!(persistfile_load(persist_file, lnk_wide.as_ptr(), STGM_READWRITE) >= 0);
                let mut buf = [0u16; MAX_PATH];
                assert!(shelllink_get_icon_location(shell_link, &mut buf) >= 0);
                unknown_release(persist_file);
                unknown_release(shell_link);
                wide_to_string(&buf)
            }
        }

        fn ctx(dir: &Path, icon: Option<&Path>) -> Context {
            Context {
                exe: exe_path().unwrap(),
                icon_wide: icon.map(|p| to_wide(&p.to_string_lossy())),
                owned_icons_dir: Some(dir.join("icons")),
                default_ico: dir.join("app_icon.ico"),
            }
        }

        /// 核心回归契约：无文件系统目标的快捷方式（资源管理器/UWP 任务栏
        /// pin 的形态）绝不能被盖上 FluxDown 图标——旧守卫在这里 fail-open,
        /// 把 Explorer pin 的图标改成了 FD icon。
        #[test]
        fn pidl_style_shortcut_is_never_stamped() {
            let _com = ComGuard::init();
            let dir = test_dir("pidl");
            let lnk = dir.join("no_target.lnk");
            create_lnk(&lnk, None, None);

            let new_icon = dir.join("icons").join("bolt_icon.ico");
            let outcome = unsafe { update_one(&lnk, &ctx(&dir, Some(&new_icon))) }.unwrap();

            assert!(matches!(outcome, Outcome::Skipped));
            assert_eq!(read_icon(&lnk), "");
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 自愈契约：外来快捷方式的 IconLocation 若指向 FluxDown 自有图标
        /// （旧 bug 留下的劫持），必须被清空恢复目标默认图标。
        #[test]
        fn hijacked_foreign_shortcut_is_repaired() {
            let _com = ComGuard::init();
            let dir = test_dir("repair");
            let lnk = dir.join("hijacked.lnk");
            let owned_icon = dir.join("icons").join("custom_icon.ico");
            create_lnk(&lnk, None, Some(&owned_icon));

            let outcome = unsafe { update_one(&lnk, &ctx(&dir, None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Repaired));
            assert_eq!(read_icon(&lnk), "");
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 外来快捷方式带无关图标：目标不匹配、图标非 FluxDown 所有,
        /// 必须原样保留（不许动别的应用的 pin）。
        #[test]
        fn foreign_shortcut_with_unrelated_icon_untouched() {
            let _com = ComGuard::init();
            let dir = test_dir("foreign");
            let lnk = dir.join("foreign.lnk");
            let target = Path::new(r"C:\Windows\System32\notepad.exe");
            let icon = Path::new(r"C:\Windows\System32\imageres.dll");
            create_lnk(&lnk, Some(target), Some(icon));

            let new_icon = dir.join("icons").join("bolt_icon.ico");
            let outcome = unsafe { update_one(&lnk, &ctx(&dir, Some(&new_icon))) }.unwrap();

            assert!(matches!(outcome, Outcome::Skipped));
            assert!(
                read_icon(&lnk).eq_ignore_ascii_case(&icon.to_string_lossy()),
                "unrelated icon must be preserved"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 正向契约：目标解析为本 exe 的快捷方式照常改图标。
        #[test]
        fn own_shortcut_gets_new_icon() {
            let _com = ComGuard::init();
            let dir = test_dir("own");
            let lnk = dir.join("fluxdown.lnk");
            create_lnk(&lnk, Some(&std::env::current_exe().unwrap()), None);

            let new_icon = dir.join("icons").join("bolt_icon.ico");
            let outcome = unsafe { update_one(&lnk, &ctx(&dir, Some(&new_icon))) }.unwrap();

            assert!(matches!(outcome, Outcome::Updated));
            assert!(
                read_icon(&lnk).eq_ignore_ascii_case(&new_icon.to_string_lossy()),
                "own shortcut must carry the new icon"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 跨位置自愈契约：劫持图标指向**另一处** FluxDown 安装
        /// （`…\FluxDown\app_icon.ico`，旁有 flux_down.exe），而当前进程
        /// 从别的目录运行（ctx 的精确路径都不命中）——仍必须被修复。
        /// 复刻实机案例：dev 构建启动时清理安装版留下的 Explorer pin 劫持。
        #[test]
        fn hijack_from_other_fluxdown_install_is_repaired() {
            let _com = ComGuard::init();
            let dir = test_dir("xinstall");
            let install = dir.join("Programs").join("FluxDown");
            std::fs::create_dir_all(&install).unwrap();
            std::fs::write(install.join("flux_down.exe"), b"").unwrap();
            let lnk = dir.join("explorer_pin.lnk");
            create_lnk(&lnk, None, Some(&install.join("app_icon.ico")));

            // ctx 的 default_ico / owned_icons_dir 指向无关目录。
            let unrelated = dir.join("elsewhere");
            let outcome = unsafe { update_one(&lnk, &ctx(&unrelated, None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Repaired));
            assert_eq!(read_icon(&lnk), "");
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 卸载残留场景：FluxDown 已卸载（app_icon.ico 与 flux_down.exe 都
        /// 不在了），仅凭父目录名 `FluxDown` 也要能修复。
        #[test]
        fn hijack_from_uninstalled_fluxdown_is_repaired() {
            let _com = ComGuard::init();
            let dir = test_dir("uninst");
            let lnk = dir.join("explorer_pin.lnk");
            let gone = dir.join("FluxDown").join("app_icon.ico");
            create_lnk(&lnk, None, Some(&gone));

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Repaired));
            assert_eq!(read_icon(&lnk), "");
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 反例：别的 Flutter 应用同名 `app_icon.ico`（目录非 FluxDown、
        /// 旁边没有 flux_down.exe）绝不能被误清。
        #[test]
        fn other_apps_app_icon_ico_is_not_repaired() {
            let _com = ComGuard::init();
            let dir = test_dir("otherapp");
            let other = dir.join("Programs").join("OtherApp");
            std::fs::create_dir_all(&other).unwrap();
            std::fs::write(other.join("other_app.exe"), b"").unwrap();
            let icon = other.join("app_icon.ico");
            let lnk = dir.join("other_pin.lnk");
            create_lnk(&lnk, None, Some(&icon));

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Skipped));
            assert!(
                read_icon(&lnk).eq_ignore_ascii_case(&icon.to_string_lossy()),
                "another app's app_icon.ico must be preserved"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 修复扫描（icon = None）不改写自家快捷方式的图标——启动自愈只
        /// 清劫持，不覆盖用户当前选择。
        #[test]
        fn repair_sweep_leaves_own_shortcut_alone() {
            let _com = ComGuard::init();
            let dir = test_dir("sweep");
            let lnk = dir.join("fluxdown.lnk");
            let existing = dir.join("icons").join("custom_icon.ico");
            create_lnk(
                &lnk,
                Some(&std::env::current_exe().unwrap()),
                Some(&existing),
            );

            let outcome = unsafe { update_one(&lnk, &ctx(&dir, None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Skipped));
            assert!(
                read_icon(&lnk).eq_ignore_ascii_case(&existing.to_string_lossy()),
                "repair-only sweep must not touch own shortcut icons"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        /// Explorer pin 契约：被劫持的「文件资源管理器」pin（AUMID 识别）
        /// 必须恢复出厂图标 `<windir>\explorer.exe`，而不是清空——清空会
        /// 让 PIDL 推导出通用黄色文件夹图标。
        #[test]
        fn hijacked_explorer_pin_restores_factory_icon() {
            let _com = ComGuard::init();
            let dir = test_dir("exppin");
            let install = dir.join("FluxDown");
            std::fs::create_dir_all(&install).unwrap();
            let lnk = dir.join("File Explorer.lnk");
            create_pin(&lnk, Some(&install.join("app_icon.ico")), EXPLORER_AUMID);

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Repaired));
            assert!(
                read_icon(&lnk).eq_ignore_ascii_case(&explorer_icon()),
                "explorer pin must get its factory icon back"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 二次自愈契约：Explorer pin 图标被（旧修复逻辑）清空后，再跑
        /// 一次扫描也要补回出厂图标。
        #[test]
        fn cleared_explorer_pin_is_restored() {
            let _com = ComGuard::init();
            let dir = test_dir("expclear");
            let lnk = dir.join("File Explorer.lnk");
            create_pin(&lnk, None, EXPLORER_AUMID);

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Repaired));
            assert!(
                read_icon(&lnk).eq_ignore_ascii_case(&explorer_icon()),
                "cleared explorer pin must be restored"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 幂等：已恢复出厂图标的 Explorer pin 再扫描时不动。
        #[test]
        fn restored_explorer_pin_is_left_alone() {
            let _com = ComGuard::init();
            let dir = test_dir("expidem");
            let lnk = dir.join("File Explorer.lnk");
            let icon = PathBuf::from(explorer_icon());
            create_pin(&lnk, Some(&icon), EXPLORER_AUMID);

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Skipped));
            std::fs::remove_dir_all(&dir).ok();
        }

        /// 反例：非 Explorer 的 PIDL pin（UWP 形态，AUMID 非 Explorer）
        /// 图标为空是出厂状态，不许动。
        #[test]
        fn uwp_pin_with_empty_icon_untouched() {
            let _com = ComGuard::init();
            let dir = test_dir("uwppin");
            let lnk = dir.join("Some App.lnk");
            create_pin(&lnk, None, "SomeVendor.SomeApp_abc123!App");

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Skipped));
            assert_eq!(read_icon(&lnk), "");
            std::fs::remove_dir_all(&dir).ok();
        }

        /// UWP 形态 pin 被劫持时清空恢复（其出厂 IconLocation 即空）。
        #[test]
        fn hijacked_uwp_pin_is_cleared() {
            let _com = ComGuard::init();
            let dir = test_dir("uwphij");
            let install = dir.join("FluxDown");
            std::fs::create_dir_all(&install).unwrap();
            let lnk = dir.join("Some App.lnk");
            create_pin(
                &lnk,
                Some(&install.join("app_icon.ico")),
                "SomeVendor.SomeApp_abc123!App",
            );

            let outcome = unsafe { update_one(&lnk, &ctx(&dir.join("elsewhere"), None)) }.unwrap();

            assert!(matches!(outcome, Outcome::Repaired));
            assert_eq!(read_icon(&lnk), "");
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod inner {
    /// No-op — Linux/macOS handle their shortcut-icon equivalents outside
    /// Rust (see module docs above).
    pub async fn listen() {}
}

pub use inner::listen;
