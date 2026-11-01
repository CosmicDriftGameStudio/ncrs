//! What the search takes from the app: data it reads, never state it changes.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use iced::keyboard::{key::Physical, Key, Modifiers};
use iced::Size;
use ncrs_i18n::Language;
use ncrs_index::{QueryHit, QuerySpec, SavedQueryId};
use ncrs_ui_kit::palette::Palette;

use crate::message::SearchMessage;

/// The questions the search asks the document index. Runs on a blocking
/// thread, so it may take its time.
pub trait SearchIndex: Send + Sync {
    /// The documents of `spec` at or below `under`.
    fn run_under(&self, spec: &QuerySpec, under: &Path) -> Result<Vec<QueryHit>, String>;

    /// The pieces of text around `words` of the documents at `paths`.
    fn snippets(&self, paths: &[PathBuf], words: &str) -> Result<Vec<(PathBuf, String)>, String>;

    /// Overwrites the criteria of the saved query `id` and keeps its name.
    fn replace_spec(&self, id: SavedQueryId, spec: &QuerySpec) -> Result<(), String>;
}

/// What `update` reads from the app.
pub struct Ctx {
    pub window_size: Size,
    /// The panel the search stands in for shows a server, which the search
    /// cannot walk.
    pub side_is_remote: bool,
    /// The search is up and its panel is the active one.
    pub has_keys: bool,
    /// `None` while the filing has no index to ask.
    pub index: Option<Arc<dyn SearchIndex>>,
}

impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("window_size", &self.window_size)
            .field("side_is_remote", &self.side_is_remote)
            .field("has_keys", &self.has_keys)
            .field("has_index", &self.index.is_some())
            .finish()
    }
}

/// The keys of the three toggles as the keymap spells them, for the tooltips.
/// `None` for a toggle without a key.
#[derive(Debug, Clone, Default)]
pub struct ToggleShortcuts {
    pub case: Option<String>,
    pub whole_word: Option<String>,
    pub regex: Option<String>,
}

/// Resolves a key press to the toggle it is bound to, if any.
pub type TogglePress<'a> = Rc<dyn Fn(Key, Physical, Modifiers) -> Option<SearchMessage> + 'a>;

/// What `view` reads from the app.
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    /// The panel of the search is the active one.
    pub is_active: bool,
    pub shortcuts: ToggleShortcuts,
    /// The toggle a press is bound to, so the fields claim it before they
    /// type its character.
    pub toggle_for_press: TogglePress<'a>,
}

impl std::fmt::Debug for ViewCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewCtx")
            .field("lang", &self.lang)
            .field("is_active", &self.is_active)
            .field("shortcuts", &self.shortcuts)
            .finish_non_exhaustive()
    }
}
