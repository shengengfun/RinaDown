//! 设备互联加密原语：指纹、SAS 短认证串、链路密钥派生、数据面 HMAC 鉴权 +
//! AEAD 加密。
//!
//! 全部基于已在引擎中的 `sha2 0.10`（digest 0.10）+ `hkdf 0.12` + `hmac 0.12`，
//! 三者 digest 版本一致，避免类型 trait bound 冲突；数据面 body 加密另加
//! `chacha20poly1305 0.10`（RustCrypto 同族，与前三者独立但风格一致）。

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Mac, SimpleHmac};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// SAS 短认证串位数（6 位数字，双端肉眼核对）。
const SAS_DIGITS: u32 = 6;
const SAS_MODULO: u32 = 1_000_000; // 10^SAS_DIGITS

/// 数据面链路鉴权时间戳容忍窗口（秒）——防重放，两端时钟偏移容错。
pub const LINK_AUTH_SKEW_SECS: i64 = 120;

/// 计算 Ed25519（或任意）公钥的展示指纹：`hex(sha256(pub))`（64 hex 小写）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::link::crypto::fingerprint;
/// let fp = fingerprint(&[0u8; 32]);
/// assert_eq!(fp.len(), 64);
/// ```
#[must_use]
pub fn fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    hex::encode(digest)
}

/// 从 X25519 ECDH 共享密钥 `z` + 双方临时公钥派生 6 位 SAS。
///
/// 两端公钥排序后拼接（顺序无关），确保 initiator 与 responder 计算出**相同** SAS。
/// 中间人会与两端各自建立不同的 `z` → 两端 SAS 不一致 → 用户肉眼核对即可发现。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::link::crypto::derive_sas;
/// let z = [7u8; 32];
/// let a = [1u8; 32];
/// let b = [2u8; 32];
/// // 顺序无关：交换 a/b 得到同一 SAS。
/// assert_eq!(derive_sas(&z, &a, &b), derive_sas(&z, &b, &a));
/// assert_eq!(derive_sas(&z, &a, &b).len(), 6);
/// ```
#[must_use]
pub fn derive_sas(z: &[u8], pub_a: &[u8; 32], pub_b: &[u8; 32]) -> String {
    let (lo, hi) = if pub_a <= pub_b {
        (pub_a, pub_b)
    } else {
        (pub_b, pub_a)
    };
    let mut info = Vec::with_capacity(64);
    info.extend_from_slice(lo);
    info.extend_from_slice(hi);

    let hk = Hkdf::<Sha256>::new(Some(b"fluxdown-link-sas-v1"), z);
    let mut okm = [0u8; 4];
    // 长度固定 4 字节 << 255*32，expand 不会失败；仍显式处理错误不 unwrap。
    if hk.expand(&info, &mut okm).is_err() {
        return "000000".to_string();
    }
    let n = u32::from_be_bytes(okm) % SAS_MODULO;
    format!("{n:0width$}", width = SAS_DIGITS as usize)
}

/// 从 ECDH 共享密钥派生**每对设备独立**的 32 字节链路密钥（数据面 HMAC 用）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::link::crypto::derive_link_key;
/// let k = derive_link_key(&[9u8; 32]);
/// assert_eq!(k.len(), 32);
/// ```
#[must_use]
pub fn derive_link_key(z: &[u8]) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::new(Some(b"fluxdown-link-key-salt-v1"), z);
    let mut okm = [0u8; 32];
    if hk.expand(b"fluxdown-link-key-v1", &mut okm).is_err() {
        return z.to_vec();
    }
    okm.to_vec()
}

/// 从每对设备独立的 `link_secret` 派生数据面 AEAD 加密密钥。
///
/// **域分隔**：HKDF salt/info 标签与 [`derive_sas`]（`"fluxdown-link-sas-v1"`）、
/// [`derive_link_key`]（`"fluxdown-link-key-salt-v1"`/`"fluxdown-link-key-v1"`）
/// 均不同——AEAD 加密密钥绝不能等于 HMAC 鉴权用的 `link_secret` 本身或与
/// SAS 相关，否则一把密钥材料挪作多用途，任一用途的密码学分析结果都可能
/// 波及其余用途。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::link::crypto::derive_link_aead_key;
/// let k = derive_link_aead_key(&[6u8; 32]);
/// assert_eq!(k.len(), 32);
/// ```
#[must_use]
pub fn derive_link_aead_key(link_secret: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"fluxdown-link-aead-salt-v1"), link_secret);
    let mut okm = [0u8; 32];
    // 长度固定 32 字节 << 255*32，expand 不会失败；仍显式处理错误分支
    // （clippy 禁 unwrap/expect）。
    if hk.expand(b"fluxdown-link-aead-v1", &mut okm).is_err() {
        return [0u8; 32];
    }
    okm
}

/// 加密数据面请求体（ChaCha20-Poly1305）：随机 12 字节 nonce，返回
/// `nonce(12B) || ciphertext_with_tag`。
///
/// 每次调用生成独立随机 nonce——同一密钥绝不复用 nonce，否则 ChaCha20-
/// Poly1305 的机密性与完整性均被破坏。数据面请求量级远低于随机数生日
/// 碰撞有意义的阈值（2^32 条消息级别），随机 nonce 足够安全，不需要跨
/// 请求持久化计数器的额外状态同步复杂度。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::link::crypto::{open_link_body, seal_link_body};
/// let key = [1u8; 32];
/// let sealed = seal_link_body(&key, b"hello");
/// assert_eq!(open_link_body(&key, &sealed).as_deref(), Some(b"hello".as_slice()));
/// ```
#[must_use]
pub fn seal_link_body(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let mut sealed = Vec::with_capacity(12 + plaintext.len() + 16);
    sealed.extend_from_slice(&nonce_bytes);
    // 对本用法（有界大小的下载任务 JSON，远小于 ChaCha20 计数器上限）加密
    // 实践中不会失败；仍显式处理错误分支（clippy 禁 unwrap/expect）——
    // 失败时只返回裸 nonce（无密文/tag），对端 `open_link_body` 因长度
    // 不足直接拒绝，不会误用无效密文。
    if let Ok(ciphertext) = cipher.encrypt(nonce, plaintext) {
        sealed.extend_from_slice(&ciphertext);
    }
    sealed
}

/// 解密数据面请求体：拆出前 12 字节 nonce，解密其余部分。长度不足（不足以
/// 容纳 nonce + 至少一个认证 tag）或解密失败（tag 不匹配/密钥不对）均返回
/// `None`，调用方一律按鉴权失败处理，不区分具体原因——避免向攻击者泄露
/// 「是长度错还是密钥/篡改错」这类旁路信息。
///
/// # Examples
///
/// 见 [`seal_link_body`] 的往返示例。
#[must_use]
pub fn open_link_body(key: &[u8; 32], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 12 + 16 {
        return None;
    }
    let (nonce_bytes, ciphertext) = sealed.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).ok()
}

/// 数据面链路鉴权标签：`HMAC-SHA256(link_secret, method\npath\nts\nnonce\nSHA256(body))` 的 hex。
///
/// 密钥永不上网络；只发送 HMAC 标签 + 明文 method/path/ts/nonce + 请求体，对端用存储
/// 的同一 `link_secret` 重算比对（常量时间）。body 摘要纳入签名，防止 on-path 攻击者
/// 保留头部改写请求体。适配任意传输（Direct/未来 iroh/relay）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::link::crypto::{link_auth_tag, verify_link_auth_tag};
/// let key = [3u8; 32];
/// let body = b"{}";
/// let tag = link_auth_tag(&key, "POST", "/api/v1/link/tasks", 1000, "abc", body);
/// assert!(verify_link_auth_tag(&key, "POST", "/api/v1/link/tasks", 1000, "abc", body, &tag));
/// assert!(!verify_link_auth_tag(&key, "GET", "/api/v1/link/tasks", 1000, "abc", body, &tag));
/// ```
#[must_use]
pub fn link_auth_tag(
    secret: &[u8],
    method: &str,
    path: &str,
    ts: i64,
    nonce: &str,
    body: &[u8],
) -> String {
    // HMAC 接受任意长度密钥，new_from_slice 对本用法永不返回 InvalidLength；
    // 仍显式处理错误分支（clippy 禁 unwrap/expect）——极端情况下返回空标签，
    // 校验侧 ct_eq 因长度不符必然 false，安全。
    let Ok(mut mac) = <SimpleHmac<Sha256> as Mac>::new_from_slice(secret) else {
        return String::new();
    };
    mac.update(method.as_bytes());
    mac.update(b"\n");
    mac.update(path.as_bytes());
    mac.update(b"\n");
    mac.update(ts.to_string().as_bytes());
    mac.update(b"\n");
    mac.update(nonce.as_bytes());
    mac.update(b"\n");
    // body 摘要纳入签名，防止 on-path 攻击者保留头部改写请求体（url/saveDir）。
    mac.update(&Sha256::digest(body));
    hex::encode(mac.finalize().into_bytes())
}

/// 常量时间校验数据面链路鉴权标签。
#[must_use]
pub fn verify_link_auth_tag(
    secret: &[u8],
    method: &str,
    path: &str,
    ts: i64,
    nonce: &str,
    body: &[u8],
    tag_hex: &str,
) -> bool {
    let expected = link_auth_tag(secret, method, path, ts, nonce, body);
    // hex::encode 输出等长，用 ct 比较避免时序侧信道。
    ct_eq(expected.as_bytes(), tag_hex.as_bytes())
}

/// 常量时间字节比较（长度不等直接 false）。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn sas_is_order_independent_and_six_digits() {
        let z = [42u8; 32];
        let a = [1u8; 32];
        let b = [200u8; 32];
        let s1 = derive_sas(&z, &a, &b);
        let s2 = derive_sas(&z, &b, &a);
        assert_eq!(s1, s2);
        assert_eq!(s1.len(), 6);
        assert!(s1.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn different_shared_secret_yields_different_sas() {
        // 中间人场景：两端 z 不同 → SAS 不同（用户可发现）。
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_ne!(
            derive_sas(&[7u8; 32], &a, &b),
            derive_sas(&[9u8; 32], &a, &b)
        );
    }

    #[test]
    fn link_key_deterministic_per_secret() {
        assert_eq!(derive_link_key(&[5u8; 32]), derive_link_key(&[5u8; 32]));
        assert_ne!(derive_link_key(&[5u8; 32]), derive_link_key(&[6u8; 32]));
    }

    #[test]
    fn auth_tag_roundtrip_and_tamper_detection() {
        let key = [3u8; 32];
        let body = br#"{"url":"http://x/f"}"#;
        let tag = link_auth_tag(&key, "POST", "/api/v1/link/tasks", 1000, "nonce1", body);
        assert!(verify_link_auth_tag(
            &key,
            "POST",
            "/api/v1/link/tasks",
            1000,
            "nonce1",
            body,
            &tag
        ));
        // 篡改任一字段都失败。
        assert!(!verify_link_auth_tag(
            &key,
            "PUT",
            "/api/v1/link/tasks",
            1000,
            "nonce1",
            body,
            &tag
        ));
        assert!(!verify_link_auth_tag(
            &key, "POST", "/x", 1000, "nonce1", body, &tag
        ));
        assert!(!verify_link_auth_tag(
            &key,
            "POST",
            "/api/v1/link/tasks",
            1001,
            "nonce1",
            body,
            &tag
        ));
        assert!(!verify_link_auth_tag(
            &key,
            "POST",
            "/api/v1/link/tasks",
            1000,
            "nonce2",
            body,
            &tag
        ));
        // 篡改请求体（换 URL）→ 失败（body 已纳入签名）。
        assert!(!verify_link_auth_tag(
            &key,
            "POST",
            "/api/v1/link/tasks",
            1000,
            "nonce1",
            br#"{"url":"http://evil/f"}"#,
            &tag
        ));
        // 换密钥失败。
        assert!(!verify_link_auth_tag(
            &[4u8; 32],
            "POST",
            "/api/v1/link/tasks",
            1000,
            "nonce1",
            body,
            &tag
        ));
    }

    #[test]
    fn fingerprint_is_64_hex() {
        let fp = fingerprint(&[0xabu8; 32]);
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = [9u8; 32];
        let plaintext = br#"{"url":"http://x/f","saveDir":"","fileName":""}"#;
        let sealed = seal_link_body(&key, plaintext);
        // nonce(12) + 明文 + tag(16) 的开销下限。
        assert_eq!(sealed.len(), plaintext.len() + 12 + 16);
        let opened = open_link_body(&key, &sealed).expect("decrypt should succeed");
        assert_eq!(opened, plaintext);
        // 每次调用随机 nonce：同一明文两次密封结果不同。
        let sealed2 = seal_link_body(&key, plaintext);
        assert_ne!(sealed, sealed2);
    }

    #[test]
    fn open_rejects_tampered_ciphertext() {
        let key = [9u8; 32];
        let plaintext = b"hello link";
        let mut sealed = seal_link_body(&key, plaintext);
        // 翻转密文最后一字节（落在 Poly1305 tag 内）。
        let last = sealed.len() - 1;
        sealed[last] ^= 0xFF;
        assert!(open_link_body(&key, &sealed).is_none());
    }

    #[test]
    fn open_rejects_wrong_key() {
        let key = [9u8; 32];
        let other = [8u8; 32];
        let plaintext = b"hello link";
        let sealed = seal_link_body(&key, plaintext);
        assert!(open_link_body(&other, &sealed).is_none());
        // 长度不足（只剩 nonce、没有 tag）也必须拒绝。
        assert!(open_link_body(&key, &sealed[..12]).is_none());
    }

    #[test]
    fn aead_key_differs_from_sas_and_link_key() {
        // 用同一份 32 字节输入喂给三个派生函数：只要 HKDF salt/info 标签不同，
        // 输出就必然不同。纯回归测试——防止未来有人手滑复制了已有标签，导致
        // AEAD 密钥能被 SAS / 握手期链路密钥反推，破坏域分隔。
        let secret = [77u8; 32];
        let pub_a = [1u8; 32];
        let pub_b = [2u8; 32];

        let aead_key = derive_link_aead_key(&secret);
        let link_key = derive_link_key(&secret);
        let sas = derive_sas(&secret, &pub_a, &pub_b);

        assert_ne!(aead_key.as_slice(), link_key.as_slice());
        assert_ne!(aead_key.as_slice(), sas.as_bytes());
    }
}
