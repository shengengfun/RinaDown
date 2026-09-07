//! Windows self-heal for a spurious `RUNASADMIN` compatibility flag on the
//! running executable.
//!
//! The Program Compatibility Assistant (PCA) and Windows installer-detection
//! heuristics may flag an executable that lacks an explicit
//! `requestedExecutionLevel` manifest — or whose name resembles an installer —
//! as "Run as administrator". The flag is stored as a per-exe named value under
//! `HKCU\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers`,
//! where the value name is the full exe path and the data is a space-separated
//! list of compatibility layers, e.g. `~ RUNASADMIN HIGHDPIAWARE`.
//!
//! Once set, launching the exe via `CreateProcess` (e.g. an Inno Setup `[Run]`
//! entry, or a parent process spawning it) fails with error 740
//! (`ERROR_ELEVATION_REQUIRED`), because `CreateProcess` cannot elevate.
//!
//! The correct long-term fix is the embedded `asInvoker` manifest (ships in new
//! builds), but that does not retroactively clear a flag PCA already wrote on an
//! installed machine. This module clears it on every startup: idempotent, HKCU
//! only (no elevation), and it removes **only** the `RUNASADMIN` token while
//! preserving any other layers the user deliberately set.

/// Detect and clear a spurious `RUNASADMIN` compatibility flag on the current
/// executable. No-op on non-Windows and when no such flag is present.
#[cfg(target_os = "windows")]
pub fn clear_runasadmin_self() {
    inner::clear_runasadmin_self();
}

/// No-op on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
pub fn clear_runasadmin_self() {}

#[cfg(target_os = "windows")]
mod inner {
    use crate::logger::log_info;
    use std::io;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    const LAYERS_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";
    const RUNASADMIN: &str = "RUNASADMIN";

    /// Canonical path of the running executable, with any `\\?\` prefix
    /// stripped for clean comparison with registry value names.
    fn exe_path() -> Result<String, io::Error> {
        let path = std::env::current_exe()?;
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        let s = canonical.to_string_lossy().into_owned();
        Ok(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
    }

    /// Compare two Windows exe paths for equivalence: canonicalize when
    /// possible (resolving symlinks / `\\?\`), then compare case-insensitively
    /// to match Windows' case-insensitive file system. Falls back to a raw
    /// case-insensitive string compare when canonicalization fails (e.g. the
    /// registered path points at a since-moved exe). Mirrors
    /// `protocol_registry::paths_equivalent`.
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

    /// Remove the `RUNASADMIN` token from a space-separated layer string,
    /// preserving order and every other token (case-insensitive match).
    ///
    /// Returns `None` when the input did not contain `RUNASADMIN` (nothing to
    /// do). Otherwise returns the rewritten layer string, which may be empty or
    /// contain only the leading `~` marker if `RUNASADMIN` was the sole layer.
    fn strip_runasadmin(layers: &str) -> Option<String> {
        let had = layers
            .split_whitespace()
            .any(|t| t.eq_ignore_ascii_case(RUNASADMIN));
        if !had {
            return None;
        }
        let kept: Vec<&str> = layers
            .split_whitespace()
            .filter(|t| !t.eq_ignore_ascii_case(RUNASADMIN))
            .collect();
        Some(kept.join(" "))
    }

    /// Whether the remaining layer string carries no real compatibility layer —
    /// i.e. it is empty or only the leading `~` sentinel. Such a value should be
    /// deleted rather than rewritten to an inert stub.
    fn is_effectively_empty(layers: &str) -> bool {
        layers.split_whitespace().all(|t| t == "~")
    }

    pub fn clear_runasadmin_self() {
        let exe = match exe_path() {
            Ok(p) => p,
            Err(e) => {
                log_info!("[compat] cannot resolve current exe: {}", e);
                return;
            }
        };

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let layers = match hkcu.open_subkey_with_flags(LAYERS_KEY, KEY_READ | KEY_WRITE) {
            Ok(k) => k,
            // Key absent => no compat flag was ever set for any exe. Nothing to do.
            Err(_) => return,
        };

        // The value name Windows writes is the launch-time path verbatim, whose
        // casing/form is not guaranteed to equal our canonical path. Enumerate
        // every value name and match by path equivalence rather than an exact
        // `get_value`, so a case- or form-mismatch does not silently skip the fix.
        let matched: Option<(String, String)> = layers
            .enum_values()
            .filter_map(Result::ok)
            .find(|(name, _)| paths_equivalent(name, &exe))
            .and_then(|(name, _)| {
                // Read as REG_SZ; a non-string type means it is not a compat
                // layer entry we understand, so skip it.
                layers.get_value::<String, _>(&name).ok().map(|d| (name, d))
            });

        let Some((name, current)) = matched else {
            // No layer value for this exe => not flagged.
            return;
        };

        let Some(rewritten) = strip_runasadmin(&current) else {
            // Value exists but does not contain RUNASADMIN — leave it untouched.
            return;
        };

        if is_effectively_empty(&rewritten) {
            if let Err(e) = layers.delete_value(&name) {
                log_info!("[compat] failed to delete RUNASADMIN layer value: {}", e);
            } else {
                log_info!(
                    "[compat] cleared spurious RUNASADMIN flag (removed empty layer value) for {}",
                    name
                );
            }
        } else if let Err(e) = layers.set_value(&name, &rewritten) {
            log_info!("[compat] failed to rewrite compat layers: {}", e);
        } else {
            log_info!(
                "[compat] cleared spurious RUNASADMIN flag for {} (kept layers: {})",
                name,
                rewritten
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{is_effectively_empty, strip_runasadmin};

        #[test]
        fn strips_sole_runasadmin_leaving_marker_only() {
            assert_eq!(strip_runasadmin("~ RUNASADMIN").as_deref(), Some("~"));
        }

        #[test]
        fn preserves_other_layers() {
            assert_eq!(
                strip_runasadmin("~ RUNASADMIN HIGHDPIAWARE").as_deref(),
                Some("~ HIGHDPIAWARE")
            );
        }

        #[test]
        fn case_insensitive_match() {
            assert_eq!(
                strip_runasadmin("~ RunAsAdmin HIGHDPIAWARE").as_deref(),
                Some("~ HIGHDPIAWARE")
            );
        }

        #[test]
        fn returns_none_when_absent() {
            assert_eq!(strip_runasadmin("~ HIGHDPIAWARE"), None);
        }

        #[test]
        fn marker_only_is_empty() {
            assert!(is_effectively_empty("~"));
            assert!(is_effectively_empty(""));
        }

        #[test]
        fn real_layer_not_empty() {
            assert!(!is_effectively_empty("~ HIGHDPIAWARE"));
        }
    }
}
