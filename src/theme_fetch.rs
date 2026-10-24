//! State of the card that fetches a VS Code color theme from Open VSX. No
//! widgets and no network: `App` owns it and changes it from `update`, the
//! view is `ui::theme_fetch`.

use crate::config::Config;
use crate::open_vsx::{ExtensionId, FetchError, FetchedExtension};
use crate::vsix::ContributedTheme;

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

    /// Enter in the field: the extension to fetch, or the reason the text
    /// names none. Only the input stage can start.
    pub fn start(&mut self) -> Option<ExtensionId> {
        if !matches!(self.stage, Stage::Input) {
            return None;
        }
        match ExtensionId::parse(&self.text) {
            Some(id) => {
                self.error = None;
                self.stage = Stage::Fetching;
                Some(id)
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
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::vsix::Brightness;

    pub fn fetched(labels: &[&str]) -> Fetched {
        Fetched {
            extension: FetchedExtension {
                id: ExtensionId::parse("example.dark-themes").unwrap(),
                display_name: "Example Themes".to_owned(),
                version: "1.2.3".to_owned(),
                license: Some("MIT".to_owned()),
                vsix: Vec::new().into(),
            },
            themes: labels
                .iter()
                .map(|label| ContributedTheme {
                    label: (*label).to_owned(),
                    brightness: Some(Brightness::Dark),
                    path: format!("themes/{label}.json"),
                })
                .collect(),
        }
    }

    fn typed(text: &str) -> ThemeFetch {
        let mut card = ThemeFetch::new();
        card.set_text(text.to_owned());
        card
    }

    #[test]
    fn a_valid_id_starts_the_download_and_locks_the_field() {
        let mut card = typed("example.dark-themes");
        let id = card.start().unwrap();
        assert_eq!(id.to_string(), "example.dark-themes");
        assert_eq!(card.stage(), &Stage::Fetching);

        card.set_text("other.thing".to_owned());
        assert_eq!(
            card.text(),
            "example.dark-themes",
            "typing is ignored while it runs"
        );
        assert_eq!(card.start(), None, "no second download");
    }

    #[test]
    fn an_invalid_text_starts_nothing_and_says_why() {
        let mut card = typed("../etc");
        assert_eq!(card.start(), None);
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.error(), Some(&FetchError::InvalidId));

        card.set_text("example.dark-themes".to_owned());
        assert_eq!(card.error(), None, "typing clears the complaint");
    }

    #[test]
    fn a_failed_download_returns_to_the_input_with_the_text_kept() {
        let mut card = typed("example.dark-themes");
        card.start();
        assert_eq!(card.finish_fetching(Err(FetchError::Timeout)), None);
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.text(), "example.dark-themes");
        assert_eq!(card.error(), Some(&FetchError::Timeout));
        assert!(card.start().is_some(), "and it can be tried again");
    }

    #[test]
    fn one_theme_is_installed_without_asking() {
        let mut card = typed("example.dark-themes");
        card.start();
        let (extension, theme) = card
            .finish_fetching(Ok(fetched(&["Example Dark"])))
            .unwrap();
        assert_eq!(extension.extension.display_name, "Example Themes");
        assert_eq!(theme.label, "Example Dark");
        assert_eq!(card.stage(), &Stage::Installing);
    }

    #[test]
    fn several_themes_are_chosen_from_and_the_package_stays() {
        let mut card = typed("example.dark-themes");
        card.start();
        assert_eq!(
            card.finish_fetching(Ok(fetched(&["Example A", "Example B", "Example C"]))),
            None
        );
        assert!(matches!(card.stage(), Stage::Choosing { cursor: 0, .. }));

        card.move_cursor(1);
        card.move_cursor(1);
        card.move_cursor(1);
        assert!(
            matches!(card.stage(), Stage::Choosing { cursor: 2, .. }),
            "stops at the end"
        );
        card.move_cursor(-5);
        assert!(
            matches!(card.stage(), Stage::Choosing { cursor: 0, .. }),
            "and at the top"
        );

        card.select(1);
        card.select(9);
        let (extension, theme) = card.choose().unwrap();
        assert_eq!(theme.label, "Example B");
        assert_eq!(extension.themes.len(), 3, "the package comes along");
        assert_eq!(card.stage(), &Stage::Installing);
        assert_eq!(card.choose(), None);
    }

    #[test]
    fn a_result_nobody_waits_for_changes_nothing() {
        let mut card = typed("example.dark-themes");
        assert_eq!(card.finish_fetching(Ok(fetched(&["Example Dark"]))), None);
        assert_eq!(card.stage(), &Stage::Input);
    }

    #[test]
    fn a_failed_installation_returns_to_the_input() {
        let mut card = typed("example.dark-themes");
        card.start();
        card.finish_fetching(Ok(fetched(&["Example Dark"])));
        card.fail(FetchError::NoConfigPath);
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.text(), "example.dark-themes");
        assert_eq!(card.error(), Some(&FetchError::NoConfigPath));
    }
}
