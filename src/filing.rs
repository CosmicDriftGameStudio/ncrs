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
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use ncrs_index::{Index, IndexError, IndexStatistics, KeyStore, RecoveryKey};
use ncrs_intake::Location;
use ncrs_pipeline::{intake_steps, IndexAccess, Load, Runner, Settings, SystemClock};

use crate::config::{Filing, FilingLocation};

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
            Self::Failed(_) => "Failed",
            Self::Dismissed => "Dismissed",
        }
    }

    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::AskEncryption { busy: true, .. } | Self::AskRecoveryKey { busy: true, .. }
        )
    }
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
    /// The blocking thread did not finish (it panicked).
    Crashed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenOutcome {
    NoIndexYet,
    Opened,
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
    Start { locations: Vec<FilingLocation> },
    Create { encrypted: bool },
    Restore { key: RecoveryKey },
    Replace { key: RecoveryKey },
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

fn put_index(slot: &IndexSlot, index: Index) {
    *lock(slot) = Some(index);
}

/// Never creates the file or its directory: the user has not chosen yet.
fn open_existing(db_path: &Path, store: &dyn KeyStore, slot: &IndexSlot) -> OpenOutcome {
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
                    Ok(None) | Err(_) => OpenOutcome::Opened,
                }
            };
            put_index(slot, index);
            outcome
        }
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
        let mut module = Self {
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
    pub fn start_pipeline(&self) -> Result<Runner, HeldReason> {
        let handle = self.intake_index()?;
        let locations: Vec<Location> = self.reachable_locations().cloned().collect();
        let logical_cores = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Ok(Runner::start(
            Settings::new(self.load, logical_cores),
            intake_steps(Arc::new(handle)),
            locations,
            Arc::new(SystemClock),
        ))
    }

    fn intake_index(&self) -> Result<IntakeIndex, HeldReason> {
        match &self.phase {
            Phase::Ready => {}
            Phase::Starting => return Err(HeldReason::Starting),
            Phase::AskEncryption { .. }
            | Phase::ShowRecoveryKey { .. }
            | Phase::AskRecoveryKey { .. } => return Err(HeldReason::WaitingForUser),
            Phase::PlainFileWarning => return Err(HeldReason::PlainFileUnconfirmed),
            Phase::Dismissed => return Err(HeldReason::Dismissed),
            Phase::Failed(_) => return Err(HeldReason::Failed),
        }
        if lock(&self.index).is_none() {
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
            Phase::AskEncryption { .. }
                | Phase::ShowRecoveryKey { .. }
                | Phase::AskRecoveryKey { .. }
                | Phase::PlainFileWarning
                | Phase::Failed(_)
        )
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
                // The recovery key is shown once; leaving its card would lose it.
                let stays = matches!(
                    self.phase,
                    Phase::Ready | Phase::Dismissed | Phase::ShowRecoveryKey { .. }
                );
                if !stays && !self.phase.is_busy() {
                    self.generation = next_generation();
                    self.phase = Phase::Dismissed;
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
            WorkOutcome::Crashed(reason) => self.finish_crashed(reason),
        }
        // Safe on the UI thread: no pipeline holds the index before `Ready`,
        // and no work is running once its result is here.
        if self.index_encrypted.is_none() {
            self.index_encrypted = lock(&self.index).as_ref().map(Index::is_encrypted);
        }
    }

    fn enter(&mut self, opened: OpenOutcome) {
        self.phase = match opened {
            OpenOutcome::NoIndexYet => Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
            OpenOutcome::Opened => Phase::Ready,
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

    fn finish_crashed(&mut self, reason: String) {
        if matches!(self.phase, Phase::Starting) {
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
        }
    }

    /// Like `in_phase`, as if the open index had the given encryption.
    pub fn in_phase_with_index(phase: Phase, encrypted: bool) -> Self {
        Self {
            index_encrypted: Some(encrypted),
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
    fn a_ready_module_with_an_index_starts_a_pipeline_that_stops_on_drop() {
        let dir = TempDir::new().unwrap();
        let module = FilingModule::in_phase(Phase::Ready);
        put_index(
            &module.index,
            Index::create_plain(&dir.path().join("index.db")).unwrap(),
        );

        let runner = module.start_pipeline().unwrap();

        assert_ne!(runner.snapshot().state, ncrs_pipeline::RunnerState::Off);
        drop(runner);
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
}
