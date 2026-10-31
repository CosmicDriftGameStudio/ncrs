//! Takes documents in from scan locations: a backlog scan at start, then a
//! file watcher. Nothing in this crate ever writes to a location; the only
//! writes go to the index.

mod archive_profile;
mod classify;
mod conditions;
mod default_pattern;
mod error;
mod extract;
mod filing_plan;
mod location;
mod ocr;
mod office;
mod recipient_detection;
#[cfg(any(test, feature = "test-support"))]
pub mod recognition;
mod reorganisation;
mod reorganisation_plan;
mod rule_suggestions;
mod sanitise;
mod scan;
mod sender_detection;
mod stability;
mod take_in;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod text;
mod watcher;

pub use archive_profile::{
    learn_archive, pattern_per_document, propose_target, subtree_per_document, ArchiveDocument,
    DocumentPattern,
};
pub use classify::{classify, is_image_file, text_is_readable, Skip};
pub use conditions::{condition_match_ranges, word_condition_holds, MAX_MATCH_RANGES};
pub use default_pattern::{
    choose_pattern, default_pattern_for, pattern_without_tax_context, upgraded_pattern,
    ChosenPattern, PatternSource, DEFAULT_PATTERNS, FALLBACK_PATTERN,
};
pub use error::IntakeError;
pub use extract::{
    extract_amount, extract_date, extract_number, parse_amount, parse_currency, parse_date,
};
pub use filing_plan::{
    contains_condition_holds, missing_field, pattern_is_valid, plan, render_pattern,
    render_pattern_parts, unknown_placeholder, FilingPlan, PatternPart, PlanInput, Planning,
    AI_FIELD_ORDER, FIELD_ORDER, FILING_PLACEHOLDERS,
};
pub use location::{Location, LocationMode};
pub use ocr::{
    install_model, verify_model_bytes, LazyOcrText, OcrModelError, OcrModelFile, OcrText,
    OCR_MODELS_FOLDER, OCR_MODELS_HOST, OCR_MODELS_PROJECT, OCR_MODEL_FILES,
};
pub use office::{extract_text_from_bytes, OfficeText, PlainText};
pub use recipient_detection::{detect_recipient, kind_from_heading};
pub use reorganisation::{
    plan_reorganisation, PlanFile, PlanRule, ReorganisationInput, DUPLICATES_FOLDER,
    FINDING_PRIORITY,
};
pub use reorganisation_plan::plan_location;
pub use rule_suggestions::{
    suggest_rules, ArchiveText, RuleCandidate, MAX_CANDIDATES_PER_FOLDER, MAX_PHRASE_WORDS,
    MAX_SHARE_ELSEWHERE_PERCENT, MIN_FOLDER_DOCUMENTS, MIN_PHRASE_CHARS,
    MIN_SHARE_IN_FOLDER_PERCENT,
};
pub use sanitise::{is_plain_literal, sanitise_value, stays_below};
pub use scan::{
    scan_location, settle_location, walk_location, Found, ScanReport, MAX_REPORTED_ERRORS,
};
pub use sender_detection::{
    detect_sender, AliasOffer, DetectedSender, OwnIdentifiers, SenderDetection, SenderSource,
};
pub use stability::{StabilityTracker, STABLE_AFTER};
pub use take_in::{
    has_symlink_ancestor_under, hash_path, open_without_following, take_in, Outcome,
};
pub use text::{
    pdfium_library_candidates, pdfium_library_dir, Extracted, ImageTextSource,
    ImageTextWithFallback, PageImage, PdfText, PdfUnavailable, PreviewPage, TextChain, TextError,
    TextSource,
};
pub use watcher::{LocationWatcher, WatchEvent};
