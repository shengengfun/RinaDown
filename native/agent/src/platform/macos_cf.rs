//! macOS Core Foundation 最小 FFI：`file_association`（文档类型）与
//! `protocol_registry`（URL scheme）共用的 CFString/CFBundle 辅助。
//!
//! 只需五个符号，直接声明而不引入 `core-foundation` crate。

use std::ffi::{CString, c_char, c_void};
use std::io;

/// `kCFStringEncodingUTF8`。
const CF_ENCODING_UTF8: u32 = 0x0800_0100;

pub type CFStringRef = *const c_void;
type CFBundleRef = *const c_void;
type CFAllocatorRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCStringPtr(the_string: CFStringRef, encoding: u32) -> *const c_char;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(cf: *const c_void);
    fn CFBundleGetMainBundle() -> CFBundleRef;
    fn CFBundleGetIdentifier(bundle: CFBundleRef) -> CFStringRef;
}

/// 持有 `Create`/`Copy` 返回的 CF 引用，drop 时释放；空指针视为无需释放。
pub struct CfOwned(CFStringRef);

impl CfOwned {
    /// 接管 `Create`/`Copy` 调用返回的引用。
    pub fn new(cf: CFStringRef) -> Self {
        Self(cf)
    }

    /// 借用底层引用；所有权仍归本守卫。
    pub fn raw(&self) -> CFStringRef {
        self.0
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` 是我们持有的非空 CF 引用（来自 Create/Copy），
            // 只在此处释放一次。
            unsafe { CFRelease(self.0) };
        }
    }
}

/// 由 `&str` 创建自持 `CFString`。
pub fn cf_string(s: &str) -> Result<CfOwned, io::Error> {
    let c = CString::new(s).map_err(|_| io::Error::other("string contains interior NUL"))?;
    // SAFETY: `c` 是有效的 NUL 结尾 C 字符串且在调用期间存活；默认分配器
    // （null）会把字节复制进新的 CFString。
    let cf = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), CF_ENCODING_UTF8) };
    if cf.is_null() {
        return Err(io::Error::other("CFStringCreateWithCString failed"));
    }
    Ok(CfOwned(cf))
}

/// 把借用的 `CFStringRef` 转成 Rust `String`。
pub fn cf_to_string(cf: CFStringRef) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    // 快路径：部分 CFString 直接暴露 UTF-8 缓冲。
    // SAFETY: `cf` 是有效的 CFStringRef；返回指针非空时由 `cf` 持有，
    // 在 `cf` 存活期间有效。
    let ptr = unsafe { CFStringGetCStringPtr(cf, CF_ENCODING_UTF8) };
    if !ptr.is_null() {
        // SAFETY: `ptr` 是 `cf` 持有的有效 NUL 结尾 UTF-8 缓冲。
        return unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(str::to_owned);
    }
    // 慢路径：复制到本地缓冲（bundle id 很短）。
    let mut buf = [0_i8; 512];
    // SAFETY: `buf` 是 `buf.len()` 字节的可写缓冲；成功时函数在此范围内写入
    // NUL 结尾。返回 CF `Boolean`（u8）：非零表示整串已写入。
    let ok = unsafe {
        CFStringGetCString(
            cf,
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len() as isize,
            CF_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    // SAFETY: 成功时缓冲内是 NUL 结尾的 C 字符串。
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr().cast::<c_char>()) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// 当前进程所在 `.app` 的 bundle id（如 `com.fluxdown.app`）。
///
/// agent 与 `fluxdown-desktop` 同处 `FluxDown.app/Contents/MacOS/`，Core
/// Foundation 会从可执行文件路径向上解析出外层 bundle；在 bundle 之外运行
/// （如 `target/release`）时没有 Info.plist，返回 `None`，此时 Launch Services
/// 注册不可用。
pub fn main_bundle_id() -> Option<String> {
    // SAFETY: `CFBundleGetMainBundle` 返回借用引用或 null；`CFBundleGetIdentifier`
    // 同样返回借用引用——均不在此释放。
    let id = unsafe {
        let bundle = CFBundleGetMainBundle();
        if bundle.is_null() {
            return None;
        }
        CFBundleGetIdentifier(bundle)
    };
    cf_to_string(id)
}
