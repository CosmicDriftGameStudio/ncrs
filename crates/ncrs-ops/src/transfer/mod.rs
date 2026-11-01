//! What the copy and move dialog knows: the selection, what a background scan
//! found in it, how many names the target already has, and whether the target
//! may be written to at all.
//!
//! Everything that touches the disk is a free function taking plain paths, so
//! the app runs it on the blocking pool and the tests run it on a tempdir.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub mod view;

pub use crate::message::TransferDialogMessage;

use ncrs_core::TransferKind;
use ncrs_fs::archive;
use ncrs_fs::remote::{self, RemotePath};
use ncrs_fs::transfer::OnConflict;

/// Up to this many entries the dialog names them all; above it, numbers.
pub const NAMES_DIRECTLY: usize = 20;

/// How many names stand in front of "and N more" in the summary.
pub const FIRST_NAMES: usize = 3;

/// The expanded list shows this many names at most: a text widget per name
/// makes a list of ten thousand unusable, and the scrollable would not help.
pub const LISTED_NAMES: usize = 300;

/// What to do when the target already has a name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Policy {
    /// Ask per file; the answer dialog offers "for all".
    #[default]
    Ask,
    Overwrite,
    Skip,
}

impl Policy {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Overwrite, Self::Skip];

    /// What the filesystem layer does with the first row. `Ask` lets it fail
    /// on a taken name, which is what opens the question.
    pub const fn on_conflict(self) -> OnConflict {
        match self {
            Self::Ask => OnConflict::Fail,
            Self::Overwrite => OnConflict::Overwrite,
            Self::Skip => OnConflict::Skip,
        }
    }

    /// The answer that holds for every later row, if the user already gave it.
    pub const fn standing_rule(self) -> Option<OnConflict> {
        match self {
            Self::Ask => None,
            Self::Overwrite => Some(OnConflict::Overwrite),
            Self::Skip => Some(OnConflict::Skip),
        }
    }
}

/// What a walk through the selection counted. Links are never followed: one
/// counts as a file of the size of the link itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scan {
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    /// Entries that could not be read or listed; they are left out of the
    /// other numbers.
    pub unreadable: usize,
}

impl Scan {
    /// An entry that is on no disk: counted from the archive's index, or
    /// unreadable when it is not in one either.
    fn count_archive_entry(&mut self, path: &Path) {
        match archive::measure(path) {
            Some(inside) => {
                self.files += inside.files;
                self.folders += inside.folders;
                self.bytes += inside.bytes;
            }
            None => self.unreadable += 1,
        }
    }
}

/// Walks `sources` without following links, until done or `cancel` is set.
/// Blocking: the app calls it on the blocking pool.
pub fn scan(sources: &[PathBuf], cancel: &AtomicBool) -> Scan {
    let mut found = Scan::default();
    let mut pending: Vec<PathBuf> = sources.to_vec();
    while let Some(path) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            found.count_archive_entry(&path);
            continue;
        };
        if !meta.is_dir() {
            found.files += 1;
            found.bytes += meta.len();
            continue;
        }
        found.folders += 1;
        match std::fs::read_dir(&path) {
            Ok(children) => {
                for listed in children {
                    match listed {
                        Ok(child) => pending.push(child.path()),
                        Err(_) => found.unreadable += 1,
                    }
                }
            }
            Err(_) => found.unreadable += 1,
        }
    }
    found
}

/// How many top-level names of `sources` the folder `target` already has.
/// Looked up without following links, so a dangling link counts as taken:
/// the copy would run into it too. A folder inside an archive is read from the
/// archive's index.
pub fn count_conflicts(sources: &[PathBuf], target: &Path, cancel: &AtomicBool) -> usize {
    let names = sources
        .iter()
        .take_while(|_| !cancel.load(Ordering::Relaxed))
        .filter_map(|source| source.file_name());
    if let Some(location) = archive::locate_in_archive(target) {
        let Ok(listing) = archive::list(&location) else {
            return 0;
        };
        return names
            .filter(|name| listing.entries.iter().any(|entry| entry.name == **name))
            .count();
    }
    names
        .filter(|name| std::fs::symlink_metadata(target.join(name)).is_ok())
        .count()
}

/// Why a target is refused before anything is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Missing,
    NotAFolder,
    /// The target is the folder a selected entry is in.
    SameFolder,
    /// The target is a selected folder or lies below one.
    InsideSource,
    /// The system would not say (permission, a mount that does not answer).
    Unreadable,
    /// A move was asked into an archive or a folder inside one; only copying
    /// goes in.
    IntoArchive,
    /// Entries of an archive were asked into an archive, or the target lies
    /// inside an archive that is itself inside an archive.
    ArchiveInArchive,
    /// Entries on a server were asked into an archive.
    ServerToArchive,
    /// A move was asked for entries that lie inside an archive.
    MoveFromArchive,
    /// A move was asked to a folder on a server.
    MoveToServer,
    /// A move was asked for entries on a server.
    MoveFromServer,
    /// The entries and the target are on servers.
    ServerToServer,
    /// The target starts like an address (`sftp://`) but is none: it is not a
    /// path of this computer either, so the disk is never asked about it.
    BadAddress,
}

fn refusal_of(error: &io::Error) -> Refusal {
    if error.kind() == io::ErrorKind::NotFound {
        Refusal::Missing
    } else {
        Refusal::Unreadable
    }
}

/// Checks `target` against the selection and returns the resolved folder to
/// transfer into.
///
/// Both sides are resolved through links first (`canonicalize`), so a link
/// into the source or a `..` in the typed text cannot get past the check the
/// way it would past a comparison of the text. The last component of a source
/// is not resolved: copying a link copies the link, so the source *is* the
/// link, not what it points to.
pub fn validate(
    kind: TransferKind,
    sources: &[PathBuf],
    target: &Path,
) -> Result<PathBuf, Refusal> {
    // Before the disk is asked anything: an address is no path of this
    // computer, and text that starts like one but is none is not either. A
    // server is only copied to and from.
    if looks_like_address(&target.to_string_lossy()) && RemotePath::parse(target).is_none() {
        return Err(Refusal::BadAddress);
    }
    let target_on_server = RemotePath::parse(target).is_some();
    let source_on_server = sources
        .iter()
        .any(|source| RemotePath::parse(source).is_some());
    if kind == TransferKind::Move && (target_on_server || source_on_server) {
        return Err(if source_on_server {
            Refusal::MoveFromServer
        } else {
            Refusal::MoveToServer
        });
    }
    if target_on_server && source_on_server {
        return Err(Refusal::ServerToServer);
    }
    if target_on_server {
        return Ok(target.to_path_buf());
    }
    // `into` is the way through the archive when the target is one, or a
    // place in one.
    let (resolved, into) = match std::fs::canonicalize(target) {
        Ok(resolved) if archive::is_archive_file(&resolved) => (resolved, Some(Vec::new())),
        Ok(resolved) => (resolved, None),
        Err(error) => match archive::locate_in_archive(target) {
            Some(location) => {
                let archive = std::fs::canonicalize(&location.archive)
                    .map_err(|canonical| refusal_of(&canonical))?;
                (archive, Some(location.inner))
            }
            None => return Err(refusal_of(&error)),
        },
    };
    if let Some(inner) = &into {
        if kind == TransferKind::Move {
            return Err(Refusal::IntoArchive);
        }
        if inner
            .iter()
            .any(|part| archive::is_archive_name(OsStr::new(part)))
        {
            return Err(Refusal::ArchiveInArchive);
        }
    } else if !resolved.is_dir() {
        return Err(Refusal::NotAFolder);
    }
    for source in sources {
        if RemotePath::parse(source).is_some() {
            if into.is_some() {
                return Err(Refusal::ServerToArchive);
            }
            // An entry on a server is on no disk of this computer either: its
            // folder cannot be this target's, and the target is no part of it.
            continue;
        }
        // An entry of an archive is on no disk: it has no folder to be the same
        // as and nothing the target could lie inside.
        if lies_in_archive(source) {
            if kind == TransferKind::Move {
                return Err(Refusal::MoveFromArchive);
            }
            if into.is_some() {
                return Err(Refusal::ArchiveInArchive);
            }
            continue;
        }
        let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
            continue;
        };
        let resolved_parent = std::fs::canonicalize(parent).map_err(|error| refusal_of(&error))?;
        if into.is_none() && resolved_parent == resolved {
            return Err(Refusal::SameFolder);
        }
        // For an archive this also catches the archive being a selected
        // entry, or lying below a selected folder.
        if resolved.starts_with(resolved_parent.join(name)) {
            return Err(Refusal::InsideSource);
        }
    }
    Ok(match into {
        Some(inner) => inner.iter().fold(resolved, |path, part| path.join(part)),
        None => resolved,
    })
}

/// Whether `text` starts like a server address, whatever follows.
fn looks_like_address(text: &str) -> bool {
    text.trim()
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("sftp://"))
}

fn lies_in_archive(source: &Path) -> bool {
    std::fs::symlink_metadata(source).is_err() && archive::locate_in_archive(source).is_some()
}

/// The open confirmation. The selection is resolved once, when the key is
/// pressed: by the time the user confirms, the cursor may have moved.
#[derive(Debug)]
pub struct TransferDialog {
    id: u64,
    kind: TransferKind,
    sources: Vec<PathBuf>,
    /// What a relative target is relative to: the other panel's folder.
    base: PathBuf,
    /// The home folder on the server of `base`, for a `~` typed in the target.
    remote_home: Option<String>,
    target: String,
    policy: Policy,
    scan: Option<Scan>,
    conflicts: Option<usize>,
    edit: u64,
    list_open: bool,
    refusal: Option<Refusal>,
    /// Set when the dialog goes away, which stops the scan and the count.
    stop_background: Arc<AtomicBool>,
    /// Sources that share a name, with the name each gets in the target.
    numbered: Vec<(PathBuf, OsString)>,
    /// The folder of the hit list the sources come from, if they do.
    hit_scope: Option<PathBuf>,
}

impl Drop for TransferDialog {
    fn drop(&mut self) {
        self.stop_background.store(true, Ordering::Relaxed);
    }
}

impl TransferDialog {
    pub fn new(id: u64, kind: TransferKind, sources: Vec<PathBuf>, base: PathBuf) -> Self {
        let target = base.display().to_string();
        Self {
            numbered: ncrs_fs::flat_names::members_of_shared_names(&sources),
            hit_scope: None,
            remote_home: None,
            id,
            kind,
            sources,
            base,
            target,
            policy: Policy::default(),
            scan: None,
            conflicts: None,
            edit: 0,
            list_open: false,
            refusal: None,
            stop_background: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Tells the dialog the sources are the rows of a hit list that ran in
    /// `scope`, so the hint names their folders relative to it.
    pub fn with_hit_scope(mut self, scope: Option<PathBuf>) -> Self {
        self.hit_scope = scope;
        self
    }

    pub fn numbered(&self) -> &[(PathBuf, OsString)] {
        &self.numbered
    }

    pub fn hit_scope(&self) -> Option<&Path> {
        self.hit_scope.as_deref()
    }

    /// Tells the dialog where `~` leads when the target is on a server.
    pub fn with_remote_home(mut self, home: Option<String>) -> Self {
        self.remote_home = home;
        self
    }

    /// Some of what is to be copied is on a server, so nothing was counted.
    pub fn counts_unknown(&self) -> bool {
        self.sources
            .iter()
            .any(|source| RemotePath::parse(source).is_some())
    }

    /// Some of what is to be copied is on a server, or the target is.
    pub fn involves_server(&self) -> bool {
        self.counts_unknown()
            || looks_like_address(&self.target)
            || RemotePath::parse(&self.target_path()).is_some()
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn kind(&self) -> TransferKind {
        self.kind
    }

    pub fn set_kind(&mut self, kind: TransferKind) {
        self.kind = kind;
    }

    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    pub fn target_text(&self) -> &str {
        &self.target
    }

    /// The typed target as a path: a relative one means "below the other
    /// panel's folder".
    pub fn target_path(&self) -> PathBuf {
        let typed = self.target.trim();
        // An address typed in full is the target, whatever the other panel shows.
        if looks_like_address(typed) {
            return remote::parse_url(typed)
                .ok()
                .and_then(|url| url.remote_path())
                .map_or_else(|| PathBuf::from(typed), |place| place.to_path_buf());
        }
        // Over a server's folder the text is read on the server, as text.
        if let Some(place) = RemotePath::parse(&self.base) {
            let home = self.remote_home.as_deref().unwrap_or("/");
            return place.resolve_typed(typed, home).to_path_buf();
        }
        self.base.join(&self.target)
    }

    pub const fn policy(&self) -> Policy {
        self.policy
    }

    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    pub const fn scan(&self) -> Option<Scan> {
        self.scan
    }

    pub fn set_scan(&mut self, scan: Option<Scan>) {
        self.scan = scan;
    }

    pub const fn conflicts(&self) -> Option<usize> {
        self.conflicts
    }

    pub const fn edit(&self) -> u64 {
        self.edit
    }

    pub fn set_conflicts(&mut self, edit: u64, count: usize) {
        if edit == self.edit {
            self.conflicts = Some(count);
        }
    }

    pub const fn list_open(&self) -> bool {
        self.list_open
    }

    pub fn toggle_list(&mut self) {
        self.list_open = !self.list_open;
    }

    pub const fn refusal(&self) -> Option<Refusal> {
        self.refusal
    }

    pub fn refuse(&mut self, refusal: Refusal) {
        self.refusal = Some(refusal);
    }

    /// Typing changes the target: what was counted for the old text and the
    /// refusal of the old text no longer hold.
    pub fn type_target(&mut self, text: String) {
        self.target = text;
        self.edit = self.edit.wrapping_add(1);
        self.conflicts = None;
        self.refusal = None;
    }

    /// The flag the background work polls.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_background)
    }

    /// Whether the names are listed one by one rather than summed up.
    pub fn names_directly(&self) -> bool {
        self.sources.len() <= NAMES_DIRECTLY
    }

    /// The top-level names, as they are shown.
    pub fn names(&self) -> impl Iterator<Item = String> + '_ {
        self.sources
            .iter()
            .filter_map(|source| source.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
