//! Build script: lets the audio backend link on Linux systems without ALSA's development
//! package.
//!
//! quad-snd links `-lasound`, which needs the development symlink `libasound.so`. Many
//! desktops only have the runtime library `libasound.so.2`; then a symlink to it is made in
//! `OUT_DIR` (inside `target/`) and added to the link search path. Nothing outside `target/`
//! is touched.

use std::path::{Path, PathBuf};

const LIB_DIRS: &[&str] = &[
    "/usr/lib64",
    "/usr/lib",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/lib64",
    "/lib/x86_64-linux-gnu",
    "/lib/aarch64-linux-gnu",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let audio = std::env::var_os("CARGO_FEATURE_AUDIO").is_some();
    let linux = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux");
    if !audio || !linux {
        return;
    }
    if LIB_DIRS.iter().any(|d| Path::new(d).join("libasound.so").exists()) {
        return;
    }
    let Some(runtime) = LIB_DIRS.iter().map(|d| Path::new(d).join("libasound.so.2")).find(|p| p.exists()) else {
        println!("cargo:warning=libasound not found: install ALSA (alsa-lib) or build with --no-default-features");
        return;
    };
    let dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("alsa-link");
    let link = dir.join("libasound.so");
    if std::fs::create_dir_all(&dir).is_ok() && (link.exists() || symlink(&runtime, &link)) {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(not(unix))]
fn symlink(_: &Path, _: &Path) -> bool {
    false
}
