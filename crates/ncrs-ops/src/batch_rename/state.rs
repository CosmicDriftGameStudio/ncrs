//! The dialog of a batch rename and the messages it sends.

use std::path::PathBuf;

use chrono::NaiveDate;
use iced::mouse::ScrollDelta;
use ncrs_core::PanelSide;
use ncrs_ui_kit::list_window::ListWindow;

use super::pattern::{preview, Case, EntryFacts, NameRules, Options, Preview};

/// The rows of the preview that show at once.
pub const VISIBLE_ROWS: usize = 8;

/// The most digits a counter is padded to.
pub const MOST_DIGITS: usize = 12;

/// What the "Add" buttons put into the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Name,
    PartOfName,
    Counter,
    Modified,
    Taken,
    Extension,
    Sender,
    DocumentDate,
    Kind,
    Amount,
}

impl Token {
    pub const fn text(self) -> &'static str {
        match self {
            Token::Name => "{name}",
            Token::PartOfName => "{name:1-8}",
            Token::Counter => "{counter}",
            Token::Modified => "{date modified}",
            Token::Taken => "{date taken}",
            Token::Extension => "{extension}",
            Token::Sender => "{sender}",
            Token::DocumentDate => "{document date}",
            Token::Kind => "{kind}",
            Token::Amount => "{amount}",
        }
    }
}

/// What reading the files and the index gave for one entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extras {
    pub taken: Option<NaiveDate>,
    pub sender: Option<String>,
    pub kind: Option<String>,
    pub document_date: Option<NaiveDate>,
    pub amount: Option<String>,
}

/// Why a batch did not go through. Nothing stays changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyFailure {
    /// Taking back: the entry that was renamed is not there any more.
    Gone(String),
    /// Taking back: the old name belongs to another entry now.
    Taken(String),
    Failed {
        name: String,
        reason: String,
        /// The renames before it could not all be put back.
        rollback_failed: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BatchRenameMessage {
    PatternInput(String),
    StartInput(String),
    DigitsInput(String),
    FindInput(String),
    ReplaceInput(String),
    Insert(Token),
    SetCase(Case),
    ToggleRegex,
    UseHermes,
    Scrolled(ScrollDelta),
    Confirm,
    Cancel,
    /// The files and the index have been read for the dialog `dialog`.
    ExtrasLoaded {
        dialog: u64,
        extras: Vec<Extras>,
    },
    /// What Hermes thinks the names of the folder follow, as a pattern.
    Proposed {
        dialog: u64,
        pattern: Option<String>,
    },
    /// The renames are over. Dropped when `generation` is not the current one.
    Applied {
        generation: u64,
        result: Result<(), ApplyFailure>,
    },
    /// Take the last batch back.
    Undo,
}

#[derive(Debug, Clone)]
pub struct BatchRename {
    pub id: u64,
    pub side: PanelSide,
    pub folder: PathBuf,
    /// The entries in the order of the panel.
    pub entries: Vec<(PathBuf, EntryFacts)>,
    /// Every name of the folder.
    pub folder_names: Vec<String>,
    pub rules: NameRules,
    pub pattern: String,
    pub start: String,
    pub digits: String,
    pub find: String,
    pub replace: String,
    pub regex: bool,
    pub case: Case,
    /// The pattern Hermes proposes for this folder.
    pub suggestion: Option<String>,
    pub preview: Preview,
    pub window: ListWindow,
}

/// What opens the dialog.
#[derive(Debug, Clone)]
pub struct Opening {
    pub id: u64,
    pub side: PanelSide,
    pub folder: PathBuf,
    pub entries: Vec<(PathBuf, EntryFacts)>,
    pub folder_names: Vec<String>,
    pub rules: NameRules,
}

impl BatchRename {
    pub fn new(opening: Opening) -> Self {
        let mut dialog = Self {
            id: opening.id,
            side: opening.side,
            folder: opening.folder,
            entries: opening.entries,
            folder_names: opening.folder_names,
            rules: opening.rules,
            pattern: "{name}{extension}".to_owned(),
            start: "1".to_owned(),
            digits: "3".to_owned(),
            find: String::new(),
            replace: String::new(),
            regex: false,
            case: Case::Keep,
            suggestion: None,
            preview: Preview::default(),
            window: ListWindow::default(),
        };
        dialog.refresh();
        dialog
    }

    fn options(&self) -> Options<'_> {
        Options {
            pattern: &self.pattern,
            start: self.start.parse().unwrap_or(1),
            digits: self.digits.parse().unwrap_or(0).min(MOST_DIGITS),
            find: &self.find,
            replace: &self.replace,
            regex: self.regex,
            case: self.case,
        }
    }

    /// The preview again, after anything the names depend on changed.
    pub fn refresh(&mut self) {
        let facts: Vec<EntryFacts> = self
            .entries
            .iter()
            .map(|(_, facts)| facts.clone())
            .collect();
        self.preview = preview(&self.options(), &facts, &self.folder_names, self.rules);
    }

    pub fn set_extras(&mut self, extras: Vec<Extras>) {
        for ((_, facts), found) in self.entries.iter_mut().zip(extras) {
            facts.taken = found.taken;
            facts.sender = found.sender;
            facts.kind = found.kind;
            facts.document_date = found.document_date;
            facts.amount = found.amount;
        }
        self.refresh();
    }

    pub fn insert(&mut self, token: Token) {
        self.pattern.push_str(token.text());
        self.refresh();
    }
}

/// What a number field keeps of what was typed.
pub fn digits_only(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_digit)
        .take(MOST_DIGITS)
        .collect()
}
