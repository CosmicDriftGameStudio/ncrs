//! The live plan of the filing tab: every file of the source with the state
//! it has right now, from "waiting" to where it would be filed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use ncrs_ai::{AiError, ChatModel};
use ncrs_index::{ContentHash, Transfer};

use super::groups::numbered_name;
use super::picks::ask_until_done;
use super::reading::{list_files, read_folder_only, UnknownFiles};
use super::trial::{
    PlacedBy, PlanFailure, StayReason, TrialEntry, TrialOutcome, TrialPlan, FOLDER_PICK_VERSION,
};
use super::{ChatAccess, FolderReading, ModelScope, ReadProgress, RunItem, RunPlan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveState {
    Waiting,
    Looking,
    /// Left without a folder because the AI cannot choose one; the plan is
    /// finished, nothing is being read.
    NeedsModel,
    /// `folder` is relative to the target.
    Filed {
        folder: PathBuf,
        name: String,
        unsure: bool,
    },
    AlreadyThere {
        at: PathBuf,
    },
    NoFolder,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFile {
    /// Relative to the source.
    pub from: PathBuf,
    pub hash: Option<ContentHash>,
    pub state: LiveState,
    pub by_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePlan {
    pub source_root: PathBuf,
    pub target_root: PathBuf,
    pub files: Vec<LiveFile>,
    pub done: bool,
    /// The model cannot choose folders (own server): the files it would have
    /// decided stay untouched, and the plan is done without them.
    pub folder_choice_unsupported: bool,
}

/// What the user decided for a file, by its path relative to the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Override {
    Folder(PathBuf),
    Name(String),
    Stay,
}

/// A running plan, shared with the UI that polls it on a tick.
#[derive(Debug, Default)]
pub struct LiveRun {
    pub progress: ReadProgress,
    pub plan: Mutex<Option<LivePlan>>,
    /// Counts the plans published, so a poller copies only a new one.
    pub version: AtomicU64,
    pub stop: AtomicBool,
}

impl LiveRun {
    fn publish(&self, plan: LivePlan) {
        *self.plan.lock().unwrap_or_else(PoisonError::into_inner) = Some(plan);
        self.version.fetch_add(1, Ordering::Release);
    }
}

/// The tag names, in the user's language, that mark files nothing could be
/// done with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTags {
    pub unreadable: String,
    pub no_folder: String,
    /// The text of `no_folder` in every language, to find the tag an earlier
    /// run put on a file under another language.
    pub no_folder_in_every_language: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub waiting: usize,
    pub looking: usize,
    pub filed: usize,
    /// The distinct folders the filed files go to.
    pub folders: usize,
    pub already_there: usize,
    pub no_folder: usize,
    pub unreadable: usize,
}

fn read_failed(err: impl std::fmt::Debug) -> String {
    format!("{err:?}")
}

/// Walks the source and publishes every file as waiting, then reads both
/// folders and lets the model pick page by page, publishing after each round.
/// Ends early, without error, when `run.stop` is set. At the end the files
/// that are unreadable or have no folder get their tag (see `mark_files`),
/// also after a stop.
pub fn plan_live(
    access: &ChatAccess,
    source: &Path,
    target: &Path,
    model: &dyn ChatModel,
    tags: &LiveTags,
    run: &LiveRun,
) -> Result<(), PlanFailure> {
    let mut last = None;
    let asked = plan_from(
        access,
        source,
        target,
        PlanScope {
            listed: None,
            tags: Some(tags),
        },
        model,
        run,
        &mut last,
    );
    // Files the model did not answer for are still looking, and `mark_files`
    // leaves those alone, so a failed model tags nothing it did not decide.
    if let Some(planned) = last {
        let marked = mark_files(access, &planned, tags);
        if planned.done {
            run.publish(planned);
        }
        marked?;
    }
    asked
}

/// `plan_live` for the files in `listed` only, by their path relative to
/// `source`; nothing is tagged. The model is asked in the background lane,
/// once. Returns the plan, `None` when the run was stopped before the first
/// round.
pub fn plan_listed(
    access: &ChatAccess,
    source: &Path,
    target: &Path,
    listed: &[PathBuf],
    model: &dyn ChatModel,
    run: &LiveRun,
) -> Result<Option<LivePlan>, String> {
    let mut last = None;
    plan_from(
        access,
        source,
        target,
        PlanScope {
            listed: Some(listed),
            tags: None,
        },
        model,
        run,
        &mut last,
    )
    .map_err(|failure| failure.to_string())?;
    if let Some(planned) = last.as_ref().filter(|planned| planned.done) {
        run.publish(planned.clone());
    }
    Ok(last)
}

/// What a planning run covers: all files of the source, or only `listed`.
/// Only a run with `tags` is the filing tab's, which clears stale answers.
struct PlanScope<'a> {
    listed: Option<&'a [PathBuf]>,
    tags: Option<&'a LiveTags>,
}

/// Plans into `last`; whatever it holds is good even when the run failed.
fn plan_from(
    access: &ChatAccess,
    source: &Path,
    target: &Path,
    covered: PlanScope<'_>,
    model: &dyn ChatModel,
    run: &LiveRun,
    last: &mut Option<LivePlan>,
) -> Result<(), PlanFailure> {
    let PlanScope { listed, tags } = covered;
    let wanted: Option<BTreeSet<&PathBuf>> = listed.map(|files| files.iter().collect());
    let is_wanted = |relative: &PathBuf| wanted.as_ref().is_none_or(|set| set.contains(relative));
    let waiting: Vec<LiveFile> = list_files(source)
        .map_err(read_failed)?
        .into_iter()
        .filter(is_wanted)
        .map(|from| LiveFile {
            from,
            hash: None,
            state: LiveState::Waiting,
            by_user: false,
        })
        .collect();
    run.publish(LivePlan {
        source_root: source.to_path_buf(),
        target_root: target.to_path_buf(),
        files: waiting,
        done: false,
        folder_choice_unsupported: false,
    });

    // A "none fits" answer stored before the model could choose folders is not
    // an answer, and the tag it put on the file is not true.
    if let Some(clearing) = tags {
        access
            .with_index(|index| {
                index
                    .forget_empty_picks_before(
                        FOLDER_PICK_VERSION,
                        &clearing.no_folder_in_every_language,
                    )
                    .map_err(|err| err.to_string())
            })
            .map_err(PlanFailure::Other)?;
    }
    let source_reading = read_folder_only(
        access,
        source,
        &run.progress,
        None,
        wanted.as_ref(),
        UnknownFiles::Extract,
    )
    .map_err(read_failed)?;
    let target_reading = read_folder_only(
        access,
        target,
        &ReadProgress::default(),
        None,
        None,
        UnknownFiles::Skip,
    )
    .map_err(read_failed)?;
    let scope = match listed {
        Some(files) => ModelScope::Only(files.iter().map(|path| source.join(path)).collect()),
        None => ModelScope::FirstPage,
    };
    ask_until_done(
        access,
        &source_reading,
        &target_reading,
        model,
        &scope,
        Some(&run.stop),
        &mut |trial_plan| {
            let mut live = classify(trial_plan, &source_reading);
            live.folder_choice_unsupported = trial_plan.model_failure == Some(AiError::Unsupported);
            live.done = trial_plan.model_left.is_empty() || live.folder_choice_unsupported;
            if live.folder_choice_unsupported {
                for file in &mut live.files {
                    if file.state == LiveState::Looking {
                        file.state = LiveState::NeedsModel;
                    }
                }
            }
            // `done` goes out only after the tags are written, so nothing
            // that acts on a finished plan races with `mark_files` below.
            run.publish(LivePlan {
                done: false,
                ..live.clone()
            });
            *last = Some(live);
        },
    )
}

/// Tags the unreadable and folderless files of `plan` and takes both tags off
/// the files that are filed or already there; waiting and looking files keep
/// what they have. Call it again with the user's overrides applied.
pub fn mark_files(access: &ChatAccess, plan: &LivePlan, tags: &LiveTags) -> Result<(), String> {
    access.with_index(|index| {
        for file in &plan.files {
            let Some(hash) = &file.hash else { continue };
            let marked = match file.state {
                LiveState::Unreadable => Some(&tags.unreadable),
                LiveState::NoFolder => Some(&tags.no_folder),
                LiveState::Filed { .. } | LiveState::AlreadyThere { .. } => None,
                LiveState::Waiting | LiveState::Looking | LiveState::NeedsModel => continue,
            };
            for tag in [&tags.unreadable, &tags.no_folder] {
                if Some(tag) == marked {
                    index.add_document_tag(hash, tag)
                } else {
                    index.remove_document_tag(hash, tag).map(|()| true)
                }
                .map_err(|err| err.to_string())?;
            }
        }
        Ok(())
    })
}

/// The state of each file of `source`, in its order. A file the trial has no
/// entry for stays without a folder, so the plan has every file of the read.
pub fn classify(plan: &TrialPlan, source: &FolderReading) -> LivePlan {
    let entries: BTreeMap<&Path, &TrialEntry> = plan
        .entries
        .iter()
        .map(|entry| (entry.from.as_path(), entry))
        .collect();
    let mut taken = plan.taken.clone();
    let files = source
        .files
        .iter()
        .map(|file| {
            let from = source.root.join(&file.relative_path);
            // A broken PDF comes back with an empty text, not with none.
            let without_text = file
                .text
                .as_deref()
                .is_none_or(|text| text.trim().is_empty());
            let state = if plan.model_left.contains(&from) {
                LiveState::Looking
            } else {
                match entries.get(from.as_path()) {
                    Some(entry) => state_of(entry, without_text, &mut taken),
                    None if without_text => LiveState::Unreadable,
                    None => LiveState::NoFolder,
                }
            };
            LiveFile {
                from: file.relative_path.clone(),
                hash: Some(file.hash),
                state,
                by_user: false,
            }
        })
        .collect();
    LivePlan {
        source_root: plan.source_root.clone(),
        target_root: plan.target_root.clone(),
        files,
        done: false,
        folder_choice_unsupported: false,
    }
}

fn state_of(entry: &TrialEntry, without_text: bool, taken: &mut BTreeSet<PathBuf>) -> LiveState {
    let unsure = |folder: PathBuf, name: String| LiveState::Filed {
        folder,
        name,
        unsure: true,
    };
    match &entry.outcome {
        TrialOutcome::AlreadyThere { at } => LiveState::AlreadyThere { at: at.clone() },
        _ if without_text => LiveState::Unreadable,
        TrialOutcome::Filed { to } => {
            let folder = parent_of(to);
            let name = name_in(&folder, entry, file_name_of(to), taken);
            LiveState::Filed {
                folder,
                name,
                unsure: !entry.placed_by.is_none_or(PlacedBy::is_sure),
            }
        }
        TrialOutcome::Unsure {
            choices,
            file_names,
            ..
        } => match (choices.first(), file_names.first()) {
            (Some(folder), Some(name)) => {
                unsure(folder.clone(), name_in(folder, entry, name.clone(), taken))
            }
            _ => LiveState::NoFolder,
        },
        TrialOutcome::Stays(StayReason::NoPattern) => match &entry.neighbour {
            Some(folder) => unsure(
                folder.clone(),
                name_in(folder, entry, entry.name.clone(), taken),
            ),
            None => LiveState::NoFolder,
        },
        TrialOutcome::Stays(StayReason::NameTaken) => match &entry.wanted {
            Some(wanted) => {
                let free = numbered_name(wanted, taken);
                unsure(parent_of(&free), file_name_of(&free))
            }
            None => LiveState::NoFolder,
        },
        TrialOutcome::Stays(_) => LiveState::NoFolder,
    }
}

/// A name from the document's fields, "2020-04-04 Alfa Rechnung.pdf": the
/// date, the sender and the kind, whichever are known, with the extension of
/// `original`. `None` without a date and without a sender: the scan name
/// stays then.
fn proposed_name(entry: &TrialEntry, original: &str) -> Option<String> {
    let fields = &entry.fields;
    let date = fields.date.map(|date| date.format("%Y-%m-%d").to_string());
    let sender = fields
        .sender
        .as_deref()
        .and_then(ncrs_intake::sanitise_value);
    if date.is_none() && sender.is_none() {
        return None;
    }
    let kind = entry
        .kind_word
        .as_deref()
        .and_then(ncrs_intake::sanitise_value);
    let stem = [date, sender, kind]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    Some(match Path::new(original).extension() {
        Some(extension) => format!("{stem}.{}", extension.to_string_lossy()),
        None => stem,
    })
}

/// The planned name of a file that would keep its source name: the name the
/// fields give instead, numbered when the folder already has it.
fn name_in(
    folder: &Path,
    entry: &TrialEntry,
    planned: String,
    taken: &mut BTreeSet<PathBuf>,
) -> String {
    if planned != entry.name {
        return planned;
    }
    match proposed_name(entry, &planned) {
        Some(name) => file_name_of(&numbered_name(&folder.join(name), taken)),
        None => planned,
    }
}

fn parent_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The plan with the user's decisions on top; a changed file is `by_user`.
pub fn apply_overrides(plan: &LivePlan, overrides: &BTreeMap<PathBuf, Override>) -> LivePlan {
    let mut files: Vec<LiveFile> = plan
        .files
        .iter()
        .map(|file| {
            let Some(decision) = overrides.get(&file.from) else {
                return file.clone();
            };
            let state = match (decision, &file.state) {
                (Override::Folder(folder), LiveState::Filed { name, .. }) => LiveState::Filed {
                    folder: folder.clone(),
                    name: name.clone(),
                    unsure: false,
                },
                (Override::Folder(folder), _) => LiveState::Filed {
                    folder: folder.clone(),
                    name: file_name_of(&file.from),
                    unsure: false,
                },
                (Override::Name(new_name), LiveState::Filed { folder, .. }) => LiveState::Filed {
                    folder: folder.clone(),
                    name: new_name.clone(),
                    unsure: false,
                },
                (Override::Name(_), _) => return file.clone(),
                (Override::Stay, _) => LiveState::NoFolder,
            };
            LiveFile {
                state,
                by_user: true,
                ..file.clone()
            }
        })
        .collect();
    number_colliding_overrides(&mut files, overrides);
    LivePlan {
        files,
        ..plan.clone()
    }
}

/// Two files may not end up at the same place because of what the user typed
/// or picked: the later one gets the next free number.
fn number_colliding_overrides(files: &mut [LiveFile], overrides: &BTreeMap<PathBuf, Override>) {
    let mut taken: BTreeSet<PathBuf> = files
        .iter()
        .filter(|file| !overrides.contains_key(&file.from))
        .filter_map(|file| match &file.state {
            LiveState::Filed { folder, name, .. } => Some(folder.join(name)),
            _ => None,
        })
        .collect();
    for file in files.iter_mut().filter(|file| file.by_user) {
        if let LiveState::Filed { folder, name, .. } = &mut file.state {
            let free = numbered_name(&folder.join(&*name), &mut taken);
            *name = file_name_of(&free);
        }
    }
}

/// The run that files every `Filed` file; the others stay where they are.
/// `to` is below the target root, `from` is absolute, as the chat's runs have it.
pub fn run_plan(plan: &LivePlan, transfer: Transfer, run_name: String) -> RunPlan {
    let items = plan
        .files
        .iter()
        .filter_map(|file| match (&file.state, file.hash) {
            (
                LiveState::Filed {
                    folder,
                    name: file_name,
                    ..
                },
                Some(hash),
            ) => Some(RunItem {
                from: plan.source_root.join(&file.from),
                to: folder.join(file_name),
                hash,
            }),
            _ => None,
        })
        .collect();
    RunPlan {
        name: run_name,
        transfer,
        source_root: plan.source_root.clone(),
        target_root: plan.target_root.clone(),
        items,
        inside: Vec::new(),
    }
}

pub fn tally(plan: &LivePlan) -> Tally {
    let mut tally = Tally::default();
    let mut folders = BTreeSet::new();
    for file in &plan.files {
        match &file.state {
            LiveState::Waiting => tally.waiting += 1,
            LiveState::Looking | LiveState::NeedsModel => tally.looking += 1,
            LiveState::Filed { folder, .. } => {
                tally.filed += 1;
                folders.insert(folder);
            }
            LiveState::AlreadyThere { .. } => tally.already_there += 1,
            LiveState::NoFolder => tally.no_folder += 1,
            LiveState::Unreadable => tally.unreadable += 1,
        }
    }
    tally.folders = folders.len();
    tally
}
