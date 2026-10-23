//! The command palette (Cmd/Ctrl+Shift+P): every key action that can be run by
//! name in the current area of the window, fuzzy-filtered by its label. No
//! widgets: the view reads the hits, the typed text goes to
//! [`CommandPalette::set_query`].

use std::cmp::Ordering;
use std::ops::Range;

use nucleo_matcher::{Config, Matcher};

use crate::i18n::{Language, Msg};
use crate::keymap::{KeyAction, KeyScope, Keymap};
use crate::quick_open::{pattern_for, score_with_highlight};

/// The label of an action in the palette, or `None` for an action it does not
/// list: cursor and tag movement is not something to look up by name, and the
/// palette does not list itself. No catch-all arm: a new action has to say
/// whether it is listed.
fn label_of(action: KeyAction) -> Option<Msg> {
    match action {
        KeyAction::CursorUp
        | KeyAction::CursorDown
        | KeyAction::PageUp
        | KeyAction::PageDown
        | KeyAction::First
        | KeyAction::Last
        | KeyAction::TagUp
        | KeyAction::TagDown
        | KeyAction::CommandPalette => None,
        KeyAction::Quit => Some(Msg::ActionQuit),
        KeyAction::GoUp => Some(Msg::ActionGoUp),
        KeyAction::Open => Some(Msg::ActionOpen),
        KeyAction::SwitchPanel => Some(Msg::ActionSwitchPanel),
        KeyAction::SwitchLanguage => Some(Msg::ActionSwitchLanguage),
        KeyAction::ToggleTag => Some(Msg::ActionToggleTag),
        KeyAction::TagAll => Some(Msg::ActionTagAll),
        KeyAction::ClearTags => Some(Msg::ActionClearTags),
        KeyAction::AbortJob => Some(Msg::ActionAbortJob),
        KeyAction::CopyPath => Some(Msg::ActionCopyPath),
        KeyAction::CopyName => Some(Msg::ActionCopyName),
        KeyAction::View => Some(Msg::ActionView),
        KeyAction::Edit => Some(Msg::ActionEdit),
        KeyAction::Copy => Some(Msg::ActionCopy),
        KeyAction::Move => Some(Msg::ActionMove),
        KeyAction::Delete => Some(Msg::ActionDelete),
        KeyAction::DeletePermanently => Some(Msg::ActionDeletePermanently),
        KeyAction::Mkdir => Some(Msg::ActionMkdir),
        KeyAction::DriveMenuLeft => Some(Msg::ActionDriveMenuLeft),
        KeyAction::DriveMenuRight => Some(Msg::ActionDriveMenuRight),
        KeyAction::SortByName => Some(Msg::ActionSortByName),
        KeyAction::SortByModified => Some(Msg::ActionSortByModified),
        KeyAction::SortBySize => Some(Msg::ActionSortBySize),
        KeyAction::ContextMenu => Some(Msg::ActionContextMenu),
        KeyAction::Search => Some(Msg::ActionSearch),
        KeyAction::QuickOpen => Some(Msg::ActionQuickOpen),
        KeyAction::FilingStatus => Some(Msg::ActionFilingStatus),
        KeyAction::FilingSetup => Some(Msg::ActionFilingSetup),
        KeyAction::SearchCase => Some(Msg::ActionSearchCase),
        KeyAction::SearchWholeWord => Some(Msg::ActionSearchWholeWord),
        KeyAction::SearchRegex => Some(Msg::ActionSearchRegex),
    }
}

/// Entries shown at once; the list scrolls past that.
pub const VISIBLE_ENTRIES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub action: KeyAction,
    pub label: &'static str,
    /// The first key bound to the action, spelled for this platform.
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Position in the list of entries.
    pub entry: usize,
    pub score: u32,
    /// Byte ranges into the label, consecutive matched characters merged.
    pub highlight: Vec<Range<usize>>,
}

#[derive(Debug)]
pub struct CommandPalette {
    query: String,
    entries: Vec<Entry>,
    hits: Vec<Hit>,
    cursor: usize,
    /// The first hit in view.
    top: usize,
}

impl CommandPalette {
    /// The actions that work in `scope`, labelled in `lang`, with the keys of
    /// `keymap`.
    pub fn new(scope: KeyScope, lang: Language, keymap: &Keymap) -> Self {
        let mut palette = Self {
            query: String::new(),
            entries: entries(scope, lang, keymap),
            hits: Vec::new(),
            cursor: 0,
            top: 0,
        };
        palette.rank();
        palette
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    pub fn entry(&self, hit: &Hit) -> Option<&Entry> {
        self.entries.get(hit.entry)
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The hits in view, by position in [`Self::hits`].
    pub fn visible(&self) -> Range<usize> {
        self.top..(self.top + VISIBLE_ENTRIES).min(self.hits.len())
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.rank();
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.hits.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
        self.keep_cursor_in_view();
    }

    pub fn select(&mut self, index: usize) {
        if index < self.hits.len() {
            self.cursor = index;
            self.keep_cursor_in_view();
        }
    }

    pub fn selected_action(&self) -> Option<KeyAction> {
        let hit = self.hits.get(self.cursor)?;
        self.entry(hit).map(|entry| entry.action)
    }

    fn keep_cursor_in_view(&mut self) {
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + VISIBLE_ENTRIES {
            self.top = self.cursor + 1 - VISIBLE_ENTRIES;
        }
    }

    /// Best score first; equal scores, and every entry for an empty query,
    /// stay in the order of the list.
    fn rank(&mut self) {
        let pattern = pattern_for(&self.query);
        self.hits = if pattern.atoms.is_empty() {
            (0..self.entries.len())
                .map(|entry| Hit {
                    entry,
                    score: 0,
                    highlight: Vec::new(),
                })
                .collect()
        } else {
            let mut matcher = Matcher::new(Config::DEFAULT);
            let mut buffer = Vec::new();
            let mut hits: Vec<Hit> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(entry, candidate)| {
                    let (score, highlight) =
                        score_with_highlight(&pattern, &mut matcher, &mut buffer, candidate.label)?;
                    Some(Hit {
                        entry,
                        score,
                        highlight,
                    })
                })
                .collect();
            hits.sort_by(|first, second| match second.score.cmp(&first.score) {
                Ordering::Equal => first.entry.cmp(&second.entry),
                other => other,
            });
            hits
        };
        self.cursor = 0;
        self.top = 0;
    }
}

fn entries(scope: KeyScope, lang: Language, keymap: &Keymap) -> Vec<Entry> {
    KeyAction::ALL
        .iter()
        .filter(|action| action.works_in(scope))
        .filter_map(|&action| {
            Some(Entry {
                action,
                label: lang.text(label_of(action)?),
                shortcut: keymap.shortcut_label(&action.message()),
            })
        })
        .collect()
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
mod tests {
    use super::*;

    fn palette(scope: KeyScope, lang: Language) -> CommandPalette {
        CommandPalette::new(scope, lang, &Keymap::default())
    }

    fn labels(palette: &CommandPalette) -> Vec<&'static str> {
        palette
            .hits()
            .iter()
            .map(|hit| palette.entry(hit).unwrap().label)
            .collect()
    }

    fn listed() -> Vec<KeyAction> {
        KeyAction::ALL
            .iter()
            .copied()
            .filter(|action| label_of(*action).is_some())
            .collect()
    }

    #[test]
    fn every_listed_action_has_a_label_in_both_languages() {
        let listed = listed();
        assert!(!listed.is_empty());
        for action in listed {
            let msg = label_of(action).unwrap();
            for lang in [Language::English, Language::German] {
                assert!(
                    !lang.text(msg).trim().is_empty(),
                    "{} has no {lang:?} label",
                    action.name()
                );
            }
            assert_eq!(
                msg.key(),
                format!("action.{}", action.name()),
                "the label key follows the config name"
            );
        }
    }

    #[test]
    fn only_movement_and_the_palette_itself_stay_out() {
        let mut unlisted: Vec<&str> = KeyAction::ALL
            .iter()
            .filter(|action| label_of(**action).is_none())
            .map(|action| action.name())
            .collect();
        unlisted.sort_unstable();
        assert_eq!(
            unlisted,
            [
                "command_palette",
                "cursor_down",
                "cursor_up",
                "first",
                "last",
                "page_down",
                "page_up",
                "tag_down",
                "tag_up"
            ]
        );
    }

    #[test]
    fn an_empty_query_lists_the_actions_of_the_scope_in_a_fixed_order() {
        let panel = palette(KeyScope::Panel, Language::English);
        let expected: Vec<KeyAction> = listed()
            .into_iter()
            .filter(|action| action.works_in(KeyScope::Panel))
            .collect();
        let actual: Vec<KeyAction> = panel
            .hits()
            .iter()
            .map(|hit| panel.entry(hit).unwrap().action)
            .collect();
        assert_eq!(actual, expected);
        assert!(panel.hits().iter().all(|hit| hit.highlight.is_empty()));
        assert_eq!(
            labels(&panel),
            labels(&palette(KeyScope::Panel, Language::English))
        );
    }

    #[test]
    fn the_search_lists_its_own_actions_and_not_the_panel_ones() {
        let search = palette(KeyScope::Search, Language::English);
        let actions: Vec<KeyAction> = search
            .hits()
            .iter()
            .map(|hit| search.entry(hit).unwrap().action)
            .collect();
        assert!(actions.contains(&KeyAction::SearchRegex));
        assert!(actions.contains(&KeyAction::Quit));
        assert!(!actions.contains(&KeyAction::Mkdir));
        let panel = palette(KeyScope::Panel, Language::English);
        assert!(!panel
            .hits()
            .iter()
            .any(|hit| panel.entry(hit).unwrap().action == KeyAction::SearchRegex));
    }

    #[test]
    fn typing_filters_and_marks_the_matched_letters() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.set_query("insp".to_owned());
        assert_eq!(labels(&palette), ["Inspection: Hermes status and errors"]);
        let hit = &palette.hits()[0];
        let marked: Vec<&str> = hit
            .highlight
            .iter()
            .map(|range| &palette.entry(hit).unwrap().label[range.clone()])
            .collect();
        assert_eq!(marked, ["Insp"]);
    }

    #[test]
    fn two_words_both_have_to_match() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.set_query("inspection errors".to_owned());
        assert_eq!(labels(&palette), ["Inspection: Hermes status and errors"]);
    }

    #[test]
    fn losch_finds_loeschen_without_breaking_the_marks() {
        let mut palette = palette(KeyScope::Panel, Language::German);
        palette.set_query("losch".to_owned());
        let labels = labels(&palette);
        assert!(labels.contains(&"Löschen"), "{labels:?}");
        for hit in palette.hits() {
            let label = palette.entry(hit).unwrap().label;
            for range in &hit.highlight {
                assert!(label.get(range.clone()).is_some(), "{label}: {range:?}");
            }
        }
        let delete = palette
            .hits()
            .iter()
            .find(|hit| palette.entry(hit).unwrap().label == "Löschen")
            .unwrap();
        assert_eq!(delete.highlight, vec![0.."Lösch".len()]);
    }

    #[test]
    fn equal_scores_keep_the_order_of_the_list() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.set_query("o".to_owned());
        assert!(palette.hits().len() > 2);
        for pair in palette.hits().windows(2) {
            let [first, second] = pair else {
                panic!("a window of two");
            };
            assert!(
                first.score > second.score
                    || (first.score == second.score && first.entry < second.entry),
                "{pair:?}"
            );
        }
        let mut again = super::tests::palette(KeyScope::Panel, Language::English);
        again.set_query("o".to_owned());
        assert_eq!(palette.hits(), again.hits());
    }

    #[test]
    fn a_new_query_puts_the_cursor_back_on_the_first_hit() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.move_cursor(5);
        assert_eq!(palette.cursor(), 5);
        palette.set_query("e".to_owned());
        assert_eq!(palette.cursor(), 0);
    }

    #[test]
    fn the_cursor_stays_in_view_and_inside_the_list() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.move_cursor(-1);
        assert_eq!(palette.cursor(), 0);
        palette.move_cursor(isize::MAX);
        assert_eq!(palette.cursor(), palette.hits().len() - 1);
        let visible = palette.visible();
        assert!(visible.contains(&palette.cursor()), "{visible:?}");
        assert_eq!(visible.len(), VISIBLE_ENTRIES);
        palette.move_cursor(isize::MIN);
        assert_eq!(palette.visible().start, 0);
    }

    #[test]
    fn selecting_past_the_end_changes_nothing() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.select(2);
        palette.select(10_000);
        assert_eq!(palette.cursor(), 2);
    }

    #[test]
    fn the_entry_carries_the_first_key_of_its_action() {
        let palette = palette(KeyScope::Panel, Language::English);
        let key_of = |action| {
            palette
                .hits()
                .iter()
                .map(|hit| palette.entry(hit).unwrap())
                .find(|entry| entry.action == action)
                .unwrap()
                .shortcut
                .clone()
        };
        assert_eq!(key_of(KeyAction::Copy).as_deref(), Some("F5"));
        assert_eq!(key_of(KeyAction::Open).as_deref(), Some("Enter"));
    }
}
