//! Chrome Native Messaging Host (NMH) manifest generation and registry registration.
//!
//! Registers `com.fluxdown.nmh` for Chrome, Edge, and Firefox so that the
//! browser extension can use `chrome.runtime.connectNative("com.fluxdown.nmh")`
//! to communicate with the FluxDown desktop app via the NMH relay binary.
//!
//! Registry keys (all HKCU — no admin required):
//!   Chrome:  `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.fluxdown.nmh`
//!   Edge:    `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.fluxdown.nmh`
//!   Firefox: `HKCU\Software\Mozilla\NativeMessagingHosts\com.fluxdown.nmh`
//!
//! Each key's default value points to a JSON manifest file that describes the NMH.
//!
//! These keys and the two manifest JSON files (written next to the NMH exe)
//! sit outside the Windows installer's [Registry]/[Files] tracking, so
//! `installer/windows/setup.iss` removes them explicitly on uninstall
//! (`CurUninstallStepChanged` + `[UninstallDelete]`) — keep both in sync.

/// 单个浏览器的 NMH 注册状态。
#[derive(Debug, Clone)]
pub struct NmhTarget {
    /// 展示名，如 `"Chrome"` / `"Firefox"` / `"Brave (Flatpak)"`。
    pub label: String,
    /// 注册位置：Windows = `HKCU\Software\...\com.fluxdown.nmh`；类 Unix = 清单文件绝对路径。
    pub location: String,
    /// 该浏览器是否安装（配置根目录存在）。false 时 Doctor 只报 `info`，不算故障。
    pub installed: bool,
    /// 已注册且指向当前中继 = true。
    pub ok: bool,
    /// `ok == false` 时的具体原因（英文技术描述，不翻译）；ok 时为空串。
    /// 例：`"registry key missing"` / `"manifest file missing: <path>"` /
    /// `"manifest points to <old exe>"` / `"missing Edge origin"`。
    pub issue: String,
}

/// NMH 注册整体诊断快照。
#[derive(Debug, Clone)]
pub struct NmhDiagnosis {
    /// NMH 中继可执行文件绝对路径；空 = 未找到。
    pub exe_path: String,
    /// 未找到中继时的原因原文；找到时为空串。
    pub exe_error: String,
    /// Chromium 清单文件绝对路径（**期望**路径，可能尚未写出）；无法推导时为空串。
    pub chromium_manifest: String,
    /// Firefox 清单文件绝对路径（同上；Linux 与 chromium 同名不同目录时给第一个候选）。
    pub firefox_manifest: String,
    /// 每个浏览器一条。未找到中继时可为空 vec。
    pub targets: Vec<NmhTarget>,
}

impl NmhDiagnosis {
    /// All-empty snapshot; each platform's `diagnose()` fills it in.
    fn empty() -> Self {
        Self {
            exe_path: String::new(),
            exe_error: String::new(),
            chromium_manifest: String::new(),
            firefox_manifest: String::new(),
            targets: Vec::new(),
        }
    }
}

#[cfg(target_os = "windows")]
mod inner {
    use crate::logger::log_info;
    use serde::Serialize;
    use std::io;
    use std::path::{Path, PathBuf};
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    const NMH_NAME: &str = "com.fluxdown.nmh";
    const NMH_DESCRIPTION: &str = "FluxDown Native Messaging Host";
    const NMH_EXE_NAME: &str = "fluxdown_nmh.exe";

    /// Manifest filename for Chrome/Edge (contains `allowed_origins`).
    const MANIFEST_FILENAME_CHROMIUM: &str = "com.fluxdown.nmh.json";
    /// Manifest filename for Firefox (contains `allowed_extensions`, NO `allowed_origins`).
    /// Firefox schema validation (NativeManifests.sys.mjs via Schemas.normalize) rejects any
    /// field not in its native_manifest.json schema. `allowed_origins` is Chrome-only and
    /// causes Firefox to report "No such native application" (Bugzilla #1361459).
    const MANIFEST_FILENAME_FIREFOX: &str = "com.fluxdown.nmh.firefox.json";

    /// Chrome extension ID — pinned via `key` in wxt.config.ts manifest.
    const CHROME_EXTENSION_ID: &str = "chrome-extension://meleenglfggcmcajknpeeeiobnpfmahc/";

    /// Edge Add-ons store extension ID. Edge ignores the manifest `key` field, so
    /// its store build gets a different ID than Chrome and must be listed
    /// explicitly (Chromium native messaging `allowed_origins` has no wildcard).
    /// Without this, Edge store users get "Access to the specified native
    /// messaging host is forbidden" → extension stuck on "未连接".
    const EDGE_EXTENSION_ID: &str = "chrome-extension://nglkkjbogjghekbhhcnccnpfedjbdhhd/";

    /// Firefox extension ID (matches `browser_specific_settings.gecko.id` in manifest).
    const FIREFOX_EXTENSION_ID: &str = "fluxdown@fluxdown.app";

    /// Chromium (Chrome/Edge) NMH manifest — uses `allowed_origins`.
    #[derive(Serialize)]
    struct NmhManifestChromium {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_origins: Vec<String>,
    }

    /// Firefox NMH manifest — uses `allowed_extensions` ONLY.
    /// Firefox schema (native_manifest.json) does not define `allowed_origins`;
    /// including it causes schema validation to fail with "No such native application".
    #[derive(Serialize)]
    struct NmhManifestFirefox {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_extensions: Vec<String>,
    }

    /// Strip `\\?\` UNC prefix from a path string (if present).
    fn strip_unc_prefix(s: &str) -> String {
        s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
    }

    /// Find the NMH executable, searching multiple locations.
    ///
    /// Search order:
    /// 1. Same directory as the current app exe (production deployment)
    /// 2. Cargo workspace `target/debug/` (development — `flutter run`)
    /// 3. Cargo workspace `target/release/` (development — release build)
    fn find_nmh_exe() -> Result<PathBuf, io::Error> {
        // 1. Next to current exe (production: NMH ships alongside the app)
        if let Ok(exe) = std::env::current_exe() {
            let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
            if let Some(dir) = canonical.parent() {
                let candidate = dir.join(NMH_EXE_NAME);
                if candidate.exists() {
                    log_info!(
                        "[nmh_registry] found NMH exe next to app: {}",
                        candidate.display()
                    );
                    return Ok(candidate);
                }
            }
        }

        // 2+3. Cargo workspace target directory (development)
        // CARGO_MANIFEST_DIR is baked in at compile time for the hub crate.
        // hub crate is at <workspace>/native/hub, so workspace root is 2 levels up.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = Path::new(manifest_dir).parent().and_then(|p| p.parent());

        if let Some(ws) = workspace_root {
            for profile in &["debug", "release"] {
                let candidate = ws.join("target").join(profile).join(NMH_EXE_NAME);
                if candidate.exists() {
                    log_info!(
                        "[nmh_registry] found NMH exe in cargo target: {}",
                        candidate.display()
                    );
                    return Ok(candidate);
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} not found. Build it with: cargo build -p fluxdown_nmh",
                NMH_EXE_NAME
            ),
        ))
    }

    /// Write two NMH manifest JSON files next to the NMH executable:
    /// - Chromium manifest (Chrome/Edge): contains `allowed_origins`
    /// - Firefox manifest: contains `allowed_extensions` ONLY (no `allowed_origins`)
    ///
    /// Returns `(chromium_manifest_path, firefox_manifest_path)`.
    fn write_manifests(nmh_exe: &Path) -> Result<(PathBuf, PathBuf), io::Error> {
        let nmh_path_str = strip_unc_prefix(&nmh_exe.to_string_lossy());
        let dir = nmh_exe
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no parent dir"))?;

        // Chromium manifest (Chrome + Edge)
        let chromium = NmhManifestChromium {
            name: NMH_NAME.to_string(),
            description: NMH_DESCRIPTION.to_string(),
            path: nmh_path_str.clone(),
            host_type: "stdio".to_string(),
            allowed_origins: vec![
                CHROME_EXTENSION_ID.to_string(),
                EDGE_EXTENSION_ID.to_string(),
            ],
        };
        let chromium_json = serde_json::to_string_pretty(&chromium)
            .map_err(|e| io::Error::other(format!("JSON serialize error: {}", e)))?;
        let chromium_path = dir.join(MANIFEST_FILENAME_CHROMIUM);
        std::fs::write(&chromium_path, chromium_json)?;

        // Firefox manifest — NO `allowed_origins` field (Bugzilla #1361459)
        let firefox = NmhManifestFirefox {
            name: NMH_NAME.to_string(),
            description: NMH_DESCRIPTION.to_string(),
            path: nmh_path_str,
            host_type: "stdio".to_string(),
            allowed_extensions: vec![FIREFOX_EXTENSION_ID.to_string()],
        };
        let firefox_json = serde_json::to_string_pretty(&firefox)
            .map_err(|e| io::Error::other(format!("JSON serialize error: {}", e)))?;
        let firefox_path = dir.join(MANIFEST_FILENAME_FIREFOX);
        std::fs::write(&firefox_path, firefox_json)?;

        Ok((chromium_path, firefox_path))
    }

    /// Chromium-family registry paths on Windows.
    ///
    /// Brave, Vivaldi, Opera and most other Chromium forks fall back to reading
    /// Chrome's `Software\Google\Chrome\NativeMessagingHosts` registry key when
    /// their own key is absent (verified via KeePassXC source and Chromium
    /// source).  Only Chrome and Edge need dedicated keys.
    const CHROMIUM_REG_PATHS: &[&str] = &[
        r"Software\Google\Chrome\NativeMessagingHosts",
        r"Software\Microsoft\Edge\NativeMessagingHosts",
    ];

    /// Register each browser's registry key pointing to its dedicated manifest.
    /// Chrome and Edge use the Chromium manifest; Firefox uses the Firefox-only manifest.
    /// Other Chromium browsers (Brave, Vivaldi, Opera) fall back to Chrome's key.
    fn register_registry(chromium_manifest: &str, firefox_manifest: &str) -> Result<(), io::Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        for reg_path in CHROMIUM_REG_PATHS {
            let full_path = format!("{}\\{}", reg_path, NMH_NAME);
            let (key, _) = hkcu.create_subkey_with_flags(&full_path, KEY_WRITE)?;
            key.set_value("", &chromium_manifest)?;
            log_info!("[nmh_registry] registered at HKCU\\{}", full_path);
        }

        let firefox_reg = format!("{}\\{}", r"Software\Mozilla\NativeMessagingHosts", NMH_NAME);
        let (key, _) = hkcu.create_subkey_with_flags(&firefox_reg, KEY_WRITE)?;
        key.set_value("", &firefox_manifest)?;
        log_info!("[nmh_registry] registered at HKCU\\{}", firefox_reg);

        Ok(())
    }

    /// Returns `true` if NMH registration is missing or stale and needs to be (re)written.
    ///
    /// Checks that:
    ///   1. Chrome/Edge registry keys exist and point to the Chromium manifest.
    ///   2. Each registered manifest file exists and references the current NMH exe.
    ///   3. The registered NMH's parent directory matches the current exe's directory
    ///      (detects version switches: dev → portable / installed).
    ///   4. Firefox is treated as optional — its absence does not trigger re-registration.
    ///
    /// If the NMH exe cannot be found, returns `true` so that `register()` can
    /// report the proper "exe not found" error.
    pub fn needs_update() -> bool {
        let Ok(nmh_exe) = find_nmh_exe() else {
            return true;
        };
        // 清单由 serde_json 写出，路径中的 `\` 被转义为 `\\`；
        // 用转义后的形式做内容匹配，否则 Windows 上永远不匹配、每次启动都重注册。
        let expected_exe_json = strip_unc_prefix(&nmh_exe.to_string_lossy()).replace('\\', "\\\\");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // --- 版本切换检测 ---
        // 读取已注册 Chrome 清单中的 NMH path，与当前 exe 目录对比。
        // 目录不同说明用户切换了版本（dev → portable / installed），强制重新注册。
        // canonicalize 会加 `\\?\` UNC 前缀，而清单里的 path 写入时已去前缀；
        // 比较前同样去掉，否则永远判定"目录变了"、每次启动都重注册。
        let current_exe_dir = std::env::current_exe()
            .ok()
            .map(|exe| std::fs::canonicalize(&exe).unwrap_or(exe))
            .and_then(|p| {
                p.parent()
                    .map(|d| PathBuf::from(strip_unc_prefix(&d.to_string_lossy())))
            });

        if let Some(exe_dir) = &current_exe_dir {
            let chrome_reg = format!(
                "{}\\{}",
                r"Software\Google\Chrome\NativeMessagingHosts", NMH_NAME
            );
            if let Ok(key) = hkcu.open_subkey_with_flags(&chrome_reg, KEY_READ)
                && let Ok(manifest_str) = key.get_value::<String, _>("")
                && let Ok(content) = std::fs::read_to_string(&manifest_str)
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(registered_str) = json["path"].as_str()
            {
                let registered_dir = Path::new(registered_str).parent();
                if registered_dir
                    .map(|d| d != exe_dir.as_path())
                    .unwrap_or(true)
                {
                    log_info!(
                        "[nmh_registry] exe dir changed: registered NMH dir={:?}, current exe dir={:?} → needs update",
                        registered_dir,
                        exe_dir
                    );
                    return true;
                }
            }
        }
        // ---------------------

        // Check Chrome and Edge point to the Chromium manifest with the correct path.
        // Other Chromium browsers (Brave, Vivaldi, Opera) fall back to Chrome's key.
        for reg_path in CHROMIUM_REG_PATHS {
            let full_path = format!("{}\\{}", reg_path, NMH_NAME);
            let Ok(key) = hkcu.open_subkey_with_flags(&full_path, KEY_READ) else {
                return true;
            };
            let Ok(manifest_str): Result<String, _> = key.get_value("") else {
                return true;
            };
            if !manifest_str.ends_with(MANIFEST_FILENAME_CHROMIUM) {
                return true; // pointing to wrong manifest
            }
            if !Path::new(&manifest_str).exists() {
                return true;
            }
            let Ok(content) = std::fs::read_to_string(&manifest_str) else {
                return true;
            };
            if !content.contains(&expected_exe_json) {
                return true;
            }
            // Content versioning: an existing manifest predating Edge support
            // lacks the Edge origin. Force a rewrite so upgraded users get it
            // (path-only checks above would otherwise return false and skip register()).
            if !content.contains(EDGE_EXTENSION_ID) {
                return true;
            }
        }

        // Firefox 键缺失也要重注册：能走到这里说明 Chromium 键完好（本机曾完整
        // 注册过），此时 Firefox 键被外部删除（杀毒/清理工具）应当自愈；
        // register() 无条件写 Firefox 键，对未安装 Firefox 的机器同样无害幂等。
        let firefox_reg = format!("{}\\{}", r"Software\Mozilla\NativeMessagingHosts", NMH_NAME);
        match hkcu.open_subkey_with_flags(&firefox_reg, KEY_READ) {
            Err(_) => return true,
            Ok(key) => {
                let Ok(manifest_str): Result<String, _> = key.get_value("") else {
                    return true;
                };
                if !manifest_str.ends_with(MANIFEST_FILENAME_FIREFOX) {
                    return true; // still pointing to old shared manifest
                }
                if !Path::new(&manifest_str).exists() {
                    return true;
                }
                let Ok(content) = std::fs::read_to_string(&manifest_str) else {
                    return true;
                };
                if !content.contains(&expected_exe_json) {
                    return true;
                }
            }
        }

        false
    }

    /// `true` if `%VAR%\<rest…>` exists and is a directory.
    /// Used as the "browser installed" proxy on Windows (user-data root).
    fn env_dir_exists(var: &str, rest: &[&str]) -> bool {
        let Ok(base) = std::env::var(var) else {
            return false;
        };
        let mut path = PathBuf::from(base);
        for segment in rest {
            path.push(segment);
        }
        path.is_dir()
    }

    /// Read-only registration check for one browser key.
    /// Returns the failure reason, or an empty string when everything matches.
    /// Mirrors `needs_update()` step by step — do not diverge.
    fn diagnose_registry(
        hkcu: &RegKey,
        reg_path: &str,
        manifest_filename: &str,
        expected_exe_json: &str,
        require_edge_origin: bool,
    ) -> String {
        let full_path = format!("{}\\{}", reg_path, NMH_NAME);
        let Ok(key) = hkcu.open_subkey_with_flags(&full_path, KEY_READ) else {
            return format!("registry key missing: HKCU\\{}", full_path);
        };
        let Ok(manifest_str): Result<String, _> = key.get_value("") else {
            return format!("registry default value unreadable: HKCU\\{}", full_path);
        };
        if !manifest_str.ends_with(manifest_filename) {
            return format!("registry points to unexpected manifest: {}", manifest_str);
        }
        if !Path::new(&manifest_str).exists() {
            return format!("manifest file missing: {}", manifest_str);
        }
        let content = match std::fs::read_to_string(&manifest_str) {
            Ok(c) => c,
            Err(e) => return format!("manifest unreadable: {}: {e:#}", manifest_str),
        };
        if !content.contains(expected_exe_json) {
            return format!("manifest does not point to current relay: {}", manifest_str);
        }
        if require_edge_origin && !content.contains(EDGE_EXTENSION_ID) {
            return format!("missing Edge origin in manifest: {}", manifest_str);
        }
        String::new()
    }

    /// Read-only snapshot of the NMH registration state for the Doctor page.
    ///
    /// Never writes the registry, manifests or directories — `needs_update()`
    /// judgement rules are reused verbatim so Doctor and startup self-heal
    /// never disagree.
    pub fn diagnose() -> super::NmhDiagnosis {
        let mut diag = super::NmhDiagnosis::empty();

        let nmh_exe = match find_nmh_exe() {
            Ok(p) => p,
            Err(e) => {
                diag.exe_error = format!("{e:#}");
                return diag;
            }
        };
        diag.exe_path = strip_unc_prefix(&nmh_exe.to_string_lossy());
        if let Some(dir) = nmh_exe.parent() {
            diag.chromium_manifest =
                strip_unc_prefix(&dir.join(MANIFEST_FILENAME_CHROMIUM).to_string_lossy());
            diag.firefox_manifest =
                strip_unc_prefix(&dir.join(MANIFEST_FILENAME_FIREFOX).to_string_lossy());
        }

        // 与 needs_update() 同口径：清单由 serde_json 写出，路径里的 `\` 被转义为 `\\`。
        let expected_exe_json = diag.exe_path.replace('\\', "\\\\");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // CHROMIUM_REG_PATHS 顺序 = [Chrome, Edge]，与安装判定目录一一对应。
        let chromium_installed = [
            env_dir_exists("LOCALAPPDATA", &["Google", "Chrome", "User Data"]),
            env_dir_exists("LOCALAPPDATA", &["Microsoft", "Edge", "User Data"]),
        ];
        for ((reg_path, label), installed) in CHROMIUM_REG_PATHS
            .iter()
            .zip(["Chrome", "Edge"])
            .zip(chromium_installed)
        {
            let issue = diagnose_registry(
                &hkcu,
                reg_path,
                MANIFEST_FILENAME_CHROMIUM,
                &expected_exe_json,
                true,
            );
            diag.targets.push(super::NmhTarget {
                label: label.to_string(),
                location: format!("HKCU\\{}\\{}", reg_path, NMH_NAME),
                installed,
                ok: issue.is_empty(),
                issue,
            });
        }

        let firefox_reg = r"Software\Mozilla\NativeMessagingHosts";
        let issue = diagnose_registry(
            &hkcu,
            firefox_reg,
            MANIFEST_FILENAME_FIREFOX,
            &expected_exe_json,
            false,
        );
        diag.targets.push(super::NmhTarget {
            label: "Firefox".to_string(),
            location: format!("HKCU\\{}\\{}", firefox_reg, NMH_NAME),
            installed: env_dir_exists("APPDATA", &["Mozilla", "Firefox"]),
            ok: issue.is_empty(),
            issue,
        });

        diag
    }

    /// Register the NMH for all supported browsers.
    ///
    /// Writes two separate manifest files:
    /// - Chromium manifest (Chrome/Edge): contains `allowed_origins`
    /// - Firefox manifest: contains `allowed_extensions` ONLY
    ///
    /// This is idempotent — safe to call on every startup.
    pub fn register() -> Result<(), io::Error> {
        let nmh_exe = find_nmh_exe()?;
        let (chromium_path, firefox_path) = write_manifests(&nmh_exe)?;
        let chromium_str = strip_unc_prefix(&chromium_path.to_string_lossy());
        let firefox_str = strip_unc_prefix(&firefox_path.to_string_lossy());
        let nmh_str = strip_unc_prefix(&nmh_exe.to_string_lossy());
        register_registry(&chromium_str, &firefox_str)?;
        log_info!(
            "[nmh_registry] NMH registered: exe={}, chromium_manifest={}, firefox_manifest={}",
            nmh_str,
            chromium_str,
            firefox_str,
        );
        Ok(())
    }

    /// Remove NMH registration for all browsers and delete manifest files.
    #[allow(dead_code)]
    pub fn unregister() -> Result<(), io::Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Remove all Chromium-family browser registry keys
        for reg_path in CHROMIUM_REG_PATHS {
            match hkcu.open_subkey_with_flags(reg_path, KEY_WRITE) {
                Ok(parent) => {
                    let _ = parent.delete_subkey(NMH_NAME);
                }
                Err(_) => continue,
            }
        }
        // Remove Firefox registry key
        if let Ok(parent) =
            hkcu.open_subkey_with_flags(r"Software\Mozilla\NativeMessagingHosts", KEY_WRITE)
        {
            let _ = parent.delete_subkey(NMH_NAME);
        }

        // Remove both manifest files if NMH exe is found.
        if let Ok(nmh_exe) = find_nmh_exe()
            && let Some(dir) = nmh_exe.parent()
        {
            let _ = std::fs::remove_file(dir.join(MANIFEST_FILENAME_CHROMIUM));
            let _ = std::fs::remove_file(dir.join(MANIFEST_FILENAME_FIREFOX));
        }

        log_info!("[nmh_registry] NMH registration removed");
        Ok(())
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::{NMH_NAME, needs_update, register};
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

        /// 本机注册表冒烟：Firefox 键被外部删除后 `needs_update()` 必须自愈判定。
        ///
        /// 依赖真实 HKCU 注册表与已构建的 `fluxdown_nmh.exe`（`cargo build -p fluxdown_nmh`），
        /// 会改写本机 NMH 注册（指向 workspace target 目录，安装版启动时会自行纠正），
        /// 故标记 ignore，手动执行：
        /// `cargo test -p hub -- --ignored firefox_key_self_heal`
        #[test]
        #[ignore]
        fn firefox_key_self_heal() {
            // 基线：全量注册后一切匹配。
            register().expect("register");
            assert!(!needs_update(), "fresh register must be up to date");

            // 模拟外部删除 Firefox 键（杀毒/清理工具场景）。
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let parent = hkcu
                .open_subkey_with_flags(r"Software\Mozilla\NativeMessagingHosts", KEY_WRITE)
                .expect("open Mozilla NMH parent");
            parent.delete_subkey(NMH_NAME).expect("delete firefox key");

            // 修复点：缺失必须触发重注册（旧代码此处返回 false）。
            assert!(
                needs_update(),
                "missing Firefox key must trigger re-registration"
            );

            // register() 自愈恢复。
            register().expect("re-register");
            assert!(
                !needs_update(),
                "self-healed registration must be up to date"
            );
        }
    }
}

// Linux: write NMH manifest files to XDG browser directories.
#[cfg(target_os = "linux")]
mod inner {
    use crate::logger::log_info;
    use serde::Serialize;
    use std::io;
    use std::path::{Path, PathBuf};

    const NMH_NAME: &str = "com.fluxdown.nmh";
    const NMH_DESCRIPTION: &str = "FluxDown Native Messaging Host";
    const NMH_EXE_NAME: &str = "fluxdown_nmh";
    /// Shell wrapper script registered in the NMH manifest.
    /// Provides a stable path even for AppImage builds where the real binary
    /// lives at a random FUSE mount point that changes on every launch.
    const NMH_WRAPPER_NAME: &str = "fluxdown_nmh.sh";
    const MANIFEST_FILENAME_CHROMIUM: &str = "com.fluxdown.nmh.json";
    const MANIFEST_FILENAME_FIREFOX: &str = "com.fluxdown.nmh.json";
    const CHROME_EXTENSION_ID: &str = "chrome-extension://meleenglfggcmcajknpeeeiobnpfmahc/";
    /// Edge Add-ons store extension ID — differs from Chrome (Edge ignores the
    /// manifest `key`) and must be whitelisted explicitly, else Edge store users
    /// get "forbidden" on connectNative → stuck on "未连接".
    const EDGE_EXTENSION_ID: &str = "chrome-extension://nglkkjbogjghekbhhcnccnpfedjbdhhd/";
    const FIREFOX_EXTENSION_ID: &str = "fluxdown@fluxdown.app";

    #[derive(Serialize)]
    struct NmhManifestChromium {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_origins: Vec<String>,
    }

    #[derive(Serialize)]
    struct NmhManifestFirefox {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_extensions: Vec<String>,
    }

    fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }

    /// All Chromium-family NMH manifest directories on Linux.
    ///
    /// Covers standard deb/rpm/tar.gz installs as well as Flatpak and Snap
    /// variants, which use isolated profile directories under ~/.var/app/ and
    /// ~/snap/ respectively.
    fn chromium_nmh_dirs() -> Vec<PathBuf> {
        let Some(home) = home_dir() else {
            return vec![];
        };
        let config = home.join(".config");
        let var_app = home.join(".var").join("app");
        let snap = home.join("snap");
        vec![
            // ── Standard deb/rpm/tar.gz installs ──
            config.join("google-chrome").join("NativeMessagingHosts"),
            config.join("chromium").join("NativeMessagingHosts"),
            config.join("microsoft-edge").join("NativeMessagingHosts"),
            // Brave Browser (verified via KeePassXC source)
            config
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("NativeMessagingHosts"),
            // Vivaldi (verified via KeePassXC source)
            config.join("vivaldi").join("NativeMessagingHosts"),
            // ── Flatpak variants ──
            // Flatpak Chrome
            var_app
                .join("com.google.Chrome")
                .join("config")
                .join("google-chrome")
                .join("NativeMessagingHosts"),
            // Flatpak Chromium
            var_app
                .join("org.chromium.Chromium")
                .join("config")
                .join("chromium")
                .join("NativeMessagingHosts"),
            // Flatpak Edge
            var_app
                .join("com.microsoft.Edge")
                .join("config")
                .join("microsoft-edge")
                .join("NativeMessagingHosts"),
            // Flatpak Brave
            var_app
                .join("com.brave.Browser")
                .join("config")
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("NativeMessagingHosts"),
            // ── Snap variants ──
            // Snap Chromium
            snap.join("chromium")
                .join("common")
                .join(".config")
                .join("chromium")
                .join("NativeMessagingHosts"),
        ]
    }

    /// All Firefox-family NMH manifest directories on Linux.
    ///
    /// Returns multiple paths: standard location, Flatpak sandboxed variants,
    /// and Firefox-fork browsers (LibreWolf, Waterfox).
    /// Registration writes to every dir whose browser profile root exists;
    /// needs_update requires each such dir's manifest to exist and match
    /// (self-heals external deletion and browsers installed later, #159).
    fn firefox_nmh_dirs() -> Vec<PathBuf> {
        let Some(home) = home_dir() else {
            return vec![];
        };
        let var_app = home.join(".var").join("app");
        vec![
            // Standard Firefox
            home.join(".mozilla").join("native-messaging-hosts"),
            // Flatpak Firefox
            var_app
                .join("org.mozilla.firefox")
                .join(".mozilla")
                .join("native-messaging-hosts"),
            // LibreWolf (privacy-focused Firefox fork, verified via official FAQ)
            home.join(".librewolf").join("native-messaging-hosts"),
            // Zen Browser (Firefox fork, uses its own ~/.zen profile root, #313)
            home.join(".zen").join("native-messaging-hosts"),
            // Flatpak LibreWolf
            var_app
                .join("io.gitlab.librewolf-community")
                .join(".librewolf")
                .join("native-messaging-hosts"),
        ]
    }

    fn find_nmh_exe() -> Result<PathBuf, io::Error> {
        // 1. Next to current exe (production deployment, including AppImage mount)
        if let Ok(exe) = std::env::current_exe() {
            let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
            if let Some(dir) = canonical.parent() {
                let candidate = dir.join(NMH_EXE_NAME);
                if candidate.exists() {
                    log_info!(
                        "[nmh_registry] found NMH exe next to app: {}",
                        candidate.display()
                    );
                    return Ok(candidate);
                }
            }
        }

        // 2. Cargo workspace target directory (development)
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = Path::new(manifest_dir).parent().and_then(|p| p.parent());

        if let Some(ws) = workspace_root {
            for profile in &["debug", "release"] {
                let candidate = ws.join("target").join(profile).join(NMH_EXE_NAME);
                if candidate.exists() {
                    log_info!(
                        "[nmh_registry] found NMH exe in cargo target: {}",
                        candidate.display()
                    );
                    return Ok(candidate);
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} not found. Build it with: cargo build -p fluxdown_nmh",
                NMH_EXE_NAME
            ),
        ))
    }

    /// Stable wrapper script path: ~/.local/share/fluxdown/fluxdown_nmh.sh
    ///
    /// NMH manifests always point to this wrapper rather than the real binary.
    /// This decouples the manifest from AppImage mount points (which change on
    /// every launch) and from Cargo target directories (which are dev-only).
    fn wrapper_path() -> Option<PathBuf> {
        home_dir().map(|h| {
            h.join(".local")
                .join("share")
                .join("fluxdown")
                .join(NMH_WRAPPER_NAME)
        })
    }

    /// Write the shell wrapper script that exec's the real NMH binary.
    ///
    /// By registering a wrapper script instead of the binary directly, we
    /// provide a stable path even when the binary lives in a temporary AppImage
    /// mount point.  On every app launch the wrapper is rewritten to point at
    /// the current binary path, so it stays correct after updates.
    fn write_wrapper_script(nmh_exe: &Path) -> Result<PathBuf, io::Error> {
        let Some(wp) = wrapper_path() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "cannot determine home directory for wrapper script",
            ));
        };
        if let Some(parent) = wp.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let exe_str = nmh_exe.to_string_lossy();
        let script = format!("#!/bin/sh\nexec '{}' \"$@\"\n", exe_str);
        std::fs::write(&wp, script)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wp, std::fs::Permissions::from_mode(0o755))?;
        Ok(wp)
    }

    fn write_chromium_manifest(wrapper: &Path, dir: &Path) -> Result<PathBuf, io::Error> {
        std::fs::create_dir_all(dir)?;
        let manifest = NmhManifestChromium {
            name: NMH_NAME.to_string(),
            description: NMH_DESCRIPTION.to_string(),
            path: wrapper.to_string_lossy().into_owned(),
            host_type: "stdio".to_string(),
            allowed_origins: vec![
                CHROME_EXTENSION_ID.to_string(),
                EDGE_EXTENSION_ID.to_string(),
            ],
        };
        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::other(format!("JSON error: {}", e)))?;
        let path = dir.join(MANIFEST_FILENAME_CHROMIUM);
        std::fs::write(&path, json)?;
        Ok(path)
    }

    fn write_firefox_manifest(wrapper: &Path, dir: &Path) -> Result<PathBuf, io::Error> {
        std::fs::create_dir_all(dir)?;
        let manifest = NmhManifestFirefox {
            name: NMH_NAME.to_string(),
            description: NMH_DESCRIPTION.to_string(),
            path: wrapper.to_string_lossy().into_owned(),
            host_type: "stdio".to_string(),
            allowed_extensions: vec![FIREFOX_EXTENSION_ID.to_string()],
        };
        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::other(format!("JSON error: {}", e)))?;
        let path = dir.join(MANIFEST_FILENAME_FIREFOX);
        std::fs::write(&path, json)?;
        Ok(path)
    }

    /// Proxy for "browser is installed": the NMH dir's parent is the browser's
    /// profile/config root (e.g. `~/.config/microsoft-edge`, `~/.mozilla`),
    /// which only exists once the browser has run at least once. Same heuristic
    /// as Bitwarden desktop. Scopes register()/needs_update() to browsers actually
    /// present instead of spraying manifests into never-used dirs (#159).
    fn browser_installed(nmh_dir: &Path) -> bool {
        nmh_dir.parent().is_some_and(|p| p.is_dir())
    }

    pub fn needs_update() -> bool {
        let Ok(nmh_exe) = find_nmh_exe() else {
            return true;
        };
        let expected_exe = nmh_exe.to_string_lossy().into_owned();

        // Check that the wrapper script exists and points at the current binary.
        let Some(wp) = wrapper_path() else {
            return true;
        };
        if !wp.exists() {
            return true;
        }
        let wrapper_ok = std::fs::read_to_string(&wp)
            .map(|c| c.contains(&expected_exe))
            .unwrap_or(false);
        if !wrapper_ok {
            log_info!("[nmh_registry] wrapper script outdated → needs update");
            return true;
        }

        let wrapper_str = wp.to_string_lossy().into_owned();

        // Per-installed-browser check (#159): every Chromium browser whose
        // profile root exists must have a manifest pointing at the wrapper AND
        // containing the Edge origin (content versioning: rewrite manifests
        // predating Edge support). A single missing/stale manifest — e.g. a
        // browser installed after FluxDown first registered — must trigger
        // re-register; the old `.any()` let one healthy browser mask the rest.
        let chromium_ok = chromium_nmh_dirs()
            .iter()
            .filter(|dir| browser_installed(dir))
            .all(|dir| {
                std::fs::read_to_string(dir.join(MANIFEST_FILENAME_CHROMIUM))
                    .map(|c| c.contains(&wrapper_str) && c.contains(EDGE_EXTENSION_ID))
                    .unwrap_or(false)
            });

        // Firefox 同规则：装了才要求清单有效（自愈外部删除 / 后装浏览器）。
        let firefox_ok = firefox_nmh_dirs()
            .iter()
            .filter(|dir| browser_installed(dir))
            .all(|dir| {
                std::fs::read_to_string(dir.join(MANIFEST_FILENAME_FIREFOX))
                    .map(|c| c.contains(&wrapper_str))
                    .unwrap_or(false)
            });

        !(chromium_ok && firefox_ok)
    }

    /// Human-readable browser name for an NMH manifest directory.
    ///
    /// The profile root (the NMH dir's parent) identifies the browser; Flatpak
    /// installs live under `~/.var/app/<app-id>/…` and Snap ones under
    /// `~/snap/<name>/…`, which the suffix keeps distinguishable.
    fn label_for_dir(dir: &Path) -> String {
        let flatpak = dir
            .components()
            .any(|c| c.as_os_str().to_str() == Some(".var"));
        let snap = !flatpak
            && dir
                .components()
                .any(|c| c.as_os_str().to_str() == Some("snap"));
        let root = dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let base = match root {
            "google-chrome" => "Chrome",
            "chromium" => "Chromium",
            "microsoft-edge" => "Edge",
            "Brave-Browser" => "Brave",
            "vivaldi" => "Vivaldi",
            ".mozilla" => "Firefox",
            ".librewolf" => "LibreWolf",
            ".zen" => "Zen Browser",
            "" => "Unknown browser",
            other => other,
        };
        if flatpak {
            format!("{} (Flatpak)", base)
        } else if snap {
            format!("{} (Snap)", base)
        } else {
            base.to_string()
        }
    }

    /// Read-only manifest check for one browser directory.
    /// `wrapper_issue` surfaces a broken wrapper script once the manifest
    /// itself checks out (manifests point at the wrapper, not the binary).
    fn diagnose_dir(
        dir: &Path,
        manifest_filename: &str,
        wrapper_str: &str,
        require_edge_origin: bool,
        wrapper_issue: Option<&str>,
    ) -> super::NmhTarget {
        let location = dir.join(manifest_filename).to_string_lossy().into_owned();
        let issue = match std::fs::read_to_string(dir.join(manifest_filename)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                format!("manifest file missing: {}", location)
            }
            Err(e) => format!("manifest unreadable: {}: {e:#}", location),
            Ok(content) => {
                if !content.contains(wrapper_str) {
                    format!("manifest does not point to current relay: {}", location)
                } else if require_edge_origin && !content.contains(EDGE_EXTENSION_ID) {
                    format!("missing Edge origin in manifest: {}", location)
                } else {
                    wrapper_issue.unwrap_or_default().to_string()
                }
            }
        };
        super::NmhTarget {
            label: label_for_dir(dir),
            location,
            installed: browser_installed(dir),
            ok: issue.is_empty(),
            issue,
        }
    }

    /// Read-only snapshot of the NMH registration state for the Doctor page.
    ///
    /// Never writes manifests, the wrapper script or any directory — the
    /// judgement rules mirror `needs_update()` exactly.
    pub fn diagnose() -> super::NmhDiagnosis {
        let mut diag = super::NmhDiagnosis::empty();

        let chromium_dirs = chromium_nmh_dirs();
        let firefox_dirs = firefox_nmh_dirs();
        if let Some(first) = chromium_dirs.first() {
            diag.chromium_manifest = first
                .join(MANIFEST_FILENAME_CHROMIUM)
                .to_string_lossy()
                .into_owned();
        }
        if let Some(first) = firefox_dirs.first() {
            diag.firefox_manifest = first
                .join(MANIFEST_FILENAME_FIREFOX)
                .to_string_lossy()
                .into_owned();
        }

        let nmh_exe = match find_nmh_exe() {
            Ok(p) => p,
            Err(e) => {
                diag.exe_error = format!("{e:#}");
                return diag;
            }
        };
        diag.exe_path = nmh_exe.to_string_lossy().into_owned();

        // 无 HOME 时 wrapper 路径与上面两个目录列表同样为空，直接返回空快照。
        let Some(wp) = wrapper_path() else {
            return diag;
        };
        let wrapper_str = wp.to_string_lossy().into_owned();
        let wrapper_issue = if !wp.exists() {
            Some(format!("wrapper script missing: {}", wrapper_str))
        } else if std::fs::read_to_string(&wp)
            .map(|c| c.contains(&diag.exe_path))
            .unwrap_or(false)
        {
            None
        } else {
            Some(format!(
                "wrapper script does not point to current relay: {}",
                wrapper_str
            ))
        };

        for dir in &chromium_dirs {
            diag.targets.push(diagnose_dir(
                dir,
                MANIFEST_FILENAME_CHROMIUM,
                &wrapper_str,
                true,
                wrapper_issue.as_deref(),
            ));
        }
        for dir in &firefox_dirs {
            diag.targets.push(diagnose_dir(
                dir,
                MANIFEST_FILENAME_FIREFOX,
                &wrapper_str,
                false,
                wrapper_issue.as_deref(),
            ));
        }

        diag
    }

    pub fn register() -> Result<(), io::Error> {
        let nmh_exe = find_nmh_exe()?;

        // Write wrapper script first; manifests point to it.
        let wrapper = write_wrapper_script(&nmh_exe)?;
        log_info!("[nmh_registry] NMH wrapper script: {}", wrapper.display());

        for dir in chromium_nmh_dirs() {
            if !browser_installed(&dir) {
                // 未安装（profile 根不存在）的浏览器不写清单，
                // 避免凭空创建其 profile / Flatpak / Snap 目录（#159）。
                continue;
            }
            match write_chromium_manifest(&wrapper, &dir) {
                Ok(path) => {
                    log_info!("[nmh_registry] Chromium manifest: {}", path.display());
                }
                Err(e) => {
                    log_info!(
                        "[nmh_registry] Chromium manifest error ({}): {}",
                        dir.display(),
                        e
                    );
                }
            }
        }

        for dir in firefox_nmh_dirs() {
            if !browser_installed(&dir) {
                continue;
            }
            match write_firefox_manifest(&wrapper, &dir) {
                Ok(path) => {
                    log_info!("[nmh_registry] Firefox manifest: {}", path.display());
                }
                Err(e) => {
                    log_info!(
                        "[nmh_registry] Firefox manifest error ({}): {}",
                        dir.display(),
                        e
                    );
                }
            }
        }

        log_info!(
            "[nmh_registry] NMH registered: exe={}, wrapper={}",
            nmh_exe.display(),
            wrapper.display()
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub fn unregister() -> Result<(), io::Error> {
        for dir in chromium_nmh_dirs() {
            let _ = std::fs::remove_file(dir.join(MANIFEST_FILENAME_CHROMIUM));
        }
        for dir in firefox_nmh_dirs() {
            let _ = std::fs::remove_file(dir.join(MANIFEST_FILENAME_FIREFOX));
        }
        if let Some(wp) = wrapper_path() {
            let _ = std::fs::remove_file(wp);
        }
        log_info!("[nmh_registry] NMH registration removed");
        Ok(())
    }
}

// macOS: write NMH manifest files to ~/Library/Application Support browser directories.
#[cfg(target_os = "macos")]
mod inner {
    use crate::logger::log_info;
    use serde::Serialize;
    use std::io;
    use std::path::{Path, PathBuf};

    const NMH_NAME: &str = "com.fluxdown.nmh";
    const NMH_DESCRIPTION: &str = "FluxDown Native Messaging Host";
    const NMH_EXE_NAME: &str = "fluxdown_nmh";
    /// Shell wrapper script name registered in NMH manifest.
    /// Chrome/Firefox spawn this shell (a system-signed binary) which then
    /// exec's the actual fluxdown_nmh binary, bypassing macOS AMFI's
    /// requirement that processes spawned by Hardened-Runtime apps must
    /// carry a trusted Developer ID signature (adhoc-only binaries are
    /// rejected with "Unrecoverable CT signature issue").
    const NMH_WRAPPER_NAME: &str = "fluxdown_nmh.sh";
    const MANIFEST_FILENAME: &str = "com.fluxdown.nmh.json";
    const CHROME_EXTENSION_ID: &str = "chrome-extension://meleenglfggcmcajknpeeeiobnpfmahc/";
    /// Edge Add-ons store extension ID — differs from Chrome (Edge ignores the
    /// manifest `key`) and must be whitelisted explicitly, else Edge store users
    /// get "forbidden" on connectNative → stuck on "未连接".
    const EDGE_EXTENSION_ID: &str = "chrome-extension://nglkkjbogjghekbhhcnccnpfedjbdhhd/";
    const FIREFOX_EXTENSION_ID: &str = "fluxdown@fluxdown.app";

    #[derive(Serialize)]
    struct NmhManifestChromium {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_origins: Vec<String>,
    }

    #[derive(Serialize)]
    struct NmhManifestFirefox {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_extensions: Vec<String>,
    }

    /// Returns the current user's home directory.
    ///
    /// Prefers `$HOME` but falls back to the passwd database via `getpwuid_r`
    /// so that the correct path is returned even when the process is launched
    /// by a system service (launchd) that may not set `$HOME`.
    fn home_dir() -> Option<PathBuf> {
        if let Ok(h) = std::env::var("HOME") {
            if !h.is_empty() {
                return Some(PathBuf::from(h));
            }
        }
        use std::ffi::CStr;
        let uid = unsafe { libc::getuid() };
        let buf_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
        let buf_size = if buf_size > 0 {
            buf_size as usize
        } else {
            1024
        };
        let mut buf = vec![0i8; buf_size];
        let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let ret = unsafe {
            libc::getpwuid_r(
                uid,
                pwd.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf_size,
                &mut result,
            )
        };
        if ret == 0 && !result.is_null() {
            let pwd = unsafe { pwd.assume_init() };
            if !pwd.pw_dir.is_null() {
                let cstr = unsafe { CStr::from_ptr(pwd.pw_dir) };
                if let Ok(s) = cstr.to_str() {
                    if !s.is_empty() {
                        return Some(PathBuf::from(s));
                    }
                }
            }
        }
        None
    }

    /// macOS Chromium-family NMH manifest directories.
    /// Ref: https://developer.chrome.com/docs/apps/nativeMessaging/#native-messaging-host-location-macos
    fn chromium_nmh_dirs() -> Vec<PathBuf> {
        let Some(home) = home_dir() else {
            return vec![];
        };
        let lib = home.join("Library").join("Application Support");
        vec![
            // Google Chrome (stable / beta / canary)
            lib.join("Google")
                .join("Chrome")
                .join("NativeMessagingHosts"),
            lib.join("Google")
                .join("Chrome Beta")
                .join("NativeMessagingHosts"),
            lib.join("Google")
                .join("Chrome Canary")
                .join("NativeMessagingHosts"),
            // Open-source Chromium
            lib.join("Chromium").join("NativeMessagingHosts"),
            // Microsoft Edge (stable / beta)
            lib.join("Microsoft Edge").join("NativeMessagingHosts"),
            lib.join("Microsoft Edge Beta").join("NativeMessagingHosts"),
            // Arc
            lib.join("Arc")
                .join("User Data")
                .join("NativeMessagingHosts"),
            // Brave Browser (verified via KeePassXC source)
            lib.join("BraveSoftware")
                .join("Brave-Browser")
                .join("NativeMessagingHosts"),
            // Vivaldi (verified via KeePassXC source)
            lib.join("Vivaldi").join("NativeMessagingHosts"),
        ]
    }

    /// macOS Firefox NMH manifest directory.
    fn firefox_nmh_dir() -> Option<PathBuf> {
        home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("Mozilla")
                .join("NativeMessagingHosts")
        })
    }

    fn find_nmh_exe() -> Result<PathBuf, io::Error> {
        // 1. Next to current exe (production: inside .app bundle Contents/MacOS/)
        if let Ok(exe) = std::env::current_exe() {
            let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
            if let Some(dir) = canonical.parent() {
                let candidate = dir.join(NMH_EXE_NAME);
                if candidate.exists() {
                    log_info!(
                        "[nmh_registry] found NMH exe next to app: {}",
                        candidate.display()
                    );
                    return Ok(candidate);
                }
            }
        }

        // 2. Cargo workspace target directory (development)
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = Path::new(manifest_dir).parent().and_then(|p| p.parent());

        if let Some(ws) = workspace_root {
            for profile in &["debug", "release"] {
                let candidate = ws.join("target").join(profile).join(NMH_EXE_NAME);
                if candidate.exists() {
                    log_info!(
                        "[nmh_registry] found NMH exe in cargo target: {}",
                        candidate.display()
                    );
                    return Ok(candidate);
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} not found. Build it with: cargo build -p fluxdown_nmh",
                NMH_EXE_NAME
            ),
        ))
    }

    /// Write a shell wrapper script that exec's the real NMH binary.
    ///
    /// macOS AMFI rejects adhoc-signed (non-Developer-ID) binaries when they
    /// are spawned by Hardened Runtime processes such as Chrome or Firefox.
    /// `/bin/sh` is a system binary with an Apple-signed certificate and is
    /// always permitted. By registering the *shell script* as the NMH path,
    /// the browser spawns `/bin/sh`, which in turn exec's `fluxdown_nmh`.
    /// The shell inherits the NMH stdin/stdout pipe and transparently relays
    /// it to the binary — zero overhead, no extra process.
    fn write_wrapper_script(nmh_exe: &Path) -> Result<PathBuf, io::Error> {
        let Some(home) = home_dir() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "cannot determine home directory",
            ));
        };
        let dir = home
            .join("Library")
            .join("Application Support")
            .join("fluxdown");
        std::fs::create_dir_all(&dir)?;
        let script_path = dir.join(NMH_WRAPPER_NAME);
        let exe_str = nmh_exe.to_string_lossy();
        // Use `exec` so the shell process is replaced by the binary (no extra
        // zombie process). Pass "$@" to forward any arguments Chrome may add.
        let script = format!("#!/bin/sh\nexec '{}' \"$@\"\n", exe_str);
        std::fs::write(&script_path, script)?;
        // The script must be executable.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
        Ok(script_path)
    }

    fn write_chromium_manifest(wrapper: &Path, dir: &Path) -> Result<PathBuf, io::Error> {
        std::fs::create_dir_all(dir)?;
        let manifest = NmhManifestChromium {
            name: NMH_NAME.to_string(),
            description: NMH_DESCRIPTION.to_string(),
            path: wrapper.to_string_lossy().into_owned(),
            host_type: "stdio".to_string(),
            allowed_origins: vec![
                CHROME_EXTENSION_ID.to_string(),
                EDGE_EXTENSION_ID.to_string(),
            ],
        };
        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::other(format!("JSON error: {}", e)))?;
        let path = dir.join(MANIFEST_FILENAME);
        std::fs::write(&path, json)?;
        Ok(path)
    }

    fn write_firefox_manifest(wrapper: &Path, dir: &Path) -> Result<PathBuf, io::Error> {
        std::fs::create_dir_all(dir)?;
        let manifest = NmhManifestFirefox {
            name: NMH_NAME.to_string(),
            description: NMH_DESCRIPTION.to_string(),
            path: wrapper.to_string_lossy().into_owned(),
            host_type: "stdio".to_string(),
            allowed_extensions: vec![FIREFOX_EXTENSION_ID.to_string()],
        };
        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::other(format!("JSON error: {}", e)))?;
        let path = dir.join(MANIFEST_FILENAME);
        std::fs::write(&path, json)?;
        Ok(path)
    }

    /// Proxy for "browser is installed": the NMH dir's parent is the browser's
    /// profile/user-data root (e.g. `~/Library/Application Support/Microsoft Edge`),
    /// which only exists once the browser has run at least once. Same heuristic
    /// as Bitwarden desktop. Scopes register()/needs_update() to browsers actually
    /// present instead of spraying manifests into never-used dirs (#159).
    fn browser_installed(nmh_dir: &Path) -> bool {
        nmh_dir.parent().is_some_and(|p| p.is_dir())
    }

    /// Firefox reads NMH manifests from `…/Mozilla/NativeMessagingHosts`, but its
    /// actual profile root is `…/Application Support/Firefox` — the `Mozilla` dir
    /// is not guaranteed to exist on a machine with Firefox installed. Treat
    /// either directory as evidence of an install.
    fn firefox_installed() -> bool {
        home_dir().is_some_and(|h| {
            let lib = h.join("Library").join("Application Support");
            lib.join("Firefox").is_dir() || lib.join("Mozilla").is_dir()
        })
    }

    pub fn needs_update() -> bool {
        let Ok(nmh_exe) = find_nmh_exe() else {
            return true;
        };
        // The manifest now points to the shell wrapper, but the wrapper
        // contains the path to the real binary. Check that the wrapper exists
        // and that its content references the current NMH exe path.
        let expected_exe = nmh_exe.to_string_lossy().into_owned();

        // 版本切换检测：wrapper 内容里包含的 NMH exe 路径是否与当前一致。
        let wrapper_path = home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("fluxdown")
                .join(NMH_WRAPPER_NAME)
        });

        if let Some(ref wp) = wrapper_path {
            if !wp.exists() {
                return true;
            }
            let wrapper_ok = std::fs::read_to_string(wp)
                .map(|c| c.contains(&expected_exe))
                .unwrap_or(false);
            if !wrapper_ok {
                log_info!(
                    "[nmh_registry] wrapper script outdated or missing exe path → needs update"
                );
                return true;
            }
        } else {
            return true;
        }

        let wrapper_str = wrapper_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        // Per-installed-browser check (#159): every Chromium browser whose
        // profile root exists must have a manifest pointing at the wrapper AND
        // containing the Edge origin (content versioning: rewrite manifests
        // predating Edge support). A single missing/stale manifest — e.g. Edge
        // installed after FluxDown first registered — must trigger re-register;
        // the old `.any()` let one healthy browser mask all the others.
        let chromium_ok = chromium_nmh_dirs()
            .iter()
            .filter(|dir| browser_installed(dir))
            .all(|dir| {
                std::fs::read_to_string(dir.join(MANIFEST_FILENAME))
                    .map(|c| c.contains(&wrapper_str) && c.contains(EDGE_EXTENSION_ID))
                    .unwrap_or(false)
            });

        // Firefox 同规则：装了才要求清单有效（自愈外部删除 / 后装浏览器）。
        let firefox_ok = !firefox_installed()
            || firefox_nmh_dir()
                .map(|dir| {
                    std::fs::read_to_string(dir.join(MANIFEST_FILENAME))
                        .map(|c| c.contains(&wrapper_str))
                        .unwrap_or(false)
                })
                .unwrap_or(true);

        !(chromium_ok && firefox_ok)
    }

    /// Human-readable browser name for an NMH manifest directory.
    ///
    /// The profile root (the NMH dir's parent) identifies the browser, except
    /// for Arc which nests its manifests under `Arc/User Data/`.
    fn label_for_dir(dir: &Path) -> String {
        // 闭包返回借用自参数的 &str 会触发生命周期推断报错，用具名 fn。
        fn dir_name(p: Option<&Path>) -> &str {
            p.and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        }
        let parent = dir.parent();
        let mut root = dir_name(parent);
        if root == "User Data" {
            root = dir_name(parent.and_then(|p| p.parent()));
        }
        match root {
            "Chrome" => "Chrome",
            "Chrome Beta" => "Chrome Beta",
            "Chrome Canary" => "Chrome Canary",
            "Chromium" => "Chromium",
            "Microsoft Edge" => "Edge",
            "Microsoft Edge Beta" => "Edge Beta",
            "Arc" => "Arc",
            "Brave-Browser" => "Brave",
            "Vivaldi" => "Vivaldi",
            "Mozilla" => "Firefox",
            "" => "Unknown browser",
            other => other,
        }
        .to_string()
    }

    /// Read-only manifest check for one browser directory.
    /// `wrapper_issue` surfaces a broken wrapper script once the manifest
    /// itself checks out (manifests point at the wrapper, not the binary).
    fn diagnose_dir(
        dir: &Path,
        installed: bool,
        wrapper_str: &str,
        require_edge_origin: bool,
        wrapper_issue: Option<&str>,
    ) -> super::NmhTarget {
        let location = dir.join(MANIFEST_FILENAME).to_string_lossy().into_owned();
        let issue = match std::fs::read_to_string(dir.join(MANIFEST_FILENAME)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                format!("manifest file missing: {}", location)
            }
            Err(e) => format!("manifest unreadable: {}: {e:#}", location),
            Ok(content) => {
                if !content.contains(wrapper_str) {
                    format!("manifest does not point to current relay: {}", location)
                } else if require_edge_origin && !content.contains(EDGE_EXTENSION_ID) {
                    format!("missing Edge origin in manifest: {}", location)
                } else {
                    wrapper_issue.unwrap_or_default().to_string()
                }
            }
        };
        super::NmhTarget {
            label: label_for_dir(dir),
            location,
            installed,
            ok: issue.is_empty(),
            issue,
        }
    }

    /// Read-only snapshot of the NMH registration state for the Doctor page.
    ///
    /// Never writes manifests, the wrapper script or any directory — the
    /// judgement rules mirror `needs_update()` exactly.
    pub fn diagnose() -> super::NmhDiagnosis {
        let mut diag = super::NmhDiagnosis::empty();

        let chromium_dirs = chromium_nmh_dirs();
        let firefox_dir = firefox_nmh_dir();
        if let Some(first) = chromium_dirs.first() {
            diag.chromium_manifest = first.join(MANIFEST_FILENAME).to_string_lossy().into_owned();
        }
        if let Some(dir) = &firefox_dir {
            diag.firefox_manifest = dir.join(MANIFEST_FILENAME).to_string_lossy().into_owned();
        }

        let nmh_exe = match find_nmh_exe() {
            Ok(p) => p,
            Err(e) => {
                diag.exe_error = format!("{e:#}");
                return diag;
            }
        };
        diag.exe_path = nmh_exe.to_string_lossy().into_owned();

        // 无 home 时上面的目录列表同样为空，直接返回空快照。
        let Some(home) = home_dir() else {
            return diag;
        };
        let wp = home
            .join("Library")
            .join("Application Support")
            .join("fluxdown")
            .join(NMH_WRAPPER_NAME);
        let wrapper_str = wp.to_string_lossy().into_owned();
        let wrapper_issue = if !wp.exists() {
            Some(format!("wrapper script missing: {}", wrapper_str))
        } else if std::fs::read_to_string(&wp)
            .map(|c| c.contains(&diag.exe_path))
            .unwrap_or(false)
        {
            None
        } else {
            Some(format!(
                "wrapper script does not point to current relay: {}",
                wrapper_str
            ))
        };

        for dir in &chromium_dirs {
            diag.targets.push(diagnose_dir(
                dir,
                browser_installed(dir),
                &wrapper_str,
                true,
                wrapper_issue.as_deref(),
            ));
        }
        if let Some(dir) = &firefox_dir {
            diag.targets.push(diagnose_dir(
                dir,
                firefox_installed(),
                &wrapper_str,
                false,
                wrapper_issue.as_deref(),
            ));
        }

        diag
    }

    pub fn register() -> Result<(), io::Error> {
        let nmh_exe = find_nmh_exe()?;

        // Write the shell wrapper script first; manifests point to it.
        let wrapper = write_wrapper_script(&nmh_exe)?;
        log_info!("[nmh_registry] NMH wrapper script: {}", wrapper.display());

        for dir in chromium_nmh_dirs() {
            if !browser_installed(&dir) {
                // 未安装（profile 根不存在）的浏览器不写清单，
                // 避免凭空创建其 profile 目录（#159 修复建议）。
                continue;
            }
            match write_chromium_manifest(&wrapper, &dir) {
                Ok(path) => {
                    log_info!("[nmh_registry] Chromium manifest: {}", path.display());
                }
                Err(e) => {
                    log_info!(
                        "[nmh_registry] Chromium manifest error ({}): {}",
                        dir.display(),
                        e
                    );
                }
            }
        }

        if firefox_installed()
            && let Some(dir) = firefox_nmh_dir()
        {
            match write_firefox_manifest(&wrapper, &dir) {
                Ok(path) => {
                    log_info!("[nmh_registry] Firefox manifest: {}", path.display());
                }
                Err(e) => {
                    log_info!("[nmh_registry] Firefox manifest error: {}", e);
                }
            }
        }

        log_info!(
            "[nmh_registry] NMH registered: exe={}, wrapper={}",
            nmh_exe.display(),
            wrapper.display()
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub fn unregister() -> Result<(), io::Error> {
        for dir in chromium_nmh_dirs() {
            let _ = std::fs::remove_file(dir.join(MANIFEST_FILENAME));
        }
        if let Some(dir) = firefox_nmh_dir() {
            let _ = std::fs::remove_file(dir.join(MANIFEST_FILENAME));
        }
        // Remove wrapper script.
        if let Some(home) = home_dir() {
            let wrapper = home
                .join("Library")
                .join("Application Support")
                .join("fluxdown")
                .join(NMH_WRAPPER_NAME);
            let _ = std::fs::remove_file(wrapper);
        }
        log_info!("[nmh_registry] NMH registration removed");
        Ok(())
    }
}

// All other non-Windows, non-Linux, non-macOS platforms — no-op.
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod inner {
    use std::io;

    pub fn needs_update() -> bool {
        false
    }

    pub fn diagnose() -> super::NmhDiagnosis {
        let mut diag = super::NmhDiagnosis::empty();
        diag.exe_error = "unsupported platform".into();
        diag
    }

    pub fn register() -> Result<(), io::Error> {
        Ok(())
    }

    #[allow(dead_code)]
    pub fn unregister() -> Result<(), io::Error> {
        Ok(())
    }
}

#[allow(unused_imports)]
pub use inner::{diagnose, needs_update, register, unregister};
