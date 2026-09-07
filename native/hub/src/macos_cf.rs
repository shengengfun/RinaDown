//! Minimal Core Foundation helpers shared by the macOS branches of
//! `file_association` (document types) and `protocol_registry` (URL schemes).
//!
//! Raw FFI is used instead of the `core-foundation` crate: the App shell needs
//! four symbols, and pulling a dependency in for them is not worth it.

use std::ffi::{CString, c_char, c_void};
use std::io;

/// `kCFStringEncodingUTF8`.
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

/// RAII guard that releases a Core Foundation reference on drop.
///
/// Only wraps references we own (returned from `Create`/`Copy` functions).
/// A null pointer is treated as "nothing to release".
pub struct CfOwned(CFStringRef);

impl CfOwned {
    /// Take ownership of a CF reference obtained from a `Create`/`Copy` call.
    pub fn new(cf: CFStringRef) -> Self {
        Self(cf)
    }

    /// Borrow the underlying reference; it stays owned by this guard.
    pub fn raw(&self) -> CFStringRef {
        self.0
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` is a non-null CF reference we own (obtained
            // from a Create/Copy call), released exactly once here.
            unsafe { CFRelease(self.0) };
        }
    }
}

/// Create an owned `CFString` from a Rust `&str`.
pub fn cf_string(s: &str) -> Result<CfOwned, io::Error> {
    let c = CString::new(s).map_err(|_| io::Error::other("string contains interior NUL"))?;
    // SAFETY: `c` is a valid NUL-terminated C string that outlives the call;
    // the default allocator (null) copies the bytes into the new CFString.
    let cf = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), CF_ENCODING_UTF8) };
    if cf.is_null() {
        return Err(io::Error::other("CFStringCreateWithCString failed"));
    }
    Ok(CfOwned(cf))
}

/// Convert a borrowed (non-owned) `CFStringRef` to a Rust `String`.
pub fn cf_to_string(cf: CFStringRef) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    // Fast path: some CFStrings expose their UTF-8 buffer directly.
    // SAFETY: `cf` is a valid CFStringRef; the returned pointer, if
    // non-null, is owned by `cf` and valid for the lifetime of `cf`.
    let ptr = unsafe { CFStringGetCStringPtr(cf, CF_ENCODING_UTF8) };
    if !ptr.is_null() {
        // SAFETY: `ptr` is a valid NUL-terminated UTF-8 buffer owned by `cf`.
        return unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(str::to_owned);
    }
    // Slow path: copy into a local buffer (bundle ids are short).
    let mut buf = [0_i8; 512];
    // SAFETY: `buf` is a valid writable buffer of `buf.len()` bytes; the
    // function NUL-terminates within that bound on success. Returns a CF
    // `Boolean` (u8): non-zero means the whole string was written.
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
    // SAFETY: on success the buffer holds a NUL-terminated C string.
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr().cast::<c_char>()) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Return this app's bundle identifier (e.g. `dev.zerx.fluxdown`).
pub fn main_bundle_id() -> Option<String> {
    // SAFETY: `CFBundleGetMainBundle` returns a borrowed (non-owned) ref or
    // null; `CFBundleGetIdentifier` likewise returns a borrowed ref — neither
    // is released here.
    let id = unsafe {
        let bundle = CFBundleGetMainBundle();
        if bundle.is_null() {
            return None;
        }
        CFBundleGetIdentifier(bundle)
    };
    cf_to_string(id)
}
