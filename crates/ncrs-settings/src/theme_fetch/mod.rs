//! State of the card that fetches a VS Code color theme from Open VSX. No
//! widgets and no network: `App` owns it and changes it from `update`, the
//! view is `ui::theme_fetch`.

pub mod install;
pub mod view;

use crate::open_vsx::{FetchError, FetchedExtension, ThemeSource};
use crate::vsix::ContributedTheme;
use ncrs_config::config::Config;

/// An extension downloaded and read: its themes, ready to be picked from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub extension: FetchedExtension,
    pub themes: Vec<ContributedTheme>,
}

/// A theme that is on disk and switched on in the config.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub config: Config,
    pub label: String,
    pub license: Option<String>,
}

/// What a download and an installation hand to the work that follows.
pub type Pick = (Fetched, ContributedTheme);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// The text field asks for an extension.
    Input,
    Fetching,
    /// The extension has several themes. The package stays here, so picking
    /// one never downloads again.
    Choosing {
        fetched: Fetched,
        cursor: usize,
    },
    Installing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFetch {
    text: String,
    stage: Stage,
    /// Why the last attempt failed. The text stays, so it can be corrected.
    error: Option<FetchError>,
}

impl Default for ThemeFetch {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeFetch {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            stage: Stage::Input,
            error: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    pub fn error(&self) -> Option<&FetchError> {
        self.error.as_ref()
    }

    /// A new keystroke ends the previous complaint.
    pub fn set_text(&mut self, text: String) {
        if matches!(self.stage, Stage::Input) {
            self.text = text;
            self.error = None;
        }
    }

    /// Enter in the field: the extension or theme file to fetch, or the reason
    /// the text names none. Only the input stage can start.
    pub fn start(&mut self) -> Option<ThemeSource> {
        if !matches!(self.stage, Stage::Input) {
            return None;
        }
        match ThemeSource::parse(&self.text) {
            Some(source) => {
                self.error = None;
                self.stage = Stage::Fetching;
                Some(source)
            }
            None => {
                self.error = Some(FetchError::InvalidId);
                None
            }
        }
    }

    /// The download is done. A single theme is installed straight away, so it
    /// is handed back; several are listed to choose from.
    pub fn finish_fetching(&mut self, result: Result<Fetched, FetchError>) -> Option<Pick> {
        if !matches!(self.stage, Stage::Fetching) {
            return None;
        }
        match result {
            Err(error) => {
                self.fail(error);
                None
            }
            Ok(fetched) => {
                if let [only] = fetched.themes.as_slice() {
                    let theme = only.clone();
                    self.stage = Stage::Installing;
                    return Some((fetched, theme));
                }
                self.stage = Stage::Choosing { fetched, cursor: 0 };
                None
            }
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if let Stage::Choosing { fetched, cursor } = &mut self.stage {
            let last = fetched.themes.len().saturating_sub(1);
            *cursor = cursor.saturating_add_signed(delta).min(last);
        }
    }

    pub fn select(&mut self, index: usize) {
        if let Stage::Choosing { fetched, cursor } = &mut self.stage {
            if index < fetched.themes.len() {
                *cursor = index;
            }
        }
    }

    /// Installs the theme under the cursor.
    pub fn choose(&mut self) -> Option<Pick> {
        let theme = match &self.stage {
            Stage::Choosing { fetched, cursor } => fetched.themes.get(*cursor)?.clone(),
            _ => return None,
        };
        match std::mem::replace(&mut self.stage, Stage::Installing) {
            Stage::Choosing { fetched, .. } => Some((fetched, theme)),
            other => {
                self.stage = other;
                None
            }
        }
    }

    /// Back to the input with the text kept and the reason shown.
    pub fn fail(&mut self, error: FetchError) {
        self.stage = Stage::Input;
        self.error = Some(error);
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
