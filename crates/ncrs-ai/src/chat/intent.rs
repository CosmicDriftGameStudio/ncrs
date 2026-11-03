//! What the chat understood from a typed message. Like a field of a
//! document, it is data: there is no variant that runs or deletes anything.

use std::fmt;

use super::turn::{ChatTurn, SearchQuery};
use crate::error::AiError;
use crate::status::LocalModelStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    Once,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    ByMonth,
    ByYear,
    BySender,
    ByKind,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    Sum,
    Deadline,
    Number,
    Date,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatIntent {
    TidyFolder {
        source: String,
        target: Option<String>,
        schedule: Option<Schedule>,
        transfer: Option<Transfer>,
    },
    ChangeStructure {
        area: String,
        scope: Option<String>,
        order: Order,
    },
    Undo,
    Search(SearchQuery),
    AskContent {
        question_kind: QuestionKind,
        sender: Option<String>,
        kind: Option<String>,
    },
    Explain {
        file: String,
    },
    BulkMove {
        filter_kind: Option<String>,
        filter_sender: Option<String>,
        filter_year: Option<u16>,
        target: String,
    },
    /// Index into the turn's question options.
    Answer {
        choice: usize,
    },
    /// A greeting, thanks or "what can you do?": Hermes says what it does.
    SmallTalk,
    /// "Keep everything as it is", "never mind": nothing is to be changed.
    LeaveAsIs,
    Other,
}

pub trait ChatModel: Send + Sync + fmt::Debug {
    fn understand(&self, turn: &ChatTurn) -> Result<ChatIntent, AiError>;
    /// Never blocks and never reads the disk.
    fn status(&self) -> LocalModelStatus;
    /// Looks for the model file again (after a download).
    fn refresh(&self);
}
