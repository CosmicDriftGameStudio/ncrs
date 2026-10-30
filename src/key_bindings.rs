//! The keyboard shortcuts dialog: every action with its keys, found by name
//! or by pressing a key, rebound by pressing the new one. No widgets: the
//! view reads the rows; an edit returns the `[keys]` lines to write, and the
//! keymap read back from the file is the next state of the rows.
//!
//! A key another action holds is never taken silently: the edit stops at a
//! [`Clash`] until it is confirmed, and confirming writes the other action's
//! remaining keys too. What reaches the file obeys the rules of
//! [`Keymap::with_overrides`], which the write checks again before it
//! replaces the file.

use iced::mouse::ScrollDelta;

use crate::commands::{self, CommandGroup};
use crate::config::KeyChange;
use crate::i18n::{Language, Msg};
use crate::keymap::{
    binding_label, built_in_bindings_of, configurable_chord, Binding, KeyAction, KeyRefusal,
    Keymap, MAX_KEYS_PER_ACTION,
};
use crate::list_window::ListWindow;

/// Rows of actions shown at once; the list scrolls past that.
pub const VISIBLE_ROWS: usize = 16;

/// One action as the dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub action: KeyAction,
    pub label: &'static str,
    pub theme_name: Option<&'static str>,
    pub group: CommandGroup,
    pub keys: Vec<Binding>,
    /// The action has the keys it ships with.
    pub is_built_in: bool,
}

/// An edit that would take keys from other actions, shown before anything is
/// written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    pub action: KeyAction,
    /// All keys `action` gets.
    pub keys: Vec<Binding>,
    /// The keys among them that other actions hold, and who holds each.
    pub taken: Vec<(KeyAction, Binding)>,
}

/// What the dialog is doing besides listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Browsing,
    /// The next key pressed becomes a key of the selected action, in place
    /// of `replacing` or as a further one.
    Recording {
        replacing: Option<Binding>,
    },
    Clash(Clash),
    /// "Reset all" asks first.
    ResetAllQuestion,
}

/// Why the last edit did not happen; shown until the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Refused(KeyRefusal),
    TooMany,
    /// The key is already one of the action's.
    AlreadyBound(Binding),
    /// The write failed; the text says why.
    NotWritten(String),
}

#[derive(Debug)]
pub struct KeyBindings {
    lang: Language,
    query: String,
    /// Set by a key pressed in the search field: only the actions on exactly
    /// that key are listed.
    key_filter: Option<Binding>,
    rows: Vec<Row>,
    cursor: usize,
    /// The focused key cap of the selected row; one past its keys is the slot
    /// that adds a key.
    key_cursor: usize,
    window: ListWindow,
    mode: Mode,
    notice: Option<Notice>,
    /// A write is on its way; edits wait for the keymap it brings back.
    writing: bool,
}

/// The label of an action, also of the cursor keys the command list leaves
/// out because they make no sense as commands.
fn action_label(action: KeyAction) -> Option<Msg> {
    if let Some(command) = commands::command_of(action) {
        return Some(command.label);
    }
    match action {
        KeyAction::CursorUp => Some(Msg::ActionCursorUp),
        KeyAction::CursorDown => Some(Msg::ActionCursorDown),
        KeyAction::PageUp => Some(Msg::ActionPageUp),
        KeyAction::PageDown => Some(Msg::ActionPageDown),
        KeyAction::First => Some(Msg::ActionFirst),
        KeyAction::Last => Some(Msg::ActionLast),
        KeyAction::TagUp => Some(Msg::ActionTagUp),
        KeyAction::TagDown => Some(Msg::ActionTagDown),
        _ => None,
    }
}

fn row_of(action: KeyAction, lang: Language, keymap: &Keymap) -> Row {
    let command = commands::command_of(action);
    Row {
        action,
        label: action_label(action).map_or(action.name(), |label| lang.text(label)),
        theme_name: command
            .and_then(|found| found.theme_name)
            .and_then(|theme_name| lang.futurama_name(theme_name)),
        // The cursor keys move in the panel, which is where Go's commands lead.
        group: command.map_or(CommandGroup::Go, |found| found.group),
        keys: keymap.bindings_of(action).cloned().collect(),
        is_built_in: keymap.has_built_in_keys(action),
    }
}

impl Row {
    fn matches(&self, words: &[String], lang: Language) -> bool {
        let haystack = [
            self.label.to_lowercase(),
            self.theme_name.unwrap_or_default().to_lowercase(),
            lang.text(self.group.label()).to_lowercase(),
            self.action.name().replace('_', " "),
        ]
        .into_iter()
        .chain(
            self.keys
                .iter()
                .filter_map(binding_label)
                .map(|label| label.to_lowercase()),
        )
        .collect::<Vec<_>>();
        words
            .iter()
            .all(|word| haystack.iter().any(|text| text.contains(word.as_str())))
    }
}

impl KeyBindings {
    pub fn new(lang: Language, keymap: &Keymap) -> Self {
        let mut dialog = Self {
            lang,
            query: String::new(),
            key_filter: None,
            rows: Vec::new(),
            cursor: 0,
            key_cursor: 0,
            window: ListWindow::default(),
            mode: Mode::Browsing,
            notice: None,
            writing: false,
        };
        dialog.refresh(keymap);
        dialog
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn key_cursor(&self) -> usize {
        self.key_cursor
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.mode, Mode::Recording { .. })
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    /// The name of `action` in the dialog's language, also when no row lists it.
    pub fn action_name(&self, action: KeyAction) -> &'static str {
        action_label(action).map_or(action.name(), |label| self.lang.text(label))
    }

    /// The first row shown.
    pub fn offset(&self) -> usize {
        self.window.offset(self.rows.len(), VISIBLE_ROWS)
    }

    /// Lists the actions again with the keys of `keymap`, keeping the
    /// selected action selected while it is still listed.
    pub fn refresh(&mut self, keymap: &Keymap) {
        let selected = self.selected().map(|row| row.action);
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut rows: Vec<Row> = KeyAction::ALL
            .iter()
            .filter(|action| action.is_offered())
            .map(|action| row_of(*action, self.lang, keymap))
            .filter(|row| match &self.key_filter {
                Some(binding) => row.keys.contains(binding),
                None => row.matches(&words, self.lang),
            })
            .collect();
        rows.sort_by_key(|row| row.group);
        self.rows = rows;
        self.cursor = selected
            .and_then(|action| self.rows.iter().position(|row| row.action == action))
            .unwrap_or(0);
        self.clamp_key_cursor();
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
    }

    pub fn set_query(&mut self, query: String, keymap: &Keymap) {
        self.query = query;
        self.key_filter = None;
        self.cursor = 0;
        self.refresh(keymap);
    }

    /// Lists only the actions on `binding`; the field shows the key.
    pub fn search_by_key(&mut self, binding: Binding, keymap: &Keymap) {
        self.query = binding_label(&binding).unwrap_or_default();
        self.key_filter = Some(binding);
        self.cursor = 0;
        self.refresh(keymap);
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if !self.is_browsing() {
            return;
        }
        let last = self.rows.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
        self.key_cursor = 0;
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
    }

    pub fn move_key_cursor(&mut self, delta: isize) {
        if !self.is_browsing() {
            return;
        }
        self.key_cursor = self.key_cursor.saturating_add_signed(delta);
        self.clamp_key_cursor();
    }

    pub fn select(&mut self, row: usize, key: Option<usize>) {
        if !self.is_browsing() || row >= self.rows.len() {
            return;
        }
        self.cursor = row;
        self.key_cursor = key.unwrap_or(0);
        self.clamp_key_cursor();
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
    }

    pub fn scroll(&mut self, delta: ScrollDelta) {
        if !self.is_browsing() {
            return;
        }
        let before = self.cursor;
        self.window
            .scroll(delta, self.rows.len(), VISIBLE_ROWS, &mut self.cursor);
        if self.cursor != before {
            self.key_cursor = 0;
        }
    }

    /// Waits for the key that replaces the focused one, or for a further key
    /// on the slot that adds one.
    pub fn record(&mut self) {
        let Some(row) = self
            .selected()
            .filter(|_| self.is_browsing() && !self.writing)
        else {
            return;
        };
        let replacing = row.keys.get(self.key_cursor).cloned();
        self.notice = None;
        self.mode = Mode::Recording { replacing };
    }

    /// Waits for a further key of the selected action.
    pub fn record_additional(&mut self) {
        if self.selected().is_none() || !self.is_browsing() || self.writing {
            return;
        }
        self.notice = None;
        self.mode = Mode::Recording { replacing: None };
    }

    pub fn cancel(&mut self) {
        self.mode = Mode::Browsing;
    }

    /// The key pressed while recording. Returns the lines to write, or
    /// `None` when there is nothing to write yet: the key is refused, or
    /// another action holds it and the clash waits for an answer.
    pub fn recorded(&mut self, binding: Binding, keymap: &Keymap) -> Option<Vec<KeyChange>> {
        let Mode::Recording { replacing } = std::mem::replace(&mut self.mode, Mode::Browsing)
        else {
            return None;
        };
        let action = self.selected()?.action;
        if replacing.as_ref() == Some(&binding) {
            return None;
        }
        let mut keys: Vec<Binding> = keymap.bindings_of(action).cloned().collect();
        if keys.contains(&binding) {
            self.notice = Some(Notice::AlreadyBound(binding));
            return None;
        }
        if let Err(refusal) = configurable_chord(action, &binding) {
            self.notice = Some(Notice::Refused(refusal));
            return None;
        }
        match replacing.and_then(|old| keys.iter().position(|key| *key == old)) {
            Some(place) => {
                if let Some(slot) = keys.get_mut(place) {
                    *slot = binding.clone();
                }
            }
            None if keys.len() >= MAX_KEYS_PER_ACTION => {
                self.notice = Some(Notice::TooMany);
                return None;
            }
            None => keys.push(binding.clone()),
        }
        self.change_or_clash(action, keys, &[binding], keymap)
    }

    /// Delete or Backspace: the focused key goes; an action may end up with
    /// none, as `[]` in `[keys]` allows.
    pub fn remove_key(&mut self) -> Option<Vec<KeyChange>> {
        if !self.is_browsing() || self.writing {
            return None;
        }
        let row = self.selected()?;
        let action = row.action;
        let removed = row.keys.get(self.key_cursor)?.clone();
        let keys: Vec<Binding> = row
            .keys
            .iter()
            .filter(|key| **key != removed)
            .cloned()
            .collect();
        self.notice = None;
        self.changes(&[(action, keys)])
    }

    /// The selected action gets its built-in keys back.
    pub fn reset(&mut self, keymap: &Keymap) -> Option<Vec<KeyChange>> {
        if !self.is_browsing() || self.writing {
            return None;
        }
        let row = self.selected()?;
        if row.is_built_in {
            return None;
        }
        let action = row.action;
        let keys = built_in_bindings_of(action);
        self.notice = None;
        self.change_or_clash(action, keys.clone(), &keys, keymap)
    }

    pub fn ask_reset_all(&mut self) {
        if self.is_browsing() && !self.writing {
            self.notice = None;
            self.mode = Mode::ResetAllQuestion;
        }
    }

    /// The answer to a clash or to "reset all": the lines to write.
    pub fn confirm(&mut self, keymap: &Keymap) -> Option<Vec<KeyChange>> {
        match std::mem::replace(&mut self.mode, Mode::Browsing) {
            Mode::Clash(clash) => {
                let mut lines = vec![(clash.action, clash.keys)];
                for (holder, _) in &clash.taken {
                    if lines.iter().any(|(action, _)| action == holder) {
                        continue;
                    }
                    let kept: Vec<Binding> = keymap
                        .bindings_of(*holder)
                        .filter(|key| !clash.taken.contains(&(*holder, (*key).clone())))
                        .cloned()
                        .collect();
                    lines.push((*holder, kept));
                }
                self.changes(&lines)
            }
            Mode::ResetAllQuestion => Some(
                KeyAction::ALL
                    .iter()
                    .map(|action| KeyChange {
                        action: *action,
                        chords: None,
                    })
                    .collect(),
            ),
            other @ (Mode::Browsing | Mode::Recording { .. }) => {
                self.mode = other;
                None
            }
        }
    }

    /// Leaves a clash or the question; nothing is written.
    pub fn dismiss(&mut self) {
        if matches!(self.mode, Mode::Clash(_) | Mode::ResetAllQuestion) {
            self.mode = Mode::Browsing;
        }
    }

    /// The write of an edit has started.
    pub fn start_writing(&mut self) {
        self.writing = true;
    }

    /// The write is over: the rows show `keymap`, the one read back or the
    /// one still in effect, and `failure` says why the write did not happen.
    pub fn written(&mut self, keymap: &Keymap, failure: Option<String>) {
        self.writing = false;
        if let Some(reason) = failure {
            self.notice = Some(Notice::NotWritten(reason));
        }
        self.refresh(keymap);
    }

    /// Enter: answers a clash or the question with yes, or records a key.
    pub fn activate(&mut self, keymap: &Keymap) -> Option<Vec<KeyChange>> {
        if matches!(self.mode, Mode::Clash(_) | Mode::ResetAllQuestion) {
            return self.confirm(keymap);
        }
        self.record();
        None
    }

    /// Escape outside recording: leaves a clash or the question first.
    /// Returns whether the dialog stays open.
    pub fn escape(&mut self) -> bool {
        match self.mode {
            Mode::Clash(_) | Mode::ResetAllQuestion | Mode::Recording { .. } => {
                self.mode = Mode::Browsing;
                true
            }
            Mode::Browsing => false,
        }
    }

    fn is_browsing(&self) -> bool {
        self.mode == Mode::Browsing
    }

    fn clamp_key_cursor(&mut self) {
        let slots = self.selected().map_or(0, |row| row.keys.len());
        self.key_cursor = self.key_cursor.min(slots);
    }

    /// The lines for `action` getting `keys`, or a clash when other actions
    /// hold some of `new_keys`.
    fn change_or_clash(
        &mut self,
        action: KeyAction,
        keys: Vec<Binding>,
        new_keys: &[Binding],
        keymap: &Keymap,
    ) -> Option<Vec<KeyChange>> {
        let taken: Vec<(KeyAction, Binding)> = new_keys
            .iter()
            .filter_map(|key| {
                keymap
                    .holder_of(action, key)
                    .map(|holder| (holder, key.clone()))
            })
            .collect();
        if taken.is_empty() {
            return self.changes(&[(action, keys)]);
        }
        self.mode = Mode::Clash(Clash {
            action,
            keys,
            taken,
        });
        None
    }

    /// `[keys]` lines that give each action its keys: an action back at its
    /// built-in keys loses its line, so a later release can change them.
    fn changes(&mut self, lines: &[(KeyAction, Vec<Binding>)]) -> Option<Vec<KeyChange>> {
        let mut changes = Vec::with_capacity(lines.len());
        for (action, keys) in lines {
            if *keys == built_in_bindings_of(*action) {
                changes.push(KeyChange {
                    action: *action,
                    chords: None,
                });
                continue;
            }
            let mut chords = Vec::with_capacity(keys.len());
            for key in keys {
                match configurable_chord(*action, key) {
                    Ok(chord) => chords.push(chord),
                    Err(refusal) => {
                        self.notice = Some(Notice::Refused(refusal));
                        return None;
                    }
                }
            }
            changes.push(KeyChange {
                action: *action,
                chords: Some(chords),
            });
        }
        Some(changes)
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
mod tests {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};

    use super::*;
    use crate::config;

    fn key(named: Named) -> Binding {
        Binding::key(Key::Named(named))
    }

    /// A config with `keys` as its `[keys]` lines, and its path for writes.
    fn config_with(keys: &str) -> (tempfile::TempDir, std::path::PathBuf, Keymap) {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, format!("[keys]\n{keys}")).unwrap();
        let keymap = (*config::load(&path).unwrap().keymap).clone();
        (base, path, keymap)
    }

    fn select(dialog: &mut KeyBindings, action: KeyAction) {
        let row = dialog
            .rows()
            .iter()
            .position(|row| row.action == action)
            .unwrap();
        dialog.select(row, None);
    }

    /// Writes `changes` and returns the keymap read back, as the app does.
    fn written(path: &std::path::Path, changes: &[KeyChange]) -> Keymap {
        (*config::write_keys(path, changes).unwrap().keymap).clone()
    }

    /// The keymap of the file at `path` as a fresh start reads it.
    fn reloaded(path: &std::path::Path) -> Keymap {
        (*config::load(path).unwrap().keymap).clone()
    }

    fn holder(keymap: &Keymap, binding: &Binding) -> Option<KeyAction> {
        KeyAction::ALL
            .iter()
            .copied()
            .find(|action| keymap.bindings_of(*action).any(|held| held == binding))
    }

    #[test]
    fn the_grant_action_is_not_listed_outside_the_sandbox() {
        let keymap = Keymap::built_in();
        let dialog = KeyBindings::new(Language::English, &keymap);
        assert!(!crate::fs::sandbox_active());
        assert!(dialog
            .rows()
            .iter()
            .all(|row| row.action != KeyAction::GrantFolderAccess));
        // The Finder's trash command is the other action only a Mac lists.
        let hidden = if cfg!(target_os = "macos") { 1 } else { 2 };
        assert_eq!(dialog.rows().len(), KeyAction::ALL.len() - hidden);
    }

    #[test]
    fn only_the_futurama_variants_show_theme_names() {
        let keymap = Keymap::built_in();
        for lang in [Language::English, Language::German] {
            let dialog = KeyBindings::new(lang, &keymap);
            assert!(dialog.rows().iter().all(|row| row.theme_name.is_none()));
        }
        let dialog = KeyBindings::new(Language::EnglishFuturama, &keymap);
        let search = dialog
            .rows()
            .iter()
            .find(|row| row.action == KeyAction::Search)
            .unwrap();
        assert_eq!(search.theme_name, Some("Smell-o-scope"));
    }

    #[test]
    fn every_action_is_listed_by_its_name_in_the_ui_language() {
        let keymap = Keymap::built_in();
        let dialog = KeyBindings::new(Language::German, &keymap);
        let offered = KeyAction::ALL.iter().filter(|a| a.is_offered()).count();
        assert_eq!(dialog.rows().len(), offered);
        for row in dialog.rows() {
            assert!(
                action_label(row.action).is_some(),
                "{} has no label",
                row.action.name()
            );
            assert!(row.is_built_in, "{}", row.action.name());
        }
        let groups: Vec<CommandGroup> = dialog.rows().iter().map(|row| row.group).collect();
        assert!(groups.is_sorted(), "{groups:?}");
    }

    #[test]
    fn a_recorded_key_replaces_the_focused_one_and_takes_effect() {
        let (_base, path, keymap) = config_with("");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);

        dialog.record();
        assert!(dialog.is_recording());
        let changes = dialog.recorded(key(Named::F12), &keymap).unwrap();
        assert_eq!(
            changes,
            [KeyChange {
                action: KeyAction::View,
                chords: Some(vec!["f12".to_owned()]),
            }]
        );

        let rebound = written(&path, &changes);
        assert_eq!(holder(&rebound, &key(Named::F12)), Some(KeyAction::View));
        assert_eq!(holder(&rebound, &key(Named::F3)), None);
        dialog.refresh(&rebound);
        let row = dialog.selected().unwrap();
        assert_eq!(row.action, KeyAction::View);
        assert!(!row.is_built_in);
    }

    #[test]
    fn a_further_key_is_added_after_the_others() {
        let keymap = Keymap::built_in();
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        dialog.move_key_cursor(1);
        assert_eq!(dialog.key_cursor(), 1, "the slot that adds a key");
        dialog.record();
        let changes = dialog.recorded(key(Named::F12), &keymap).unwrap();
        assert_eq!(
            changes[0].chords,
            Some(vec!["f3".to_owned(), "f12".to_owned()])
        );
    }

    #[test]
    fn a_key_of_another_action_waits_for_the_takeover_and_moves_with_it() {
        let (_base, path, keymap) = config_with("");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        dialog.record();

        assert_eq!(dialog.recorded(key(Named::F5), &keymap), None);
        let Mode::Clash(clash) = dialog.mode() else {
            panic!("{:?}", dialog.mode());
        };
        assert_eq!(clash.taken, [(KeyAction::Copy, key(Named::F5))]);

        let changes = dialog.confirm(&keymap).unwrap();
        let rebound = written(&path, &changes);
        assert_eq!(holder(&rebound, &key(Named::F5)), Some(KeyAction::View));
        assert_eq!(rebound.bindings_of(KeyAction::Copy).count(), 0);
        assert_eq!(dialog.mode(), &Mode::Browsing);
    }

    #[test]
    fn an_only_key_taken_away_leaves_its_action_without_one_after_a_restart() {
        let (_base, path, keymap) = config_with("copy = [\"f12\"]\n");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        dialog.record();
        assert_eq!(dialog.recorded(key(Named::F12), &keymap), None);
        written(&path, &dialog.confirm(&keymap).unwrap());

        let written_text = std::fs::read_to_string(&path).unwrap();
        assert!(written_text.contains("copy = []"), "{written_text}");
        let restarted = reloaded(&path);
        assert_eq!(restarted.bindings_of(KeyAction::Copy).count(), 0);
        assert_eq!(holder(&restarted, &key(Named::F5)), None);
        assert_eq!(holder(&restarted, &key(Named::F12)), Some(KeyAction::View));
    }

    #[test]
    fn a_dismissed_clash_writes_nothing() {
        let keymap = Keymap::built_in();
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        dialog.record();
        assert_eq!(dialog.recorded(key(Named::F5), &keymap), None);
        dialog.dismiss();
        assert_eq!(dialog.mode(), &Mode::Browsing);
        assert_eq!(dialog.confirm(&keymap), None);
    }

    #[test]
    fn a_key_the_config_cannot_hold_is_refused_before_anything_is_written() {
        let keymap = Keymap::built_in();
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        dialog.record();
        let bare_letter = Binding::key(Key::Character("v".into()));
        assert_eq!(dialog.recorded(bare_letter, &keymap), None);
        assert_eq!(
            dialog.notice(),
            Some(&Notice::Refused(KeyRefusal::NeedsModifier))
        );

        select(&mut dialog, KeyAction::SearchCase);
        dialog.record();
        assert_eq!(dialog.recorded(key(Named::ArrowUp), &keymap), None);
        assert_eq!(
            dialog.notice(),
            Some(&Notice::Refused(KeyRefusal::FixedInSearch))
        );
    }

    #[test]
    fn a_removed_key_leaves_the_action_and_the_last_one_leaves_it_without() {
        let (_base, path, keymap) = config_with("");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        let changes = dialog.remove_key().unwrap();
        assert_eq!(changes[0].chords, Some(Vec::new()));
        let rebound = written(&path, &changes);
        assert_eq!(rebound.bindings_of(KeyAction::View).count(), 0);
        assert_eq!(holder(&rebound, &key(Named::F3)), None);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[keys]\nview = []\n"
        );
        let restarted = reloaded(&path);
        assert_eq!(restarted.bindings_of(KeyAction::View).count(), 0);
        assert_eq!(holder(&restarted, &key(Named::F3)), None);
    }

    #[test]
    fn the_last_configured_key_removed_stays_removed_after_a_restart() {
        let (_base, path, keymap) = config_with("view = [\"f12\"]\n");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        written(&path, &dialog.remove_key().unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[keys]\nview = []\n"
        );
        let restarted = reloaded(&path);
        assert_eq!(restarted.bindings_of(KeyAction::View).count(), 0);
        assert_eq!(holder(&restarted, &key(Named::F3)), None);
        assert_eq!(holder(&restarted, &key(Named::F12)), None);
    }

    #[test]
    fn a_reset_drops_the_line_and_takes_back_a_key_given_away() {
        let (_base, path, keymap) = config_with("view = \"f12\"\ncopy = [\"f3\", \"f5\"]\n");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        select(&mut dialog, KeyAction::View);
        assert!(!dialog.selected().unwrap().is_built_in);

        assert_eq!(dialog.reset(&keymap), None, "F3 is Copy's now");
        let Mode::Clash(clash) = dialog.mode() else {
            panic!("{:?}", dialog.mode());
        };
        assert_eq!(clash.taken, [(KeyAction::Copy, key(Named::F3))]);
        let changes = dialog.confirm(&keymap).unwrap();
        assert!(changes.contains(&KeyChange {
            action: KeyAction::View,
            chords: None,
        }));
        assert!(
            changes.contains(&KeyChange {
                action: KeyAction::Copy,
                chords: None,
            }),
            "Copy is back at F5 alone, its built-in key"
        );

        let rebound = written(&path, &changes);
        assert!(rebound.has_built_in_keys(KeyAction::View));
        assert!(rebound.has_built_in_keys(KeyAction::Copy));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[keys]\n");
    }

    #[test]
    fn reset_all_asks_first_and_then_drops_every_line() {
        let (_base, path, keymap) = config_with("view = \"f12\"\nquit = \"f10\"\n");
        let mut dialog = KeyBindings::new(Language::English, &keymap);
        dialog.ask_reset_all();
        assert_eq!(dialog.mode(), &Mode::ResetAllQuestion);
        let changes = dialog.confirm(&keymap).unwrap();
        let rebound = written(&path, &changes);
        assert_eq!(rebound, *Keymap::built_in());
    }

    #[test]
    fn a_pressed_key_finds_the_action_on_it_and_text_finds_names_and_keys() {
        let keymap = Keymap::built_in();
        let mut dialog = KeyBindings::new(Language::English, &keymap);

        let listed = |shown: &KeyBindings| -> Vec<KeyAction> {
            shown.rows().iter().map(|row| row.action).collect()
        };
        dialog.search_by_key(key(Named::F5), &keymap);
        assert_eq!(listed(&dialog), [KeyAction::Copy]);
        assert_eq!(dialog.query(), "F5");

        let primary_k = Binding::with_modifiers(Key::Character("k".into()), Modifiers::COMMAND);
        dialog.search_by_key(primary_k, &keymap);
        assert_eq!(listed(&dialog), [KeyAction::KeyBindings]);

        dialog.set_query("keyboard short".to_owned(), &keymap);
        assert_eq!(listed(&dialog), [KeyAction::KeyBindings]);

        dialog.set_query("f8".to_owned(), &keymap);
        assert!(dialog
            .rows()
            .iter()
            .any(|row| row.action == KeyAction::Delete));
    }

    #[test]
    fn the_german_names_are_searched_in_german() {
        let keymap = Keymap::built_in();
        let mut dialog = KeyBindings::new(Language::German, &keymap);
        dialog.set_query("tastenkürzel".to_owned(), &keymap);
        let found: Vec<KeyAction> = dialog.rows().iter().map(|row| row.action).collect();
        assert_eq!(found, [KeyAction::KeyBindings]);
    }
}
