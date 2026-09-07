//! daemon 持久化配置键的唯一目录：类型、范围、默认值与可写性。
//!
//! daemon 用它校验 `daemon.config.patch`，UI 用它渲染控件与解析快照；
//! 两侧不得各自维护第二份键表。

use std::collections::BTreeMap;

/// 单个 daemon 配置键的值域。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DaemonConfigKind {
    Bool,
    Integer {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
    },
    Enum(&'static [&'static str]),
    Text,
    /// 引擎自行维护的键：可读、不可经 patch 写入。
    ReadOnly,
}

/// daemon 配置键描述。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DaemonConfigField {
    pub key: &'static str,
    pub kind: DaemonConfigKind,
    /// 未持久化时的有效默认值（wire 字符串形式）。
    pub default: &'static str,
}

const fn field(
    key: &'static str,
    kind: DaemonConfigKind,
    default: &'static str,
) -> DaemonConfigField {
    DaemonConfigField { key, kind, default }
}

pub const BT_SEED_TIME_UNITS: &[&str] = &["minutes", "hours", "days"];
pub const FILE_EXISTS_BEHAVIORS: &[&str] = &["rename", "overwrite"];
pub const FILE_MISSING_ACTIONS: &[&str] = &["keep", "delete"];
pub const BT_SEED_LIMIT_OPERATORS: &[&str] = &["or", "and"];
pub const BT_SEED_THEN_ACTIONS: &[&str] = &["stop", "delete", "delete_files"];
pub const BT_MSE_MODES: &[&str] = &["disabled", "enabled", "forced"];
pub const PROXY_MODES: &[&str] = &["none", "system", "manual", "auto"];
pub const PROXY_TYPES: &[&str] = &["http", "https", "socks4", "socks5"];

/// 全部 daemon 配置键。顺序无语义。
pub const DAEMON_CONFIG_FIELDS: &[DaemonConfigField] = &[
    // ── 下载 ──
    field("default_save_dir", DaemonConfigKind::Text, ""),
    field(
        "default_segments",
        DaemonConfigKind::Integer { min: 0, max: 64 },
        "0",
    ),
    field(
        "auto_max_connections",
        DaemonConfigKind::Integer { min: 0, max: 128 },
        "16",
    ),
    field("cdn_multi_enabled", DaemonConfigKind::Bool, "false"),
    field(
        "cdn_max_nodes",
        DaemonConfigKind::Integer { min: 0, max: 8 },
        "0",
    ),
    field(
        "max_concurrent_tasks",
        DaemonConfigKind::Integer { min: 1, max: 1024 },
        "5",
    ),
    field(
        "speed_limit_bytes",
        DaemonConfigKind::Integer {
            min: 0,
            max: i64::MAX,
        },
        "0",
    ),
    field(
        "upload_limit_bytes",
        DaemonConfigKind::Integer {
            min: 0,
            max: i64::MAX,
        },
        "0",
    ),
    field(
        "max_auto_retries",
        DaemonConfigKind::Integer { min: -1, max: 20 },
        "3",
    ),
    field(
        "auto_retry_delay_secs",
        DaemonConfigKind::Integer {
            min: 0,
            max: 86_400,
        },
        "5",
    ),
    field("auto_resume_on_start", DaemonConfigKind::Bool, "false"),
    field("use_server_time", DaemonConfigKind::Bool, "false"),
    field(
        "file_exists_behavior",
        DaemonConfigKind::Enum(FILE_EXISTS_BEHAVIORS),
        "rename",
    ),
    field(
        "file_missing_action",
        DaemonConfigKind::Enum(FILE_MISSING_ACTIONS),
        "keep",
    ),
    field("global_user_agent", DaemonConfigKind::Text, ""),
    field("default_queue_id", DaemonConfigKind::Text, ""),
    field("domain_conn_caps", DaemonConfigKind::ReadOnly, ""),
    // ── BT ──
    field("bt_enable_dht", DaemonConfigKind::Bool, "true"),
    field("bt_enable_upnp", DaemonConfigKind::Bool, "true"),
    field(
        "bt_port_start",
        DaemonConfigKind::Integer {
            min: 1,
            max: 65_535,
        },
        "6881",
    ),
    field(
        "bt_port_end",
        DaemonConfigKind::Integer {
            min: 1,
            max: 65_535,
        },
        "6891",
    ),
    field(
        "bt_mse_mode",
        DaemonConfigKind::Enum(BT_MSE_MODES),
        "enabled",
    ),
    field("bt_custom_trackers", DaemonConfigKind::Text, ""),
    field("bt_tracker_sub_enabled", DaemonConfigKind::Bool, "true"),
    field("bt_tracker_sub_urls", DaemonConfigKind::Text, ""),
    field("bt_tracker_sub_cache", DaemonConfigKind::ReadOnly, ""),
    field("bt_tracker_sub_updated_at", DaemonConfigKind::ReadOnly, "0"),
    field("bt_seed_enabled", DaemonConfigKind::Bool, "true"),
    field("bt_auto_reseed", DaemonConfigKind::Bool, "true"),
    field(
        "bt_seed_max_active",
        DaemonConfigKind::Integer {
            min: 0,
            max: i64::MAX,
        },
        "0",
    ),
    field(
        "bt_seed_ratio_limit",
        DaemonConfigKind::Float { min: 0.0 },
        "0",
    ),
    field(
        "bt_seed_post_ratio_limit",
        DaemonConfigKind::Float { min: 0.0 },
        "0",
    ),
    field(
        "bt_seed_time_limit_minutes",
        DaemonConfigKind::Integer {
            min: 0,
            max: i64::MAX,
        },
        "0",
    ),
    field(
        "bt_seed_time_limit_unit",
        DaemonConfigKind::Enum(BT_SEED_TIME_UNITS),
        "minutes",
    ),
    field(
        "bt_seed_inactive_time_limit_minutes",
        DaemonConfigKind::Integer {
            min: 0,
            max: i64::MAX,
        },
        "0",
    ),
    field(
        "bt_seed_inactive_time_limit_unit",
        DaemonConfigKind::Enum(BT_SEED_TIME_UNITS),
        "minutes",
    ),
    field(
        "bt_seed_limit_operator",
        DaemonConfigKind::Enum(BT_SEED_LIMIT_OPERATORS),
        "or",
    ),
    field(
        "bt_seed_then_action",
        DaemonConfigKind::Enum(BT_SEED_THEN_ACTIONS),
        "stop",
    ),
    // ── ED2K ──
    field("ed2k_enable_kad", DaemonConfigKind::Bool, "true"),
    field("ed2k_enable_upnp", DaemonConfigKind::Bool, "true"),
    field(
        "ed2k_listen_port",
        DaemonConfigKind::Integer {
            min: 0,
            max: 65_535,
        },
        "0",
    ),
    field("ed2k_server_list", DaemonConfigKind::Text, ""),
    field("ed2k_server_sub_enabled", DaemonConfigKind::Bool, "true"),
    field("ed2k_server_sub_urls", DaemonConfigKind::Text, ""),
    field("ed2k_server_sub_cache", DaemonConfigKind::ReadOnly, ""),
    field(
        "ed2k_server_sub_updated_at",
        DaemonConfigKind::ReadOnly,
        "0",
    ),
    field("ed2k_nodes_dat_url", DaemonConfigKind::Text, ""),
    // ── 代理 ──
    field("proxy_mode", DaemonConfigKind::Enum(PROXY_MODES), "none"),
    field("proxy_type", DaemonConfigKind::Enum(PROXY_TYPES), "http"),
    field("proxy_host", DaemonConfigKind::Text, ""),
    field("proxy_port", DaemonConfigKind::Text, ""),
    field("proxy_username", DaemonConfigKind::Text, ""),
    field("proxy_password", DaemonConfigKind::Text, ""),
    field("proxy_no_list", DaemonConfigKind::Text, ""),
    // ── Webhook ──
    field("webhook.endpoints", DaemonConfigKind::Text, ""),
    // ── 受管组件手动路径（空 = 自动解析）──
    field("component.ffmpeg.path", DaemonConfigKind::Text, ""),
    field("component.ytdlp.path", DaemonConfigKind::Text, ""),
];

/// 校验失败原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonConfigError {
    UnknownField(String),
    ReadOnly(String),
    InvalidValue { field: String, message: String },
}

impl std::fmt::Display for DaemonConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownField(key) => write!(f, "unknown daemon config field: {key}"),
            Self::ReadOnly(key) => write!(f, "daemon config field is read-only: {key}"),
            Self::InvalidValue { field, message } => {
                write!(f, "invalid daemon config value for {field}: {message}")
            }
        }
    }
}

impl std::error::Error for DaemonConfigError {}

#[must_use]
pub fn daemon_config_field(key: &str) -> Option<&'static DaemonConfigField> {
    DAEMON_CONFIG_FIELDS.iter().find(|field| field.key == key)
}

/// 键在 `daemon.config.get` 投影中是否可见（含只读键）。
#[must_use]
pub fn is_public_daemon_config_key(key: &str) -> bool {
    daemon_config_field(key).is_some()
}

/// 未持久化时的默认值；未知键返回空串。
#[must_use]
pub fn daemon_config_default(key: &str) -> &'static str {
    daemon_config_field(key).map_or("", |field| field.default)
}

/// 规范化单个可写键的值。
pub fn normalize_daemon_config_value(key: &str, value: &str) -> Result<String, DaemonConfigError> {
    let field =
        daemon_config_field(key).ok_or_else(|| DaemonConfigError::UnknownField(key.to_owned()))?;
    let invalid = |message: String| DaemonConfigError::InvalidValue {
        field: key.to_owned(),
        message,
    };
    match field.kind {
        DaemonConfigKind::ReadOnly => Err(DaemonConfigError::ReadOnly(key.to_owned())),
        DaemonConfigKind::Bool => match value.trim() {
            "true" | "1" => Ok("true".to_owned()),
            "false" | "0" => Ok("false".to_owned()),
            other => Err(invalid(format!("expected boolean, got {other:?}"))),
        },
        DaemonConfigKind::Integer { min, max } => {
            let parsed = value
                .trim()
                .parse::<i64>()
                .map_err(|error| invalid(error.to_string()))?;
            if parsed < min || parsed > max {
                return Err(invalid(format!("must be between {min} and {max}")));
            }
            Ok(parsed.to_string())
        }
        DaemonConfigKind::Float { min } => {
            let parsed = value
                .trim()
                .parse::<f64>()
                .map_err(|error| invalid(error.to_string()))?;
            if !parsed.is_finite() || parsed < min {
                return Err(invalid(format!("must be a finite number >= {min}")));
            }
            Ok(parsed.to_string())
        }
        DaemonConfigKind::Enum(allowed) => {
            let trimmed = value.trim();
            if allowed.contains(&trimmed) {
                Ok(trimmed.to_owned())
            } else {
                Err(invalid(format!("must be one of {}", allowed.join(", "))))
            }
        }
        DaemonConfigKind::Text => Ok(value.trim().to_owned()),
    }
}

/// 校验并规范化一整个 patch；任一键失败即整体失败。
pub fn normalize_daemon_config_patch(
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, DaemonConfigError> {
    values
        .iter()
        .map(|(key, value)| normalize_daemon_config_value(key, value).map(|v| (key.clone(), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keys_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for field in DAEMON_CONFIG_FIELDS {
            assert!(seen.insert(field.key), "duplicate key {}", field.key);
        }
    }

    #[test]
    fn sync_catalog_daemon_keys_exist_in_config_catalog() {
        for spec in crate::settings::SYNC_SETTING_SPECS {
            if spec.owner == crate::settings::SettingOwner::Daemon {
                let field = daemon_config_field(spec.storage_key);
                assert!(field.is_some(), "missing daemon field {}", spec.storage_key);
                assert_ne!(
                    field.map(|f| f.kind),
                    Some(DaemonConfigKind::ReadOnly),
                    "{} must be writable",
                    spec.storage_key
                );
            }
        }
    }

    #[test]
    fn defaults_are_valid_for_writable_fields() {
        for field in DAEMON_CONFIG_FIELDS {
            if field.kind == DaemonConfigKind::ReadOnly {
                continue;
            }
            normalize_daemon_config_value(field.key, field.default)
                .unwrap_or_else(|error| panic!("{}: {error}", field.key));
        }
    }

    #[test]
    fn normalizes_and_rejects() {
        assert_eq!(
            normalize_daemon_config_value("bt_enable_dht", "1").as_deref(),
            Ok("true")
        );
        assert!(normalize_daemon_config_value("cdn_max_nodes", "9").is_err());
        assert!(normalize_daemon_config_value("bt_tracker_sub_cache", "x").is_err());
        assert!(normalize_daemon_config_value("nope", "x").is_err());
        assert_eq!(
            normalize_daemon_config_value("proxy_mode", " manual ").as_deref(),
            Ok("manual")
        );
    }
}
