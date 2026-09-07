//! ED2K 分块哈希数学 —— 纯函数，零 I/O、零网络。
//!
//! eD2K 文件标识（root hash）算法：文件按 [`PART_SIZE`] 分块，每块取 MD4；
//! 单块文件 root 即该块 MD4；多块文件 root = MD4(所有块 MD4 拼接)。
//!
//! **关键边界（phantom tail）**：文件大小恰为 [`PART_SIZE`] 整数倍时，eD2K
//! 网络实际语义要求在块哈希序列尾部追加一个空块 `MD4("")`（[`MD4_EMPTY`]）
//! 再计算 root。[`build_root_input`] 是把"网络收到的 part_count 个块哈希"
//! 转换为"root 计算输入序列"的**唯一**转换点，集中处理这一 off-by-one 陷阱。

use md4::{Digest, Md4};

/// eD2K 分块大小：9.28 MB。文件按此定长切块，每块独立 MD4。
pub const PART_SIZE: u64 = 9_728_000;

/// 32/64 位协议变体分界（约 4 GiB）。文件大小超过此值时 GETSOURCES 用
/// 扩展格式、peer 用 `*_I64` opcode。值 = floor(u32::MAX / PART_SIZE) * PART_SIZE。
pub const OLD_MAX_FILE_SIZE: u64 = 4_290_048_000;
/// eD2K 块请求粒度：180 KB。单次 `OP_REQUESTPARTS` 请求区间的上限。
pub const BLOCK_SIZE: u64 = 184_320;

/// 空输入的 MD4（RFC 1320 测试向量 `31d6cfe0d16ae931b73c59d7e0c089c0`）。
///
/// phantom-tail 场景追加的空尾块哈希即此值。作为常量而非每次现算，
/// 既省一次哈希，又让该魔法值有据可查。
pub const MD4_EMPTY: [u8; 16] = [
    0x31, 0xd6, 0xcf, 0xe0, 0xd1, 0x6a, 0xe9, 0x31, 0xb7, 0x3c, 0x59, 0xd7, 0xe0, 0xc0, 0x89, 0xc0,
];

/// 文件按 `part_size` 分块后的**真实块数**（不含 phantom tail）。
///
/// `total_size == 0` 视为 1 块（0 字节文件 = 一个空块，root == [`MD4_EMPTY`]）。
///
/// `part_size` 作为参数而非硬编码 [`PART_SIZE`]：测试可传入小值（如 100）
/// 跑完整多块流程，无需真的传输 9.28 MB × N 字节。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::part_count;
/// assert_eq!(part_count(0, 100), 1);
/// assert_eq!(part_count(1, 100), 1);
/// assert_eq!(part_count(100, 100), 1);
/// assert_eq!(part_count(101, 100), 2);
/// assert_eq!(part_count(300, 100), 3);
/// ```
#[must_use]
pub fn part_count(total_size: u64, part_size: u64) -> u64 {
    if total_size == 0 {
        1
    } else {
        total_size.div_ceil(part_size)
    }
}

/// 第 `index` 块在文件中的 `[start, end)` 半开区间（末块可不满 `part_size`）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::part_span;
/// assert_eq!(part_span(0, 250, 100), (0, 100));
/// assert_eq!(part_span(2, 250, 100), (200, 250)); // 末块不满
/// ```
#[must_use]
pub fn part_span(index: u64, total_size: u64, part_size: u64) -> (u64, u64) {
    let start = index.saturating_mul(part_size);
    let end = start.saturating_add(part_size).min(total_size);
    (start.min(total_size), end)
}

/// 文件大小是否恰为 `part_size` 整数倍（触发 phantom-tail 空尾块）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::is_phantom_tail;
/// assert!(is_phantom_tail(200, 100));
/// assert!(!is_phantom_tail(0, 100));   // 0 字节不追加
/// assert!(!is_phantom_tail(150, 100));
/// ```
#[must_use]
pub fn is_phantom_tail(total_size: u64, part_size: u64) -> bool {
    total_size > 0 && total_size.is_multiple_of(part_size)
}

/// 单块数据的 MD4。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::{hash_part, MD4_EMPTY};
/// assert_eq!(hash_part(b""), MD4_EMPTY);
/// ```
#[must_use]
pub fn hash_part(data: &[u8]) -> [u8; 16] {
    let mut hasher = Md4::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 由块哈希序列计算 eD2K root hash。
///
/// 单元素直接返回该元素（单块文件语义）；多元素返回 MD4(所有元素拼接)。
///
/// 调用方负责在 phantom-tail 场景先经 [`build_root_input`] 追加 [`MD4_EMPTY`]。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::{compute_root, hash_part};
/// let h = hash_part(b"abc");
/// assert_eq!(compute_root(&[h]), h); // 单块 root == 块哈希本身
/// ```
#[must_use]
pub fn compute_root(part_hashes: &[[u8; 16]]) -> [u8; 16] {
    if part_hashes.len() == 1 {
        return part_hashes[0];
    }
    let mut hasher = Md4::new();
    for h in part_hashes {
        hasher.update(h);
    }
    hasher.finalize().into()
}

/// 把"网络收到的 `part_count` 个块哈希"转换为"root 计算输入序列"。
///
/// phantom-tail 场景（`is_phantom_tail` 为真）在尾部追加 [`MD4_EMPTY`]，
/// 否则原样克隆。**这是网络载荷（`part_count` 个）与 root 计算输入
/// （可能 `part_count + 1` 个）之间唯一的转换点** —— 集中处理 off-by-one，
/// 避免落库/终验各处重复追加造成的双重计入。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::{build_root_input, MD4_EMPTY};
/// let net = vec![[1u8; 16], [2u8; 16]];
/// // total=200, ps=100 → phantom tail，追加空块
/// let input = build_root_input(&net, 200, 100);
/// assert_eq!(input.len(), 3);
/// assert_eq!(input[2], MD4_EMPTY);
/// // total=150, ps=100 → 非整数倍，原样
/// let input2 = build_root_input(&net, 150, 100);
/// assert_eq!(input2.len(), 2);
/// ```
#[must_use]
pub fn build_root_input(net_hashes: &[[u8; 16]], total_size: u64, part_size: u64) -> Vec<[u8; 16]> {
    let mut out = net_hashes.to_vec();
    if is_phantom_tail(total_size, part_size) {
        out.push(MD4_EMPTY);
    }
    out
}

/// 校验一份网络收到的 hashset 是否与期望 root hash 一致（防投毒）。
///
/// 先校验元素数等于真实块数（`part_count`），再经 [`build_root_input`]
/// 处理 phantom-tail 后 [`compute_root`] 与 `expected_root` 逐字节比对。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::ed2k::hash::{build_root_input, compute_root, hash_part, verify_hashset_root};
/// // total = ps = 100 → 单个真实块 + phantom 空尾块
/// let block = vec![0u8; 100];
/// let net = vec![hash_part(&block)];
/// let root = compute_root(&build_root_input(&net, 100, 100));
/// assert!(verify_hashset_root(&net, &root, 100, 100));
/// // 元素数不符（part_count 应为 1）
/// assert!(!verify_hashset_root(&[[0u8; 16], [1u8; 16]], &root, 100, 100));
/// ```
#[must_use]
pub fn verify_hashset_root(
    net_hashes: &[[u8; 16]],
    expected_root: &[u8; 16],
    total_size: u64,
    part_size: u64,
) -> bool {
    if net_hashes.len() as u64 != part_count(total_size, part_size) {
        return false;
    }
    &compute_root(&build_root_input(net_hashes, total_size, part_size)) == expected_root
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_SIZE, MD4_EMPTY, PART_SIZE, build_root_input, compute_root, hash_part,
        is_phantom_tail, part_count, part_span, verify_hashset_root,
    };

    #[test]
    fn part_count_boundaries() {
        assert_eq!(part_count(0, 100), 1, "0 字节 = 1 空块");
        assert_eq!(part_count(1, 100), 1);
        assert_eq!(part_count(99, 100), 1);
        assert_eq!(part_count(100, 100), 1, "恰满 1 块（真实块数不含 phantom）");
        assert_eq!(part_count(101, 100), 2);
        assert_eq!(part_count(300, 100), 3);
        assert_eq!(part_count(301, 100), 4);
    }

    #[test]
    fn part_span_last_block_short() {
        assert_eq!(part_span(0, 250, 100), (0, 100));
        assert_eq!(part_span(1, 250, 100), (100, 200));
        assert_eq!(part_span(2, 250, 100), (200, 250));
    }

    #[test]
    fn is_phantom_tail_truth_table() {
        assert!(!is_phantom_tail(0, 100), "0 字节不追加");
        assert!(!is_phantom_tail(1, 100));
        assert!(!is_phantom_tail(99, 100));
        assert!(is_phantom_tail(100, 100));
        assert!(!is_phantom_tail(101, 100));
        assert!(is_phantom_tail(200, 100));
        assert!(is_phantom_tail(300, 100));
    }

    #[test]
    fn hash_part_rfc1320_vectors() {
        // RFC 1320 附录 A.5 已知向量。
        assert_eq!(hash_part(b""), MD4_EMPTY);
        assert_eq!(
            hash_part(b"abc"),
            [
                0xa4, 0x48, 0x01, 0x7a, 0xaf, 0x21, 0xd8, 0x52, 0x5f, 0xc1, 0x0a, 0xe8, 0x7a, 0xa6,
                0x72, 0x9d,
            ]
        );
    }

    #[test]
    fn compute_root_single_returns_element() {
        let h = hash_part(b"abc");
        assert_eq!(compute_root(&[h]), h);
    }

    #[test]
    fn compute_root_multi_is_md4_of_concat() {
        let a = [1u8; 16];
        let b = [2u8; 16];
        let mut concat = Vec::new();
        concat.extend_from_slice(&a);
        concat.extend_from_slice(&b);
        assert_eq!(compute_root(&[a, b]), hash_part(&concat));
    }

    #[test]
    fn build_root_input_phantom_tail_appends_empty() {
        let net = vec![[1u8; 16], [2u8; 16]];
        // total = ps*2 → phantom tail
        let input = build_root_input(&net, 200, 100);
        assert_eq!(input.len(), 3, "part_count(2) + phantom(1)");
        assert_eq!(input[2], MD4_EMPTY);
        // 非整数倍 → 原样
        assert_eq!(build_root_input(&net, 150, 100).len(), 2);
    }

    #[test]
    fn verify_hashset_root_phantom_tail_case() {
        // total = ps = 100：1 个真实块 + 1 个 phantom 空块参与 root。
        let block = vec![0u8; 100];
        let net = vec![hash_part(&block)];
        let root = compute_root(&build_root_input(&net, 100, 100));
        assert!(verify_hashset_root(&net, &root, 100, 100));
        // 内部 root 输入应为 2 个元素（真实块 + MD4_EMPTY）。
        assert_eq!(build_root_input(&net, 100, 100).len(), 2);
    }

    #[test]
    fn verify_hashset_root_rejects_wrong_count() {
        let root = [0u8; 16];
        // part_count(100,100)=1，传 2 个元素应拒绝。
        assert!(!verify_hashset_root(
            &[[0u8; 16], [1u8; 16]],
            &root,
            100,
            100
        ));
    }

    #[test]
    fn verify_hashset_root_rejects_mismatch() {
        let net = vec![hash_part(b"real")];
        assert!(!verify_hashset_root(&net, &[0xffu8; 16], 4, 100));
    }

    #[test]
    fn constants_match_protocol() {
        assert_eq!(PART_SIZE, 9_728_000);
        assert_eq!(BLOCK_SIZE, 184_320);
    }
}
