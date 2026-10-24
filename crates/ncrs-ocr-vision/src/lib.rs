//! Text recognition with Apple Vision. Only exists on macOS; elsewhere the
//! crate is empty.
//!
//! This is the one crate of the workspace that may contain unsafe code, and
//! only in the few bindings that the Vision headers do not mark safe.

#[cfg(target_os = "macos")]
mod vision;

#[cfg(target_os = "macos")]
pub use vision::VisionText;
