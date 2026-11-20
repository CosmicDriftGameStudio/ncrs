//! Pixel comparisons for tests, shared by every crate that renders headless.
//!
//! The reference images were made on macOS and font rasterisation differs per
//! operating system, so a comparison elsewhere would assert nothing true. The
//! rest of such a test (clicks, keys, state) still runs everywhere; only the
//! pixel verdict is left to macOS.

use std::path::Path;

use iced_test::simulator::Snapshot;
use iced_test::Error;

/// Whether the pixel references can be compared on this operating system.
pub const REFERENCES_COMPARABLE: bool = cfg!(target_os = "macos");

pub trait ReferenceSnapshot {
    /// Like `matches_image`, but only compares where the references were made;
    /// elsewhere it reports a match without looking.
    fn matches_reference_image(&self, path: impl AsRef<Path>) -> Result<bool, Error>;

    /// The same for `matches_hash`.
    fn matches_reference_hash(&self, path: impl AsRef<Path>) -> Result<bool, Error>;
}

impl ReferenceSnapshot for Snapshot {
    fn matches_reference_image(&self, path: impl AsRef<Path>) -> Result<bool, Error> {
        if REFERENCES_COMPARABLE {
            self.matches_image(path)
        } else {
            Ok(true)
        }
    }

    fn matches_reference_hash(&self, path: impl AsRef<Path>) -> Result<bool, Error> {
        if REFERENCES_COMPARABLE {
            self.matches_hash(path)
        } else {
            Ok(true)
        }
    }
}
