//! What a run over a source would do. Nothing is moved here; files the index
//! does not know are read into it when the caller allows that.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ncrs_fs::search::PathFilter;
use ncrs_index::{
    ContentHash, DocumentId, Field, FileStamp, Kinds, Rule, RuleId, SenderAliases, TargetId,
};
use ncrs_intake::{
    canonical_path, decide, has_symlink_ancestor_under, hash_path, Decision, Location,
    LocationMode, LocationTransfer, Place, SortSource, SortTarget, Subject, TargetRules, TextState,
    Words,
};

use super::access::{text, SortingAccess};
use super::read_unknown::{find, stamp_at, Found, KnownFile, ReadProblem, ReadUnknown};
use super::ready::WaitReason;
use crate::execute::is_respelling;
use crate::reading_support::{
    in_parallel, is_stopped, read_error, walk_sorted, ReadError, ReadProgress, Walked,
};
use crate::rules_work::{order_moves, ArchiveMove};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRequest {
    /// Need not be a configured one: a one-time run builds its own.
    pub source: SortSource,
    pub files: PlanFiles,
    pub read: ReadUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanFiles {
    All,
    Listed(Vec<PathBuf>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    Source(ReadError),
    /// The include or exclude globs of the source.
    Filter(String),
    Index(String),
    Stopped,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(err) => write!(f, "the source cannot be read: {err:?}"),
            Self::Filter(reason) | Self::Index(reason) => f.write_str(reason),
            Self::Stopped => f.write_str("stopped"),
        }
    }
}

impl std::error::Error for PlanError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortPlan {
    /// The folder of the source; the rows are below it.
    pub root: PathBuf,
    pub rows: Vec<PlanRow>,
    /// The targets that cannot be filed into. While there is one, nothing of
    /// the source may be run.
    pub waiting: Vec<(TargetId, WaitReason)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    /// Below `SortPlan::root`.
    pub relative: PathBuf,
    pub path: PathBuf,
    pub stamp: FileStamp,
    pub hash: Option<ContentHash>,
    pub document: Option<DocumentId>,
    pub state: RowState,
}

/// Where one file goes in one target.
pub type RowPlace = Place;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowState {
    Place(RowPlace),
    Several(Vec<RowPlace>),
    NameTaken {
        place: RowPlace,
        by: Taken,
    },
    NoRule,
    NoText,
    MissingValue {
        target: TargetId,
        rule: RuleId,
        field: Field,
    },
    BadName {
        target: TargetId,
        rule: RuleId,
    },
    AlreadyThere {
        target: TargetId,
        path: PathBuf,
    },
    NotRead(ReadProblem),
    /// Only in plans of the watched sources: the user left it, took its run
    /// back, or it was copied from here.
    SetAside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    OnDisk,
    /// By another file of this plan.
    InPlan,
}

/// Every row counts once, in all but `read`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SortCounts {
    /// All rows but the ones in `not_read`.
    pub read: usize,
    pub by_rule: usize,
    pub already_there: usize,
    pub no_rule: usize,
    pub no_text: usize,
    pub several: usize,
    pub missing_value: usize,
    pub bad_name: usize,
    pub name_taken: usize,
    pub not_read: usize,
    pub aside: usize,
}

impl SortPlan {
    pub fn counts(&self) -> SortCounts {
        let mut counts = SortCounts::default();
        for row in &self.rows {
            let pot = match row.state {
                RowState::Place(_) => &mut counts.by_rule,
                RowState::Several(_) => &mut counts.several,
                RowState::NameTaken { .. } => &mut counts.name_taken,
                RowState::NoRule => &mut counts.no_rule,
                RowState::NoText => &mut counts.no_text,
                RowState::MissingValue { .. } => &mut counts.missing_value,
                RowState::BadName { .. } => &mut counts.bad_name,
                RowState::AlreadyThere { .. } => &mut counts.already_there,
                RowState::NotRead(_) => &mut counts.not_read,
                RowState::SetAside => &mut counts.aside,
            };
            *pot += 1;
        }
        counts.read = self.rows.len() - counts.not_read;
        counts
    }
}

pub fn plan(
    access: &SortingAccess,
    request: &PlanRequest,
    progress: &ReadProgress,
    stop: Option<&AtomicBool>,
) -> Result<SortPlan, PlanError> {
    plan_over(access, request, PlanFor::Once, None, progress, stop)
}

/// The plan of a watched source, which nobody looks at before it is run.
pub(super) fn plan_watched(
    access: &SortingAccess,
    request: &PlanRequest,
) -> Result<SortPlan, PlanError> {
    plan_over(
        access,
        request,
        PlanFor::Watched,
        None,
        &ReadProgress::default(),
        None,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanFor {
    /// The user looks at it and says what is done.
    Once,
    /// What the user set aside stays where it is.
    Watched,
}

/// `plan` with `rules` in place of the saved rules of `target`.
pub fn plan_with_rules(
    access: &SortingAccess,
    request: &PlanRequest,
    target: &TargetId,
    rules: Vec<Rule>,
    progress: &ReadProgress,
    stop: Option<&AtomicBool>,
) -> Result<SortPlan, PlanError> {
    let replaced = Some((target, rules));
    plan_over(access, request, PlanFor::Once, replaced, progress, stop)
}

/// The folder of a source as its files are spelled.
pub(super) fn source_root(source: &SortSource) -> Result<PathBuf, ReadError> {
    canonical_path(&source.path).map_err(|err| read_error(&source.path, &err))
}

pub(super) fn filter_of(source: &SortSource, root: &Path) -> Result<PathFilter, String> {
    PathFilter::new(root.to_path_buf(), &source.include, &source.exclude, false).map_err(text)
}

fn plan_over(
    access: &SortingAccess,
    request: &PlanRequest,
    plan_for: PlanFor,
    replaced: Option<(&TargetId, Vec<Rule>)>,
    progress: &ReadProgress,
    stop: Option<&AtomicBool>,
) -> Result<SortPlan, PlanError> {
    let _work = access.index_work.begin();
    let root = source_root(&request.source).map_err(PlanError::Source)?;
    let filter = filter_of(&request.source, &root).map_err(PlanError::Filter)?;
    let mut files = files_of(access, &root, &request.files, progress).map_err(PlanError::Source)?;
    files.retain(|file| filter.admits_file(&root.join(&file.relative)));
    progress.total.store(files.len(), Ordering::Relaxed);

    let decider = Decider::new(access, &root, request, plan_for, replaced)?;
    let rows = in_parallel(&files, access.load, |file| {
        if is_stopped(stop) {
            return Err(PlanError::Stopped);
        }
        let row = decider.row(file).map_err(PlanError::Index);
        progress.done.fetch_add(1, Ordering::Relaxed);
        row
    });
    let rows = rows.into_iter().collect::<Result<Vec<_>, _>>()?;
    let mut drafts: Vec<Draft> = rows.into_iter().flatten().collect();
    decider.settle_names(&mut drafts);
    Ok(SortPlan {
        waiting: access.waiting(&request.source.targets),
        rows: drafts.into_iter().map(|draft| draft.row).collect(),
        root,
    })
}

/// The files of the source with nothing filtered out but hidden ones, links
/// and what lies in a target below it.
pub(super) fn files_of(
    access: &SortingAccess,
    root: &Path,
    files: &PlanFiles,
    progress: &ReadProgress,
) -> Result<Vec<Walked>, ReadError> {
    let omitted = access.targets_below(root);
    match files {
        PlanFiles::All => walk_sorted(root, &omitted, progress),
        PlanFiles::Listed(paths) => {
            std::fs::read_dir(root).map_err(|err| read_error(root, &err))?;
            let mut found: Vec<Walked> = paths
                .iter()
                .filter_map(|path| listed(root, path))
                .filter(|file| {
                    let path = root.join(&file.relative);
                    !omitted.iter().any(|folder| path.starts_with(folder))
                })
                .collect();
            found.sort_by(|left, right| left.relative.cmp(&right.relative));
            found.dedup_by(|left, right| left.relative == right.relative);
            Ok(found)
        }
    }
}

/// A file named by its path; `None` for one that is gone, hidden, a link or
/// not below `root`.
fn listed(root: &Path, path: &Path) -> Option<Walked> {
    let spelled = if path.starts_with(root) {
        path.to_path_buf()
    } else {
        canonical_path(path.parent()?).ok()?.join(path.file_name()?)
    };
    let relative = spelled.strip_prefix(root).ok()?.to_path_buf();
    let hidden = |part: std::path::Component<'_>| match part {
        std::path::Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => true,
    };
    if relative.components().any(hidden) {
        return None;
    }
    if has_symlink_ancestor_under(root, &spelled).unwrap_or(true) {
        return None;
    }
    let stamp = stamp_at(&spelled)?;
    Some(Walked { relative, stamp })
}

/// A row before the names of the plan are compared with each other.
struct Draft {
    row: PlanRow,
    /// The place of the row is held by this file now, which may be about to
    /// make room in the same run.
    held_by: Option<PathBuf>,
}

struct Decider<'a> {
    access: &'a SortingAccess,
    root: &'a Path,
    request: &'a PlanRequest,
    plan_for: PlanFor,
    location: Location,
    source: SortSource,
    targets: Vec<SortTarget>,
    /// One list per target.
    rules: Vec<Vec<Rule>>,
    kinds: Kinds,
    sender_aliases: SenderAliases,
    in_target: bool,
}

/// What lies at the place a file would go to.
enum Spot {
    Free,
    /// The file itself, or another file with the same content.
    Same,
    Other(PathBuf),
}

impl<'a> Decider<'a> {
    fn new(
        access: &'a SortingAccess,
        root: &'a Path,
        request: &'a PlanRequest,
        plan_for: PlanFor,
        replaced: Option<(&TargetId, Vec<Rule>)>,
    ) -> Result<Self, PlanError> {
        let targets: Vec<SortTarget> = request
            .source
            .targets
            .iter()
            .filter_map(|id| access.target(id))
            .collect();
        let (mut rules, kinds, sender_aliases) = access
            .with_index(|index| {
                let rules = targets
                    .iter()
                    .map(|target| index.rules_of(&target.id).map_err(text))
                    .collect::<Result<Vec<_>, _>>()?;
                let aliases = index.confirmed_sender_aliases().map_err(text)?;
                Ok((rules, access.kinds(index), aliases))
            })
            .map_err(PlanError::Index)?;
        if let Some((id, wanted)) = replaced {
            for (target, rules_of_target) in targets.iter().zip(&mut rules) {
                if target.id == *id {
                    rules_of_target.clone_from(&wanted);
                }
            }
        }
        let in_target = access.is_in_target(root);
        // The files of a target are filed already; they are not taken in as
        // new ones that wait for a place.
        let mode = if in_target {
            LocationMode::Archive
        } else {
            LocationMode::Inbox
        };
        let location = Location::new(root, mode, true).map_err(|err| {
            PlanError::Source(read_error(root, &io::Error::other(err.to_string())))
        })?;
        Ok(Self {
            access,
            root,
            request,
            plan_for,
            location,
            source: SortSource {
                path: root.to_path_buf(),
                ..request.source.clone()
            },
            targets,
            rules,
            kinds,
            sender_aliases,
            in_target,
        })
    }

    /// `None` for a file that is no row of the plan.
    fn row(&self, file: &Walked) -> Result<Option<Draft>, String> {
        let path = self.root.join(&file.relative);
        let found = find(
            self.access,
            &self.location,
            &path,
            file.stamp,
            self.request.read,
        )?;
        let (hash, document, (state, held_by)) = match found {
            Found::Nothing => return Ok(None),
            Found::NotRead(left) => (
                left.hash,
                left.document,
                (RowState::NotRead(left.problem), None),
            ),
            Found::Known(known) => (
                Some(known.hash),
                Some(known.document.id),
                self.state(&file.relative, &path, &known),
            ),
        };
        Ok(Some(Draft {
            row: PlanRow {
                relative: file.relative.clone(),
                path,
                stamp: file.stamp,
                hash,
                document,
                state,
            },
            held_by,
        }))
    }

    fn state(
        &self,
        relative: &Path,
        path: &Path,
        known: &KnownFile,
    ) -> (RowState, Option<PathBuf>) {
        if self.plan_for == PlanFor::Watched {
            if known.aside {
                return (RowState::SetAside, None);
            }
            if known.copied_from_here && !self.in_target {
                return (self.copied_away(known), None);
            }
        }
        let target_rules: Vec<TargetRules<'_>> = self
            .targets
            .iter()
            .zip(&self.rules)
            .map(|(target, rules)| TargetRules { target, rules })
            .collect();
        let manual = known.origins.manual_fields();
        let subject = Subject {
            relative,
            extension: path.extension().and_then(OsStr::to_str),
            text: if known.text.trim().is_empty() {
                TextState::Empty
            } else {
                TextState::Text(&known.text)
            },
            fields: &known.document.fields,
            origins: &known.origins,
            manual: &manual,
            known_at: &known.known_at,
        };
        let words = Words {
            kinds: &self.kinds,
            sender_aliases: &self.sender_aliases,
        };
        match decide(&subject, &self.source, &target_rules, &words) {
            Decision::File(place) => self.placed(path, known.hash, place),
            Decision::Several(places) => (self.several(path, known.hash, places), None),
            Decision::NoRule => (RowState::NoRule, None),
            Decision::NoText => (RowState::NoText, None),
            Decision::NotRead => (RowState::NotRead(ReadProblem::Unknown), None),
            Decision::MissingValue {
                target,
                rule,
                field,
            } => (
                RowState::MissingValue {
                    target,
                    rule,
                    field,
                },
                None,
            ),
            Decision::BadName { target, rule } => (RowState::BadName { target, rule }, None),
            Decision::AlreadyThere { target, path } => {
                (RowState::AlreadyThere { target, path }, None)
            }
        }
    }

    fn copied_away(&self, known: &KnownFile) -> RowState {
        for target in &self.targets {
            if let Some(path) = known
                .known_at
                .iter()
                .find(|place| place.starts_with(&target.path))
            {
                return RowState::AlreadyThere {
                    target: target.id.clone(),
                    path: path.clone(),
                };
            }
        }
        RowState::SetAside
    }

    fn placed(
        &self,
        path: &Path,
        hash: ContentHash,
        place: RowPlace,
    ) -> (RowState, Option<PathBuf>) {
        let Some(to) = self.absolute(&place) else {
            return (RowState::NoRule, None);
        };
        match spot(path, &to, hash) {
            Spot::Free => (RowState::Place(place), None),
            Spot::Same => (
                RowState::AlreadyThere {
                    target: place.target,
                    path: to,
                },
                None,
            ),
            Spot::Other(holder) if self.renames_within(&place.target) => {
                (RowState::Place(place), Some(holder))
            }
            Spot::Other(_) => (
                RowState::NameTaken {
                    place,
                    by: Taken::OnDisk,
                },
                None,
            ),
        }
    }

    /// One taken place leaves the whole file to the user, as a run would only
    /// half file it.
    fn several(&self, path: &Path, hash: ContentHash, places: Vec<RowPlace>) -> RowState {
        let mut there = None;
        let mut open = Vec::new();
        for place in places {
            let Some(to) = self.absolute(&place) else {
                continue;
            };
            match spot(path, &to, hash) {
                Spot::Free => open.push(place),
                Spot::Same => there = Some((place.target, to)),
                Spot::Other(_) => {
                    return RowState::NameTaken {
                        place,
                        by: Taken::OnDisk,
                    }
                }
            }
        }
        match (open.len(), there) {
            (0, Some((target, path))) => RowState::AlreadyThere { target, path },
            (0, None) => RowState::NoRule,
            (1, _) => RowState::Place(open.remove(0)),
            _ => RowState::Several(open),
        }
    }

    fn target_path(&self, id: &TargetId) -> Option<&Path> {
        self.targets
            .iter()
            .find(|target| target.id == *id)
            .map(|target| target.path.as_path())
    }

    fn absolute(&self, place: &RowPlace) -> Option<PathBuf> {
        Some(self.target_path(&place.target)?.join(&place.relative))
    }

    /// The run moves the files of this source around inside the target.
    fn renames_within(&self, target: &TargetId) -> bool {
        self.source.transfer == LocationTransfer::Move
            && self
                .target_path(target)
                .is_some_and(|folder| self.root.starts_with(folder))
    }

    /// The first row by path gets a name two rows want; a place held by a file
    /// counts as free only when that file moves away in the same run.
    fn settle_names(&self, drafts: &mut [Draft]) {
        self.drop_held_places(drafts);
        // Case is folded on every system: a name that differs only in case is
        // left to the user on Linux too. The Unicode form is caught on the disk.
        let mut taken: HashSet<String> = HashSet::new();
        for draft in drafts.iter_mut() {
            let places: &[RowPlace] = match &draft.row.state {
                RowState::Place(place) => std::slice::from_ref(place),
                RowState::Several(places) => places,
                _ => continue,
            };
            let keys: Vec<String> = places
                .iter()
                .filter_map(|place| self.absolute(place))
                .map(|to| to.to_string_lossy().to_lowercase())
                .collect();
            let lost = keys.iter().position(|key| taken.contains(key));
            if let Some(place) = lost.and_then(|at| places.get(at)).cloned() {
                draft.row.state = RowState::NameTaken {
                    place,
                    by: Taken::InPlan,
                };
            } else {
                taken.extend(keys);
            }
        }
    }

    fn drop_held_places(&self, drafts: &mut [Draft]) {
        if drafts.iter().all(|draft| draft.held_by.is_none()) {
            return;
        }
        let moves: Vec<(usize, ArchiveMove)> = drafts
            .iter()
            .enumerate()
            .filter_map(|(at, draft)| Some((at, self.archive_move(&draft.row)?)))
            .collect();
        let can_run = movable(
            moves
                .iter()
                .map(|(at, found)| {
                    let held_by = drafts.get(*at).and_then(|draft| draft.held_by.clone());
                    (found.clone(), held_by)
                })
                .collect(),
        );
        for (at, found) in moves {
            let Some(draft) = drafts.get_mut(at) else {
                continue;
            };
            if draft.held_by.is_some() && !can_run.contains(&found.root.join(&found.from)) {
                if let RowState::Place(place) = draft.row.state.clone() {
                    draft.row.state = RowState::NameTaken {
                        place,
                        by: Taken::OnDisk,
                    };
                }
            }
        }
    }

    fn archive_move(&self, row: &PlanRow) -> Option<ArchiveMove> {
        let RowState::Place(place) = &row.state else {
            return None;
        };
        let folder = self.target_path(&place.target)?;
        Some(ArchiveMove {
            root: folder.to_path_buf(),
            from: row.path.strip_prefix(folder).ok()?.to_path_buf(),
            to: place.relative.clone(),
            hash: row.hash?,
        })
    }
}

/// The files that can move: a file held by another one waits for it, so a
/// circle of renames and everything behind it stays.
fn movable(mut moves: Vec<(ArchiveMove, Option<PathBuf>)>) -> HashSet<PathBuf> {
    loop {
        let leaving: HashSet<PathBuf> = moves
            .iter()
            .map(|(found, _)| found.root.join(&found.from))
            .collect();
        let before = moves.len();
        moves.retain(|(_, held_by)| {
            held_by
                .as_ref()
                .is_none_or(|holder| leaving.contains(holder))
        });
        let (ordered, _) = order_moves(moves.iter().map(|(found, _)| found.clone()).collect());
        let runs: HashSet<PathBuf> = ordered
            .iter()
            .map(|found| found.root.join(&found.from))
            .collect();
        moves.retain(|(found, _)| runs.contains(&found.root.join(&found.from)));
        if moves.len() == before {
            return runs;
        }
    }
}

fn spot(path: &Path, to: &Path, hash: ContentHash) -> Spot {
    match std::fs::symlink_metadata(to) {
        // Anything else than "not there" cannot be told free.
        Err(err) if err.kind() == io::ErrorKind::NotFound => Spot::Free,
        Err(_) => Spot::Other(to.to_path_buf()),
        Ok(_) if to == path => Spot::Same,
        Ok(_) if is_respelling(path, to) => Spot::Free,
        Ok(_) => match hash_path(to) {
            Ok(there) if there == hash => Spot::Same,
            _ => Spot::Other(to.to_path_buf()),
        },
    }
}
