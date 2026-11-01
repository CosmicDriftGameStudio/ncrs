use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::*;
use ncrs_config::config;

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
    assert!(!ncrs_fs::sandbox_active());
    assert!(dialog
        .rows()
        .iter()
        .all(|row| row.action != KeyAction::GrantFolderAccess));
    // The Finder's trash command is the other action only a Mac lists; the
    // old document search is listed nowhere.
    let hidden = if cfg!(target_os = "macos") { 2 } else { 3 };
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
