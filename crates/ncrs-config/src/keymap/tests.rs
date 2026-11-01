use super::*;
use crate::config;
use iced::keyboard::key::Named;

#[test]
fn restoring_the_last_deletion_is_on_primary_z_and_yields_it_to_a_config() {
    let keys: Vec<_> = default_bindings()
        .into_iter()
        .filter(|(action, _)| *action == KeyAction::RestoreLastDeletion)
        .map(|(_, binding)| binding)
        .collect();
    assert_eq!(
        keys,
        [Binding::with_modifiers(
            Key::Character("z".into()),
            Modifiers::COMMAND
        )]
    );
    assert!(KeyAction::RestoreLastDeletion.yields_its_built_in_key());
    assert!(!KeyAction::Trash.yields_its_built_in_key());
    assert_eq!(
        KeyAction::from_name("restore_last_deletion"),
        Some(KeyAction::RestoreLastDeletion)
    );
}

fn function_keys(lang: Language) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
    Keymap::built_in().function_keys(lang, KeyScope::Panel)
}

fn written(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, content).unwrap();
    (dir, path)
}

/// The keymap `[keys]` text produces, or the line and reason it fails with.
fn keymap_from(keys: &str) -> Result<Keymap, (Option<usize>, String)> {
    let (_dir, path) = written(keys);
    config::load(&path)
        .map(|config| (*config.keymap).clone())
        .map_err(|err| (err.line(), err.reason().to_owned()))
}

fn rebound(keys: &str) -> Keymap {
    keymap_from(keys).unwrap()
}

fn failure(keys: &str) -> (Option<usize>, String) {
    keymap_from(keys).unwrap_err()
}

fn named(key: Named) -> Key {
    Key::Named(key)
}

/// The point of this module: a label on the bar whose key does nothing, or
/// a binding the bar never shows. Both used to be possible.
#[test]
fn every_labelled_slot_has_a_binding_that_responds() {
    for (index, label) in function_keys(Language::English).iter().enumerate() {
        let Some(text) = label else { continue };
        let number = index + 1;
        let bound = Keymap::built_in()
            .bindings()
            .iter()
            .any(|(action, binding)| {
                action.hint().is_some()
                    && binding.modifiers == Modifiers::default()
                    && function_key_number(&binding.key) == Some(number)
            });
        assert!(bound, "F{number} is labelled {text:?} with no binding");
    }
}

#[test]
fn the_bar_shows_the_function_keys_and_nothing_else() {
    let en = function_keys(Language::English);
    assert_eq!(
        en,
        [
            Some("Commands"),
            Some("Cmd line"),
            Some("View"),
            Some("Edit"),
            Some("Copy"),
            Some("Move"),
            Some("Mkdir"),
            Some("Delete"),
            Some("Lang"),
            Some("Quit"),
        ]
    );
}

#[test]
fn the_labels_are_translated_and_fit_a_slot() {
    let en = function_keys(Language::English);
    let de = function_keys(Language::German);
    assert_eq!(de[7], Some("Löschen"));
    for (en_label, de_label) in en.iter().zip(de.iter()) {
        assert_eq!(en_label.is_some(), de_label.is_some());
        for label in [en_label, de_label].into_iter().flatten() {
            assert!(
                label.chars().count() <= 8,
                "{label:?} is too long for a slot"
            );
        }
    }
}

#[test]
fn only_the_command_line_gives_its_built_in_key_away() {
    let (_, clash_with_view) = failure("[keys]\ncopy = \"F3\"\n");
    assert!(
        clash_with_view.contains("already bound to view"),
        "{clash_with_view}"
    );
    let (_, clash_on_f2) = failure("[keys]\ncopy = \"F2\"\nmove = \"F2\"\n");
    assert!(
        clash_on_f2.contains("already bound to copy"),
        "{clash_on_f2}"
    );
}

#[test]
fn a_key_section_that_used_the_free_f1_is_told_how_to_keep_it() {
    let (_, reason) = failure("[keys]\ncopy = \"F1\"\n");
    assert!(
        reason.contains("already bound to command_palette; give command_palette other keys too"),
        "{reason}"
    );
}

#[test]
fn action_names_round_trip_and_are_unique() {
    for action in KeyAction::ALL {
        assert_eq!(KeyAction::from_name(action.name()), Some(*action));
    }
    let mut names: Vec<_> = KeyAction::ALL.iter().map(|action| action.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), KeyAction::ALL.len());
    assert_eq!(KeyAction::from_name("nope"), None);
}

#[test]
fn paste_and_copy_files_are_on_the_command_chord() {
    let bindings = default_bindings();
    let paste: Vec<_> = bindings
        .iter()
        .filter(|(action, _)| *action == KeyAction::Paste)
        .map(|(_, binding)| binding)
        .collect();
    assert_eq!(
        paste,
        [&Binding::with_modifiers(
            Key::Character("v".into()),
            Modifiers::COMMAND
        )]
    );
    let copy_files: Vec<_> = bindings
        .iter()
        .filter(|(action, _)| *action == KeyAction::CopyFiles)
        .map(|(_, binding)| binding)
        .collect();
    assert_eq!(
        copy_files,
        [&Binding::with_modifiers(
            Key::Character("c".into()),
            Modifiers::COMMAND
        )]
    );
    assert_eq!(KeyAction::from_name("paste"), Some(KeyAction::Paste));
    assert_eq!(
        KeyAction::from_name("copy_files"),
        Some(KeyAction::CopyFiles)
    );
}

#[test]
fn every_action_has_a_default_key_and_no_key_is_twice_bound() {
    // Reached through the command palette (or a menu) only.
    let mut without_key = vec![
        KeyAction::RemoveFavourite,
        KeyAction::Favourites,
        KeyAction::FetchTheme,
        KeyAction::About,
        KeyAction::DownloadOcrModels,
        KeyAction::DownloadAiModel,
        KeyAction::GrantFolderAccess,
        KeyAction::ConnectServer,
        KeyAction::DisconnectServer,
        KeyAction::Trash,
        KeyAction::OpenTrashInFinder,
        KeyAction::ComputeFolderSizes,
        KeyAction::HermesHandToInbox,
        KeyAction::HermesReprocessFolder,
        KeyAction::HermesSetInbox,
        KeyAction::HermesSetFilingLocation,
        KeyAction::HermesSetArchive,
        KeyAction::HermesExclude,
        KeyAction::HermesInspect,
        KeyAction::SearchDocuments,
        KeyAction::FileListTagsToggle,
        KeyAction::FileListTagsChoose,
        KeyAction::OpenHitsLeft,
        KeyAction::OpenHitsRight,
    ];
    // Ctrl+C is copy_files there; Escape and the cancel button stop a job.
    if !cfg!(target_os = "macos") {
        without_key.push(KeyAction::AbortJob);
    }
    let bindings = default_bindings();
    for action in KeyAction::ALL
        .iter()
        .filter(|action| !without_key.contains(action))
    {
        assert!(
            bindings.iter().any(|(bound, _)| bound == action),
            "{} has no default key",
            action.name()
        );
    }
    for (index, (_, binding)) in bindings.iter().enumerate() {
        assert!(
            !bindings[index + 1..]
                .iter()
                .any(|(_, other)| other == binding),
            "{binding:?} is bound twice"
        );
    }
}

/// A default the user could not write would make `[keys]` unable to say
/// what the app does by itself, and a bare letter would shadow type-ahead.
#[test]
fn the_defaults_obey_the_rules_for_configured_keys() {
    for (action, binding) in default_bindings() {
        if let Key::Character(character) = &binding.key {
            let types_a_name = character.chars().all(char::is_alphanumeric);
            let has_chord_modifier = binding
                .modifiers
                .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO);
            assert!(
                !types_a_name || has_chord_modifier,
                "{} would shadow type-ahead",
                action.name()
            );
            assert!(
                !binding.modifiers.shift() || character.chars().all(char::is_alphabetic),
                "{} uses shift with a character it changes",
                action.name()
            );
        }
        assert_ne!(binding.key, Key::Named(Named::Escape));
    }
}

#[test]
fn a_file_without_keys_leaves_the_built_in_keymap() {
    assert_eq!(
        rebound("[open]\nview = [\"viewer\"]\n"),
        *Keymap::built_in()
    );
    assert_eq!(rebound(""), *Keymap::built_in());
}

#[test]
fn a_key_taken_from_an_unnamed_action_is_refused_at_its_line() {
    let (line, reason) = failure("[keys]\n\nview = \"F4\"\n");
    assert_eq!(line, Some(3));
    assert!(reason.contains("edit"), "{reason}");
}

/// `move` comes after `copy` in the enum but first in the file: the later
/// line is the one that is reported, not the later action.
#[test]
fn a_clash_of_two_named_actions_is_reported_at_the_later_line() {
    let (line, reason) = failure("[keys]\nmove = \"F12\"\n\ncopy = \"F12\"\n");
    assert_eq!(line, Some(4));
    assert!(reason.contains("move"), "{reason}");
    let (reversed_line, _) = failure("[keys]\ncopy = \"F12\"\nmove = \"F12\"\n");
    assert_eq!(reversed_line, Some(3));
}

#[test]
fn the_same_key_twice_for_one_action_is_refused() {
    let (line, reason) = failure("[keys]\nquit = [\"F10\",\n  \"f10\"]\n");
    assert_eq!(line, Some(3));
    assert!(reason.contains("listed twice"), "{reason}");
}

#[test]
fn an_unknown_action_is_reported_at_its_own_line() {
    let (line, reason) = failure("[keys]\ncopy = \"F12\"\n\nfoo = \"F7\"\n");
    assert_eq!(line, Some(4));
    assert_eq!(reason, "unknown action \"foo\"");
}

#[test]
fn a_bad_key_is_reported_at_its_own_line_also_inside_a_list() {
    let (line, reason) = failure("[keys]\nquit = [\n  \"F10\",\n  \"ctrl+xyz\",\n]\n");
    assert_eq!(line, Some(4));
    assert_eq!(reason, "unknown key \"xyz\" in \"ctrl+xyz\"");
    let (second_line, _) = failure("[keys]\ncopy = \"F12\"\nquit = \"hyper+q\"\n");
    assert_eq!(second_line, Some(3));
}

#[test]
fn keys_that_would_break_the_app_are_refused() {
    for bad in [
        "escape",
        "esc",
        "Ctrl+Esc",
        "a",
        "shift+a",
        "5",
        "shift+5",
        "shift+8",
        "shift+*",
        "",
        "  ",
        "ctrl+",
        "ctrl+ctrl+c",
        "ctrl+control+c",
        "ab",
    ] {
        let result = keymap_from(&format!("[keys]\ncopy = \"{bad}\"\n"));
        assert!(result.is_err(), "{bad:?} was accepted");
    }
    let (_, reason) = failure("[keys]\ncopy = \"escape\"\n");
    assert!(reason.starts_with("Escape is reserved"), "{reason}");
    let (_, letter_reason) = failure("[keys]\ncopy = \"a\"\n");
    assert!(letter_reason.contains("type-ahead"), "{letter_reason}");
}

#[test]
fn a_broken_keymap_drops_the_whole_file_including_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[open]\nview = [\"viewer\"]\n[keys]\ncopy = \"escape\"\n",
    )
    .unwrap();
    config::load(&path).unwrap_err();
}

#[test]
fn too_many_keys_for_one_action_are_refused_at_the_first_excess_one() {
    let keys: Vec<String> = (0..=MAX_KEYS_PER_ACTION)
        .filter_map(|index| char::from_u32(u32::from(b'a') + index as u32))
        .map(|letter| format!("alt+shift+{letter}"))
        .collect();
    let list = keys
        .iter()
        .map(|key| format!("  \"{key}\",\n"))
        .collect::<String>();
    let (line, reason) = failure(&format!("[keys]\ncopy = [\n{list}]\n"));
    assert_eq!(reason, "at most 16 keys per action");
    assert_eq!(line, Some(3 + MAX_KEYS_PER_ACTION));
}

#[test]
fn invisible_format_characters_are_not_keys() {
    for invisible in ["\u{202E}", "\u{200B}", "\u{FEFF}", "ctrl+\u{202E}"] {
        let (_, reason) = failure(&format!("[keys]\ncopy = \"{invisible}\"\n"));
        assert!(reason.contains("unknown key"), "{invisible:?}: {reason}");
    }
}

/// The dialog writes what it records; every default key has to survive
/// the trip through the file unchanged.
#[test]
fn every_default_key_is_written_as_text_that_reads_back_as_itself() {
    for (action, binding) in default_bindings() {
        let text = configurable_chord(action, &binding)
            .unwrap_or_else(|refusal| panic!("{}: {refusal:?}", action.name()));
        assert_eq!(parse_chord(&text), Ok(binding), "{}", action.name());
    }
    let primary_k = Binding::with_modifiers(Key::Character("k".into()), Modifiers::COMMAND);
    assert_eq!(
        configurable_chord(KeyAction::KeyBindings, &primary_k),
        Ok("primary+k".to_owned())
    );
}

#[test]
fn a_recorded_key_keeps_shift_only_where_the_lookup_does() {
    let bare = |key: Key| Binding::with_modifiers(key, Modifiers::default());
    let shift_ctrl = Modifiers::SHIFT | Modifiers::CTRL;
    assert_eq!(
        recorded_binding(&Key::Character("A".into()), shift_ctrl),
        Some(Binding::with_modifiers(
            Key::Character("a".into()),
            shift_ctrl
        ))
    );
    assert_eq!(
        recorded_binding(&Key::Character("*".into()), shift_ctrl),
        Some(Binding::with_modifiers(
            Key::Character("*".into()),
            Modifiers::CTRL
        ))
    );
    assert_eq!(
        recorded_binding(&named(Named::F8), Modifiers::SHIFT),
        Some(Binding::with_modifiers(named(Named::F8), Modifiers::SHIFT))
    );
    assert_eq!(
        recorded_binding(&named(Named::Shift), Modifiers::SHIFT),
        None
    );
    assert_eq!(
        configurable_chord(KeyAction::Copy, &bare(Key::Character("x".into()))),
        Err(KeyRefusal::NeedsModifier)
    );
    assert_eq!(
        configurable_chord(KeyAction::Copy, &bare(named(Named::F13))),
        Err(KeyRefusal::Unnamed)
    );
    assert_eq!(
        configurable_chord(KeyAction::Copy, &bare(named(Named::Escape))),
        Err(KeyRefusal::Escape)
    );
}

#[test]
fn two_panel_actions_cannot_share_a_key() {
    let (_, reason) = failure("[keys]\nmkdir = \"alt+x\"\ngo_up = \"alt+x\"\n");
    assert!(reason.contains("already bound"), "{reason}");
}

#[test]
fn an_action_that_works_in_both_areas_conflicts_with_a_search_toggle() {
    let (_, reason) = failure("[keys]\nview = \"alt+c\"\n");
    assert!(reason.contains("search_case"), "{reason}");
}

#[test]
fn the_trash_actions_are_offered_only_where_they_work() {
    let on_a_mac = cfg!(target_os = "macos");
    assert_eq!(KeyAction::OpenTrashInFinder.is_offered(), on_a_mac);
    assert!(KeyAction::Trash.is_offered());
    assert!(KeyAction::RestoreLastDeletion.is_offered());
}
