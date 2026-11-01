//! The filing module: opens the document index, asks the questions that need
//! the user (encrypted or not, the recovery key) and hands the index to the
//! intake only when nothing is left open.
//!
//! The index reaches the intake only through `FilingModule::start_pipeline`.
//! The handle it gives the runner is issued only in the `Ready` phase, and
//! `Ready` is terminal, so the guarantee holds for the runner's lifetime. The
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
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread;

use ncrs_index::{
    HermesCounts, Index, IndexError, IndexStatistics, KeyStore, KindCatalog, KindLanguage,
    RecoveryKey,
};
#[cfg(target_os = "macos")]
use ncrs_intake::ImageTextWithFallback;
use ncrs_intake::{
    ImageTextSource, LazyOcrText, Location, LocationMode, OfficeText, PdfText, PdfUnavailable,
    PlainText, TextChain, TextError, OCR_MODELS_FOLDER,
};
use ncrs_pipeline::{intake_steps, IndexAccess, Load, OwnDetails, Runner, Settings, SystemClock};

use crate::config::{AiSetting, Filing, FilingLocation};
use crate::filing_work::FilingAccess;
use crate::hermes_column::{read_column, ColumnReading, ColumnRequest};
use crate::query_work::QueryAccess;
use crate::reorganisation_work::PlanAccess;
use crate::replan_work::ReplanAccess;
use crate::review_work::ReviewAccess;
use crate::rules_work::RulesAccess;
use crate::suggestion_work::SuggestionAccess;
use crate::tags_work::TagsAccess;

/// A recovery key is 13 groups of four characters.
const GROUP_COUNT: usize = 13;

/// The reason of the `Failed` phase when the system names no data directory.
pub const NO_DATA_DIRECTORY: &str = "no data directory";

type SharedStore = Arc<dyn KeyStore + Send + Sync>;
type IndexSlot = Arc<Mutex<Option<Index>>>;

const INDEX_NOT_OPEN: &str = "index not open";

// ---------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLocation {
    pub configured: FilingLocation,
    pub status: LocationStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocationStatus {
    Ready(Location),
    NotReachable {
        reason: String,
    },
    /// Equal to or nested in another location, by file identity. Both
    /// members of the pair get this, so neither the target nor one spelling of
    /// a linked inbox is silently dropped.
    Overlaps {
        with: PathBuf,
    },
}

/// Looks every configured folder up on disk. Blocking: a dead mount can hang
/// `canonicalize`, so this runs in the start work and never on the UI thread.
pub fn resolve_locations(configured: &[FilingLocation]) -> Vec<ResolvedLocation> {
    let mut resolved: Vec<ResolvedLocation> = configured
        .iter()
        .map(|entry| ResolvedLocation {
            configured: entry.clone(),
            status: match Location::new(&entry.path, entry.mode, entry.images) {
                Ok(location) => LocationStatus::Ready(location),
                Err(err) => LocationStatus::NotReachable {
                    reason: err.to_string(),
                },
            },
        })
        .collect();

    let ready: Vec<(usize, &Path)> = resolved
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match &entry.status {
            LocationStatus::Ready(location) => Some((index, location.path.as_path())),
            LocationStatus::NotReachable { .. } | LocationStatus::Overlaps { .. } => None,
        })
        .collect();
    let mut overlapping_pairs = Vec::new();
    for (position, (first, first_path)) in ready.iter().enumerate() {
        for (second, second_path) in ready.iter().skip(position + 1) {
            if paths_overlap(first_path, second_path) {
                overlapping_pairs.push((*first, *second));
            }
        }
    }

    for (first, second) in overlapping_pairs {
        mark_overlap(&mut resolved, first, second);
        mark_overlap(&mut resolved, second, first);
    }
    resolved
}

fn mark_overlap(resolved: &mut [ResolvedLocation], index: usize, other: usize) {
    let Some(other_path) = resolved
        .get(other)
        .map(|entry| entry.configured.path.clone())
    else {
        return;
    };
    if let Some(entry) = resolved.get_mut(index) {
        if matches!(entry.status, LocationStatus::Ready(_)) {
            entry.status = LocationStatus::Overlaps { with: other_path };
        }
    }
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    lies_within(first, second) || lies_within(second, first)
}

/// `inner` is `outer` or below it. Compared by file identity, not by spelling:
/// case-insensitive file systems and symlinks name one folder in several ways.
fn lies_within(inner: &Path, outer: &Path) -> bool {
    inner
        .ancestors()
        .any(|ancestor| same_file::is_same_file(ancestor, outer).unwrap_or(false))
}

// ---------------------------------------------------------------------------
// Secrets held by the state
// ---------------------------------------------------------------------------

/// Text a user typed that may be, or may contain, key material.
#[derive(Clone, PartialEq, Default)]
pub struct SecretText(String);

impl SecretText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SecretText {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl From<String> for SecretText {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

/// The new recovery key while its card is up. Cloned only as a handle, so the
/// key text exists nowhere but in the card.
#[derive(Clone)]
pub struct ShownRecoveryKey(Arc<RecoveryKey>);

impl ShownRecoveryKey {
    pub fn new(key: RecoveryKey) -> Self {
        Self(Arc::new(key))
    }

    /// Called by the card view and the copy button, nowhere else.
    pub fn display_text(&self) -> String {
        self.0.to_string()
    }
}

impl PartialEq for ShownRecoveryKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ShownRecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShownRecoveryKey(<redacted>)")
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProblem {
    /// The credential store has no key for the encrypted index.
    Missing,
    /// The stored key does not open the index.
    Mismatch,
}

/// Why the recovery key was not applied. Worded in the view.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyError {
    InvalidFormat,
    DoesNotOpen,
    ConfirmFirst,
    /// The `Display` of an `IndexError`, which carries no key material.
    Other(String),
}

#[derive(Debug, PartialEq)]
pub enum Phase {
    Starting,
    AskEncryption {
        encrypted: bool,
        busy: bool,
        error: Option<String>,
    },
    ShowRecoveryKey {
        key: ShownRecoveryKey,
        /// 0-based group the user has to type back.
        group: usize,
        typed: SecretText,
        mismatch: bool,
    },
    AskRecoveryKey {
        reason: KeyProblem,
        typed: SecretText,
        confirm_replace: bool,
        busy: bool,
        error: Option<KeyError>,
    },
    PlainFileWarning,
    Ready,
    /// The plain index is being turned into an encrypted one.
    Encrypting,
    /// The index files and the key are being removed.
    StartingOver,
    Failed(String),
    Dismissed,
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::AskEncryption { .. } => "AskEncryption",
            Self::ShowRecoveryKey { .. } => "ShowRecoveryKey",
            Self::AskRecoveryKey { .. } => "AskRecoveryKey",
            Self::PlainFileWarning => "PlainFileWarning",
            Self::Ready => "Ready",
            Self::Encrypting => "Encrypting",
            Self::StartingOver => "StartingOver",
            Self::Failed(_) => "Failed",
            Self::Dismissed => "Dismissed",
        }
    }

    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::AskEncryption { busy: true, .. }
                | Self::AskRecoveryKey { busy: true, .. }
                | Self::Encrypting
                | Self::StartingOver
        )
    }
}

/// Why the last attempt to change the protection of the index did not go
/// through, worded in the view. Never carries key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionNote {
    EncryptFailed(String),
    StartOverFailed(String),
    /// The index is gone; its entry in the credential store could not be removed.
    KeyEntryNotRemoved(String),
}

/// Why the intake does not get the index right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldReason {
    Starting,
    WaitingForUser,
    PlainFileUnconfirmed,
    Dismissed,
    Failed,
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
    /// What the card said when "Not now" left it, so the overview can still
    /// offer the way back to it.
    dismissed_key_problem: Option<KeyProblem>,
    /// The document kinds the config asks for; every index of this module
    /// gets them as soon as it is open.
    kinds: Arc<KindCatalog>,
}

/// The name of the person using the app, as the system has it; `None` where
/// the system has none to give, as nothing else would be more than a guess.
#[cfg(target_os = "macos")]
fn own_full_name() -> Option<String> {
    let full_name = objc2_foundation::NSFullUserName().to_string();
    let trimmed = full_name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(not(target_os = "macos"))]
fn own_full_name() -> Option<String> {
    None
}

/// What is the user's own: the system's full name and what the config lists.
fn own_details(filing: &Filing) -> OwnDetails {
    OwnDetails {
        names: own_full_name()
            .into_iter()
            .chain(filing.own_names.iter().cloned())
            .collect(),
        ibans: filing.own_ibans.clone(),
        tax_ids: filing.own_tax_ids.clone(),
        kind_language: KindLanguage::default(),
    }
}

/// How much text of a document the local model takes in; longer ones are cut.
#[cfg(feature = "local-ai")]
const AI_CONTEXT_TOKENS: NonZeroU32 = match NonZeroU32::new(4096) {
    Some(tokens) => tokens,
    None => NonZeroU32::MIN,
};

#[derive(Clone)]
enum AiWiring {
    Off,
    #[cfg(feature = "local-ai")]
    Local(Arc<ncrs_ai::LazyLocalModel>),
    Http {
        provider: Arc<ncrs_ai::OpenAiCompatible>,
        origin: String,
        model: String,
    },
}

/// What the status page says about the AI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AiStatus {
    #[default]
    Off,
    NotDownloaded,
    // Only the local model reports these; the build without it never does.
    #[cfg_attr(not(feature = "local-ai"), allow(dead_code))]
    NotLoadedYet,
    #[cfg_attr(not(feature = "local-ai"), allow(dead_code))]
    Loaded {
        name: String,
        memory_bytes: u64,
    },
    #[cfg_attr(not(feature = "local-ai"), allow(dead_code))]
    Failed(&'static str),
    Http {
        origin: String,
        model: String,
    },
}

/// Builds nothing that reads a model or reaches a server: the local model is
/// loaded when the first document needs it, a remote endpoint is only parsed.
fn wire_ai(setting: &AiSetting, db_path: &Path) -> AiWiring {
    match setting {
        AiSetting::Off => AiWiring::Off,
        #[cfg(feature = "local-ai")]
        AiSetting::Local { threads } => {
            let folder = ai_models_dir(db_path)
                .unwrap_or_else(|| PathBuf::from(ncrs_ai::LOCAL_MODEL_FOLDER));
            AiWiring::Local(Arc::new(ncrs_ai::LazyLocalModel::new(
                folder.join(ncrs_ai::LOCAL_MODEL.name),
                ncrs_ai::LOCAL_MODEL,
                local_threads(*threads),
                AI_CONTEXT_TOKENS,
            )))
        }
        #[cfg(not(feature = "local-ai"))]
        AiSetting::Local { .. } => {
            let _ = db_path;
            AiWiring::Off
        }
        AiSetting::Http { endpoint, model } => {
            let config = ncrs_ai::OpenAiCompatibleConfig {
                endpoint: endpoint.clone(),
                model: model.clone(),
                api_key: None,
            };
            // No way to ask for consent yet: a far endpoint is refused at the
            // first document, a local one needs none.
            let ledger = Arc::new(ncrs_ai::NoConsentGiven);
            match (
                ncrs_ai::OpenAiCompatible::new(config, ledger),
                ncrs_ai::endpoint_origin(endpoint),
            ) {
                (Ok(provider), Ok(origin)) => AiWiring::Http {
                    provider: Arc::new(provider),
                    origin,
                    model: model.clone(),
                },
                (Err(_), _) | (_, Err(_)) => AiWiring::Off,
            }
        }
    }
}

/// The configured count, or half the logical cores: the model should not take
/// the whole machine from the person using it.
#[cfg(feature = "local-ai")]
fn local_threads(configured: Option<NonZeroU32>) -> NonZeroU32 {
    configured.unwrap_or_else(|| {
        let cores = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        u32::try_from(cores.div_euclid(2))
            .ok()
            .and_then(NonZeroU32::new)
            .unwrap_or(NonZeroU32::MIN)
    })
}

fn next_generation() -> u64 {
    static LAST_GENERATION: AtomicU64 = AtomicU64::new(0);
    LAST_GENERATION
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
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

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum FilingMessage {
    ChooseEncrypted(bool),
    CreateIndex,
    CopyRecoveryKey,
    GroupInput(SecretText),
    GroupSubmit,
    KeyInput(SecretText),
    ConfirmReplace(bool),
    KeySubmit,
    ConfirmPlainFile,
    Dismiss,
    /// Brings back the key card that "Not now" left.
    AskKeyAgain,
    WorkDone {
        generation: u64,
        outcome: WorkOutcome,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkOutcome {
    Started {
        locations: Vec<ResolvedLocation>,
        opened: OpenOutcome,
    },
    /// `Some` holds the recovery key of a new encrypted index.
    Created(Result<Option<ShownRecoveryKey>, String>),
    KeyApplied(Result<OpenOutcome, KeyError>),
    Encrypted(EncryptReport),
    StartedOver(StartOverReport),
    /// The blocking thread did not finish (it panicked).
    Crashed(String),
}

/// How `Job::Encrypt` ended, and what the index looks like now.
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

#[derive(Debug, Clone, PartialEq)]
pub enum OpenOutcome {
    NoIndexYet,
    Opened,
    /// An unencrypted index opened, but the credential store cannot be reached.
    OpenedWithoutKeyStore,
    PlainFileWithKeyEntry,
    KeyMissing,
    KeyMismatch,
    Failed(String),
}

// ---------------------------------------------------------------------------
// Work
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Job {
    Start {
        locations: Vec<FilingLocation>,
    },
    Create {
        encrypted: bool,
    },
    Restore {
        key: RecoveryKey,
    },
    Replace {
        key: RecoveryKey,
    },
    Encrypt,
    /// `data_dir` is the folder the module keeps its files in; only an
    /// `index.db` directly in it is removed.
    StartOver {
        data_dir: PathBuf,
    },
}

/// One blocking step. Synchronous; the app runs it on a blocking thread.
pub struct Work {
    generation: u64,
    db_path: PathBuf,
    store: SharedStore,
    index: IndexSlot,
    job: Job,
}

impl fmt::Debug for Work {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Work")
            .field("generation", &self.generation)
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl Work {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn run(self) -> FilingMessage {
        let db_path = self.db_path.as_path();
        let store = self.store.as_ref();
        let outcome = match self.job {
            Job::Start { locations } => WorkOutcome::Started {
                locations: resolve_locations(&locations),
                opened: open_existing(db_path, store, &self.index),
            },
            Job::Create { encrypted } => {
                WorkOutcome::Created(create_index(db_path, store, &self.index, encrypted))
            }
            Job::Restore { key } => {
                WorkOutcome::KeyApplied(apply_key(db_path, store, &self.index, |path, target| {
                    Index::restore_key(path, &key, target)
                }))
            }
            Job::Replace { key } => {
                WorkOutcome::KeyApplied(apply_key(db_path, store, &self.index, |path, target| {
                    Index::replace_key(path, &key, target)
                }))
            }
            Job::Encrypt => WorkOutcome::Encrypted(encrypt_index(db_path, store, &self.index)),
            Job::StartOver { data_dir } => {
                WorkOutcome::StartedOver(start_over(db_path, &data_dir, store, &self.index))
            }
        };
        FilingMessage::WorkDone {
            generation: self.generation,
            outcome,
        }
    }
}

fn lock(slot: &IndexSlot) -> MutexGuard<'_, Option<Index>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the runner works through. Private to this module so that nothing else
/// can build one around the slot.
struct IntakeIndex(IndexSlot);

impl fmt::Debug for IntakeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IntakeIndex")
    }
}

impl IndexAccess for IntakeIndex {
    fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool {
        match lock(&self.0).as_mut() {
            Some(index) => {
                work(index);
                true
            }
            None => false,
        }
    }
}

/// What the status page shows about the index.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexFigures {
    pub path: PathBuf,
    pub file_bytes: Option<u64>,
    pub encrypted: bool,
    pub statistics: IndexStatistics,
}

/// Reads the figures of the index. Meant for a blocking thread: the intake may
/// hold the index for as long as it hashes a large file.
pub struct IndexFiguresReader {
    db_path: PathBuf,
    slot: IndexSlot,
}

impl fmt::Debug for IndexFiguresReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexFiguresReader")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl IndexFiguresReader {
    pub fn read(self) -> Result<IndexFigures, String> {
        let file_bytes = std::fs::metadata(&self.db_path)
            .ok()
            .map(|metadata| metadata.len());
        let (encrypted, statistics) = {
            let guard = lock(&self.slot);
            let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
            let statistics = index.statistics().map_err(|err| err.to_string())?;
            (index.is_encrypted(), statistics)
        };
        Ok(IndexFigures {
            path: self.db_path,
            file_bytes,
            encrypted,
            statistics,
        })
    }
}

/// Reads the counts of the automatic filing, on a blocking thread like
/// [`IndexFiguresReader`].
pub struct HermesCountsReader {
    slot: IndexSlot,
}

impl fmt::Debug for HermesCountsReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesCountsReader").finish_non_exhaustive()
    }
}

impl HermesCountsReader {
    pub fn read(self) -> Result<HermesCounts, String> {
        let guard = lock(&self.slot);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        index.hermes_counts().map_err(|err| err.to_string())
    }
}

/// Reads what one panel's Hermes column needs, on a blocking thread like
/// [`HermesCountsReader`].
pub struct ColumnReader {
    slot: IndexSlot,
}

impl fmt::Debug for ColumnReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColumnReader").finish_non_exhaustive()
    }
}

impl ColumnReader {
    pub fn read(self, request: &ColumnRequest) -> Result<ColumnReading, String> {
        let guard = lock(&self.slot);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        read_column(index, request).map_err(|err| err.to_string())
    }
}

/// How the text of a found document was read, as far as the file type says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSource {
    Pdf,
    Ocr,
    Text,
}

impl TextSource {
    /// The index does not store how a text was read, so this follows the file
    /// type: a scanned PDF, which was read by OCR, counts as `Pdf`.
    fn of_file(path: &Path) -> Self {
        if ncrs_intake::is_image_file(path) {
            Self::Ocr
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            Self::Pdf
        } else {
            Self::Text
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundDocument {
    pub path: PathBuf,
    pub read_by: TextSource,
}

/// Looks documents up by their text, on a blocking thread like
/// [`ColumnReader`].
pub struct DocumentFinder {
    slot: IndexSlot,
}

impl fmt::Debug for DocumentFinder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentFinder").finish_non_exhaustive()
    }
}

impl DocumentFinder {
    /// The documents whose text has all `words`, the last one as a beginning,
    /// best match first. A document whose file is gone is left out.
    pub fn find(&self, words: &str, limit: usize) -> Result<Vec<FoundDocument>, String> {
        let guard = lock(&self.slot);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let ids = index
            .search_text_prefix(words, limit)
            .map_err(|err| err.to_string())?;
        let mut found = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(document) = index.document(id).map_err(|err| err.to_string())? else {
                continue;
            };
            if document.missing_since.is_none() {
                found.push(FoundDocument {
                    read_by: TextSource::of_file(&document.path),
                    path: document.path,
                });
            }
        }
        Ok(found)
    }
}

/// Every index goes through here before the watcher and the pipeline see it,
/// so a move that was interrupted is settled before anything looks at the files.
fn put_index(slot: &IndexSlot, index: Index) {
    *lock(slot) = Some(settled(index));
}

fn settled(index: Index) -> Index {
    // A failure leaves the pending items pending; the next open settles them.
    let _settled_or_retried_next_open = crate::execute::reconcile(&index);
    index
}

/// Never creates the file or its directory: the user has not chosen yet.
fn open_existing(db_path: &Path, store: &dyn KeyStore, slot: &IndexSlot) -> OpenOutcome {
    open_into(&mut lock(slot), db_path, store)
}

/// For a job that keeps the slot locked from closing the index to opening it
/// again, so no reader sees it half way.
fn open_into(held: &mut Option<Index>, db_path: &Path, store: &dyn KeyStore) -> OpenOutcome {
    match Index::open(db_path, store) {
        Err(IndexError::NotFound) => OpenOutcome::NoIndexYet,
        Err(IndexError::KeyMissing) => OpenOutcome::KeyMissing,
        Err(IndexError::WrongKey | IndexError::InvalidKeyLength { .. }) => OpenOutcome::KeyMismatch,
        Err(other) => OpenOutcome::Failed(other.to_string()),
        Ok(index) => {
            let outcome = if index.is_encrypted() {
                OpenOutcome::Opened
            } else {
                // A plain index must not be blocked where no credential store
                // exists (Linux without Secret Service): only a stored key
                // makes an unencrypted file suspicious.
                match store.load() {
                    Ok(Some(_)) => OpenOutcome::PlainFileWithKeyEntry,
                    Ok(None) => OpenOutcome::Opened,
                    Err(_) => OpenOutcome::OpenedWithoutKeyStore,
                }
            };
            *held = Some(settled(index));
            outcome
        }
    }
}

/// After a change that did not go through: the index is back in the state it
/// was in, so a key entry beside a plain file is no reason to ask again.
fn reopen_after_failure(
    held: &mut Option<Index>,
    db_path: &Path,
    store: &dyn KeyStore,
) -> OpenOutcome {
    match open_into(held, db_path, store) {
        OpenOutcome::PlainFileWithKeyEntry => OpenOutcome::Opened,
        other => other,
    }
}

/// Closes the index, encrypts the file and opens it again, all under the lock
/// of the slot. The caller has stopped the pipeline.
fn encrypt_index(db_path: &Path, store: &dyn KeyStore, slot: &IndexSlot) -> EncryptReport {
    let mut held = lock(slot);
    drop(held.take());
    match Index::encrypt_plain(db_path, store) {
        Ok(recovery_key) => match open_into(&mut held, db_path, store) {
            OpenOutcome::Opened => EncryptReport {
                result: Ok(ShownRecoveryKey::new(recovery_key)),
                opened: OpenOutcome::Opened,
            },
            other => EncryptReport {
                result: Err(INDEX_NOT_REOPENED.to_owned()),
                opened: other,
            },
        },
        Err(err) => EncryptReport {
            result: Err(err.to_string()),
            opened: reopen_after_failure(&mut held, db_path, store),
        },
    }
}

const INDEX_NOT_REOPENED: &str = "the index could not be opened again";
const INDEX_FILE_NAME: &str = "index.db";

/// The files of an index, the one the data folder holds and what SQLite and
/// the encryption leave beside it; the database goes last. Nothing else is
/// ever removed, and a path that is not `index.db` directly in `data_dir` is
/// refused.
fn index_files(db_path: &Path, data_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let named_right = db_path
        .file_name()
        .is_some_and(|name| name == INDEX_FILE_NAME);
    let parent_folder = db_path
        .parent()
        .and_then(|parent| parent.canonicalize().ok());
    let data_folder = data_dir.canonicalize().ok();
    let folder = match (named_right, parent_folder, data_folder) {
        (true, Some(parent), Some(data)) if parent == data => parent,
        _ => return Err("the index is not where the data folder keeps it".to_owned()),
    };
    Ok([".encrypting", "-journal", "-wal", "-shm", ""]
        .iter()
        .map(|suffix| folder.join(format!("{INDEX_FILE_NAME}{suffix}")))
        .collect())
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Closes the index, removes its files and then its key. A failure on the
/// files opens the index again; one on the key only becomes a warning.
fn start_over(
    db_path: &Path,
    data_dir: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
) -> StartOverReport {
    let mut held = lock(slot);
    drop(held.take());
    let removed = index_files(db_path, data_dir).and_then(|files| {
        files
            .iter()
            .try_for_each(|file| remove_if_present(file))
            .map_err(|err| err.to_string())
    });
    match removed {
        Ok(()) => StartOverReport {
            result: Ok(()),
            key_entry_warning: store.delete().err().map(|err| err.to_string()),
            opened: open_into(&mut held, db_path, store),
        },
        Err(reason) => StartOverReport {
            result: Err(reason),
            key_entry_warning: None,
            opened: reopen_after_failure(&mut held, db_path, store),
        },
    }
}

fn create_index(
    db_path: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    encrypted: bool,
) -> Result<Option<ShownRecoveryKey>, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if encrypted {
        let (index, recovery_key) =
            Index::create_encrypted(db_path, store).map_err(|err| err.to_string())?;
        put_index(slot, index);
        Ok(Some(ShownRecoveryKey::new(recovery_key)))
    } else {
        let index = Index::create_plain(db_path).map_err(|err| err.to_string())?;
        put_index(slot, index);
        Ok(None)
    }
}

fn apply_key(
    db_path: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    write_key: impl FnOnce(&Path, &dyn KeyStore) -> Result<(), IndexError>,
) -> Result<OpenOutcome, KeyError> {
    write_key(db_path, store).map_err(key_error)?;
    let index = Index::open(db_path, store).map_err(key_error)?;
    put_index(slot, index);
    Ok(OpenOutcome::Opened)
}

fn key_error(err: IndexError) -> KeyError {
    match err {
        IndexError::WrongKey => KeyError::DoesNotOpen,
        other => KeyError::Other(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

/// The PDF source; when the pdfium library is missing, each PDF is held back
/// instead, and the other files go on. A scan is read by OCR once the user has
/// downloaded the models into `ocr_models_dir`; until then it yields no text.
/// On macOS the text recognition of the system comes first and the models are
/// the fallback when it fails.
///
/// The second value is the folder the library is missing from, when it is.
fn shipped_text_chain(ocr_models_dir: PathBuf) -> (TextChain, Option<PathBuf>) {
    let (chain, library_missing_from) = match PdfText::load_shipped() {
        Ok(pdf) => (TextChain::new().with_source(pdf), None),
        Err(err) => {
            let folder = match &err {
                TextError::LibraryMissing { path } => Some(path.clone()),
                _ => None,
            };
            (
                TextChain::new().with_source(PdfUnavailable::new(err)),
                folder,
            )
        }
    };
    (
        chain
            .with_source(PlainText)
            .with_source(OfficeText)
            .with_image_source(shipped_image_source(ocr_models_dir)),
        library_missing_from,
    )
}

#[cfg(target_os = "macos")]
fn shipped_image_source(ocr_models_dir: PathBuf) -> impl ImageTextSource + 'static {
    ImageTextWithFallback::new(
        ncrs_ocr_vision::VisionText,
        LazyOcrText::new(ocr_models_dir),
    )
}

#[cfg(not(target_os = "macos"))]
fn shipped_image_source(ocr_models_dir: PathBuf) -> impl ImageTextSource + 'static {
    LazyOcrText::new(ocr_models_dir)
}

/// Where the OCR models live: next to the index, so a portable data folder
/// carries them along.
pub fn ocr_models_dir(index_path: &Path) -> Option<PathBuf> {
    index_path
        .parent()
        .map(|data_dir| data_dir.join(OCR_MODELS_FOLDER))
}

/// Where the AI model lives, next to the index like the OCR models.
pub fn ai_models_dir(index_path: &Path) -> Option<PathBuf> {
    index_path
        .parent()
        .map(|data_dir| data_dir.join(ncrs_ai::LOCAL_MODEL_FOLDER))
}

impl FilingModule {
    /// `None` when the module is off: nothing is opened, created or resolved.
    pub fn start(
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: SharedStore,
    ) -> Option<(Self, Option<Work>)> {
        Self::start_picking(filing, db_path, store, random_group)
    }

    fn start_picking(
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: SharedStore,
        pick_group: fn(usize) -> usize,
    ) -> Option<(Self, Option<Work>)> {
        if !filing.enabled {
            return None;
        }
        let ai = db_path
            .as_deref()
            .map_or(AiWiring::Off, |path| wire_ai(&filing.ai, path));
        let mut module = Self {
            ai,
            own: own_details(filing),
            db_path: db_path.clone().unwrap_or_default(),
            store,
            index: Arc::default(),
            locations: Vec::new(),
            phase: Phase::Starting,
            load: filing.load,
            generation: 0,
            pick_group,
            has_configured_locations: !filing.locations.is_empty(),
            index_encrypted: None,
            key_store_reachable: true,
            protection_note: None,
            dismissed_key_problem: None,
            kinds: Arc::default(),
        };
        if db_path.is_none() {
            module.phase = Phase::Failed(NO_DATA_DIRECTORY.to_owned());
            return Some((module, None));
        }
        let work = module.new_work(Job::Start {
            locations: filing.locations.clone(),
        });
        Some((module, Some(work)))
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn load(&self) -> Load {
        self.load
    }

    /// Only remembers the choice for the page; the running pipeline is told
    /// separately.
    pub fn set_load(&mut self, load: Load) {
        self.load = load;
    }

    /// Only looks at the phase and never at the index, so it is safe on the UI
    /// thread; `read` then runs elsewhere.
    pub fn index_figures_reader(&self) -> Option<IndexFiguresReader> {
        match self.phase {
            Phase::Ready => Some(IndexFiguresReader {
                db_path: self.db_path.clone(),
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// Like [`Self::index_figures_reader`]: looks at the phase only.
    pub fn hermes_counts_reader(&self) -> Option<HermesCountsReader> {
        match self.phase {
            Phase::Ready => Some(HermesCountsReader {
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// Like [`Self::index_figures_reader`]: looks at the phase only.
    pub fn column_reader(&self) -> Option<ColumnReader> {
        match self.phase {
            Phase::Ready => Some(ColumnReader {
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// Like [`Self::column_reader`]: looks at the phase only.
    pub fn document_finder(&self) -> Option<DocumentFinder> {
        match self.phase {
            Phase::Ready => Some(DocumentFinder {
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// The handle of the rule suggestions view: only when the module is
    /// `Ready`, and over the archive locations that can be reached.
    pub fn suggestion_access(&self) -> Option<SuggestionAccess> {
        self.archive_handle(SuggestionAccess::new)
    }

    /// The handle of the reorganisation plan view, on the same terms.
    pub fn plan_access(&self) -> Option<PlanAccess> {
        self.archive_handle(PlanAccess::new)
            .map(|access| access.with_kind_language(self.own.kind_language))
    }

    /// Takes the document kinds of a (re)loaded config; an open index uses
    /// them from now on, a pipeline already running keeps the ones it started with.
    pub fn set_kinds(&mut self, kinds: Arc<KindCatalog>) {
        self.kinds = kinds;
        self.give_kinds_to_index();
    }

    fn give_kinds_to_index(&self) {
        if let Some(index) = lock(&self.index).as_mut() {
            if !Arc::ptr_eq(index.kinds(), &self.kinds) {
                index.set_kinds(Arc::clone(&self.kinds));
            }
        }
    }

    /// The language `{kind}` folders are worded in. The running pipeline
    /// keeps the one it started with.
    pub fn set_kind_language(&mut self, kind_language: KindLanguage) {
        self.own.kind_language = kind_language;
    }

    /// The handle of the filing job: only when the module is `Ready` and the
    /// archive marked as the target can be reached.
    pub fn filing_access(&self) -> Option<FilingAccess> {
        let root = self.target_root()?;
        Some(
            FilingAccess::new(Arc::clone(&self.index), root)
                .with_kind_language(self.own.kind_language),
        )
    }

    /// The handle of the review list, on the same terms as the filing job: it
    /// files into the same target.
    pub fn review_access(&self) -> Option<ReviewAccess> {
        let root = self.target_root()?;
        Some(
            ReviewAccess::new(Arc::clone(&self.index), root)
                .with_kind_language(self.own.kind_language),
        )
    }

    /// The handle of re-planning and refiling: the inbox locations that can be
    /// reached bound what it may touch, the archive is the filing target.
    pub fn replan_access(&self) -> Option<ReplanAccess> {
        let root = self.target_root()?;
        let inbox_roots = self
            .reachable_locations()
            .filter(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.clone())
            .collect();
        Some(
            ReplanAccess::new(Arc::clone(&self.index), inbox_roots, root)
                .with_kind_language(self.own.kind_language),
        )
    }

    /// The handle of the saved queries: they only read the index, so the
    /// module being `Ready` is enough.
    pub fn query_access(&self) -> Option<QueryAccess> {
        matches!(self.phase, Phase::Ready).then(|| QueryAccess::new(Arc::clone(&self.index)))
    }

    /// The tags page reads and changes the index only, so the module being
    /// `Ready` is enough.
    pub fn tags_access(&self) -> Option<TagsAccess> {
        matches!(self.phase, Phase::Ready).then(|| TagsAccess::new(Arc::clone(&self.index)))
    }

    /// The rules page reads and changes the index only, so the module being
    /// `Ready` is enough; the try reads the documents of the locations that
    /// can be reached.
    pub fn rules_access(&self) -> Option<RulesAccess> {
        matches!(self.phase, Phase::Ready).then(|| {
            RulesAccess::new(
                Arc::clone(&self.index),
                self.reachable_locations().cloned().collect(),
            )
            .with_kind_language(self.own.kind_language)
        })
    }

    fn target_root(&self) -> Option<PathBuf> {
        if !matches!(self.phase, Phase::Ready) {
            return None;
        }
        self.locations
            .iter()
            .filter(|entry| entry.configured.is_target)
            .find_map(|entry| match &entry.status {
                LocationStatus::Ready(location) => Some(location.path.clone()),
                LocationStatus::NotReachable { .. } | LocationStatus::Overlaps { .. } => None,
            })
    }

    fn archive_handle<T>(&self, make: fn(IndexSlot, Vec<PathBuf>) -> T) -> Option<T> {
        match self.phase {
            Phase::Ready => Some(make(
                Arc::clone(&self.index),
                self.reachable_locations()
                    .filter(|location| location.mode == LocationMode::Archive)
                    .map(|location| location.path.clone())
                    .collect(),
            )),
            _ => None,
        }
    }

    pub fn locations(&self) -> &[ResolvedLocation] {
        &self.locations
    }

    /// The folders the intake may read.
    pub fn reachable_locations(&self) -> impl Iterator<Item = &Location> {
        self.locations
            .iter()
            .filter_map(|entry| match &entry.status {
                LocationStatus::Ready(location) => Some(location),
                LocationStatus::NotReachable { .. } | LocationStatus::Overlaps { .. } => None,
            })
    }

    /// Starts the pipeline on the reachable locations. The only way to the
    /// index: it is handed over in exactly one case, the module is `Ready` and
    /// the index is there.
    ///
    /// The second value is the folder the PDF library is missing from, when it
    /// is: PDFs wait until it is there and the next start finds it.
    pub fn start_pipeline(&self) -> Result<(Runner, Option<PathBuf>), HeldReason> {
        let handle = self.intake_index()?;
        let locations: Vec<Location> = self.reachable_locations().cloned().collect();
        let logical_cores = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        let (chain, pdf_library_missing) = shipped_text_chain(
            ocr_models_dir(&self.db_path).unwrap_or_else(|| PathBuf::from(OCR_MODELS_FOLDER)),
        );
        let runner = Runner::start(
            Settings::new(self.load, logical_cores),
            intake_steps(
                Arc::new(handle),
                Arc::new(chain),
                self.ai_provider(),
                self.own.clone(),
            ),
            locations,
            Arc::new(SystemClock),
        );
        Ok((runner, pdf_library_missing))
    }

    fn ai_provider(&self) -> Option<Arc<dyn ncrs_ai::Provider>> {
        match &self.ai {
            AiWiring::Off => None,
            #[cfg(feature = "local-ai")]
            AiWiring::Local(model) => Some(Arc::clone(model) as Arc<dyn ncrs_ai::Provider>),
            AiWiring::Http { provider, .. } => {
                Some(Arc::clone(provider) as Arc<dyn ncrs_ai::Provider>)
            }
        }
    }

    /// For the status page. Reads no disk and waits for nothing.
    pub fn ai_status(&self) -> AiStatus {
        match &self.ai {
            AiWiring::Off => AiStatus::Off,
            #[cfg(feature = "local-ai")]
            AiWiring::Local(model) => match model.status() {
                ncrs_ai::LocalModelStatus::NotDownloaded => AiStatus::NotDownloaded,
                ncrs_ai::LocalModelStatus::NotLoadedYet => AiStatus::NotLoadedYet,
                ncrs_ai::LocalModelStatus::Loaded { name, memory_bytes } => {
                    AiStatus::Loaded { name, memory_bytes }
                }
                ncrs_ai::LocalModelStatus::Failed(reason) => AiStatus::Failed(reason),
            },
            AiWiring::Http { origin, model, .. } => AiStatus::Http {
                origin: origin.clone(),
                model: model.clone(),
            },
        }
    }

    /// The setting asks for the local model and its file is not there.
    pub fn ai_model_is_missing(&self) -> bool {
        self.ai_status() == AiStatus::NotDownloaded
    }

    /// After a download: the model file is looked for again.
    pub fn refresh_ai_model(&self) {
        #[cfg(feature = "local-ai")]
        if let AiWiring::Local(model) = &self.ai {
            model.refresh();
        }
    }

    fn intake_index(&self) -> Result<IntakeIndex, HeldReason> {
        match &self.phase {
            Phase::Ready => {}
            Phase::Starting => return Err(HeldReason::Starting),
            Phase::AskEncryption { .. }
            | Phase::ShowRecoveryKey { .. }
            | Phase::AskRecoveryKey { .. } => return Err(HeldReason::WaitingForUser),
            Phase::Encrypting | Phase::StartingOver => return Err(HeldReason::Starting),
            Phase::PlainFileWarning => return Err(HeldReason::PlainFileUnconfirmed),
            Phase::Dismissed => return Err(HeldReason::Dismissed),
            Phase::Failed(_) => return Err(HeldReason::Failed),
        }
        // Never waits: a job on the blocking pool may hold the index, and a
        // held index is an open one.
        let is_open = match self.index.try_lock() {
            Ok(guard) => guard.is_some(),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().is_some(),
            Err(TryLockError::WouldBlock) => true,
        };
        if !is_open {
            return Err(HeldReason::Failed);
        }
        Ok(IntakeIndex(Arc::clone(&self.index)))
    }

    /// The config this module started from listed at least one location.
    pub fn has_configured_locations(&self) -> bool {
        self.has_configured_locations
    }

    /// Only once the open index is known to be unencrypted.
    pub fn index_is_plain(&self) -> bool {
        self.index_encrypted == Some(false)
    }

    /// `None` until an index is open.
    pub fn index_encryption(&self) -> Option<bool> {
        self.index_encrypted
    }

    /// Whether a new module may take this one's place: no create, restore or
    /// replace in flight, and no recovery key on screen that would be lost.
    /// A module dismissed while starting may still run its start job; that job
    /// only opens an existing index, and its result is ignored by generation.
    pub fn can_be_replaced(&self) -> bool {
        !matches!(self.phase, Phase::Starting | Phase::ShowRecoveryKey { .. })
            && !self.phase.is_busy()
    }

    pub fn has_card(&self) -> bool {
        matches!(
            self.phase,
            Phase::ShowRecoveryKey { .. }
                | Phase::AskRecoveryKey { .. }
                | Phase::PlainFileWarning
                | Phase::Failed(_)
        )
    }

    /// Whether "Not now", the cross and Esc may leave the card. The recovery
    /// key is shown once, so leaving its card would lose it; a card that works
    /// cannot be left either.
    pub fn card_can_be_dismissed(&self) -> bool {
        self.has_card() && self.may_dismiss()
    }

    /// The index does not exist yet and the module waits for the user to have
    /// it created.
    pub fn asks_encryption(&self) -> bool {
        matches!(self.phase, Phase::AskEncryption { .. })
    }

    /// The index is being created.
    pub fn is_creating_index(&self) -> bool {
        matches!(self.phase, Phase::AskEncryption { busy: true, .. })
    }

    /// Why the last attempt to create the index failed.
    pub fn index_creation_error(&self) -> Option<&str> {
        match &self.phase {
            Phase::AskEncryption { error, .. } => error.as_deref(),
            _ => None,
        }
    }

    /// Chooses the encryption and starts creating the index. Nothing happens
    /// unless the module asks for it and is not busy.
    pub fn create_index_with(&mut self, encrypted: bool) -> Option<Work> {
        self.update(FilingMessage::ChooseEncrypted(encrypted));
        self.update(FilingMessage::CreateIndex)
    }

    /// No module that already works on configured locations: a new one may
    /// take this one's place with what the first-time setup wrote.
    pub fn can_take_first_setup(&self) -> bool {
        let works_on_locations =
            matches!(self.phase, Phase::Ready) && self.has_configured_locations;
        self.can_be_replaced() && !works_on_locations
    }

    /// The index is open, unencrypted, and the system store can keep a key.
    pub fn can_encrypt(&self) -> bool {
        matches!(self.phase, Phase::Ready)
            && self.index_encrypted == Some(false)
            && self.key_store_reachable
    }

    /// Whether the index files may be removed to begin again: the module is
    /// at rest and has a place for them.
    pub fn can_start_over(&self) -> bool {
        let at_rest = match &self.phase {
            Phase::Ready | Phase::PlainFileWarning | Phase::Failed(_) => true,
            Phase::AskRecoveryKey { busy, .. } => !busy,
            Phase::Dismissed => self.dismissed_key_problem.is_some(),
            _ => false,
        };
        at_rest && self.db_path.parent().is_some() && self.db_path.file_name().is_some()
    }

    /// An encryption or a fresh start is under way.
    pub fn is_changing_protection(&self) -> bool {
        matches!(self.phase, Phase::Encrypting | Phase::StartingOver)
    }

    pub fn protection_note(&self) -> Option<&ProtectionNote> {
        self.protection_note.as_ref()
    }

    /// The key problem the user has to solve: on the card, or left behind
    /// when the card was dismissed.
    pub fn key_problem(&self) -> Option<KeyProblem> {
        match &self.phase {
            Phase::AskRecoveryKey { reason, .. } => Some(*reason),
            Phase::Dismissed => self.dismissed_key_problem,
            _ => None,
        }
    }

    /// The work closes the index itself; the caller stops the pipeline first.
    pub fn begin_encrypting(&mut self) -> Option<Work> {
        if !self.can_encrypt() {
            return None;
        }
        self.protection_note = None;
        self.index_encrypted = None;
        self.phase = Phase::Encrypting;
        Some(self.new_work(Job::Encrypt))
    }

    /// Removes the index and its key; the caller stops the pipeline first.
    pub fn begin_starting_over(&mut self) -> Option<Work> {
        if !self.can_start_over() {
            return None;
        }
        let data_dir = self.db_path.parent()?.to_path_buf();
        self.protection_note = None;
        self.dismissed_key_problem = None;
        self.index_encrypted = None;
        self.phase = Phase::StartingOver;
        Some(self.new_work(Job::StartOver { data_dir }))
    }

    fn may_dismiss(&self) -> bool {
        let stays = matches!(
            self.phase,
            Phase::Ready | Phase::Dismissed | Phase::ShowRecoveryKey { .. }
        );
        !stays && !self.phase.is_busy()
    }

    /// For the clipboard only.
    pub fn recovery_key_text(&self) -> Option<String> {
        match &self.phase {
            Phase::ShowRecoveryKey { key, .. } => Some(key.display_text()),
            _ => None,
        }
    }

    fn new_work(&mut self, job: Job) -> Work {
        self.generation = next_generation();
        Work {
            generation: self.generation,
            db_path: self.db_path.clone(),
            store: Arc::clone(&self.store),
            index: Arc::clone(&self.index),
            job,
        }
    }

    pub fn update(&mut self, message: FilingMessage) -> Option<Work> {
        match message {
            FilingMessage::ChooseEncrypted(choice) => {
                if let Phase::AskEncryption {
                    encrypted,
                    busy: false,
                    ..
                } = &mut self.phase
                {
                    *encrypted = choice;
                }
                None
            }
            FilingMessage::CreateIndex => self.create_index(),
            FilingMessage::CopyRecoveryKey => None,
            FilingMessage::GroupInput(text) => {
                if let Phase::ShowRecoveryKey {
                    typed, mismatch, ..
                } = &mut self.phase
                {
                    *typed = text;
                    *mismatch = false;
                }
                None
            }
            FilingMessage::GroupSubmit => {
                self.submit_group();
                None
            }
            FilingMessage::KeyInput(text) => {
                if let Phase::AskRecoveryKey { typed, error, .. } = &mut self.phase {
                    *typed = text;
                    *error = None;
                }
                None
            }
            FilingMessage::ConfirmReplace(confirmed) => {
                if let Phase::AskRecoveryKey {
                    confirm_replace,
                    error,
                    ..
                } = &mut self.phase
                {
                    *confirm_replace = confirmed;
                    *error = None;
                }
                None
            }
            FilingMessage::KeySubmit => self.submit_key(),
            FilingMessage::ConfirmPlainFile => {
                if matches!(self.phase, Phase::PlainFileWarning) {
                    self.phase = Phase::Ready;
                }
                None
            }
            FilingMessage::Dismiss => {
                if self.may_dismiss() {
                    if let Phase::AskRecoveryKey { reason, .. } = &self.phase {
                        self.dismissed_key_problem = Some(*reason);
                    }
                    self.generation = next_generation();
                    self.phase = Phase::Dismissed;
                }
                None
            }
            FilingMessage::AskKeyAgain => {
                if let (Phase::Dismissed, Some(reason)) = (&self.phase, self.dismissed_key_problem)
                {
                    self.phase = ask_recovery_key(reason);
                }
                None
            }
            FilingMessage::WorkDone {
                generation,
                outcome,
            } => {
                if generation == self.generation {
                    self.finish(outcome);
                }
                None
            }
        }
    }

    fn create_index(&mut self) -> Option<Work> {
        let Phase::AskEncryption {
            encrypted: chosen,
            busy,
            error,
        } = &mut self.phase
        else {
            return None;
        };
        if *busy {
            return None;
        }
        *busy = true;
        *error = None;
        let encrypted = *chosen;
        Some(self.new_work(Job::Create { encrypted }))
    }

    fn submit_group(&mut self) {
        let Phase::ShowRecoveryKey {
            key,
            group,
            typed,
            mismatch,
        } = &mut self.phase
        else {
            return;
        };
        let shown = key.display_text();
        let matches = shown
            .split('-')
            .nth(*group)
            .is_some_and(|expected| typed.as_str().trim().eq_ignore_ascii_case(expected));
        if matches {
            self.phase = Phase::Ready;
        } else {
            *mismatch = true;
        }
    }

    fn submit_key(&mut self) -> Option<Work> {
        let job = {
            let Phase::AskRecoveryKey {
                reason,
                typed,
                confirm_replace,
                busy,
                error,
            } = &mut self.phase
            else {
                return None;
            };
            if *busy {
                return None;
            }
            let Ok(key) = RecoveryKey::from_str(typed.as_str()) else {
                *error = Some(KeyError::InvalidFormat);
                return None;
            };
            let job = match reason {
                KeyProblem::Missing => Job::Restore { key },
                KeyProblem::Mismatch if *confirm_replace => Job::Replace { key },
                KeyProblem::Mismatch => {
                    *error = Some(KeyError::ConfirmFirst);
                    return None;
                }
            };
            *busy = true;
            *error = None;
            job
        };
        Some(self.new_work(job))
    }

    fn finish(&mut self, outcome: WorkOutcome) {
        match outcome {
            WorkOutcome::Started { locations, opened } => {
                if matches!(self.phase, Phase::Starting) {
                    self.locations = locations;
                    self.enter(opened);
                }
            }
            WorkOutcome::Created(result) => self.finish_create(result),
            WorkOutcome::KeyApplied(result) => self.finish_key(result),
            WorkOutcome::Encrypted(report) => self.finish_encrypt(report),
            WorkOutcome::StartedOver(report) => self.finish_start_over(report),
            WorkOutcome::Crashed(reason) => self.finish_crashed(reason),
        }
        // Safe on the UI thread: no pipeline holds the index before `Ready`,
        // and no work is running once its result is here.
        if self.index_encrypted.is_none() {
            self.index_encrypted = lock(&self.index).as_ref().map(Index::is_encrypted);
        }
        self.give_kinds_to_index();
    }

    fn enter(&mut self, opened: OpenOutcome) {
        self.phase = match opened {
            OpenOutcome::NoIndexYet => Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
            OpenOutcome::Opened => Phase::Ready,
            OpenOutcome::OpenedWithoutKeyStore => {
                self.key_store_reachable = false;
                Phase::Ready
            }
            OpenOutcome::PlainFileWithKeyEntry => Phase::PlainFileWarning,
            OpenOutcome::KeyMissing => ask_recovery_key(KeyProblem::Missing),
            OpenOutcome::KeyMismatch => ask_recovery_key(KeyProblem::Mismatch),
            OpenOutcome::Failed(reason) => Phase::Failed(reason),
        };
    }

    fn finish_create(&mut self, result: Result<Option<ShownRecoveryKey>, String>) {
        if !matches!(self.phase, Phase::AskEncryption { busy: true, .. }) {
            return;
        }
        match result {
            Ok(Some(key)) => {
                self.phase = Phase::ShowRecoveryKey {
                    key,
                    group: (self.pick_group)(GROUP_COUNT) % GROUP_COUNT,
                    typed: SecretText::default(),
                    mismatch: false,
                };
            }
            Ok(None) => self.phase = Phase::Ready,
            Err(reason) => self.stop_waiting(reason),
        }
    }

    fn finish_key(&mut self, result: Result<OpenOutcome, KeyError>) {
        if !matches!(self.phase, Phase::AskRecoveryKey { busy: true, .. }) {
            return;
        }
        match result {
            Ok(opened) => self.enter(opened),
            Err(failure) => {
                if let Phase::AskRecoveryKey { busy, error, .. } = &mut self.phase {
                    *busy = false;
                    *error = Some(failure);
                }
            }
        }
    }

    fn finish_encrypt(&mut self, report: EncryptReport) {
        if !matches!(self.phase, Phase::Encrypting) {
            return;
        }
        match report.result {
            Ok(key) => {
                self.phase = Phase::ShowRecoveryKey {
                    key,
                    group: (self.pick_group)(GROUP_COUNT) % GROUP_COUNT,
                    typed: SecretText::default(),
                    mismatch: false,
                };
            }
            Err(reason) => {
                self.protection_note = Some(ProtectionNote::EncryptFailed(reason));
                self.enter(report.opened);
            }
        }
    }

    fn finish_start_over(&mut self, report: StartOverReport) {
        if !matches!(self.phase, Phase::StartingOver) {
            return;
        }
        match report.result {
            Ok(()) => {
                self.protection_note = report
                    .key_entry_warning
                    .map(ProtectionNote::KeyEntryNotRemoved);
            }
            Err(reason) => {
                self.protection_note = Some(ProtectionNote::StartOverFailed(reason));
            }
        }
        self.enter(report.opened);
    }

    fn finish_crashed(&mut self, reason: String) {
        if matches!(
            self.phase,
            Phase::Starting | Phase::Encrypting | Phase::StartingOver
        ) {
            self.phase = Phase::Failed(reason);
        } else {
            self.stop_waiting(reason);
        }
    }

    /// Back from a busy card, with the reason on it.
    fn stop_waiting(&mut self, reason: String) {
        match &mut self.phase {
            Phase::AskEncryption { busy, error, .. } => {
                *busy = false;
                *error = Some(reason);
            }
            Phase::AskRecoveryKey { busy, error, .. } => {
                *busy = false;
                *error = Some(KeyError::Other(reason));
            }
            Phase::Starting
            | Phase::Encrypting
            | Phase::StartingOver
            | Phase::ShowRecoveryKey { .. }
            | Phase::PlainFileWarning
            | Phase::Ready
            | Phase::Failed(_)
            | Phase::Dismissed => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
impl FilingModule {
    /// A module standing in `phase`, with nothing behind it: no work, an
    /// in-memory store, a path that is never written.
    pub fn in_phase(phase: Phase) -> Self {
        Self {
            db_path: PathBuf::from("index.db"),
            store: Arc::new(ncrs_index::InMemoryKeyStore::empty()),
            index: Arc::default(),
            locations: Vec::new(),
            phase,
            load: Load::default(),
            generation: 0,
            pick_group: random_group,
            has_configured_locations: false,
            index_encrypted: None,
            key_store_reachable: true,
            protection_note: None,
            dismissed_key_problem: None,
            kinds: Arc::default(),
            ai: AiWiring::Off,
            own: OwnDetails::default(),
        }
    }

    /// Like `in_phase`, with `index` open behind it.
    pub fn in_phase_over_index(phase: Phase, index: Index) -> Self {
        Self {
            index: Arc::new(Mutex::new(Some(index))),
            ..Self::in_phase(phase)
        }
    }

    /// A ready module over `index` whose locations are `configured`, looked up
    /// on disk like the start work does.
    pub fn ready_over_index_with_locations(index: Index, configured: &[FilingLocation]) -> Self {
        Self {
            locations: resolve_locations(configured),
            ..Self::in_phase_over_index(Phase::Ready, index)
        }
    }

    /// The card for `reason` was put off by the user.
    pub fn dismissed_over(mut self, reason: KeyProblem) -> Self {
        self.phase = Phase::Dismissed;
        self.dismissed_key_problem = Some(reason);
        self
    }

    /// Like `in_phase`, as if the open index had the given encryption.
    pub fn in_phase_with_index(phase: Phase, encrypted: bool) -> Self {
        Self {
            index_encrypted: Some(encrypted),
            ..Self::in_phase(phase)
        }
    }

    /// Like `in_phase_with_locations`, with the index at `db_path`: work the
    /// test starts really creates it.
    pub fn in_phase_with_locations_at(phase: Phase, db_path: PathBuf) -> Self {
        Self {
            db_path,
            ..Self::in_phase_with_locations(phase)
        }
    }

    /// Like `in_phase`, wired for the AI of `setting`: the local model is
    /// not downloaded, so it reports that.
    pub fn in_phase_with_ai(phase: Phase, setting: &AiSetting) -> Self {
        Self {
            ai: wire_ai(setting, Path::new("index.db")),
            ..Self::in_phase(phase)
        }
    }

    /// Like `in_phase`, started from a config that listed locations.
    pub fn in_phase_with_locations(phase: Phase) -> Self {
        Self {
            has_configured_locations: true,
            ..Self::in_phase(phase)
        }
    }

    /// The all-zero key: valid to show, opens nothing.
    pub fn fixed_recovery_key() -> ShownRecoveryKey {
        let key = "A".repeat(52).parse::<RecoveryKey>();
        ShownRecoveryKey::new(key.expect("52 times A is a canonical recovery key"))
    }

    /// A made-up key whose 13 blocks all differ: ABCD-EFGH-...-TUVA. Opens nothing.
    pub fn varied_recovery_key() -> ShownRecoveryKey {
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567BCDEFGHJKLMNPQRSTUVA".parse::<RecoveryKey>();
        ShownRecoveryKey::new(key.expect("a canonical recovery key"))
    }
}

fn ask_recovery_key(reason: KeyProblem) -> Phase {
    Phase::AskRecoveryKey {
        reason,
        typed: SecretText::default(),
        confirm_replace: false,
        busy: false,
        error: None,
    }
}

/// Unpredictable per start, without a random-number dependency: the standard
/// library seeds every `RandomState` from the system.
fn random_group(count: usize) -> usize {
    let hash = RandomState::new().build_hasher().finish();
    let groups = u64::try_from(count.max(1)).unwrap_or(1);
    usize::try_from(hash % groups).unwrap_or(0)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::fs;

    use ncrs_index::{InMemoryKeyStore, KeyStoreError};
    use ncrs_intake::LocationMode;
    use tempfile::TempDir;

    use super::*;

    fn fixed_group(_count: usize) -> usize {
        4
    }

    fn location(path: &Path) -> FilingLocation {
        FilingLocation {
            path: path.to_path_buf(),
            mode: LocationMode::Inbox,
            images: false,
            is_target: false,
        }
    }

    fn filing_with(locations: Vec<FilingLocation>) -> Filing {
        Filing {
            enabled: true,
            locations,
            load: Load::default(),
            ai: crate::config::AiSetting::Off,
            default_pattern: None,
            own_names: Vec::new(),
            own_ibans: Vec::new(),
            own_tax_ids: Vec::new(),
        }
    }

    fn data_dir(dir: &TempDir) -> PathBuf {
        dir.path().join("data")
    }

    fn db_in(dir: &TempDir) -> PathBuf {
        data_dir(dir).join("index.db")
    }

    fn shared(store: &Arc<InMemoryKeyStore>) -> SharedStore {
        Arc::<InMemoryKeyStore>::clone(store)
    }

    /// Runs the work and feeds every result back, as the app does.
    fn settle(module: &mut FilingModule, mut work: Option<Work>) {
        while let Some(next) = work {
            work = module.update(next.run());
        }
    }

    fn started(db: &Path, store: SharedStore) -> FilingModule {
        let (mut module, work) = FilingModule::start_picking(
            &filing_with(Vec::new()),
            Some(db.to_path_buf()),
            store,
            fixed_group,
        )
        .unwrap();
        settle(&mut module, work);
        module
    }

    fn send(module: &mut FilingModule, message: FilingMessage) {
        let work = module.update(message);
        settle(module, work);
    }

    fn type_key(module: &mut FilingModule, text: &str) {
        send(module, FilingMessage::KeyInput(SecretText::from(text)));
    }

    fn make_encrypted(db: &Path, store: &InMemoryKeyStore) -> String {
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        let (index, key) = Index::create_encrypted(db, store).unwrap();
        drop(index);
        key.to_string()
    }

    fn make_plain(db: &Path) {
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        drop(Index::create_plain(db).unwrap());
    }

    fn wrong_key_text() -> String {
        "A".repeat(52)
    }

    fn is_open(module: &FilingModule) -> bool {
        module.start_pipeline().is_ok()
    }

    #[derive(Debug)]
    struct UnavailableStore;

    impl KeyStore for UnavailableStore {
        fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
            Err(KeyStoreError::Unavailable("no keystore".to_owned()))
        }

        fn store_new(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Unavailable("no keystore".to_owned()))
        }

        fn replace(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Unavailable("no keystore".to_owned()))
        }

        fn delete(&self) -> Result<(), KeyStoreError> {
            Ok(())
        }
    }

    #[test]
    fn the_pipeline_is_refused_in_every_phase_but_ready() {
        let held = [
            (Phase::Starting, HeldReason::Starting),
            (
                Phase::AskEncryption {
                    encrypted: true,
                    busy: false,
                    error: None,
                },
                HeldReason::WaitingForUser,
            ),
            (
                Phase::ShowRecoveryKey {
                    key: FilingModule::fixed_recovery_key(),
                    group: 0,
                    typed: SecretText::default(),
                    mismatch: false,
                },
                HeldReason::WaitingForUser,
            ),
            (
                ask_recovery_key(KeyProblem::Missing),
                HeldReason::WaitingForUser,
            ),
            (Phase::PlainFileWarning, HeldReason::PlainFileUnconfirmed),
            (Phase::Dismissed, HeldReason::Dismissed),
            (Phase::Failed("no".to_owned()), HeldReason::Failed),
            (Phase::Ready, HeldReason::Failed),
        ];
        for (phase, reason) in held {
            let module = FilingModule::in_phase(phase);
            put_plain_index_unless_ready_is_empty(&module);
            assert_eq!(
                module.start_pipeline().err(),
                Some(reason),
                "{:?}",
                module.phase()
            );
        }
    }

    /// Every phase but the last holds an index in the slot, to show that the
    /// phase alone keeps the pipeline away; `Ready` gets an empty slot.
    fn put_plain_index_unless_ready_is_empty(module: &FilingModule) {
        if module.phase() == &Phase::Ready {
            return;
        }
        let dir = TempDir::new().unwrap();
        put_index(
            &module.index,
            Index::create_plain(&dir.path().join("index.db")).unwrap(),
        );
    }

    #[test]
    fn an_index_put_into_the_slot_has_its_interrupted_move_settled_first() {
        use ncrs_index::{BatchKind, JournalState, NewJournalItem};

        let dir = TempDir::new().unwrap();
        let source = dir.path().join("a.pdf");
        std::fs::write(&source, "alpha").unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let batch = index.begin_batch(BatchKind::Reorganisation).unwrap();
        index
            .add_journal_item(
                batch,
                &NewJournalItem {
                    from: source.clone(),
                    to: dir.path().join("b.pdf"),
                    hash: ncrs_intake::hash_path(&source).unwrap(),
                    document_id: None,
                    duplicate: false,
                    state: JournalState::Pending,
                    reason: None,
                },
            )
            .unwrap();
        let slot = IndexSlot::default();

        put_index(&slot, index);

        let guard = lock(&slot);
        let items = guard.as_ref().unwrap().journal_items(batch).unwrap();
        assert_eq!(items[0].state, JournalState::Skipped);
        assert_eq!(items[0].reason.as_deref(), Some("interrupted"));
    }

    #[test]
    fn a_ready_module_with_an_index_starts_a_pipeline_that_stops_on_drop() {
        let dir = TempDir::new().unwrap();
        let module = FilingModule::in_phase(Phase::Ready);
        put_index(
            &module.index,
            Index::create_plain(&dir.path().join("index.db")).unwrap(),
        );

        let (runner, _pdf_library_missing) = module.start_pipeline().unwrap();

        assert_ne!(runner.snapshot().state, ncrs_pipeline::RunnerState::Off);
        drop(runner);
    }

    #[test]
    fn a_document_is_found_by_the_beginning_of_a_word_only_its_text_has() {
        use ncrs_index::{ContentHash, DocumentSource, NewDocument};

        let dir = TempDir::new().unwrap();
        let module = FilingModule::in_phase(Phase::Ready);
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let mut add = |seed: u8, name: &str, text: &str| {
            let id = index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: PathBuf::from(name),
                    source: DocumentSource::Inbox,
                    received_at: chrono::Utc::now(),
                    stamp: None,
                })
                .unwrap();
            index.set_document_text(id, text).unwrap();
            id
        };
        add(1, "inbox/scan-1.pdf", "Rechnung Nr. 4711");
        let gone = add(2, "inbox/scan-2.PDF", "Rechnung Nr. 12");
        add(3, "inbox/photo.JPG", "Rechnung am Strand");
        add(4, "inbox/notes.txt", "Mahnung");
        index.mark_document_missing(gone).unwrap();
        put_index(&module.index, index);

        let found = module.document_finder().unwrap().find("rechn", 10).unwrap();

        let listed: Vec<(PathBuf, TextSource)> = found
            .into_iter()
            .map(|document| (document.path, document.read_by))
            .collect();
        assert_eq!(listed.len(), 2, "{listed:?}");
        assert!(listed.contains(&(PathBuf::from("inbox/scan-1.pdf"), TextSource::Pdf)));
        assert!(listed.contains(&(PathBuf::from("inbox/photo.JPG"), TextSource::Ocr)));
    }

    #[test]
    fn the_document_finder_exists_in_ready_only_and_fails_without_an_index() {
        assert!(FilingModule::in_phase(Phase::Starting)
            .document_finder()
            .is_none());
        let finder = FilingModule::in_phase(Phase::Ready)
            .document_finder()
            .unwrap();
        assert_eq!(finder.find("x", 5), Err(INDEX_NOT_OPEN.to_owned()));
    }

    #[test]
    fn the_figures_reader_exists_in_ready_only() {
        let others = [
            Phase::Starting,
            Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
            Phase::ShowRecoveryKey {
                key: FilingModule::fixed_recovery_key(),
                group: 0,
                typed: SecretText::default(),
                mismatch: false,
            },
            ask_recovery_key(KeyProblem::Missing),
            Phase::PlainFileWarning,
            Phase::Dismissed,
            Phase::Failed("no".to_owned()),
        ];
        for phase in others {
            let module = FilingModule::in_phase(phase);
            assert!(
                module.index_figures_reader().is_none(),
                "{:?}",
                module.phase()
            );
        }
        assert!(FilingModule::in_phase(Phase::Ready)
            .index_figures_reader()
            .is_some());
    }

    #[test]
    fn the_figures_reader_reports_a_plain_index_on_disk() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        let mut module = started(&db, shared(&store));
        send(&mut module, FilingMessage::ChooseEncrypted(false));
        send(&mut module, FilingMessage::CreateIndex);

        let figures = module.index_figures_reader().unwrap().read().unwrap();

        assert!(!figures.encrypted);
        assert_eq!(figures.path, db);
        assert!(figures.file_bytes.is_some_and(|bytes| bytes > 0));
        assert_eq!(figures.statistics.document_count, 0);
    }

    #[test]
    fn the_module_knows_whether_the_index_it_opened_is_plain() {
        for encrypted in [false, true] {
            let dir = TempDir::new().unwrap();
            let store = Arc::new(InMemoryKeyStore::empty());
            let mut module = started(&db_in(&dir), shared(&store));
            assert!(!module.index_is_plain(), "nothing is open yet");

            send(&mut module, FilingMessage::ChooseEncrypted(encrypted));
            send(&mut module, FilingMessage::CreateIndex);

            assert_eq!(module.index_is_plain(), !encrypted);
        }
        let dir = TempDir::new().unwrap();
        make_plain(&db_in(&dir));
        let store = Arc::new(InMemoryKeyStore::empty());
        assert!(started(&db_in(&dir), shared(&store)).index_is_plain());
    }

    #[test]
    fn the_module_remembers_whether_the_config_listed_locations() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(InMemoryKeyStore::empty());
        for (locations, expected) in [
            (Vec::new(), false),
            (vec![location(&dir.path().join("inbox"))], true),
        ] {
            let (module, _work) = FilingModule::start_picking(
                &filing_with(locations),
                Some(db_in(&dir)),
                shared(&store),
                fixed_group,
            )
            .unwrap();
            assert_eq!(module.has_configured_locations(), expected);
        }
    }

    #[test]
    fn only_a_module_without_work_or_a_shown_key_can_be_replaced() {
        let replaceable = [
            Phase::Ready,
            Phase::Dismissed,
            Phase::PlainFileWarning,
            Phase::Failed("broken".to_owned()),
            Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
        ];
        for phase in replaceable {
            assert!(FilingModule::in_phase(phase).can_be_replaced());
        }
        let held = [
            Phase::Starting,
            Phase::AskEncryption {
                encrypted: true,
                busy: true,
                error: None,
            },
            Phase::ShowRecoveryKey {
                key: FilingModule::fixed_recovery_key(),
                group: 0,
                typed: SecretText::default(),
                mismatch: false,
            },
        ];
        for phase in held {
            assert!(!FilingModule::in_phase(phase).can_be_replaced());
        }
    }

    #[test]
    fn the_figures_reader_fails_when_the_slot_is_empty() {
        let module = FilingModule::in_phase(Phase::Ready);

        let result = module.index_figures_reader().unwrap().read();

        assert_eq!(result, Err(INDEX_NOT_OPEN.to_owned()));
    }

    #[test]
    fn a_ready_module_stays_ready_whatever_it_is_sent() {
        let outcomes = [
            WorkOutcome::Started {
                locations: Vec::new(),
                opened: OpenOutcome::NoIndexYet,
            },
            WorkOutcome::Started {
                locations: Vec::new(),
                opened: OpenOutcome::Opened,
            },
            WorkOutcome::Started {
                locations: Vec::new(),
                opened: OpenOutcome::PlainFileWithKeyEntry,
            },
            WorkOutcome::Started {
                locations: Vec::new(),
                opened: OpenOutcome::KeyMissing,
            },
            WorkOutcome::Started {
                locations: Vec::new(),
                opened: OpenOutcome::KeyMismatch,
            },
            WorkOutcome::Started {
                locations: Vec::new(),
                opened: OpenOutcome::Failed("broken".to_owned()),
            },
            WorkOutcome::Created(Ok(None)),
            WorkOutcome::Created(Ok(Some(FilingModule::fixed_recovery_key()))),
            WorkOutcome::Created(Err("broken".to_owned())),
            WorkOutcome::KeyApplied(Ok(OpenOutcome::Opened)),
            WorkOutcome::KeyApplied(Ok(OpenOutcome::KeyMismatch)),
            WorkOutcome::KeyApplied(Err(KeyError::DoesNotOpen)),
            WorkOutcome::Crashed("broken".to_owned()),
        ];
        let mut messages = vec![
            FilingMessage::ChooseEncrypted(true),
            FilingMessage::ChooseEncrypted(false),
            FilingMessage::CreateIndex,
            FilingMessage::CopyRecoveryKey,
            FilingMessage::GroupInput(SecretText::from("ZZZZ")),
            FilingMessage::GroupSubmit,
            FilingMessage::KeyInput(SecretText::from("A")),
            FilingMessage::ConfirmReplace(true),
            FilingMessage::KeySubmit,
            FilingMessage::ConfirmPlainFile,
            FilingMessage::Dismiss,
        ];
        let mut module = FilingModule::in_phase(Phase::Ready);
        for outcome in outcomes {
            for generation in [module.generation, module.generation.wrapping_sub(1)] {
                messages.push(FilingMessage::WorkDone {
                    generation,
                    outcome: outcome.clone(),
                });
            }
        }

        for message in messages {
            let label = format!("{message:?}");
            let _ = module.update(message);
            assert_eq!(module.phase(), &Phase::Ready, "{label}");
        }
    }

    #[test]
    fn a_module_that_is_off_opens_and_creates_nothing() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        let off = Filing {
            enabled: false,
            locations: vec![location(dir.path())],
            load: Load::default(),
            ai: crate::config::AiSetting::Off,
            default_pattern: None,
            own_names: Vec::new(),
            own_ibans: Vec::new(),
            own_tax_ids: Vec::new(),
        };

        assert!(FilingModule::start(&off, Some(db.clone()), shared(&store)).is_none());

        assert!(!db.exists());
        assert!(!data_dir(&dir).exists());
        assert_eq!(store.snapshot(), None);
    }

    #[test]
    fn a_missing_folder_is_not_reachable_and_the_other_stays_ready() {
        let dir = TempDir::new().unwrap();
        let present = dir.path().join("inbox");
        fs::create_dir(&present).unwrap();
        let missing = dir.path().join("gone");

        let resolved = resolve_locations(&[location(&missing), location(&present)]);

        assert!(matches!(
            resolved[0].status,
            LocationStatus::NotReachable { .. }
        ));
        assert!(matches!(resolved[1].status, LocationStatus::Ready(_)));
    }

    fn assert_both_overlap(resolved: &[ResolvedLocation], first: &Path, second: &Path) {
        assert_eq!(resolved.len(), 2);
        assert_eq!(
            resolved[0].status,
            LocationStatus::Overlaps {
                with: second.to_path_buf()
            }
        );
        assert_eq!(
            resolved[1].status,
            LocationStatus::Overlaps {
                with: first.to_path_buf()
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_location_overlaps_it() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("a");
        let link = dir.path().join("link");
        fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let resolved = resolve_locations(&[location(&real), location(&link)]);

        assert_both_overlap(&resolved, &real, &link);
    }

    #[test]
    fn two_spellings_of_one_folder_overlap() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("a");
        fs::create_dir(&real).unwrap();
        let dotted = dir.path().join(".").join("a");

        let resolved = resolve_locations(&[location(&dotted), location(&real)]);

        assert_both_overlap(&resolved, &dotted, &real);
    }

    #[test]
    fn nested_folders_overlap() {
        let dir = TempDir::new().unwrap();
        let outer = dir.path().join("a");
        let inner = outer.join("b");
        fs::create_dir_all(&inner).unwrap();

        let resolved = resolve_locations(&[location(&outer), location(&inner)]);

        assert_both_overlap(&resolved, &outer, &inner);
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn spellings_that_differ_in_case_overlap_on_case_insensitive_systems() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("Inbox");
        fs::create_dir(&real).unwrap();
        let lower = dir.path().join("inbox");

        let resolved = resolve_locations(&[location(&real), location(&lower)]);

        assert_both_overlap(&resolved, &real, &lower);
    }

    #[test]
    fn first_start_with_encryption_asks_shows_the_key_and_opens_after_confirmation() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());

        let (mut module, work) = FilingModule::start_picking(
            &filing_with(Vec::new()),
            Some(db.clone()),
            shared(&store),
            fixed_group,
        )
        .unwrap();
        assert_eq!(module.phase(), &Phase::Starting);
        assert_eq!(module.start_pipeline().err(), Some(HeldReason::Starting));
        settle(&mut module, work);

        assert!(matches!(
            module.phase(),
            Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None
            }
        ));
        assert!(!db.exists());
        assert!(!data_dir(&dir).exists());
        assert_eq!(
            module.start_pipeline().err(),
            Some(HeldReason::WaitingForUser)
        );

        send(&mut module, FilingMessage::CreateIndex);
        assert!(matches!(
            module.phase(),
            Phase::ShowRecoveryKey { group: 4, .. }
        ));
        assert_eq!(
            module.start_pipeline().err(),
            Some(HeldReason::WaitingForUser)
        );

        send(
            &mut module,
            FilingMessage::GroupInput(SecretText::from("ZZZZ")),
        );
        send(&mut module, FilingMessage::GroupSubmit);
        assert!(matches!(
            module.phase(),
            Phase::ShowRecoveryKey { mismatch: true, .. }
        ));
        assert!(!is_open(&module));

        let shown = module.recovery_key_text().unwrap();
        let group = shown.split('-').nth(4).unwrap().to_ascii_lowercase();
        send(
            &mut module,
            FilingMessage::GroupInput(SecretText::from(format!(" {group} "))),
        );
        send(&mut module, FilingMessage::GroupSubmit);

        assert_eq!(module.phase(), &Phase::Ready);
        assert_eq!(module.recovery_key_text(), None);
        module.start_pipeline().unwrap();
        assert_eq!(
            lock(&module.index).as_ref().map(Index::is_encrypted),
            Some(true)
        );
        assert_eq!(store.snapshot().unwrap().len(), 32);
        assert!(Index::open(&db, store.as_ref()).unwrap().is_encrypted());
    }

    #[test]
    fn the_typed_block_is_taken_in_any_case_and_with_spaces_around_it() {
        let phase = |typed: &str| Phase::ShowRecoveryKey {
            key: FilingModule::varied_recovery_key(),
            group: 1,
            typed: SecretText::from(typed),
            mismatch: false,
        };
        for typed in ["EFGH", "efgh", "eFgH", "  efgh", "EFGH \t", " Efgh "] {
            let mut module = FilingModule::in_phase(phase(typed));
            send(&mut module, FilingMessage::GroupSubmit);
            assert_eq!(module.phase(), &Phase::Ready, "{typed:?}");
        }
        for typed in ["", "ABCD", "EFG", "EFGHI", "EF GH"] {
            let mut module = FilingModule::in_phase(phase(typed));
            send(&mut module, FilingMessage::GroupSubmit);
            assert!(
                matches!(
                    module.phase(),
                    Phase::ShowRecoveryKey { mismatch: true, .. }
                ),
                "{typed:?}"
            );
        }
    }

    #[test]
    fn first_start_without_encryption_opens_a_plain_index() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        let mut module = started(&db, shared(&store));

        send(&mut module, FilingMessage::ChooseEncrypted(false));
        send(&mut module, FilingMessage::CreateIndex);

        assert_eq!(module.phase(), &Phase::Ready);
        assert!(is_open(&module));
        assert_eq!(store.snapshot(), None);
        assert!(fs::read(&db).unwrap().starts_with(b"SQLite format 3\0"));
    }

    #[test]
    fn a_later_start_with_key_and_database_opens_directly() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        make_encrypted(&db, &store);

        let module = started(&db, shared(&store));

        assert_eq!(module.phase(), &Phase::Ready);
        assert!(is_open(&module));
    }

    #[test]
    fn a_missing_key_is_restored_from_the_recovery_key() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let original = Arc::new(InMemoryKeyStore::empty());
        let recovery = make_encrypted(&db, &original);
        let empty = Arc::new(InMemoryKeyStore::empty());

        let mut module = started(&db, shared(&empty));
        assert!(matches!(
            module.phase(),
            Phase::AskRecoveryKey {
                reason: KeyProblem::Missing,
                ..
            }
        ));
        assert!(!is_open(&module));

        type_key(&mut module, "not a key");
        assert!(module.update(FilingMessage::KeySubmit).is_none());
        assert!(matches!(
            module.phase(),
            Phase::AskRecoveryKey {
                error: Some(KeyError::InvalidFormat),
                ..
            }
        ));

        type_key(&mut module, &wrong_key_text());
        send(&mut module, FilingMessage::KeySubmit);
        assert!(matches!(
            module.phase(),
            Phase::AskRecoveryKey {
                busy: false,
                error: Some(KeyError::DoesNotOpen),
                ..
            }
        ));
        assert_eq!(empty.snapshot(), None);

        type_key(&mut module, &recovery);
        send(&mut module, FilingMessage::KeySubmit);
        assert_eq!(module.phase(), &Phase::Ready);
        assert!(is_open(&module));
        assert_eq!(empty.snapshot(), original.snapshot());
    }

    #[test]
    fn a_wrong_stored_key_is_replaced_only_after_confirmation_and_proof() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let original = Arc::new(InMemoryKeyStore::empty());
        let recovery = make_encrypted(&db, &original);
        let other = Arc::new(InMemoryKeyStore::with_entry(vec![7; 32]));

        let mut module = started(&db, shared(&other));
        assert!(matches!(
            module.phase(),
            Phase::AskRecoveryKey {
                reason: KeyProblem::Mismatch,
                ..
            }
        ));

        type_key(&mut module, &recovery);
        assert!(module.update(FilingMessage::KeySubmit).is_none());
        assert!(matches!(
            module.phase(),
            Phase::AskRecoveryKey {
                error: Some(KeyError::ConfirmFirst),
                ..
            }
        ));
        assert_eq!(other.snapshot().unwrap(), vec![7; 32]);

        send(&mut module, FilingMessage::ConfirmReplace(true));
        type_key(&mut module, &wrong_key_text());
        send(&mut module, FilingMessage::KeySubmit);
        assert!(matches!(
            module.phase(),
            Phase::AskRecoveryKey {
                error: Some(KeyError::DoesNotOpen),
                ..
            }
        ));
        assert_eq!(other.snapshot().unwrap(), vec![7; 32]);

        type_key(&mut module, &recovery);
        send(&mut module, FilingMessage::KeySubmit);
        assert_eq!(module.phase(), &Phase::Ready);
        assert_eq!(other.snapshot(), original.snapshot());
    }

    #[test]
    fn a_plain_file_next_to_a_stored_key_waits_for_confirmation() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);
        let store = Arc::new(InMemoryKeyStore::with_entry(vec![5; 32]));

        let mut module = started(&db, shared(&store));

        assert_eq!(module.phase(), &Phase::PlainFileWarning);
        assert_eq!(
            module.start_pipeline().err(),
            Some(HeldReason::PlainFileUnconfirmed)
        );
        send(&mut module, FilingMessage::ConfirmPlainFile);
        assert!(is_open(&module));
        assert_eq!(store.snapshot().unwrap(), vec![5; 32]);
    }

    #[test]
    fn a_plain_file_opens_where_there_is_no_credential_store() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);

        let module = started(&db, Arc::new(UnavailableStore));

        assert_eq!(module.phase(), &Phase::Ready);
        assert!(is_open(&module));
    }

    #[test]
    fn creating_an_encrypted_index_without_a_credential_store_creates_no_file() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let mut module = started(&db, Arc::new(UnavailableStore));

        send(&mut module, FilingMessage::CreateIndex);

        assert!(matches!(
            module.phase(),
            Phase::AskEncryption {
                busy: false,
                error: Some(_),
                ..
            }
        ));
        assert!(!db.exists());
        assert!(!is_open(&module));
    }

    #[test]
    fn a_result_of_an_older_generation_is_ignored() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        let (mut module, work) = FilingModule::start_picking(
            &filing_with(Vec::new()),
            Some(db),
            shared(&store),
            fixed_group,
        )
        .unwrap();

        send(&mut module, FilingMessage::Dismiss);
        settle(&mut module, work);

        assert_eq!(module.phase(), &Phase::Dismissed);
        assert_eq!(module.start_pipeline().err(), Some(HeldReason::Dismissed));
    }

    #[test]
    fn a_failed_open_offers_no_way_to_create_a_new_index_over_the_file() {
        let mut module = FilingModule::in_phase(Phase::Failed("broken".to_owned()));

        assert!(module.update(FilingMessage::CreateIndex).is_none());

        assert_eq!(module.phase(), &Phase::Failed("broken".to_owned()));
    }

    #[test]
    fn the_encryption_question_is_the_first_run_card_and_not_the_module_s() {
        let module = FilingModule::in_phase(Phase::AskEncryption {
            encrypted: true,
            busy: false,
            error: None,
        });
        assert!(!module.has_card());
        assert!(module.asks_encryption());
    }

    #[test]
    fn dismiss_does_not_discard_the_shown_recovery_key() {
        let mut module = FilingModule::in_phase(Phase::ShowRecoveryKey {
            key: FilingModule::fixed_recovery_key(),
            group: 2,
            typed: SecretText::default(),
            mismatch: false,
        });

        send(&mut module, FilingMessage::Dismiss);

        assert!(module.recovery_key_text().is_some());
        assert!(module.has_card());
    }

    #[test]
    fn only_ready_locations_are_reachable() {
        let dir = TempDir::new().unwrap();
        let present = dir.path().join("inbox");
        fs::create_dir(&present).unwrap();
        let missing = dir.path().join("gone");
        let store = Arc::new(InMemoryKeyStore::empty());
        let filing = filing_with(vec![location(&missing), location(&present)]);
        let (mut module, work) =
            FilingModule::start_picking(&filing, Some(db_in(&dir)), shared(&store), fixed_group)
                .unwrap();
        settle(&mut module, work);

        assert_eq!(module.locations().len(), 2);
        let reachable: Vec<&Location> = module.reachable_locations().collect();
        assert_eq!(reachable.len(), 1);
        assert!(reachable[0].path.ends_with("inbox"));
    }

    #[test]
    fn no_data_directory_fails_without_work() {
        let store = Arc::new(InMemoryKeyStore::empty());

        let (module, work) =
            FilingModule::start(&filing_with(Vec::new()), None, shared(&store)).unwrap();

        assert!(work.is_none());
        assert_eq!(module.phase(), &Phase::Failed(NO_DATA_DIRECTORY.to_owned()));
        assert!(module.has_card());
    }

    #[test]
    fn typed_text_and_the_key_stay_out_of_debug_output() {
        let typed = "ABCD-EFGH-SECRET";
        let shown = format!("{:?}", FilingMessage::KeyInput(SecretText::from(typed)));
        assert!(!shown.contains("SECRET"), "{shown}");

        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        let mut module = started(&db, shared(&store));
        send(&mut module, FilingMessage::CreateIndex);
        let key_text = module.recovery_key_text().unwrap();
        send(
            &mut module,
            FilingMessage::GroupInput(SecretText::from("TYPEDSECRET")),
        );

        let module_debug = format!("{module:?}");
        let phase_debug = format!("{:?}", module.phase());
        for debug_text in [&module_debug, &phase_debug] {
            assert!(!debug_text.contains("TYPEDSECRET"), "{debug_text}");
            assert!(!debug_text.contains(&key_text), "{debug_text}");
        }
        for group in key_text.split('-') {
            assert!(!phase_debug.contains(group), "{phase_debug}");
        }
    }

    #[test]
    fn nothing_that_carries_a_recovery_key_prints_it() {
        let key_text = FilingModule::varied_recovery_key().display_text();
        let key = || key_text.parse::<RecoveryKey>().unwrap();
        let report = || EncryptReport {
            result: Ok(ShownRecoveryKey::new(key())),
            opened: OpenOutcome::Opened,
        };
        let shown = [
            format!("{:?}", report()),
            format!("{:?}", WorkOutcome::Encrypted(report())),
            format!(
                "{:?}",
                FilingMessage::WorkDone {
                    generation: 1,
                    outcome: WorkOutcome::Encrypted(report()),
                }
            ),
            format!("{:?}", Job::Restore { key: key() }),
            format!("{:?}", Job::Replace { key: key() }),
            format!(
                "{:?}",
                Phase::ShowRecoveryKey {
                    key: ShownRecoveryKey::new(key()),
                    group: 0,
                    typed: SecretText::default(),
                    mismatch: false,
                }
            ),
        ];
        for debug_text in &shown {
            for group in key_text.split('-') {
                assert!(!debug_text.contains(group), "{debug_text}");
            }
        }
    }

    /// Loads fine and has no entry, but cannot keep a new one.
    #[derive(Debug)]
    struct RefusesToSave;

    impl KeyStore for RefusesToSave {
        fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
            Ok(None)
        }

        fn store_new(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Unavailable("locked".to_owned()))
        }

        fn replace(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Unavailable("locked".to_owned()))
        }

        fn delete(&self) -> Result<(), KeyStoreError> {
            Ok(())
        }
    }

    fn begin(module: &mut FilingModule, begin: fn(&mut FilingModule) -> Option<Work>) {
        let work = begin(module);
        assert!(
            work.is_some(),
            "the change was refused: {:?}",
            module.phase()
        );
        settle(module, work);
    }

    #[test]
    fn a_plain_index_is_encrypted_and_its_recovery_key_shown_once() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);
        let store = Arc::new(InMemoryKeyStore::empty());
        let mut module = started(&db, shared(&store));
        assert!(module.can_encrypt());

        begin(&mut module, FilingModule::begin_encrypting);

        assert!(matches!(module.phase(), Phase::ShowRecoveryKey { .. }));
        assert!(module.recovery_key_text().is_some());
        assert!(!Index::file_is_plain(&db).unwrap());
        assert!(store.snapshot().is_some());
        assert_eq!(module.index_encryption(), Some(true));
        assert!(!module.can_encrypt());
        let Phase::ShowRecoveryKey { group, .. } = module.phase() else {
            panic!("the key is shown");
        };
        let asked = module
            .recovery_key_text()
            .unwrap()
            .split('-')
            .nth(*group)
            .unwrap()
            .to_owned();
        send(
            &mut module,
            FilingMessage::GroupInput(SecretText::from(asked)),
        );
        send(&mut module, FilingMessage::GroupSubmit);
        assert_eq!(module.phase(), &Phase::Ready);
        assert!(is_open(&module));
    }

    #[test]
    fn a_failed_encryption_leaves_the_plain_index_open_and_says_why() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);
        let mut module = started(&db, Arc::new(RefusesToSave));

        begin(&mut module, FilingModule::begin_encrypting);

        assert_eq!(module.phase(), &Phase::Ready);
        assert!(matches!(
            module.protection_note(),
            Some(ProtectionNote::EncryptFailed(_))
        ));
        assert!(Index::file_is_plain(&db).unwrap());
        assert!(is_open(&module));
        assert_eq!(module.index_encryption(), Some(false));
        assert!(module.can_encrypt());
    }

    #[test]
    fn no_encryption_is_offered_without_a_credential_store() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);
        let mut module = started(&db, Arc::new(UnavailableStore));

        assert_eq!(module.phase(), &Phase::Ready);
        assert!(!module.can_encrypt());
        assert!(module.begin_encrypting().is_none());
    }

    #[test]
    fn an_encrypted_index_is_not_encrypted_again() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        make_encrypted(&db, &store);
        let module = started(&db, shared(&store));

        assert!(!module.can_encrypt());
    }

    #[test]
    fn starting_over_removes_the_index_files_and_the_key_and_asks_again() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        let store = Arc::new(InMemoryKeyStore::empty());
        make_encrypted(&db, &store);
        let neighbour = data_dir(&dir).join("index.db.bak");
        fs::write(&neighbour, b"mine").unwrap();
        fs::write(data_dir(&dir).join("index.db.encrypting"), b"half").unwrap();
        let mut module = started(&db, shared(&store));
        assert_eq!(module.phase(), &Phase::Ready);

        begin(&mut module, FilingModule::begin_starting_over);

        assert!(module.asks_encryption());
        assert!(!db.exists());
        assert!(!data_dir(&dir).join("index.db.encrypting").exists());
        assert!(neighbour.exists());
        assert_eq!(store.snapshot(), None);
        assert!(module.protection_note().is_none());
    }

    #[test]
    fn starting_over_a_plain_index_works_without_a_key_entry() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);
        let mut module = started(&db, Arc::new(UnavailableStore));

        begin(&mut module, FilingModule::begin_starting_over);

        assert!(module.asks_encryption());
        assert!(!db.exists());
    }

    #[test]
    fn the_files_to_remove_are_only_the_index_files_in_the_data_folder() {
        let dir = TempDir::new().unwrap();
        let data = data_dir(&dir);
        fs::create_dir_all(&data).unwrap();
        let other = dir.path().join("other");
        fs::create_dir_all(&other).unwrap();

        let right = index_files(&data.join("index.db"), &data).unwrap();
        let names: Vec<_> = right
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "index.db.encrypting",
                "index.db-journal",
                "index.db-wal",
                "index.db-shm",
                "index.db",
            ]
        );
        index_files(&data.join("index.db.bak"), &data).unwrap_err();
        index_files(&data.join("notes.txt"), &data).unwrap_err();
        index_files(&other.join("index.db"), &data).unwrap_err();
        index_files(&data.join("index.db"), &data.join("missing")).unwrap_err();
        index_files(Path::new("index.db"), &data).unwrap_err();
    }

    #[test]
    fn a_start_over_of_a_file_not_named_index_db_removes_nothing() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_plain(&db);
        let mut module = started(&db, Arc::new(UnavailableStore));
        module.db_path = data_dir(&dir).join("notes.db");

        begin(&mut module, FilingModule::begin_starting_over);

        assert!(matches!(
            module.protection_note(),
            Some(ProtectionNote::StartOverFailed(_))
        ));
        assert!(db.exists());
    }

    #[test]
    fn a_dismissed_key_card_leaves_the_problem_to_the_overview() {
        let dir = TempDir::new().unwrap();
        let db = db_in(&dir);
        make_encrypted(&db, &InMemoryKeyStore::empty());
        let mut module = started(&db, Arc::new(InMemoryKeyStore::empty()));
        assert_eq!(module.key_problem(), Some(KeyProblem::Missing));

        send(&mut module, FilingMessage::Dismiss);

        assert_eq!(module.phase(), &Phase::Dismissed);
        assert_eq!(module.key_problem(), Some(KeyProblem::Missing));
        assert!(module.can_start_over());
        send(&mut module, FilingMessage::AskKeyAgain);
        assert!(matches!(module.phase(), Phase::AskRecoveryKey { .. }));
    }

    #[test]
    fn a_dismissed_module_without_a_key_problem_cannot_start_over() {
        let mut module = FilingModule::in_phase(Phase::Dismissed);

        assert!(module.key_problem().is_none());
        assert!(!module.can_start_over());
        assert!(module.begin_starting_over().is_none());
    }
}
