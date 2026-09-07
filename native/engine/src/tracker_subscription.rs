//! BT tracker subscription — fetch community-maintained tracker lists.
//!
//! Users can subscribe to one or more tracker-list URLs (one per line in the
//! `bt_tracker_sub_urls` config).  The fetched lists are normalized, deduped
//! and cached in the `bt_tracker_sub_cache` config key; `SharedBtSession`
//! merges the cache with the user's own tracker list at session creation.
//!
//! Refresh triggers:
//! - On startup, when the cache is older than [`REFRESH_INTERVAL_SECS`].
//! - Manually, via the `UpdateTrackerSubscription` Dart signal.
//! - When the subscription URL list changes in Settings.

use std::collections::HashSet;
use std::time::Duration;

use crate::logger::{log_error, log_info};

/// Default community subscription sources.
///
/// Both serve curated "best" lists (~20-60 trackers, auto-updated, heavy
/// overlap — hence the dedup in [`merge_dedup`]):
/// - `trackerslist.com` — XIU2/TrackersListCollection official CDN, the most
///   popular list in the CN community and reachable from mainland China.
/// - `ngosang.github.io` — ngosang/trackerslist (52.9k stars, updated daily,
///   ranked by latency) via GitHub Pages, which is more reachable than
///   `raw.githubusercontent.com`.
const DEFAULT_SUBSCRIPTION_URLS: &[&str] = &[
    "https://trackerslist.com/best.txt",
    "https://ngosang.github.io/trackerslist/trackers_best.txt",
];

/// Re-fetch subscriptions when the cache is older than this (24 hours).
pub const REFRESH_INTERVAL_SECS: i64 = 24 * 3600;

/// Per-source response size cap — community lists are a few KB; anything
/// above this is not a tracker list.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// HTTP timeout for fetching a single subscription source.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Return the built-in subscription URL list as a newline-separated string.
/// Used to populate the default config value on first launch.
pub fn default_subscription_urls() -> String {
    DEFAULT_SUBSCRIPTION_URLS.join("\n")
}

/// Normalize a tracker URL into its canonical form used as the dedup key.
///
/// Normalization performed:
/// - scheme and host are lowercased (the `url` crate does not lowercase the
///   host of non-special schemes like `udp`, so we do it explicitly);
/// - default ports are stripped for http/https/ws/wss (`https://x:443/a` ==
///   `https://x/a`);
/// - trailing slashes are trimmed (`/announce/` == `/announce`);
/// - query strings are preserved (private tracker passkeys).
///
/// Returns `None` for empty lines, comments, unparsable URLs, or schemes
/// that are not valid for trackers (only `udp`/`http`/`https`/`ws`/`wss`).
fn normalize_tracker(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let parsed: url::Url = trimmed.parse().ok()?;
    if !matches!(parsed.scheme(), "udp" | "http" | "https" | "ws" | "wss") {
        return None;
    }
    // A tracker URL must have a host.
    let host = parsed.host_str()?.to_ascii_lowercase();

    // Rebuild the canonical form from components.  `Url::port()` already
    // returns `None` for the default port of special schemes (http/https/
    // ws/wss) and keeps explicit ports for non-special schemes (udp).
    let mut s = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        s.push(':');
        s.push_str(&port.to_string());
    }
    s.push_str(parsed.path().trim_end_matches('/'));
    if let Some(query) = parsed.query() {
        s.push('?');
        s.push_str(query);
    }
    Some(s)
}

/// Merge tracker URLs from multiple sources, preserving first-seen order and
/// dropping duplicates by their normalized form (see [`normalize_tracker`]).
/// Invalid lines (comments, non-tracker schemes, garbage) are skipped.
pub fn merge_dedup<'a, I>(sources: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in sources {
        if let Some(norm) = normalize_tracker(raw)
            && seen.insert(norm.clone())
        {
            out.push(norm);
        }
    }
    out
}

/// Outcome of fetching all subscription sources.
#[derive(Debug)]
pub struct FetchOutcome {
    /// Deduped, normalized trackers from all sources that responded.
    pub trackers: Vec<String>,
    /// Number of sources fetched successfully.
    pub ok_sources: usize,
    /// Total number of subscription sources attempted.
    pub total_sources: usize,
    /// Non-empty only when **all** sources failed (joined error summary).
    pub error: String,
}

impl FetchOutcome {
    /// True when at least one source was fetched successfully.
    pub fn is_success(&self) -> bool {
        self.ok_sources > 0
    }
}

/// Fetch every subscription URL (newline-separated list, `#` comments
/// allowed) and return the merged + deduped tracker list.
///
/// Individual source failures are tolerated: the outcome is a success as
/// long as at least one source responds.  All network errors are logged.
pub async fn fetch_subscriptions(urls: &str) -> FetchOutcome {
    let sources: Vec<&str> = urls
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let total_sources = sources.len();

    if total_sources == 0 {
        return FetchOutcome {
            trackers: Vec::new(),
            ok_sources: 0,
            total_sources: 0,
            error: "no subscription URLs configured".to_string(),
        };
    }

    let client = match reqwest::Client::builder().timeout(FETCH_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            return FetchOutcome {
                trackers: Vec::new(),
                ok_sources: 0,
                total_sources,
                error: format!("failed to build http client: {e}"),
            };
        }
    };

    let mut raw_lines: Vec<String> = Vec::new();
    let mut ok_sources = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for src in &sources {
        match fetch_one(&client, src).await {
            Ok(lines) => {
                log_info!("[tracker-sub] fetched {} lines from {}", lines.len(), src);
                ok_sources += 1;
                raw_lines.extend(lines);
            }
            Err(e) => {
                log_error!("[tracker-sub] fetch failed: {}: {}", src, e);
                errors.push(format!("{src}: {e}"));
            }
        }
    }

    let trackers = merge_dedup(raw_lines.iter().map(String::as_str));
    log_info!(
        "[tracker-sub] refresh done: {}/{} sources ok, {} unique trackers",
        ok_sources,
        total_sources,
        trackers.len()
    );

    FetchOutcome {
        trackers,
        ok_sources,
        total_sources,
        error: if ok_sources == 0 {
            errors.join("; ")
        } else {
            String::new()
        },
    }
}

/// Fetch a single subscription source and return its non-empty lines.
async fn fetch_one(client: &reqwest::Client, url: &str) -> Result<Vec<String>, String> {
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("http status {status}"));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(format!("response too large ({} bytes)", body.len()));
    }
    Ok(body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{default_subscription_urls, merge_dedup, normalize_tracker};

    #[test]
    fn normalize_keeps_udp_port_and_path() {
        assert_eq!(
            normalize_tracker("udp://tracker.opentrackr.org:1337/announce"),
            Some("udp://tracker.opentrackr.org:1337/announce".to_string())
        );
    }

    #[test]
    fn normalize_strips_default_https_port() {
        assert_eq!(
            normalize_tracker("https://tracker.moeblog.cn:443/announce"),
            Some("https://tracker.moeblog.cn/announce".to_string())
        );
    }

    #[test]
    fn normalize_trims_trailing_slash_and_lowercases_host() {
        assert_eq!(
            normalize_tracker("  UDP://Tracker.Example.COM:6969/announce/  "),
            Some("udp://tracker.example.com:6969/announce".to_string())
        );
    }

    #[test]
    fn normalize_rejects_garbage_comments_and_bad_schemes() {
        assert_eq!(normalize_tracker(""), None);
        assert_eq!(normalize_tracker("   "), None);
        assert_eq!(normalize_tracker("# a comment"), None);
        assert_eq!(normalize_tracker("not a url"), None);
        assert_eq!(
            normalize_tracker("ftp://tracker.example.com/announce"),
            None
        );
        assert_eq!(normalize_tracker("magnet:?xt=urn:btih:abc"), None);
    }

    #[test]
    fn normalize_preserves_query_string() {
        assert_eq!(
            normalize_tracker("https://private.example.com/announce?passkey=AbC123"),
            Some("https://private.example.com/announce?passkey=AbC123".to_string())
        );
    }

    #[test]
    fn merge_dedup_removes_equivalent_urls_keeping_order() {
        let merged = merge_dedup([
            "udp://tracker.opentrackr.org:1337/announce",
            "https://tracker.moeblog.cn:443/announce",
            // duplicates in various spellings
            "UDP://TRACKER.OPENTRACKR.ORG:1337/announce",
            "udp://tracker.opentrackr.org:1337/announce/",
            "https://tracker.moeblog.cn/announce",
            // a new one
            "udp://open.stealth.si:80/announce",
            // junk
            "# comment",
            "",
        ]);
        assert_eq!(
            merged,
            vec![
                "udp://tracker.opentrackr.org:1337/announce".to_string(),
                "https://tracker.moeblog.cn/announce".to_string(),
                "udp://open.stealth.si:80/announce".to_string(),
            ]
        );
    }

    #[test]
    fn default_urls_are_valid_https() {
        for line in default_subscription_urls().lines() {
            let parsed: url::Url = match line.parse() {
                Ok(u) => u,
                Err(e) => panic!("invalid default subscription url {line}: {e}"),
            };
            assert_eq!(parsed.scheme(), "https");
        }
    }
}
