//! The filing module: opens the document index, asks the questions that need
//! the user (encrypted or not, the recovery key) and hands the index to the
//! intake only when nothing is left open.
//!
//! The index reaches the intake only through `FilingModule::start_pipeline`.
//! The handle it gives the runner is issued only in the `Ready` phase. `Ready`
//! is left again by encrypting the index and by starting over, and a handle
//! checks the phase only when it is issued: afterwards it may find no index
//! (`INDEX_NOT_OPEN`) or a new one, and the document ids it holds are then not
//! stable. The caller stops the pipeline first and drops what it holds. The
//! slot may already hold an index in other phases (a freshly created one whose
//! recovery key has not been confirmed, a plain file nobody has accepted yet),
//! and those are held back by the phase, not by the slot being empty.
//!
//! Everything that touches the disk, the credential store or a mount runs in a
//! `Work`, on a blocking thread. `update` itself only moves between phases.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher as _, Hasher as _};
#[cfg(feature = "local-ai")]
use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::thread;

use ncrs_core::{NoteLink, INDEX_NOT_OPEN};
use ncrs_index::{
    ContentHash, Document, HermesCounts, Index, IndexError, IndexStatistics, KeyStore, KindCatalog,
    KindLanguage, RecoveryKey,
};
#[cfg(target_os = "macos")]
use ncrs_intake::ImageTextWithFallback;
use ncrs_intake::{
    ImageFileText, ImageTextSource, LazyOcrText, Location, OfficeText, PdfText, PdfUnavailable,
    PlainText, TextChain, TextError, OCR_MODELS_FOLDER,
};
use ncrs_pipeline::{intake_steps, IndexAccess, Load, OwnDetails, Runner, Settings, SystemClock};

use crate::chat_work::ChatAccess;
use crate::column_reading::{read_column, ColumnReading, ColumnRequest};
use crate::filing_work::FilingAccess;
use crate::query_work::QueryAccess;
use crate::review_work::ReviewAccess;
use crate::rules_work::RulesAccess;
use crate::tags_work::TagsAccess;
use ncrs_config::config::{AiSetting, Filing, FilingLocation, LocationTransfer};

/// A recovery key is 13 groups of four characters.
const GROUP_COUNT: usize = 13;

/// The reason of the `Failed` phase when the system names no data directory.
pub const NO_DATA_DIRECTORY: &str = "no data directory";

type SharedStore = Arc<dyn KeyStore + Send + Sync>;
type IndexSlot = Arc<Mutex<Option<Index>>>;
/// Why settling the interrupted runs of the last opened index failed, if it did.
type SettleNote = Arc<Mutex<Option<String>>>;

use crate::index_slot::try_lock;
#[cfg(feature = "local-ai")]
pub use ai::shared_local_model;
pub use ai::AiStatus;
pub use ai::{chat_model, http_ai_provider};
use ai::{wire_ai, AiWiring};
use index_ops::{
    apply_key, create_index, encrypt_index, open_existing, settle_held, start_over,
    unconfirmed_key, INDEX_NOT_REOPENED,
};
pub use locations::lies_within;
pub use locations::{resolve_locations, LocationStatus, ResolvedLocation};
pub use messages::{FilingMessage, OpenOutcome, WorkOutcome};
pub use readers::{
    ColumnReader, DocumentFinder, FoundDocument, HermesCountsReader, IndexFigures,
    IndexFiguresReader, NoteLinkFinder, TextSource,
};
use recovery_marker::{clear_unconfirmed, is_unconfirmed, mark_unconfirmed};
pub use secrets::{SecretText, ShownRecoveryKey};
use state::{next_generation, own_details};
pub use state::{HeldReason, KeyError, KeyProblem, Phase, ProtectionNote};
use text_chain::shipped_text_chain;
pub use text_chain::{ai_models_dir, ocr_models_dir};
use update::random_group;
pub use work::Work;
use work::{lock, Job};

mod access;
mod ai;
mod card;
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
mod index_ops;
mod locations;
mod messages;
mod pipeline;
mod readers;
mod recovery_marker;
mod secrets;
mod start;
mod state;
#[cfg(test)]
mod tests;
mod text_chain;
mod update;
mod work;

/// How `Job::Encrypt` ended, and what the index looks like now. `Ok` means the
/// file is encrypted, even when `opened` says it could not be opened again.
#[derive(Debug, Clone, PartialEq)]
pub struct EncryptReport {
    result: Result<ShownRecoveryKey, String>,
    opened: OpenOutcome,
}

/// How `Job::StartOver` ended, and what the index looks like now.
#[derive(Debug, Clone, PartialEq)]
pub struct StartOverReport {
    result: Result<(), String>,
    key_entry_warning: Option<String>,
    opened: OpenOutcome,
}

pub struct FilingModule {
    db_path: PathBuf,
    store: SharedStore,
    index: IndexSlot,
    locations: Vec<ResolvedLocation>,
    phase: Phase,
    load: Load,
    /// Changes with every new `Work` and with a dismissal: a result of an older
    /// generation is ignored. Unique across modules, so a module that replaces
    /// a dismissed one never takes its leftover result for its own.
    generation: u64,
    /// Which of the groups to type back, given their count. Random in the app,
    /// fixed in tests.
    pick_group: fn(usize) -> usize,
    /// Whether the config listed any location; `locations` stays empty until
    /// the start has resolved them.
    has_configured_locations: bool,
    /// Known once an index is open.
    index_encrypted: Option<bool>,
    /// The AI provider the setting asked for. Built once, so the status page
    /// and the pipeline see the same model.
    ai: AiWiring,
    /// Given to the pipeline so it calls no platform API.
    own: OwnDetails,
    /// False once the start found the credential store unreachable, so an
    /// index that is not encrypted cannot be encrypted here.
    key_store_reachable: bool,
    protection_note: Option<ProtectionNote>,
    /// Set when the index could not be opened again after its encryption: the
    /// recovery key is shown first, then the card for this outcome.
    after_recovery_key: Option<OpenOutcome>,
    /// What the card said when "Not now" left it, so the overview can still
    /// offer the way back to it.
    dismissed_key_problem: Option<KeyProblem>,
    /// The document kinds the config asks for; every index of this module
    /// gets them as soon as it is open.
    kinds: Arc<KindCatalog>,
    /// Built when the chat first needs it; the models are only loaded when a
    /// scan has to be read.
    chat_text_chain: Arc<OnceLock<Arc<TextChain>>>,
    /// The chain the pipeline reads with, kept so closing the app can clear
    /// its crash marker while a reading still holds the chain.
    pipeline_text_chain: Mutex<Option<Arc<TextChain>>>,
    /// What the last settling of an interrupted run could not do.
    settle_problem: SettleNote,
    /// The chat, the review list and the filing tab count their work here.
    index_work: crate::index_work::IndexWork,
    /// What the review list and the quick fix built from the archive.
    neighbour_cache: crate::review_work::NeighbourCache,
    /// The plain file was confirmed and what a crash left is being settled;
    /// the module is ready when that is done.
    settling: bool,
}

impl fmt::Debug for FilingModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilingModule")
            .field("phase", &self.phase.name())
            .field("db_path", &self.db_path)
            .field("locations", &self.locations.len())
            .finish_non_exhaustive()
    }
}
