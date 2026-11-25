//! Sources and targets of filing: `[[modules.filing.sources]]` and
//! `[[modules.filing.targets]]`, and the older `locations` they replace.
//!
//! Resolving never looks at the disk and never rejects the config for a
//! mistake in one entry: that entry is paused with its reason and the rest
//! works.

use super::*;
use ncrs_fs::search::PathFilter;
use ncrs_index::TargetId;
use ncrs_intake::{OnNoText, OnSeveralTargets, SortSource, SortTarget};

/// What a source without images excludes, as the old `images = false` did.
pub const IMAGE_EXCLUDE: &str = "*.jpg;*.jpeg;*.png;*.heic;*.tif;*.tiff";

const WHEN_WATCH: &str = "watch";
const WHEN_ONCE: &str = "once";
const FILES_COPY: &str = "copy";
const FILES_MOVE: &str = "move";
const ASK: &str = "ask";
const SKIP: &str = "skip";
const COPY_TO_ALL: &str = "copy_to_all";

/// One `[[modules.filing.targets]]` table, unchecked.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetEntry {
    pub id: String,
    /// Without a name the folder's name is shown.
    #[serde(default)]
    pub name: Option<String>,
    pub path: Spanned<String>,
    /// Whether the folder was a volume of its own when it was saved.
    #[serde(default)]
    pub own_volume: Option<bool>,
}

/// One `[[modules.filing.sources]]` table, unchecked. The words stay text: an
/// unknown one pauses the source instead of costing the user the config.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEntry {
    pub path: Spanned<String>,
    #[serde(default)]
    pub include: String,
    #[serde(default)]
    pub exclude: String,
    #[serde(default)]
    pub targets: Vec<String>,
    /// `watch` or `once`; `once` without the key.
    #[serde(default)]
    pub when: Option<String>,
    /// `copy` or `move`; `move` without the key.
    #[serde(default)]
    pub files: Option<String>,
    /// `ask` or `skip`; `ask` without the key.
    #[serde(default)]
    pub no_text: Option<String>,
    /// `ask` or `copy_to_all`; `ask` without the key.
    #[serde(default)]
    pub several: Option<String>,
}

/// Why a source or target does not work although it is in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PauseReason {
    /// The path is not absolute, has `..` or starts with `~` without a home.
    UnusablePath(String),
    /// The id is already taken by an earlier target.
    DuplicateId,
    /// An earlier target or source has the same folder.
    SamePath,
    /// The source names a target that does not exist.
    UnknownTarget(String),
    /// A pattern in `include` or `exclude` is not a valid glob.
    InvalidGlob(String),
    /// A word of the entry is none of the known ones: `key = "value"`.
    UnknownValue(String),
    /// A watched source in or equal to this target would feed itself.
    WatchedInTarget(TargetId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSetting {
    pub target: SortTarget,
    pub paused: Option<PauseReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSetting {
    pub source: SortSource,
    pub paused: Option<PauseReason>,
}

#[derive(Debug, Default)]
pub struct Sorting {
    pub targets: Vec<TargetSetting>,
    pub sources: Vec<SourceSetting>,
    pub dropped: Vec<String>,
}

/// Whether the file says anything about sources or targets; an empty list
/// counts. Only then `locations` stops being the source of them.
pub fn sorting_is_in_file(section: &FilingSection) -> bool {
    section.sources.is_some() || section.targets.is_some()
}

/// Targets and sources of the file, or derived from `listed` (the old
/// `locations`) when the file does not have them yet.
pub fn resolve_sorting(
    section: &FilingSection,
    listed: &[FilingLocation],
    home: Option<&Path>,
) -> Sorting {
    let mut sorting = if sorting_is_in_file(section) {
        sorting_of_entries(section, home)
    } else {
        sorting_of_locations(listed)
    };
    pause_what_breaks_the_rules(&mut sorting);
    sorting
}

fn sorting_of_entries(section: &FilingSection, home: Option<&Path>) -> Sorting {
    let mut sorting = Sorting::default();
    let target_entries = section
        .targets
        .as_ref()
        .map_or(&[][..], |listed| listed.get_ref().as_slice());
    let source_entries = section
        .sources
        .as_ref()
        .map_or(&[][..], |listed| listed.get_ref().as_slice());
    for entry in target_entries {
        let Some(id) = TargetId::new(entry.id.trim()) else {
            sorting
                .dropped
                .push(format!("target \"{}\" is not a valid id", entry.id));
            continue;
        };
        let (path, paused) = folder_of(&entry.path, home);
        let name = entry
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| folder_name(&path), str::to_owned);
        sorting.targets.push(TargetSetting {
            target: SortTarget {
                id,
                name,
                path,
                own_volume: entry.own_volume,
            },
            paused,
        });
    }
    for entry in source_entries {
        let (path, path_problem) = folder_of(&entry.path, home);
        let mut targets: Vec<TargetId> = Vec::new();
        let mut unknown = None;
        for text in &entry.targets {
            let known = TargetId::new(text.trim()).filter(|id| {
                sorting
                    .targets
                    .iter()
                    .any(|candidate| &candidate.target.id == id)
            });
            match known {
                Some(id) if !targets.contains(&id) => targets.push(id),
                Some(_) => {}
                None => {
                    unknown.get_or_insert_with(|| text.clone());
                }
            }
        }
        let words = source_words(entry);
        let (watch, transfer, no_text, several, word_problem) = match words {
            Ok((watch, transfer, no_text, several)) => (watch, transfer, no_text, several, None),
            Err(problem) => (
                false,
                LocationTransfer::default(),
                OnNoText::Ask,
                OnSeveralTargets::Ask,
                Some(problem),
            ),
        };
        let paused = path_problem
            .or_else(|| unknown.map(PauseReason::UnknownTarget))
            .or(word_problem)
            .or_else(|| {
                glob_problem(&path, &entry.include, &entry.exclude).map(PauseReason::InvalidGlob)
            });
        sorting.sources.push(SourceSetting {
            source: SortSource {
                path,
                include: entry.include.clone(),
                exclude: entry.exclude.clone(),
                targets,
                watch,
                transfer,
                no_text,
                several,
            },
            paused,
        });
    }
    sorting
}

type SourceWords = (bool, LocationTransfer, OnNoText, OnSeveralTargets);

fn source_words(entry: &SourceEntry) -> Result<SourceWords, PauseReason> {
    let word = |key: &str, text: &Option<String>, known: &[&str]| -> Result<usize, PauseReason> {
        let Some(given) = text else {
            return Ok(0);
        };
        known
            .iter()
            .position(|name| name.eq_ignore_ascii_case(given.trim()))
            .ok_or_else(|| PauseReason::UnknownValue(format!("{key} = \"{given}\"")))
    };
    let watch = word("when", &entry.when, &[WHEN_ONCE, WHEN_WATCH])? == 1;
    let transfer = match word("files", &entry.files, &[FILES_MOVE, FILES_COPY])? {
        1 => LocationTransfer::Copy,
        _ => LocationTransfer::Move,
    };
    let no_text = match word("no_text", &entry.no_text, &[ASK, SKIP])? {
        1 => OnNoText::Skip,
        _ => OnNoText::Ask,
    };
    let several = match word("several", &entry.several, &[ASK, COPY_TO_ALL])? {
        1 => OnSeveralTargets::CopyToAll,
        _ => OnSeveralTargets::Ask,
    };
    Ok((watch, transfer, no_text, several))
}

/// The one target an old config has, as `main`, and a source for each of its
/// inboxes. Archives that are not the target stay index folders in `locations`.
fn sorting_of_locations(listed: &[FilingLocation]) -> Sorting {
    let mut sorting = Sorting::default();
    let archives = listed
        .iter()
        .filter(|location| location.mode == LocationMode::Archive && location.is_target);
    for (index, location) in archives.enumerate() {
        let id = if index == 0 {
            Some(TargetId::main())
        } else {
            TargetId::new(&format!("t{}", index + 1))
        };
        let Some(id) = id else { continue };
        sorting.targets.push(TargetSetting {
            target: SortTarget {
                id,
                name: folder_name(&location.path),
                path: location.path.clone(),
                own_volume: None,
            },
            paused: None,
        });
    }
    let first_target = sorting.targets.first().map(|setting| setting.target.id.clone());
    for location in listed
        .iter()
        .filter(|location| location.mode == LocationMode::Inbox)
    {
        sorting.sources.push(SourceSetting {
            source: SortSource {
                path: location.path.clone(),
                include: String::new(),
                exclude: if location.images {
                    String::new()
                } else {
                    IMAGE_EXCLUDE.to_owned()
                },
                targets: first_target.iter().cloned().collect(),
                watch: location.continuous,
                transfer: location.transfer,
                no_text: OnNoText::Ask,
                several: OnSeveralTargets::Ask,
            },
            paused: None,
        });
    }
    sorting
}

/// Two targets with one id or one folder, two sources with one folder, a
/// watched source in a target: the later one is paused, the earlier one works.
fn pause_what_breaks_the_rules(sorting: &mut Sorting) {
    for later in 0..sorting.targets.len() {
        let reason = {
            let (before, rest) = sorting.targets.split_at(later);
            let current = &rest[0];
            if current.paused.is_some() {
                None
            } else if before
                .iter()
                .any(|earlier| earlier.target.id == current.target.id)
            {
                Some(PauseReason::DuplicateId)
            } else if before.iter().any(|earlier| {
                earlier.paused.is_none() && same_folder(&earlier.target.path, &current.target.path)
            }) {
                Some(PauseReason::SamePath)
            } else {
                None
            }
        };
        if reason.is_some() {
            sorting.targets[later].paused = reason;
        }
    }
    for later in 0..sorting.sources.len() {
        let reason = {
            let (before, rest) = sorting.sources.split_at(later);
            let current = &rest[0];
            if current.paused.is_some() {
                None
            } else if before.iter().any(|earlier| {
                earlier.paused.is_none() && same_folder(&earlier.source.path, &current.source.path)
            }) {
                Some(PauseReason::SamePath)
            } else if current.source.watch {
                sorting
                    .targets
                    .iter()
                    .find(|target| {
                        !matches!(target.paused, Some(PauseReason::UnusablePath(_)))
                            && lies_within(&current.source.path, &target.target.path)
                    })
                    .map(|target| PauseReason::WatchedInTarget(target.target.id.clone()))
            } else {
                None
            }
        };
        if reason.is_some() {
            sorting.sources[later].paused = reason;
        }
    }
}

/// The old `locations` that follow from `sorting`: the index folders the file
/// still lists, the targets that run and the sources that run.
// Goes with the old filing path
pub fn locations_of_sorting(sorting: &Sorting, listed: &[FilingLocation]) -> Vec<FilingLocation> {
    let mut locations: Vec<FilingLocation> = listed
        .iter()
        .filter(|location| location.mode == LocationMode::Archive)
        .map(|location| FilingLocation {
            is_target: false,
            transfer: LocationTransfer::default(),
            continuous: false,
            ..location.clone()
        })
        .collect();
    let mut target_marked = false;
    let running_targets = sorting
        .targets
        .iter()
        .filter(|setting| setting.paused.is_none());
    for setting in running_targets {
        let is_main = !target_marked;
        let known = locations
            .iter_mut()
            .find(|location| same_folder(&location.path, &setting.target.path));
        match known {
            Some(location) => location.is_target = location.is_target || is_main,
            None => locations.push(FilingLocation {
                path: setting.target.path.clone(),
                mode: LocationMode::Archive,
                images: true,
                is_target: is_main,
                transfer: LocationTransfer::default(),
                continuous: false,
            }),
        }
        target_marked = true;
    }
    let archives: Vec<PathBuf> = locations.iter().map(|location| location.path.clone()).collect();
    for setting in sorting
        .sources
        .iter()
        .filter(|setting| setting.paused.is_none())
    {
        let source = &setting.source;
        let overlaps_an_archive = archives.iter().any(|archive| {
            lies_within(&source.path, archive) || lies_within(archive, &source.path)
        });
        if overlaps_an_archive {
            continue;
        }
        locations.push(FilingLocation {
            path: source.path.clone(),
            mode: LocationMode::Inbox,
            images: !source.exclude.eq_ignore_ascii_case(IMAGE_EXCLUDE),
            is_target: false,
            transfer: source.transfer,
            continuous: source.watch,
        });
    }
    locations
}

pub fn same_folder(left: &Path, right: &Path) -> bool {
    lies_within(left, right) && lies_within(right, left)
}

fn folder_of(text: &Spanned<String>, home: Option<&Path>) -> (PathBuf, Option<PauseReason>) {
    match expand_location_path(text.get_ref(), home) {
        Ok(path) => (ncrs_intake::without_verbatim_prefix(path), None),
        Err(reason) => (
            PathBuf::from(text.get_ref()),
            Some(PauseReason::UnusablePath(reason)),
        ),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The reason `include` or `exclude` is no glob, checked the way the search
/// compiles them. Nothing is read from the disk.
fn glob_problem(root: &Path, include: &str, exclude: &str) -> Option<String> {
    PathFilter::new(root.to_path_buf(), include, exclude, false)
        .err()
        .map(|problem| problem.to_string())
}

pub fn watch_word(watch: bool) -> &'static str {
    if watch {
        WHEN_WATCH
    } else {
        WHEN_ONCE
    }
}

pub fn files_word(transfer: LocationTransfer) -> &'static str {
    match transfer {
        LocationTransfer::Copy => FILES_COPY,
        LocationTransfer::Move => FILES_MOVE,
    }
}

pub fn no_text_word(choice: OnNoText) -> &'static str {
    match choice {
        OnNoText::Ask => ASK,
        OnNoText::Skip => SKIP,
    }
}

pub fn several_word(choice: OnSeveralTargets) -> &'static str {
    match choice {
        OnSeveralTargets::Ask => ASK,
        OnSeveralTargets::CopyToAll => COPY_TO_ALL,
    }
}
