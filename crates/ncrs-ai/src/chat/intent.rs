//! What the chat understood from a typed message. Like a field of a
//! document, it is data: there is no variant that runs or deletes anything.

use std::fmt;

use ncrs_index::{DocumentFields, Kinds, RuleSpec};

use super::turn::{ChatTurn, SearchQuery};
use crate::error::AiError;
use crate::rules::{ContextDocument, Levels, Mismatch};
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

/// The `area` of a `change_structure` that is meant for every area.
pub const EVERY_AREA: &str = "*";

/// A part of a file name, taken from what the document says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameBlock {
    Year,
    Month,
    Day,
    Sender,
    Kind,
}

/// The characters a file name may be joined with.
pub const NAME_SEPARATORS: [char; 4] = ['_', '-', ' ', '.'];

/// How the file names are built: these blocks, joined by the separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameSpec {
    pub blocks: Vec<NameBlock>,
    pub separator: char,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    Sum,
    Deadline,
    Number,
    Date,
}

impl Order {
    /// The word the model and the eval rows use.
    pub fn word(self) -> &'static str {
        match self {
            Self::ByMonth => "by_month",
            Self::ByYear => "by_year",
            Self::BySender => "by_sender",
            Self::ByKind => "by_kind",
            Self::Flat => "flat",
        }
    }
}

impl NameBlock {
    pub fn word(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
            Self::Sender => "sender",
            Self::Kind => "kind",
        }
    }
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
        /// At least one of `order` and `name` is set.
        order: Option<Order>,
        name: Option<NameSpec>,
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
    /// "Open /Bank", "show me what is in Scans": look into one folder.
    OpenFolder {
        folder: String,
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
    /// The rule a described filing means; `documents` are what the model is
    /// shown of the archive.
    fn generate_rule(
        &self,
        description: &str,
        documents: &[ContextDocument],
        kinds: &Kinds,
    ) -> Result<RuleSpec, AiError>;
    /// The levels that wrote these paths, with the fields of each file.
    /// `folder_depth` is the number of folder levels every file of the folder
    /// has, when it is the same for all of them.
    fn recognize_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
    ) -> Result<Levels, AiError>;
    /// The same question once more, after the answer `previous` did not write
    /// the paths of the files in `misfits`.
    fn recognize_levels_again(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        previous: &Levels,
        misfits: &[Mismatch],
    ) -> Result<Levels, AiError>;
    /// Never blocks and never reads the disk.
    fn status(&self) -> LocalModelStatus;
    /// Looks for the model file again (after a download).
    fn refresh(&self);
}
