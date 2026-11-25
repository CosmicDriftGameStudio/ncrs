//! The page Einsortieren: the sources, the targets with their rules and the
//! runs of the sorting.

use std::path::{Path, PathBuf};

use ncrs_config::config::{same_folder, FilingWriteError, PauseReason, SourceSetting};
use ncrs_core::facts::RunFacts;
use ncrs_filing::sorting::{question_count, SortingAccess, TargetState, WaitReason};
use ncrs_index::TargetId;
use ncrs_intake::{canonical_path, LocationTransfer, OnNoText, OnSeveralTargets, SortSource};

use ncrs_config::config::Filing;

use crate::source_dialog::SourceField;

/// Runs listed before "Show all".
pub const RUNS_SHOWN: usize = 3;

#[derive(Debug, Default)]
pub struct SortingPage {
    /// One entry per source of the config, in its order; empty until read.
    pub states: Vec<SourceState>,
    pub runs: Vec<RunFacts>,
    pub all_runs: bool,
    /// The source whose folder is being chosen again.
    pub repick: Option<PathBuf>,
}

/// Why a source does something, or nothing, right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    Ready { questions: usize },
    /// No target, or an entry the config reader paused.
    Paused,
    DriveMissing,
    FolderMissing,
    Unreadable,
    TargetWaiting,
    GlobError,
}

impl SourceState {
    /// The folder cannot be reached; choosing it again brings it back, in the
    /// App Store build together with the access to it.
    pub fn folder_is_out_of_reach(self) -> bool {
        matches!(
            self,
            Self::DriveMissing | Self::FolderMissing | Self::Unreadable
        )
    }
}

/// What reading the state of the sources and the history gave.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub states: Vec<SourceState>,
    pub runs: Vec<RunFacts>,
}

/// Blocking: the folder of a source can be on a drive that hangs.
pub fn source_state(setting: &SourceSetting, access: Option<&SortingAccess>) -> SourceState {
    let source = &setting.source;
    match &setting.paused {
        Some(PauseReason::InvalidGlob(_)) => return SourceState::GlobError,
        Some(_) => return SourceState::Paused,
        None => {}
    }
    if source.targets.is_empty() {
        return SourceState::Paused;
    }
    if let Err(err) = std::fs::read_dir(&source.path) {
        return match err.kind() {
            std::io::ErrorKind::NotFound if source.own_volume == Some(true) => {
                SourceState::DriveMissing
            }
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => {
                SourceState::FolderMissing
            }
            _ => SourceState::Unreadable,
        };
    }
    let Some(access) = access else {
        return SourceState::Ready { questions: 0 };
    };
    let target_waits = source
        .targets
        .iter()
        .any(|id| matches!(access.target_state(id), TargetState::Waiting(reason) if reason != WaitReason::Unconfirmed));
    if target_waits {
        return SourceState::TargetWaiting;
    }
    SourceState::Ready {
        questions: question_count(access, source).unwrap_or(0),
    }
}

/// Everything the sources and the history of the page Einsortieren send.
#[derive(Debug, Clone, PartialEq)]
pub enum SortingMessage {
    Loaded(Box<Result<Loaded, String>>),
    AddSource,
    ChangeSource(usize),
    RemoveSource(usize),
    /// "Choose folder again": the entry keeps everything but its folder.
    RepickSource(usize),
    /// The files of the source that wait for an answer, in the plan.
    OpenQuestions(usize),
    ToggleAllRuns,
    Undo(i64),
    Undone(Result<usize, String>),
    /// The config was written; the filing settings read back from it.
    Written(Box<Result<Filing, FilingWriteError>>),
    FolderText(String),
    /// What the disk says of the folder of the dialog: its resolved path when
    /// it is there.
    FolderChecked {
        generation: u64,
        found: Option<PathBuf>,
    },
    PickFolder,
    FolderPicked(Option<PathBuf>),
    Include(String),
    Exclude(String),
    ToggleTarget(TargetId),
    SetWatch(bool),
    SetTransfer(LocationTransfer),
    SetNoText(OnNoText),
    SetSeveral(OnSeveralTargets),
    /// The mouse went into this field.
    Focus(SourceField),
    FocusNext,
    FocusPrevious,
    /// Left or Right on a focused choice: the other value.
    StepChoice,
    /// Space: ticks the focused target, presses the focused button or takes
    /// the other value of the focused choice.
    Press,
    /// Presses "+ Ziel…" when the keyboard is on it, saves otherwise.
    Enter,
    Save,
    ShowPlan,
    /// The source of a one-off filing with what the disk says of its volume.
    PlanSource {
        generation: u64,
        source: Box<SortSource>,
    },
    Cancel,
}

/// Whether the folder is on a volume of its own, as it is saved. Blocking.
/// `before` is the folder and what was saved for it. An unchanged folder never
/// loses its drive: asked while the drive is away, the empty mount point would
/// count as the folder from then on.
pub fn own_volume_to_save(
    before: Option<(&Path, Option<bool>)>,
    folder: &Path,
    now: Option<bool>,
) -> Option<bool> {
    let resolved = |path: &Path| canonical_path(path).unwrap_or_else(|_| path.to_path_buf());
    match before {
        Some((old, saved))
            if same_folder(old, folder) || same_folder(&resolved(old), &resolved(folder)) =>
        {
            if now == Some(true) {
                now
            } else {
                saved
            }
        }
        _ => now,
    }
}

/// A typed path: a leading `~` stands for the home folder.
pub fn typed_path(text: &str, home: &Path) -> PathBuf {
    let text = text.trim();
    match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if text == "~" => home.to_path_buf(),
        None => PathBuf::from(text),
    }
}

#[cfg(test)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_folder_keeps_the_drive_it_was_saved_on() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        let saved = |saved, now| own_volume_to_save(Some((folder, saved)), folder, now);
        assert_eq!(saved(Some(true), Some(false)), Some(true));
        assert_eq!(saved(Some(true), None), Some(true));
        assert_eq!(saved(None, Some(false)), None);
        assert_eq!(saved(None, Some(true)), Some(true));
        assert_eq!(saved(Some(false), Some(true)), Some(true));

        let other = dir.path().join("other");
        assert_eq!(
            own_volume_to_save(Some((folder, Some(true))), &other, Some(false)),
            Some(false)
        );
        assert_eq!(own_volume_to_save(None, folder, Some(false)), Some(false));
    }
}
