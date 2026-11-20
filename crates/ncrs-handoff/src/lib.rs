//! Other programs hand a file or folder to ncrs: `ncrs <path>`, a second start
//! while ncrs runs, the Finder's "Open With", the `FileManager1` D-Bus service
//! of Linux. This crate holds what is the same for all of them: the request,
//! the inbox a second start leaves it in for the running one, and the system
//! settings that make ncrs the opener of folders.
//!
//! The inbox is a folder of small files next to the instance lock, not a
//! socket: it needs no port (the App Store build may not listen on one), no
//! named pipe on Windows and no path short enough for a socket address.

#[cfg(target_os = "linux")]
mod dbus;
mod inbox;
pub mod integration;
mod request;

#[cfg(target_os = "linux")]
pub use dbus::serve_file_manager;
pub use inbox::{discard_stale, send, take};
pub use request::{path_from_argument, requests_from_uris, Request, Target};
