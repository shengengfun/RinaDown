//! Embed Windows PE version information into `fluxdown_nmh.exe`.
//!
//! A binary with a fully populated `VS_VERSION_INFO` resource (company,
//! product, description, version) looks far less suspicious to antivirus
//! heuristic engines than one with a blank version block. Everything here is
//! a no-op on non-Windows targets.

fn main() {
    #[cfg(windows)]
    embed_version_info();
}

/// Populate the standard `StringFileInfo` fields consumed by Explorer's
/// "Details" tab and by AV reputation heuristics.
#[cfg(windows)]
fn embed_version_info() {
    let mut res = winresource::WindowsResource::new();
    // Icon shared with the main application (path relative to this crate).
    res.set_icon("../../windows/runner/resources/app_icon.ico");
    res.set("CompanyName", "FluxDown");
    res.set("ProductName", "FluxDown");
    res.set(
        "FileDescription",
        "FluxDown Native Messaging Host (browser bridge)",
    );
    res.set("InternalName", "fluxdown_nmh");
    res.set("OriginalFilename", "fluxdown_nmh.exe");
    res.set(
        "LegalCopyright",
        "Copyright (C) 2026 FluxDown. All rights reserved.",
    );
    res.set("FileVersion", env!("CARGO_PKG_VERSION"));
    res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
    if let Err(e) = res.compile() {
        // Don't fail the build on resource-compiler issues; just warn so the
        // binary still links (only the version block is missing).
        println!("cargo:warning=fluxdown_nmh version resource failed: {e}");
    }
}
