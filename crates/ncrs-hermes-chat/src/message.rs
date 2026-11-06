use std::path::PathBuf;

use ncrs_ai::ChatIntent;
use ncrs_filing::chat_work::ChatResult;

use crate::state::{ChipReply, Step};

#[derive(Debug, Clone)]
pub enum Message {
    Open,
    Close,
    InputChanged(String),
    Submit,
    ChipPressed {
        label: String,
        reply: ChipReply,
    },
    Done(Box<ChatResult>),
    Failed(String),
    FolderPicked(Option<PathBuf>),
    /// A picked folder was kept for the sandbox, or the system refused it;
    /// sent by the app in place of `FolderPicked` when the build is sandboxed.
    FolderGranted {
        folder: PathBuf,
        result: Result<(), String>,
    },
    /// Switching Hermes on did not work: the reason; sent by the app.
    TurnOnFailed(String),
    /// What the language model made of a typed sentence, or why it failed.
    Understood(Result<ChatIntent, String>),
    /// What the model made of a rule or levels it was asked.
    RuleAnswered(crate::rule_flow::RuleAnswer),
    /// The check of a typed folder, for the step it was typed in.
    TypedFolderResolved {
        typed: PathBuf,
        step: Step,
        result: Result<PathBuf, String>,
    },
    /// The candidates of the folders a sentence named, checked: the ones that
    /// exist, per name.
    NamedFoldersResolved {
        source: Vec<PathBuf>,
        target: Option<Vec<PathBuf>>,
    },
    /// The model was downloaded; sent by the app when the download is done.
    ModelReady,
    /// The AI setting was switched on and applied; sent by the app.
    AiTurnedOn,
    /// The filing module finished its start; sent by the app.
    HermesReady,
    /// The settings page asks to change the order of one area of a read folder.
    ChangeArea {
        folder: PathBuf,
        area: String,
    },
    /// New documents may be waiting; sent by the app now and then while the
    /// chat is open.
    WaitingCheck,
    /// A beat while work runs; sent by the app twice a second or so.
    WorkTick,
}

impl Message {
    /// The report of work the chat launched. It is delivered even when the
    /// chat is closed, else the wait it ends would never end.
    pub fn is_work_result(&self) -> bool {
        matches!(
            self,
            Self::Done(_)
                | Self::Failed(_)
                | Self::Understood(_)
                | Self::RuleAnswered(_)
                | Self::TypedFolderResolved { .. }
                | Self::NamedFoldersResolved { .. }
        )
    }
}

/// A finished piece of work is never equal to another: its result is not
/// comparable, and nothing needs to compare it.
impl PartialEq for Message {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Open, Self::Open)
            | (Self::Close, Self::Close)
            | (Self::Submit, Self::Submit)
            | (Self::ModelReady, Self::ModelReady)
            | (Self::AiTurnedOn, Self::AiTurnedOn)
            | (Self::HermesReady, Self::HermesReady)
            | (Self::WaitingCheck, Self::WaitingCheck)
            | (Self::WorkTick, Self::WorkTick) => true,
            (Self::InputChanged(left), Self::InputChanged(right))
            | (Self::Failed(left), Self::Failed(right))
            | (Self::TurnOnFailed(left), Self::TurnOnFailed(right)) => left == right,
            (
                Self::FolderGranted {
                    folder: left_folder,
                    result: left_result,
                },
                Self::FolderGranted {
                    folder: right_folder,
                    result: right_result,
                },
            ) => left_folder == right_folder && left_result == right_result,
            (
                Self::ChipPressed {
                    label: left_label,
                    reply: left_reply,
                },
                Self::ChipPressed {
                    label: right_label,
                    reply: right_reply,
                },
            ) => left_label == right_label && left_reply == right_reply,
            (Self::FolderPicked(left), Self::FolderPicked(right)) => left == right,
            (
                Self::ChangeArea {
                    folder: left_folder,
                    area: left_area,
                },
                Self::ChangeArea {
                    folder: right_folder,
                    area: right_area,
                },
            ) => left_folder == right_folder && left_area == right_area,
            (Self::Understood(left), Self::Understood(right)) => left == right,
            (
                Self::NamedFoldersResolved {
                    source: left_source,
                    target: left_target,
                },
                Self::NamedFoldersResolved {
                    source: right_source,
                    target: right_target,
                },
            ) => left_source == right_source && left_target == right_target,
            (
                Self::TypedFolderResolved {
                    typed: left_typed,
                    step: left_step,
                    result: left_result,
                },
                Self::TypedFolderResolved {
                    typed: right_typed,
                    step: right_step,
                    result: right_result,
                },
            ) => {
                left_typed == right_typed && left_step == right_step && left_result == right_result
            }
            _ => false,
        }
    }
}
