//! Takes documents in from scan locations: a backlog scan at start, then a
//! file watcher. Nothing in this crate ever writes to a location; the only
//! writes go to the index.

mod archive_profile;
mod classify;
mod conditions;
mod date_detection;
mod duplicate_copies;
mod error;
mod extract;
mod field_detection;
mod filing_plan;
mod image_text;
mod location;
mod location_documents;
mod ocr;
mod office;
mod page_budget;
mod page_trim;
mod pattern_words;
mod pdf_text;
mod plain_words;
mod reading_marker;
mod recipient_detection;
#[cfg(any(test, feature = "test-support"))]
pub mod recognition;
mod rule_check;
mod sanitise;
mod scan;
mod sender_detection;
mod sorting;
mod stability;
mod structure;
mod take_in;
#[cfg(any(test, feature = "test-support"))]
pub mod test_archive;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod text;
mod watcher;

pub use archive_profile::{date_in_file_stem, learn_archive, ArchiveDocument};
pub use classify::{classify, is_eml_file, is_image_file, is_mail_body, text_is_readable, Skip};
pub use conditions::word_condition_holds;
pub use date_detection::detect_letter_date;
pub use duplicate_copies::{duplicate_copies, DuplicateCopy, PlanFile};
pub use error::IntakeError;
pub use extract::{
    extract_amount, extract_date, extract_number, parse_amount, parse_currency, parse_date,
};
pub use field_detection::{detect_fields, DetectedFields, OwnNames, FIELDS_READ_VERSION};
pub use filing_plan::{
    contains_condition_holds, has_lost_ending, pattern_is_valid, plan, render_pattern_in,
    unknown_placeholder, FilingPlan, PlanInput, Planning, AI_FIELD_ORDER, FIELD_ORDER,
    FILING_PLACEHOLDERS,
};
pub use image_text::ImageFileText;
pub use location::{canonical_path, location_of, without_verbatim_prefix, Location, LocationMode};
pub use location_documents::{location_documents, location_structure};
pub use ocr::{
    install_model, verify_model_bytes, LazyOcrText, OcrModelError, OcrModelFile, OcrText,
    OCR_MODELS_FOLDER, OCR_MODELS_HOST, OCR_MODELS_PROJECT, OCR_MODEL_FILES,
};
pub use office::{extract_text_from_bytes, OfficeText, PlainText};
pub use pattern_words::pattern_in_words;
pub use pdf_text::{pdfium_library_candidates, pdfium_library_dir, PdfText, PdfUnavailable};
pub use plain_words::{branch_plain, label_text, levels_plain};
pub use recipient_detection::{detect_recipient, kind_from_heading};
pub use rule_check::{
    condition_problem, pattern_problem, rule_problem, ConditionProblem, PatternProblem, RuleProblem,
};
pub use sanitise::{is_plain_literal, sanitise_value, stays_below};
pub use scan::{
    scan_location, settle_gone_path, settle_location, walk_location, Found, ScanReport,
    MAX_REPORTED_ERRORS,
};
pub use sender_detection::{detect_sender, DetectedSender, OwnIdentifiers, SenderSource};
pub use sorting::{
    decide, file_name_with_extension, Decision, LocationTransfer, OnNoText, OnSeveralTargets,
    Place, SortSource, SortTarget, Subject, TargetRules, TextState, Words,
};
pub use stability::{StabilityTracker, STABLE_AFTER};
pub use structure::{
    folder_label, learn_structure, Area, Branch, DateFormat, Level, LocationStructure, NamePart,
    NameScheme, Placeholder, PlaceholderKind, PlaceholderLabel, StructureDocument, UnorderedFolder,
};
pub use take_in::{
    finish_take_in, has_symlink_ancestor_under, hash_path, open_below, open_without_following,
    prepare_take_in, take_in, Outcome, PendingHash, Prepared, Taken,
};
pub use text::{
    ChainReading, Extracted, ImageTextSource, ImageTextWithFallback, PageImage, TextChain,
    TextError, TextSource,
};
pub use watcher::{LocationWatcher, WatchEvent};
