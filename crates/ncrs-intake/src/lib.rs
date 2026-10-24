//! Takes documents in from scan locations: a backlog scan at start, then a
//! file watcher. Nothing in this crate ever writes to a location; the only
//! writes go to the index.

mod classify;
mod error;
mod location;
mod scan;
mod stability;
mod take_in;
mod text;
mod watcher;

pub use classify::{classify, Skip};
pub use error::IntakeError;
pub use location::{Location, LocationMode};
pub use scan::{
    scan_location, settle_location, walk_location, Found, ScanReport, MAX_REPORTED_ERRORS,
};
pub use stability::{StabilityTracker, STABLE_AFTER};
pub use take_in::{take_in, Outcome};
pub use text::{
    pdfium_library_dir, Extracted, ImageTextSource, PageImage, PdfText, PdfUnavailable, TextChain,
    TextError, TextSource,
};
pub use watcher::{LocationWatcher, WatchEvent};
