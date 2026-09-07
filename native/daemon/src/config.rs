//! daemon 进程环境配置与 loopback 绑定约束。

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use fluxdown_protocol::{
    DaemonConfigError, is_public_daemon_config_key, normalize_daemon_config_patch,
};

/// daemon 默认控制端口。
pub const DEFAULT_DAEMON_BIND: &str = "127.0.0.1:17801";

/// daemon 进程级配置。
#[derive(Clone, Debug)]
pub struct DaemonConfig {
    pub bind_addr: SocketAddr,
    pub data_dir_override: Option<PathBuf>,
    pub database_url: Option<String>,
    pub token_file_override: Option<PathBuf>,
}

/// daemon 配置错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("FLUXDOWN_DAEMON_BIND is invalid: {0}")]
    InvalidBind(String),
    #[error("FLUXDOWN_DAEMON_BIND must be loopback, got {0}")]
    NonLoopback(IpAddr),
    #[error("unknown or daemon-private config field: {0}")]
    UnknownField(String),
    #[error("config field is read-only: {0}")]
    ReadOnlyField(String),
    #[error("invalid config value for {field}: {message}")]
    InvalidValue { field: String, message: String },
}

impl DaemonConfig {
    /// 从环境读取配置并拒绝非 loopback 绑定。
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_text = std::env::var("FLUXDOWN_DAEMON_BIND")
            .unwrap_or_else(|_| DEFAULT_DAEMON_BIND.to_owned());
        let bind_addr = bind_text
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::InvalidBind(error.to_string()))?;
        if !bind_addr.ip().is_loopback() {
            return Err(ConfigError::NonLoopback(bind_addr.ip()));
        }
        Ok(Self {
            bind_addr,
            data_dir_override: std::env::var_os("FLUXDOWN_DATA_DIR").map(PathBuf::from),
            database_url: std::env::var("FLUXDOWN_DATABASE_URL")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            token_file_override: std::env::var_os("FLUXDOWN_DAEMON_TOKEN_FILE").map(PathBuf::from),
        })
    }
}

/// 校验并规范化客户端可写的 daemon 设置（键表与值域唯一来源：
/// [`fluxdown_protocol::DAEMON_CONFIG_FIELDS`]）。
///
/// 规范化后的布尔值再按引擎落库编码转写：`bt_seed_enabled` /
/// `bt_auto_reseed` 在引擎侧按 `"0"` 判定关闭（Flutter 也写 `'1'`/`'0'`），
/// 若原样写入 `"false"` 引擎会视为开启。
pub fn validate_config_patch(
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ConfigError> {
    let mut normalized = normalize_daemon_config_patch(values).map_err(|error| match error {
        DaemonConfigError::UnknownField(key) => ConfigError::UnknownField(key),
        DaemonConfigError::ReadOnly(key) => ConfigError::ReadOnlyField(key),
        DaemonConfigError::InvalidValue { field, message } => {
            ConfigError::InvalidValue { field, message }
        }
    })?;
    for key in ENGINE_NUMERIC_BOOL_KEYS {
        if let Some(value) = normalized.get_mut(*key) {
            *value = if value == "true" { "1" } else { "0" }.to_owned();
        }
    }
    Ok(normalized)
}

/// 引擎以 `"1"`/`"0"` 读取的布尔键（见 `bt_downloader` / `download_manager`
/// 中的 `get_config` 判定）。
const ENGINE_NUMERIC_BOOL_KEYS: &[&str] = &["bt_seed_enabled", "bt_auto_reseed"];

/// 从完整 engine config 中只投影 daemon UI 可见的键（含只读键，如
/// `bt_tracker_sub_cache` / `domain_conn_caps`；daemon 私有键与凭据表不投影）。
#[must_use]
pub fn public_config_values(all: &HashMap<String, String>) -> BTreeMap<String, String> {
    all.iter()
        .filter(|(key, _)| is_public_daemon_config_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// 从持久化配置构建完整 BT 运行配置。
///
/// `bt_seed_time_limit_minutes` / `bt_seed_inactive_time_limit_minutes` 落库
/// 时已是分钟；`*_unit` 键仅记录设置页的展示单位（Flutter / hub 同义），
/// 引擎 [`fluxdown_engine::bt_downloader::BtConfig`] 直接取分钟值。
/// `bt_seed_enabled` / `bt_auto_reseed` 不在 `BtConfig` 内：引擎在完成 /
/// 启动时实时读库，落库即生效。
#[must_use]
pub fn bt_config_from_map(
    cfg: &HashMap<String, String>,
) -> fluxdown_engine::bt_downloader::BtConfig {
    use fluxdown_engine::bt_downloader::BtMseMode;
    use fluxdown_engine::bt_seeding::SeedingLimitOperator;

    let subscription_enabled = cfg
        .get("bt_tracker_sub_enabled")
        .map(|value| value == "true")
        .unwrap_or(true);
    fluxdown_engine::bt_downloader::BtConfig {
        enable_dht: cfg
            .get("bt_enable_dht")
            .map(|value| value == "true")
            .unwrap_or(true),
        enable_upnp: cfg
            .get("bt_enable_upnp")
            .map(|value| value == "true")
            .unwrap_or(true),
        port_start: cfg
            .get("bt_port_start")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(6881),
        port_end: cfg
            .get("bt_port_end")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(6891),
        custom_trackers: cfg.get("bt_custom_trackers").cloned().unwrap_or_default(),
        subscription_trackers: if subscription_enabled {
            cfg.get("bt_tracker_sub_cache").cloned().unwrap_or_default()
        } else {
            String::new()
        },
        seed_ratio_limit: cfg
            .get("bt_seed_ratio_limit")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0),
        seed_post_ratio_limit: cfg
            .get("bt_seed_post_ratio_limit")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0),
        seed_time_limit_minutes: cfg
            .get("bt_seed_time_limit_minutes")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0),
        seed_inactive_time_limit_minutes: cfg
            .get("bt_seed_inactive_time_limit_minutes")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0),
        seed_limit_operator: cfg
            .get("bt_seed_limit_operator")
            .map(|value| {
                if value.eq_ignore_ascii_case("and") {
                    SeedingLimitOperator::And
                } else {
                    SeedingLimitOperator::Or
                }
            })
            .unwrap_or(SeedingLimitOperator::Or),
        seed_then_action: cfg
            .get("bt_seed_then_action")
            .cloned()
            .unwrap_or_else(|| "stop".to_owned()),
        seed_max_active: cfg
            .get("bt_seed_max_active")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0),
        mse_mode: cfg
            .get("bt_mse_mode")
            .map(String::as_str)
            .map(BtMseMode::from)
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{ConfigError, public_config_values, validate_config_patch};

    #[test]
    fn loopback_predicate_rejects_public_bind() {
        let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 17801);
        let public = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 17801);
        assert!(loopback.ip().is_loopback());
        assert!(!public.ip().is_loopback());
    }

    #[test]
    fn sync_catalog_semantics_normalize_without_losing_flutter_values() {
        let values = BTreeMap::from([
            ("auto_max_connections".to_owned(), "0".to_owned()),
            ("max_auto_retries".to_owned(), "-1".to_owned()),
            ("auto_resume_on_start".to_owned(), "true".to_owned()),
            ("ed2k_enable_upnp".to_owned(), "false".to_owned()),
            ("ed2k_server_list".to_owned(), "ed2k://server".to_owned()),
            ("bt_seed_then_action".to_owned(), "delete_files".to_owned()),
            ("bt_mse_mode".to_owned(), "enabled".to_owned()),
        ]);
        let normalized = validate_config_patch(&values).expect("valid synced config");
        assert_eq!(normalized, values);
    }

    #[test]
    fn catalog_keys_new_to_daemon_are_accepted_and_engine_encoded() {
        let values = BTreeMap::from([
            ("bt_seed_enabled".to_owned(), "false".to_owned()),
            ("bt_auto_reseed".to_owned(), "1".to_owned()),
            ("bt_seed_time_limit_unit".to_owned(), "hours".to_owned()),
            (
                "bt_seed_inactive_time_limit_unit".to_owned(),
                "days".to_owned(),
            ),
            ("ed2k_listen_port".to_owned(), " 4662 ".to_owned()),
            ("default_queue_id".to_owned(), "later".to_owned()),
        ]);
        let normalized = validate_config_patch(&values).expect("catalog keys accepted");
        assert_eq!(
            normalized["bt_seed_enabled"], "0",
            "engine reads `!= \"0\"`"
        );
        assert_eq!(normalized["bt_auto_reseed"], "1");
        assert_eq!(normalized["bt_seed_time_limit_unit"], "hours");
        assert_eq!(normalized["bt_seed_inactive_time_limit_unit"], "days");
        assert_eq!(normalized["ed2k_listen_port"], "4662");
        assert_eq!(normalized["default_queue_id"], "later");
    }

    #[test]
    fn read_only_and_unknown_keys_are_rejected_on_patch() {
        for key in [
            "domain_conn_caps",
            "bt_tracker_sub_cache",
            "bt_tracker_sub_updated_at",
            "ed2k_server_sub_cache",
        ] {
            let patch = BTreeMap::from([(key.to_owned(), String::new())]);
            assert!(
                matches!(validate_config_patch(&patch), Err(ConfigError::ReadOnlyField(k)) if k == key),
                "{key} must be rejected as read-only"
            );
        }
        for key in ["site_auth_credentials", "daemon_config_revision", "nope"] {
            let patch = BTreeMap::from([(key.to_owned(), "x".to_owned())]);
            assert!(
                matches!(validate_config_patch(&patch), Err(ConfigError::UnknownField(k)) if k == key),
                "{key} must be rejected as unknown"
            );
        }
        let bad = BTreeMap::from([("ed2k_listen_port".to_owned(), "70000".to_owned())]);
        assert!(matches!(
            validate_config_patch(&bad),
            Err(ConfigError::InvalidValue { field, .. }) if field == "ed2k_listen_port"
        ));
    }

    #[test]
    fn public_projection_keeps_read_only_keys_and_hides_private_ones() {
        let all = HashMap::from([
            ("max_concurrent_tasks".to_owned(), "5".to_owned()),
            ("bt_tracker_sub_cache".to_owned(), "udp://t".to_owned()),
            ("domain_conn_caps".to_owned(), "v3".to_owned()),
            ("proxy_password".to_owned(), "secret".to_owned()),
            ("site_auth_credentials".to_owned(), "{}".to_owned()),
            ("daemon_config_revision".to_owned(), "7".to_owned()),
            ("daemon_migration_link_acked".to_owned(), "1".to_owned()),
        ]);
        let public = public_config_values(&all);
        assert_eq!(public["max_concurrent_tasks"], "5");
        assert_eq!(public["bt_tracker_sub_cache"], "udp://t");
        assert_eq!(public["domain_conn_caps"], "v3");
        assert_eq!(public["proxy_password"], "secret");
        assert!(!public.contains_key("site_auth_credentials"));
        assert!(!public.contains_key("daemon_config_revision"));
        assert!(!public.contains_key("daemon_migration_link_acked"));
    }
}
