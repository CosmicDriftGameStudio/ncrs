//! The page Einsortieren: the sources, the targets with their rules and the
//! runs of the sorting.

use std::path::{Path, PathBuf};

use ncrs_config::config::{FilingWriteError, PauseReason, SourceSetting};
use ncrs_core::facts::RunFacts;
use ncrs_filing::sorting::{question_count, SortingAccess, TargetState, WaitReason};
use ncrs_index::TargetId;
use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource};

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

/// A typed path: a leading `~` stands for the home folder.
pub fn typed_path(text: &str, home: &Path) -> PathBuf {
    let text = text.trim();
    match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if text == "~" => home.to_path_buf(),
        None => PathBuf::from(text),
    }
}
