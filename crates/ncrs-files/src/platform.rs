//! The systems whose path forms and folders differ, as a value, so the code
//! that depends on them can be tested for every system on any one.

/// The family of the system a path or a folder belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Windows,
    /// Linux and the other systems with Unix paths.
    Unix,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}
