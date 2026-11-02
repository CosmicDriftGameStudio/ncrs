use std::path::PathBuf;

use ncrs_ai::ChatIntent;
use ncrs_filing::chat_work::ChatResult;

use crate::state::ChipReply;

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
    /// What the language model made of a typed sentence, or why it failed.
    Understood(Result<ChatIntent, String>),
    /// The model was downloaded; sent by the app when the download is done.
    ModelReady,
}

/// A finished piece of work is never equal to another: its result is not
/// comparable, and nothing needs to compare it.
impl PartialEq for Message {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Open, Self::Open)
            | (Self::Close, Self::Close)
            | (Self::Submit, Self::Submit)
            | (Self::ModelReady, Self::ModelReady) => true,
            (Self::InputChanged(left), Self::InputChanged(right))
            | (Self::Failed(left), Self::Failed(right)) => left == right,
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
            _ => false,
        }
    }
}
