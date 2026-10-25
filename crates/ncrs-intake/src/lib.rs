//! Takes documents in from scan locations: a backlog scan at start, then a
//! file watcher. Nothing in this crate ever writes to a location; the only
//! writes go to the index.

mod archive_profile;
mod classify;
mod error;
mod extract;
mod filing_plan;
mod location;
mod ocr;
mod office;
mod sanitise;
mod scan;
mod stability;
mod take_in;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod text;
mod watcher;

pub use archive_profile::{learn_archive, propose_target, ArchiveDocument};
pub use classify::{classify, Skip};
pub use error::IntakeError;
pub use extract::{
    extract_amount, extract_date, extract_number, parse_amount, parse_currency, parse_date,
};
pub use filing_plan::{plan, FilingPlan, PlanInput, Planning, FIELD_ORDER};
pub use location::{Location, LocationMode};
pub use ocr::{
    install_model, verify_model_bytes, LazyOcrText, OcrModelError, OcrModelFile, OcrText,
    OCR_MODELS_FOLDER, OCR_MODELS_HOST, OCR_MODELS_PROJECT, OCR_MODEL_FILES,
};
pub use office::{OfficeText, PlainText};
pub use sanitise::{sanitise_value, stays_below};
pub use scan::{
    scan_location, settle_location, walk_location, Found, ScanReport, MAX_REPORTED_ERRORS,
};
pub use stability::{StabilityTracker, STABLE_AFTER};
pub use take_in::{take_in, Outcome};
pub use text::{
    pdfium_library_dir, Extracted, ImageTextSource, ImageTextWithFallback, PageImage, PdfText,
    PdfUnavailable, TextChain, TextError, TextSource,
};
pub use watcher::{LocationWatcher, WatchEvent};
