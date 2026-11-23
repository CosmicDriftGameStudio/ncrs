//! Compiles swift/StoreBridge.swift into a static library and links it, but
//! only for the StoreKit build on macOS. Everything else compiles nothing.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_MIN_MACOS: &str = "13.0";
const BRIDGE_LIBRARY: &str = "ncrs_store_bridge";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=swift/StoreBridge.swift");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if env::var_os("CARGO_FEATURE_STOREKIT").is_none() {
        return;
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    if let Err(message) = build_bridge() {
        println!("cargo:warning=ncrs-store: {message}");
        std::process::exit(1);
    }
}

fn build_bridge() -> Result<(), String> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|error| error.to_string())?);
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => return Err(format!("unsupported macOS architecture {other:?}")),
    };
    let min_macos =
        env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| DEFAULT_MIN_MACOS.to_owned());
    let sdk = PathBuf::from(xcrun(&["--sdk", "macosx", "--show-sdk-path"])?);
    let swiftc = PathBuf::from(xcrun(&["--find", "swiftc"])?);
    // <toolchain>/usr/bin/swiftc: the toolchain's Swift libraries sit beside bin.
    let toolchain_lib = swiftc
        .parent()
        .and_then(Path::parent)
        .map(|usr| usr.join("lib/swift/macosx"))
        .ok_or_else(|| format!("unexpected swiftc path {}", swiftc.display()))?;

    let library = out_dir.join(format!("lib{BRIDGE_LIBRARY}.a"));
    let output = Command::new("xcrun")
        .args([
            "swiftc",
            "-emit-library",
            "-static",
            "-parse-as-library",
            "-O",
        ])
        .args(["-swift-version", "5", "-module-name", "NcrsStoreBridge"])
        .arg("-target")
        .arg(format!("{arch}-apple-macos{min_macos}"))
        .arg("-sdk")
        .arg(&sdk)
        .arg("-o")
        .arg(&library)
        .arg("swift/StoreBridge.swift")
        .output()
        .map_err(|error| format!("could not run xcrun swiftc: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "swiftc failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static={BRIDGE_LIBRARY}");
    println!(
        "cargo:rustc-link-search=native={}",
        sdk.join("usr/lib/swift").display()
    );
    println!("cargo:rustc-link-search=native={}", toolchain_lib.display());
    println!("cargo:rustc-link-lib=framework=StoreKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
    Ok(())
}

fn xcrun(args: &[&str]) -> Result<String, String> {
    let output = Command::new("xcrun")
        .args(args)
        .output()
        .map_err(|error| format!("could not run xcrun: {error}"))?;
    if !output.status.success() {
        return Err(format!("xcrun {args:?} failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
