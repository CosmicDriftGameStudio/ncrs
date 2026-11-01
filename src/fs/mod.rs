//! Filesystem layer. The code lives in `ncrs-fs`; this keeps the old path.

#[cfg(test)]
#[cfg(target_os = "macos")]
mod sandbox_probe_tests;
pub mod search;

pub use ncrs_fs::*;
