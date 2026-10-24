//! Points the linker at a local libmpv on Windows and copies its DLL next to the binaries.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = mpv_lib_dir();
    println!("cargo:rustc-link-search=native={}", dir.display());
    copy_dll(&dir);
}

fn mpv_lib_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MPV_LIB_DIR") {
        return dir.into();
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    manifest.join("../../vendor/libmpv/x64")
}

fn copy_dll(dir: &std::path::Path) {
    let dll = dir.join("libmpv-2.dll");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let Some(profile_dir) = out.ancestors().nth(3) else { return };
    if dll.exists() {
        let _ = std::fs::copy(&dll, profile_dir.join("libmpv-2.dll"));
        let _ = std::fs::copy(&dll, profile_dir.join("examples").join("libmpv-2.dll"));
    }
}
