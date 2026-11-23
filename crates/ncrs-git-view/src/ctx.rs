//! What the Git tab takes from the app: data it reads, never state it changes.

use iced::Size;
use ncrs_i18n::Language;
use ncrs_ui_kit::format::DateFormat;
use ncrs_ui_kit::git_rows::CommitColumns;
use ncrs_ui_kit::palette::Palette;

/// What `update` reads from the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    pub lang: Language,
    /// The folded groups the app remembers for the repository that is opened,
    /// as [`crate::state::Fold::key`] spells them.
    pub folded: Vec<String>,
    /// The widths of the commit columns the app remembers.
    pub columns: CommitColumns,
}

impl Ctx {
    /// A tab that remembers nothing: nothing folded, the standard widths.
    pub const fn new(lang: Language) -> Self {
        Self {
            lang,
            folded: Vec::new(),
            columns: CommitColumns::STANDARD,
        }
    }
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub date_format: DateFormat,
    /// The window: the tab fills it.
    pub window: Size,
    /// The widths of the commit columns the app remembers.
    pub columns: CommitColumns,
}
