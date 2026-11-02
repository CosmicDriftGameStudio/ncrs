//! On x86_64 Linux the local model's libraries (libllama.so and the ggml ones,
//! see crates/ncrs-ai) are shipped next to the executable. The binary has no
//! other way to find them than a run path relative to itself; the dynamic
//! loader resolves $ORIGIN from /proc/self/exe, so a symlink to the binary
//! (/usr/bin/ncrs) works too. Only the binaries get it, not the test builds,
//! which cargo points at the build folder.
//!
//! The condition is the one of `dynamic_backends` in crates/ncrs-ai/build.rs.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let with_local_ai = env::var_os("CARGO_FEATURE_LOCAL_AI").is_some();
    if with_local_ai && target_os == "linux" && target_arch == "x86_64" {
        // RPATH, not RUNPATH: glibc applies RUNPATH only to the binary's own
        // NEEDED, and the bundled llama.cpp libraries carry none of their own,
        // so their dependencies must also resolve next to the binary. Explicit
        // because some linkers default to new dtags.
        println!("cargo:rustc-link-arg-bins=-Wl,--disable-new-dtags");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
