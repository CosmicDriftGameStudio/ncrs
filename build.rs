//! On x86_64 Linux the local model's libraries (libllama.so and the ggml ones,
//! see crates/ncrs-ai) are shipped next to the executable. The binary has no
//! other way to find them than a run path relative to itself; the dynamic
//! loader resolves $ORIGIN from /proc/self/exe, so a symlink to the binary
//! (/usr/bin/ncrs) works too. Only the binaries get it, not the test builds,
//! which cargo points at the build folder.
//!
//! The condition is the one of `dynamic_backends` in crates/ncrs-ai/build.rs.
//!
//! On Windows the icon (assets/ncrs.ico) and the version info (the Details tab
//! of the file's properties) are compiled into ncrs.exe.
//!
//! `NCRS_DISPLAY_VERSION` is the version the status bar shows: the plain
//! version for a build of its release tag or outside git, otherwise
//! `<version>-dev+<short commit>`.

use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=NCRS_DISPLAY_VERSION={}", display_version());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_windows_resources();
    }
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

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn display_version() -> String {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // The HEAD reflog changes with every commit and checkout, the tag refs
    // when a built commit is tagged, so the version follows both without
    // rebuilding on every file change. A path that does not exist would make
    // cargo rerun this script on every build.
    for git_path in ["logs/HEAD", "refs/tags", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", git_path]) {
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    let release_tag = format!("v{version}");
    let tags_at_head = git(&["tag", "--points-at", "HEAD"]).unwrap_or_default();
    if tags_at_head.lines().any(|tag| tag == release_tag) {
        return version;
    }
    match git(&["rev-parse", "--short", "HEAD"]) {
        Some(commit) => format!("{version}-dev+{commit}"),
        None => version,
    }
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

// A build script reports a failure by panicking.
#[allow(clippy::expect_used)]
#[cfg(windows)]
fn embed_windows_resources() {
    use std::fmt::Write as _;
    use std::path::PathBuf;

    println!("cargo:rerun-if-changed=assets/ncrs.ico");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo"));
    // The resource compiler reads backslashes in a string as escapes.
    let icon = manifest_dir
        .join("assets")
        .join("ncrs.ico")
        .display()
        .to_string()
        .replace('\\', "/");
    let part = |name: &str| env::var(name).unwrap_or_else(|_| "0".to_owned());
    let (major, minor, patch) = (
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH"),
    );
    let version = env::var("CARGO_PKG_VERSION").expect("set by cargo");

    let mut rc = String::new();
    writeln!(rc, "1 ICON \"{icon}\"").expect("writing to a String");
    // 0x0409 = English (US), 0x04B0 = Unicode.
    writeln!(
        rc,
        "1 VERSIONINFO\n\
         FILEVERSION {major},{minor},{patch},0\n\
         PRODUCTVERSION {major},{minor},{patch},0\n\
         FILEOS 0x4\n\
         FILETYPE 0x1\n\
         BEGIN\n\
         BLOCK \"StringFileInfo\"\n\
         BEGIN\n\
         BLOCK \"040904B0\"\n\
         BEGIN\n\
         VALUE \"CompanyName\", \"CosmicDriftGameStudio\"\n\
         VALUE \"FileDescription\", \"NC-rs, a dual-panel file manager\"\n\
         VALUE \"FileVersion\", \"{version}\"\n\
         VALUE \"InternalName\", \"ncrs\"\n\
         VALUE \"LegalCopyright\", \"MIT License\"\n\
         VALUE \"OriginalFilename\", \"ncrs.exe\"\n\
         VALUE \"ProductName\", \"NC-rs\"\n\
         VALUE \"ProductVersion\", \"{version}\"\n\
         END\n\
         END\n\
         BLOCK \"VarFileInfo\"\n\
         BEGIN\n\
         VALUE \"Translation\", 0x0409, 0x04B0\n\
         END\n\
         END"
    )
    .expect("writing to a String");

    let rc_path = out_dir.join("ncrs.rc");
    std::fs::write(&rc_path, rc).expect("the resource script is written to OUT_DIR");
    embed_resource::compile(rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("the icon and version info are embedded");
}
