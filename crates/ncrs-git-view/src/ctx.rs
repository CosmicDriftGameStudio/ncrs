//! What the Git tab takes from the app: data it reads, never state it changes.

use std::path::Path;

use iced::Size;
use ncrs_git::PullStrategy;
use ncrs_i18n::Language;
use ncrs_ui_kit::format::DateFormat;
use ncrs_ui_kit::git_rows::CommitColumns;
use ncrs_ui_kit::palette::Palette;

use crate::git_keys::GitKeys;
use crate::panes::PaneWidths;
use crate::picker::RepoEntry;

/// What `update` reads from the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    pub lang: Language,
    /// The folded groups the app remembers for the repository that is opened,
    /// as [`crate::state::Fold::key`] spells them.
    pub folded: Vec<String>,
    /// The widths of the commit columns the app remembers.
    pub columns: CommitColumns,
    /// The widths of the sidebar and the detail pane the app remembers.
    pub panes: PaneWidths,
    /// The repositories the app knows besides the open one, for the repository picker.
    pub repositories: Vec<RepoEntry>,
    /// How the Pull button joins the fetched commits; the Pull menu picks another way for one pull.
    pub pull_default: PullStrategy,
}

impl Ctx {
    /// A tab that remembers nothing: nothing folded, the standard widths.
    pub const fn new(lang: Language) -> Self {
        Self {
            lang,
            folded: Vec::new(),
            columns: CommitColumns::STANDARD,
            panes: PaneWidths::STANDARD,
            repositories: Vec::new(),
            pull_default: PullStrategy::FastForwardOnly,
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
    /// The widths of the sidebar and the detail pane the app remembers.
    pub panes: PaneWidths,
    /// The home folder, to write paths with `~`; empty where it is not known.
    pub home: &'a Path,
    /// The keys of the Git actions, `[keys.git]` applied.
    pub keys: &'a GitKeys,
    /// The way the Pull button takes, marked in the Pull menu.
    pub pull_default: PullStrategy,
}
