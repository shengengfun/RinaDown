//! Environment probes behind the settings page **Doctor**.
//!
//! Answers the three questions every "browser extension does not work" report
//! needs answered, without asking the user to open regedit:
//!   1. Is the NMH relay installed, registered, and pointing at *this* build?
//!   2. Is this app actually listening on the pipe/socket the relay dials, and
//!      answering?
//!   3. Is the local HTTP takeover port (userscript path) reachable?
//!
//! Plus the cheap extras that show up in the same reports: URL scheme
//! registration, `.torrent` association, and a writable log directory.
//!
//! Everything here is a *probe*: nothing is repaired. The one repair action
//! (rewrite the NMH registration) is an explicit second signal — see
//! `RepairNmhRegistration` in `signals`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::logger::log_info;
use crate::signals::{DiagnosticCheck, DiagnosticsReport};

/// Wire levels. Mirrored by the Dart `doctorLevel*` i18n keys.
const OK: &str = "ok";
const WARN: &str = "warn";
const ERROR: &str = "error";
const INFO: &str = "info";

/// Wire hint codes. Mirrored by the Dart `doctorHint*` i18n keys.
const HINT_REINSTALL_APP: &str = "reinstall_app";
const HINT_REREGISTER_NMH: &str = "reregister_nmh";
const HINT_RESTART_APP: &str = "restart_app";
const HINT_ENABLE_LOCAL_SERVER: &str = "enable_local_server";
const HINT_CHECK_FIREWALL: &str = "check_firewall";
const HINT_ENABLE_PROTOCOL: &str = "enable_protocol";
const HINT_PROTOCOL_CLAIMED: &str = "protocol_claimed";
const HINT_CHECK_DISK: &str = "check_disk";

/// How long a probe may take before it is reported as a timeout. Both probes
/// run against loopback/IPC, so a slow answer *is* the finding.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Byte window read from the end of the NMH relay log. A coarse guard so a
/// runaway log never gets slurped whole; [`NMH_LOG_TAIL_LINES`] does the
/// actual trimming.
const NMH_LOG_TAIL_BYTES: u64 = 16 * 1024;

/// Lines of relay log kept in the report. The relay logs one line per
/// connect/disconnect, so the last few dozen cover every recent attempt —
/// beyond that it is just noise in the user's clipboard.
const NMH_LOG_TAIL_LINES: usize = 80;

fn check(id: &str, target: &str, level: &str, detail: String, hint: &str) -> DiagnosticCheck {
    DiagnosticCheck {
        id: id.to_string(),
        target: target.to_string(),
        level: level.to_string(),
        detail,
        hint: hint.to_string(),
    }
}

/// Everything resolvable without `await` — registry reads, file stats, the log
/// tail. Collected in one `spawn_blocking` hop so the async part stays to the
/// two network/IPC probes.
struct SyncProbe {
    /// `nmh_binary`, `nmh_manifest`, `nmh_browser`×N.
    nmh: Vec<DiagnosticCheck>,
    /// `url_protocol`×3, `torrent_association`.
    shell: Vec<DiagnosticCheck>,
    /// `log_dir`.
    log: Vec<DiagnosticCheck>,
    environment: Vec<String>,
    nmh_log_tail: String,
}

/// Run every probe and build the report sent to Dart.
///
/// `local_server_port`/`local_server_enabled` come from Dart rather than the DB
/// so this never needs the actor or an `Engine` handle — the whole Doctor runs
/// off-actor.
pub async fn run(local_server_port: i32, local_server_enabled: bool) -> DiagnosticsReport {
    let sync = match tokio::task::spawn_blocking(probe_sync).await {
        Ok(s) => s,
        Err(e) => {
            log_info!("[doctor] sync probe panicked: {e}");
            SyncProbe {
                nmh: vec![],
                shell: vec![],
                log: vec![],
                environment: vec![format!("probe error: {e}")],
                nmh_log_tail: String::new(),
            }
        }
    };

    let listener = probe_listener().await;
    let local_server = probe_local_server(local_server_port, local_server_enabled).await;

    let mut checks = sync.nmh;
    checks.push(listener);
    checks.push(local_server);
    checks.extend(sync.shell);
    checks.extend(sync.log);

    let issues = checks
        .iter()
        .filter(|c| c.level == ERROR || c.level == WARN)
        .count();
    log_info!(
        "[doctor] ran {} checks, {} need attention",
        checks.len(),
        issues
    );

    DiagnosticsReport {
        checks,
        environment: sync.environment,
        nmh_log_tail: sync.nmh_log_tail,
    }
}

fn probe_sync() -> SyncProbe {
    let nmh_diag = crate::nmh_registry::diagnose();

    let mut nmh = Vec::with_capacity(2 + nmh_diag.targets.len());
    if nmh_diag.exe_path.is_empty() {
        nmh.push(check(
            "nmh_binary",
            "",
            ERROR,
            nmh_diag.exe_error.clone(),
            HINT_REINSTALL_APP,
        ));
    } else {
        nmh.push(check("nmh_binary", "", OK, nmh_diag.exe_path.clone(), ""));
    }
    nmh.push(manifest_check(
        &nmh_diag.chromium_manifest,
        &nmh_diag.firefox_manifest,
    ));
    for target in &nmh_diag.targets {
        let (level, detail, hint) = if !target.installed {
            (
                INFO,
                format!("{} (browser not installed)", target.location),
                "",
            )
        } else if target.ok {
            (OK, target.location.clone(), "")
        } else {
            (
                ERROR,
                format!("{} — {}", target.location, target.issue),
                HINT_REREGISTER_NMH,
            )
        };
        nmh.push(check("nmh_browser", &target.label, level, detail, hint));
    }

    SyncProbe {
        nmh,
        shell: probe_shell_registration(),
        log: vec![probe_log_dir()],
        environment: environment_lines(&nmh_diag.exe_path),
        nmh_log_tail: read_nmh_log_tail(),
    }
}

/// Both manifest files must exist for Chromium *and* Firefox interception to
/// work; a missing one is the classic symptom of a half-finished install or an
/// over-eager cleanup tool.
fn manifest_check(chromium: &str, firefox: &str) -> DiagnosticCheck {
    let mut missing = Vec::new();
    for (label, path) in [("chromium", chromium), ("firefox", firefox)] {
        if path.is_empty() || !Path::new(path).exists() {
            missing.push(label);
        }
    }
    let detail = format!("chromium: {chromium}\nfirefox: {firefox}");
    if missing.is_empty() {
        check("nmh_manifest", "", OK, detail, "")
    } else {
        check(
            "nmh_manifest",
            "",
            ERROR,
            format!("{detail}\nmissing: {}", missing.join(", ")),
            HINT_REREGISTER_NMH,
        )
    }
}

/// `fluxdown://` deep links, `magnet:`/`ed2k://` handlers, `.torrent`
/// association.
///
/// Levels differ on purpose: `fluxdown://` is auto-registered and its absence
/// is a real fault, while `magnet`/`ed2k`/`.torrent` are opt-in — reporting
/// those as errors would train users to ignore the page.
fn probe_shell_registration() -> Vec<DiagnosticCheck> {
    let mut out = Vec::with_capacity(4);

    for proto in [
        crate::protocol_registry::FLUXDOWN,
        crate::protocol_registry::MAGNET,
        crate::protocol_registry::ED2K,
    ] {
        let registered = crate::protocol_registry::is_registered(proto);
        let own = proto.scheme == crate::protocol_registry::FLUXDOWN.scheme;
        #[cfg(target_os = "windows")]
        let claimed_by_other = !registered && crate::protocol_registry::is_claimed_by_other(proto);
        #[cfg(not(target_os = "windows"))]
        let claimed_by_other = false;

        let (level, detail, hint) = if registered {
            (OK, "registered for this build".to_string(), "")
        } else if claimed_by_other {
            (
                WARN,
                "claimed by another application".to_string(),
                HINT_PROTOCOL_CLAIMED,
            )
        } else if own {
            (WARN, "not registered".to_string(), HINT_RESTART_APP)
        } else {
            (INFO, "not registered".to_string(), HINT_ENABLE_PROTOCOL)
        };
        out.push(check("url_protocol", proto.scheme, level, detail, hint));
    }

    if crate::file_association::is_associated() {
        out.push(check(
            "torrent_association",
            "",
            OK,
            ".torrent opens with FluxDown".to_string(),
            "",
        ));
    } else {
        out.push(check(
            "torrent_association",
            "",
            INFO,
            ".torrent not associated".to_string(),
            HINT_ENABLE_PROTOCOL,
        ));
    }

    out
}

/// A full-disk or permission-denied log directory silently swallows the very
/// evidence a bug report needs, so the probe actually writes.
fn probe_log_dir() -> DiagnosticCheck {
    let dir = fluxdown_engine::logger::log_dir();
    let files = fluxdown_engine::logger::list_log_files();
    let health = fluxdown_engine::logger::health();
    let total: u64 = files.iter().map(|f| f.size).sum();
    let mut summary = format!(
        "{} ({} files, {:.1} MB; writer initialized={}, failures={})",
        dir.display(),
        files.len(),
        total as f64 / (1024.0 * 1024.0),
        health.initialized,
        health.failure_count,
    );
    if let Some(last_error) = health.last_error {
        summary.push_str(" — last writer error: ");
        summary.push_str(&last_error);
    }

    if !health.initialized {
        return check("log_dir", "", ERROR, summary, HINT_CHECK_DISK);
    }

    let probe_file = dir.join(".doctor_write_probe");
    match std::fs::write(&probe_file, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe_file);
            if health.degraded {
                check("log_dir", "", WARN, summary, HINT_CHECK_DISK)
            } else {
                check("log_dir", "", OK, summary, "")
            }
        }
        Err(e) => check(
            "log_dir",
            "",
            ERROR,
            format!("{summary} — not writable: {e}"),
            HINT_CHECK_DISK,
        ),
    }
}

fn environment_lines(nmh_exe: &str) -> Vec<String> {
    let log_dir = fluxdown_engine::logger::log_dir();
    let data_dir = log_dir
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("(unknown: {e})"));
    vec![
        format!("os: {} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!("exe: {exe}"),
        format!("dataDir: {data_dir}"),
        format!("logDir: {}", log_dir.display()),
        format!(
            "nmhRelay: {}",
            if nmh_exe.is_empty() {
                "(not found)"
            } else {
                nmh_exe
            }
        ),
        format!("listener: {}", crate::native_messaging::listener_endpoint()),
    ]
}

/// Path of the NMH relay's own diagnostic log.
///
/// **Mirror of `native/nmh/src/main.rs::log_path`** — the relay is a separate
/// process with its own log outside the app's log directory, and that log is
/// where "browser could not start the host" evidence lands. Keep both in sync.
/// The relay additionally falls back to `getpwuid` when `$HOME` is unset; this
/// side only reads `$HOME`, so a `$HOME`-less session degrades to "no log
/// file" rather than reading the wrong path.
fn nmh_log_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("TEMP")
            .or_else(|_| std::env::var("TMP"))
            .ok()
            .map(|tmp| Path::new(&tmp).join("fluxdown_nmh.log"))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
        let Some(home) = home else {
            return Some(Path::new("/tmp").join("fluxdown_nmh.log"));
        };
        let dir = if cfg!(target_os = "macos") {
            Path::new(&home)
                .join("Library")
                .join("Application Support")
                .join("fluxdown")
        } else {
            Path::new(&home)
                .join(".local")
                .join("share")
                .join("fluxdown")
        };
        Some(dir.join("fluxdown_nmh.log"))
    }
}

/// Last [`NMH_LOG_TAIL_BYTES`] of the relay log, whole lines only.
fn read_nmh_log_tail() -> String {
    use std::io::{Read, Seek, SeekFrom};

    let Some(path) = nmh_log_path() else {
        return String::new();
    };
    let Ok(mut file) = std::fs::File::open(&path) else {
        return String::new();
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return String::new();
    };
    let start = len.saturating_sub(NMH_LOG_TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buf);
    // A mid-line seek leaves a partial first line — drop it so the tail always
    // starts on a timestamp.
    let trimmed = if start > 0 {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        text.as_ref()
    };
    let trimmed = trimmed.trim_end();
    let lines: Vec<&str> = trimmed.lines().collect();
    let tail = if lines.len() > NMH_LOG_TAIL_LINES {
        lines[lines.len() - NMH_LOG_TAIL_LINES..].join("\n")
    } else {
        trimmed.to_string()
    };
    format!("{}\n{}", path.display(), tail)
}

async fn probe_listener() -> DiagnosticCheck {
    let endpoint = crate::native_messaging::listener_endpoint();
    match crate::native_messaging::probe_listener(PROBE_TIMEOUT).await {
        Ok(()) => check("app_listener", "", OK, format!("{endpoint} → pong"), ""),
        Err(e) => check(
            "app_listener",
            "",
            ERROR,
            format!("{endpoint} — {e}"),
            HINT_RESTART_APP,
        ),
    }
}

async fn probe_local_server(port: i32, enabled: bool) -> DiagnosticCheck {
    if !enabled {
        return check(
            "local_server",
            "",
            INFO,
            "disabled in settings".to_string(),
            HINT_ENABLE_LOCAL_SERVER,
        );
    }
    if !(1..=65535).contains(&port) {
        return check(
            "local_server",
            "",
            ERROR,
            format!("invalid port: {port}"),
            HINT_ENABLE_LOCAL_SERVER,
        );
    }

    let url = format!("http://127.0.0.1:{port}/ping");
    // `.no_proxy()`: a system proxy would otherwise swallow the loopback probe
    // and report a healthy server as dead (same reason the CLI does it).
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(PROBE_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return check(
                "local_server",
                "",
                ERROR,
                format!("{url} — client build failed: {e}"),
                "",
            );
        }
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => check(
            "local_server",
            "",
            OK,
            format!("{url} → {}", resp.status()),
            "",
        ),
        Ok(resp) => check(
            "local_server",
            "",
            ERROR,
            format!("{url} → {}", resp.status()),
            HINT_CHECK_FIREWALL,
        ),
        Err(e) => check(
            "local_server",
            "",
            ERROR,
            format!("{url} — {e}"),
            HINT_CHECK_FIREWALL,
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{ERROR, INFO, OK, WARN, run};

    /// Every wire value the Dart side switches on. A check that leaks an
    /// unmapped `id`/`level`/`hint` renders as a raw wire string in the UI, so
    /// the allow-lists are the actual contract, not documentation.
    const KNOWN_IDS: &[&str] = &[
        "nmh_binary",
        "nmh_manifest",
        "nmh_browser",
        "app_listener",
        "local_server",
        "url_protocol",
        "torrent_association",
        "log_dir",
    ];
    const KNOWN_HINTS: &[&str] = &[
        "",
        super::HINT_REINSTALL_APP,
        super::HINT_REREGISTER_NMH,
        super::HINT_RESTART_APP,
        super::HINT_ENABLE_LOCAL_SERVER,
        super::HINT_CHECK_FIREWALL,
        super::HINT_ENABLE_PROTOCOL,
        super::HINT_PROTOCOL_CLAIMED,
        super::HINT_CHECK_DISK,
    ];

    /// Runs the real probes against this machine — outcomes are environment
    /// dependent, so only the wire shape is asserted.
    #[tokio::test]
    async fn report_shape_is_within_the_wire_contract() {
        // port 0 + disabled: the HTTP probe must short-circuit to `info`
        // instead of dialing anything.
        let report = run(0, false).await;

        assert!(
            !report.environment.is_empty(),
            "environment lines feed the copied issue report"
        );

        for c in &report.checks {
            assert!(
                KNOWN_IDS.contains(&c.id.as_str()),
                "unmapped check id: {}",
                c.id
            );
            assert!(
                [OK, WARN, ERROR, INFO].contains(&c.level.as_str()),
                "unmapped level {} on {}",
                c.level,
                c.id
            );
            assert!(
                KNOWN_HINTS.contains(&c.hint.as_str()),
                "unmapped hint {} on {}",
                c.hint,
                c.id
            );
            assert!(!c.detail.is_empty(), "empty detail on {}", c.id);
        }

        for id in [
            "nmh_binary",
            "nmh_manifest",
            "app_listener",
            "local_server",
            "torrent_association",
            "log_dir",
        ] {
            assert_eq!(
                report.checks.iter().filter(|c| c.id == id).count(),
                1,
                "{id} must appear exactly once"
            );
        }

        let schemes: Vec<&str> = report
            .checks
            .iter()
            .filter(|c| c.id == "url_protocol")
            .map(|c| c.target.as_str())
            .collect();
        assert_eq!(schemes, ["fluxdown", "magnet", "ed2k"]);

        let local = report
            .checks
            .iter()
            .find(|c| c.id == "local_server")
            .expect("local_server check");
        assert_eq!(
            local.level, INFO,
            "a disabled local server is not a failure"
        );

        // `nmh_browser` carries a target so the UI can label repeated rows.
        for c in report.checks.iter().filter(|c| c.id == "nmh_browser") {
            assert!(!c.target.is_empty(), "nmh_browser row without a label");
        }
    }
}
