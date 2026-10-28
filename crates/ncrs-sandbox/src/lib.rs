//! What the ncrs App Store build needs from macOS and the standard library does
//! not offer. Inside the App Sandbox `$HOME` points into the app's container,
//! so the real home comes from the user database, and a folder the user allowed
//! in the open panel stays reachable after a relaunch only through a
//! security-scoped bookmark.
//!
//! macOS only: on other systems this crate is empty, and the app calls it only
//! where it is built for the store.

#[cfg(target_os = "macos")]
mod bookmark;
#[cfg(target_os = "macos")]
mod home;

#[cfg(target_os = "macos")]
pub use bookmark::{bookmark_path, create_bookmark, resolve_bookmark, BookmarkError, Resolved};
#[cfg(target_os = "macos")]
pub use home::real_home;
