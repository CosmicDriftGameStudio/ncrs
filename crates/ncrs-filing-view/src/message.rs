//! What happens in the filing tab.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ncrs_index::TargetId;
use ncrs_intake::LocationTransfer;

use crate::decide::Decided;
use crate::run::Carried;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// A redraw while something runs.
    Tick,
    Planned {
        session: u64,
        /// The files the index did not know were read for it.
        read: bool,
        result: Result<Box<Decided>, String>,
    },
    /// A rule or a target changed: decide again with what is known.
    RulesChanged,
    /// Look at the source and the targets again, with reading.
    Recheck,
    Select(PathBuf),
    SelectNext,
    SelectPrevious,
    LinkNext,
    LinkPrevious,
    /// The link the keyboard is on.
    PressMarkedLink,
    MakeRule(PathBuf),
    SuggestRule(PathBuf),
    Assign(PathBuf),
    Leave(PathBuf),
    Ask(PathBuf),
    QuestionToggle(usize),
    QuestionToggleMarked,
    QuestionNext,
    QuestionPrevious,
    /// Into these places of the question.
    QuestionAnswer(Vec<usize>),
    /// What Enter does.
    QuestionConfirm,
    QuestionLeave,
    StartRename,
    RenameInput(String),
    RenameSubmit,
    RenameCancel,
    PickFolder,
    FolderPicked(Option<PathBuf>),
    /// The selected file stays where it is; again, it follows its rule.
    Stay,
    ToggleOnlyNew,
    FoldersListed(u64, BTreeSet<(TargetId, PathBuf)>),
    ExistingRead(u64, Vec<((TargetId, PathBuf), Vec<String>)>),
    ToggleTrashDupes,
    ToggleFolder(TargetId, PathBuf),
    File(LocationTransfer),
    Carried(u64, Result<Box<Carried>, String>),
    /// The answer to one open question of a watched source.
    Answered(u64, PathBuf, Result<Box<Carried>, String>),
    Cancel,
}
