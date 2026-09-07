//! engine-local 三段 semver 比较，逐字复刻 `hub/src/updater.rs` 的 `parse_semver`/
//! `is_newer` 语义：仅比较 major.minor.patch 三段整数字典序，忽略预发布/构建元数据，
//! 格式非法视为不满足。
//!
//! 不跨 crate 抽取（几十行工具函数共享收益不成比例），不引入 semver crate。

/// 解析 `"MAJOR.MINOR.PATCH"`（可带前导 `v`）为三段整数。
///
/// 预发布/构建元数据（`-rc.1`、`+build`）在解析前被裁掉——取第一个 `-` 或 `+` 之前
/// 的部分。任一段解析失败或段数不为 3 返回 `None`。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::plugin::semver::parse_semver;
///
/// assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
/// assert_eq!(parse_semver("v0.2.0"), Some((0, 2, 0)));
/// assert_eq!(parse_semver("1.2.3-rc.1"), Some((1, 2, 3)));
/// assert_eq!(parse_semver("1.2"), None);
/// assert_eq!(parse_semver("abc"), None);
/// ```
pub fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    // 裁掉预发布/构建元数据。
    let core = s.split(['-', '+']).next().unwrap_or(s);
    let mut it = core.split('.');
    let major = it.next()?.parse::<u64>().ok()?;
    let minor = it.next()?.parse::<u64>().ok()?;
    let patch = it.next()?.parse::<u64>().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// `have >= min` 时返回 `true`。任一侧格式非法视为「不满足」（返回 `false`）——
/// 用于 `minAppVersion` 门槛校验：宿主版本无法解析或低于插件要求则跳过加载。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::plugin::semver::satisfies_min;
///
/// assert!(satisfies_min("0.3.0", "0.2.0"));
/// assert!(satisfies_min("1.0.0", "1.0.0"));
/// assert!(!satisfies_min("0.1.9", "0.2.0"));
/// assert!(!satisfies_min("garbage", "0.2.0"));
/// ```
pub fn satisfies_min(have: &str, min: &str) -> bool {
    match (parse_semver(have), parse_semver(min)) {
        (Some(h), Some(m)) => h >= m,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_semver, satisfies_min};

    #[test]
    fn parse_basic() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("v10.20.30"), Some((10, 20, 30)));
        assert_eq!(parse_semver("0.0.0"), Some((0, 0, 0)));
    }

    #[test]
    fn parse_strips_prerelease_and_build() {
        assert_eq!(parse_semver("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_semver("1.2.3+build.7"), Some((1, 2, 3)));
        assert_eq!(parse_semver("1.2.3-alpha+meta"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_rejects_malformed() {
        assert_eq!(parse_semver("1.2"), None);
        assert_eq!(parse_semver("1.2.3.4"), None);
        assert_eq!(parse_semver(""), None);
        assert_eq!(parse_semver("a.b.c"), None);
        assert_eq!(parse_semver("1.2.x"), None);
    }

    #[test]
    fn satisfies_min_ordering() {
        assert!(satisfies_min("0.2.0", "0.2.0"));
        assert!(satisfies_min("0.2.1", "0.2.0"));
        assert!(satisfies_min("1.0.0", "0.9.9"));
        assert!(!satisfies_min("0.1.0", "0.2.0"));
        assert!(!satisfies_min("0.2.0", "0.2.1"));
    }

    #[test]
    fn satisfies_min_malformed_is_unsatisfied() {
        assert!(!satisfies_min("bad", "0.2.0"));
        assert!(!satisfies_min("0.2.0", "bad"));
    }
}
