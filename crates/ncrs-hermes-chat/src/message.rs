use std::path::PathBuf;
use std::sync::Arc;

use ncrs_ai::ChatIntent;
use ncrs_filing::chat_work::ChatResult;
use ncrs_space::{RunLog, Scan};

use crate::state::{ChipReply, Step};

#[derive(Debug, Clone)]
pub enum Message {
    Open,
    Close,
    /// The button of the chat list: a new chat with a fresh greeting; the old
    /// one stays in the list.
    NewChat,
    /// Something done in the list of chats.
    List(crate::chat_list_update::ListMessage),
    /// The index was started over: nothing of the conversation refers to it
    /// any more.
    IndexReplaced,
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
    Understood(Result<ChatIntent, ncrs_ai::ModelProblem>),
    /// What the model made of the levels it was asked about.
    RuleAnswered(crate::rule_flow::RuleAnswer),
    /// The check of a typed folder, for the step it was typed in.
    TypedFolderResolved {
        typed: PathBuf,
        step: Step,
        result: Result<PathBuf, String>,
    },
    /// The candidates of the folder a sentence named, checked: the ones that
    /// exist.
    NamedFoldersResolved {
        folders: Vec<PathBuf>,
    },
    /// The model was downloaded; sent by the app when the download is done.
    ModelReady,
    /// The AI setting was switched on and applied; sent by the app.
    AiTurnedOn,
    /// The filing module finished its start; sent by the app.
    HermesReady,
    /// A beat while work runs; sent by the app twice a second or so.
    WorkTick,
    /// The measure the chat asked for; sent by the app.
    SpaceMeasured(Arc<Scan>),
    SpaceMeasureFailed(String),
    /// The sentences of the model for the folders the user decides about, or
    /// why there are none.
    SpaceDescribed(Result<Vec<String>, String>),
    /// The run of the Platz tab that the chat asked for; sent by the app.
    SpaceRunDone(Arc<RunLog>),
}

impl Message {
    /// The report of work the chat launched. It is delivered even when the
    /// chat is closed, else the wait it ends would never end. The finished
    /// model download counts: a sentence that waited for the model is sent
    /// again then, not only after the chat is opened.
    pub fn is_work_result(&self) -> bool {
        matches!(
            self,
            Self::ModelReady
                | Self::Done(_)
                | Self::Failed(_)
                | Self::Understood(_)
                | Self::RuleAnswered(_)
                | Self::TypedFolderResolved { .. }
                | Self::NamedFoldersResolved { .. }
                | Self::SpaceMeasured(_)
                | Self::SpaceMeasureFailed(_)
                | Self::SpaceDescribed(_)
                | Self::SpaceRunDone(_)
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
            | (Self::NewChat, Self::NewChat)
            | (Self::IndexReplaced, Self::IndexReplaced)
            | (Self::Submit, Self::Submit)
            | (Self::ModelReady, Self::ModelReady)
            | (Self::AiTurnedOn, Self::AiTurnedOn)
            | (Self::HermesReady, Self::HermesReady)
            | (Self::WorkTick, Self::WorkTick) => true,
            (Self::List(left), Self::List(right)) => left == right,
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
            (Self::Understood(left), Self::Understood(right)) => left == right,
            (
                Self::NamedFoldersResolved { folders: left },
                Self::NamedFoldersResolved { folders: right },
            ) => left == right,
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

#[cfg(test)]
mod tests {
    use super::Message;

    #[test]
    fn a_finished_model_download_reaches_a_closed_chat() {
        assert!(Message::ModelReady.is_work_result());
        assert!(!Message::Submit.is_work_result());
    }
}
