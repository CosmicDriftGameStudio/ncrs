//! llama.cpp's Metal code checks the macOS version with `@available`. Below
//! the SDK version that compiles to a call of `__isPlatformVersionAtLeast` in
//! clang's runtime library, and rustc links with `-nodefaultlibs`, so the
//! library is named here. Without it a build with a deployment target (as in
//! the release builds) fails to link.
//!
//! It also names the one condition under which llama.cpp is built with
//! dynamic backends (`dynamic_backends`), so the code does not repeat it.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(dynamic_backends)");
    let with_llama = env::var_os("CARGO_FEATURE_LLAMA").is_some();
    // The target-specific dependency block of Cargo.toml has the same condition.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let vulkan_target = target_os == "windows" || (target_os == "linux" && target_arch == "x86_64");
    if with_llama && vulkan_target {
        println!("cargo:rustc-cfg=dynamic_backends");
    }
    let for_apple = env::var("CARGO_CFG_TARGET_VENDOR").is_ok_and(|vendor| vendor == "apple");
    if !(with_llama && for_apple) {
        return;
    }
    match clang_resource_dir() {
        Some(resource_dir) => {
            println!(
                "cargo:rustc-link-search=native={}",
                resource_dir.join("lib").join("darwin").display()
            );
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
        None => println!(
            "cargo:warning=clang's resource folder was not found; linking llama.cpp may miss __isPlatformVersionAtLeast"
        ),
    }
}

fn clang_resource_dir() -> Option<PathBuf> {
    let output = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let printed = String::from_utf8(output.stdout).ok()?;
    let trimmed = printed.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}
