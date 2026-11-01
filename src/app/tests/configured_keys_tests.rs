use crate::app::*;
use keyboard::key::{Code, Physical};

fn config_with(content: &str) -> Config {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, content).unwrap();
    config::load(&path).unwrap()
}

fn state_for(content: &str) -> PromptKeyState {
    PromptKeyState {
        keymap: config_with(content).keymap,
        ..PromptKeyState::default()
    }
}

fn named(key: Named) -> Key {
    Key::Named(key)
}

fn character(text: &str) -> Key {
    Key::Character(text.into())
}

#[test]
fn a_rebound_action_routes_on_the_new_key_only() {
    let state = state_for("[keys]\ncopy = \"F12\"\n");
    assert_eq!(
        route_key(&state, named(Named::F12), Modifiers::default()),
        Some(Message::Transfer(TransferKind::Copy))
    );
    assert_eq!(
        route_key(&state, named(Named::F5), Modifiers::default()),
        None
    );
}

#[test]
fn the_bar_and_the_context_menu_show_the_configured_key() {
    let config = config_with("[keys]\ncommand_line = []\ncopy = \"F2\"\n");
    let mut app = App::with_fixed_panels().with_config(Ok(config));
    assert_eq!(app.function_keys[1], Some("Copy"));
    assert_eq!(app.function_keys[4], None);
    assert_eq!(
        app.keymap
            .shortcut_label(&ContextAction::Copy.message())
            .as_deref(),
        Some("F2")
    );

    drop(app.update(Message::SwitchLanguage));
    assert_eq!(app.lang, Language::German);
    assert!(app.function_keys[1].is_some());
    assert_eq!(app.function_keys[4], None);
}

#[test]
fn a_failed_config_leaves_the_built_in_keys() {
    let app = App::with_fixed_panels().with_config(Err(ConfigError::Unreadable {
        path: PathBuf::from("/cfg"),
        reason: "x".into(),
    }));
    assert_eq!(*app.keymap, *Keymap::built_in());
    assert_eq!(app.function_keys[4], Some("Copy"));
}

#[test]
fn the_configured_permanent_delete_key_confirms_its_dialog() {
    let state = PromptKeyState {
        delete_dialog: DeleteKeys::Permanent,
        ..state_for("[keys]\ndelete_permanently = \"ctrl+d\"\n")
    };
    assert_eq!(
        route_key(&state, character("d"), Modifiers::CTRL),
        Some(Message::DeleteConfirm)
    );
    assert_eq!(route_key(&state, named(Named::F8), Modifiers::SHIFT), None);
}

#[test]
fn a_configured_key_cannot_take_the_fixed_dialog_keys() {
    let delete = PromptKeyState {
        delete_dialog: DeleteKeys::Permanent,
        ..state_for("[keys]\ndelete_permanently = \"right\"\n")
    };
    assert_eq!(
        route_key(&delete, named(Named::ArrowRight), Modifiers::default()),
        Some(Message::DialogFocus(1))
    );
    assert_eq!(
        route_key(&delete, named(Named::Enter), Modifiers::default()),
        Some(Message::DialogActivate)
    );
    let conflict = PromptKeyState {
        conflict_open: true,
        ..state_for("[keys]\nabort_job = \"left\"\n")
    };
    assert_eq!(
        route_key(&conflict, named(Named::ArrowLeft), Modifiers::default()),
        Some(Message::DialogFocus(-1))
    );
}

#[test]
fn the_built_in_permanent_delete_key_still_confirms_its_dialog() {
    let state = PromptKeyState {
        delete_dialog: DeleteKeys::Permanent,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&state, named(Named::F8), Modifiers::SHIFT),
        Some(Message::DeleteConfirm)
    );
}

#[test]
fn the_configured_abort_key_cancels_a_conflict() {
    let state = PromptKeyState {
        conflict_open: true,
        ..state_for("[keys]\nabort_job = \"ctrl+k\"\n")
    };
    assert_eq!(
        route_key(&state, character("k"), Modifiers::CTRL),
        Some(Message::TransferConflict(ConflictChoice::Cancel))
    );
    assert_eq!(route_key(&state, character("c"), Modifiers::CTRL), None);
}

#[test]
fn quitting_from_the_path_field_needs_a_key_the_field_does_not_type() {
    let state = PromptKeyState {
        path_editing: true,
        ..state_for("[keys]\nquit = [\"*\", \"backspace\", \"F10\", \"primary+q\"]\ntag_all = []\ngo_up = []\n")
    };
    assert_eq!(
        route_key(&state, character("*"), Modifiers::default()),
        None
    );
    assert_eq!(
        route_key(&state, named(Named::Backspace), Modifiers::default()),
        None
    );
    assert_eq!(
        route_key(&state, named(Named::F10), Modifiers::default()),
        Some(Message::Quit)
    );
    assert_eq!(
        route_key(&state, character("q"), Modifiers::COMMAND),
        Some(Message::Quit)
    );
}

#[test]
fn a_failed_trash_names_the_configured_key_or_none() {
    let deleting = Deletion {
        sources: vec![PathBuf::from("/left/a.txt")],
        permanent: false,
        side: PanelSide::Left,
    };
    let mut app = App::with_fixed_panels()
        .with_config(Ok(config_with("[keys]\ndelete_permanently = \"ctrl+d\"\n")));
    app.ops.deleting = Some(deleting.clone());
    let shown = app.with_ops_ctx(|ops, ctx| ncrs_ops::delete_failure(ops, ctx, 0, "denied"));
    assert!(
        shown.contains("a.txt") && shown.contains("denied"),
        "{shown}"
    );
    assert!(
        shown.contains(
            &app.keymap
                .shortcut_label(&Message::Delete { permanent: true })
                .unwrap()
        ),
        "{shown}"
    );
    assert!(!shown.contains("Shift+F8"), "{shown}");

    let mut unbound =
        App::with_fixed_panels().with_config(Ok(config_with("[keys]\ndelete_permanently = []\n")));
    unbound.ops.deleting = Some(deleting);
    let shown_unbound =
        unbound.with_ops_ctx(|ops, ctx| ncrs_ops::delete_failure(ops, ctx, 0, "denied"));
    assert!(shown_unbound.contains("denied"), "{shown_unbound}");
    assert!(
        !shown_unbound.contains("{key}") && !shown_unbound.contains("permanently"),
        "{shown_unbound}"
    );
}

fn app_with_a_trash_that_lists() -> (tempfile::TempDir, App) {
    let folder = tempfile::tempdir().unwrap();
    let trash = Arc::new(fs::JournaledTrash::new(
        folder.path().join("journal.toml"),
        fs::FolderBackend(folder.path().to_path_buf()),
    ));
    (folder, App::with_fixed_panels().with_trash(trash))
}

fn moved(app: &mut App, count: usize) {
    let sources = (0..count)
        .map(|index| PathBuf::from(format!("/left/{index}.txt")))
        .collect();
    app.ops.deleting = Some(Deletion {
        sources,
        permanent: false,
        side: PanelSide::Left,
    });
    let generation = app.ops.generation;
    drop(
        app.with_ops(|ops, ctx| ncrs_ops::delete_row_done(ops, Ok(()), count - 1, generation, ctx)),
    );
}

#[test]
fn the_notice_after_a_delete_names_the_restore_key_or_leaves_it_out() {
    let (_folder, mut app) = app_with_a_trash_that_lists();
    moved(&mut app, 2);
    let key = app
        .keymap
        .shortcut_label(&KeyAction::RestoreLastDeletion.message())
        .unwrap();
    assert_eq!(
        app.notice_for_test(),
        Some(format!("Moved 2 to the trash. {key} puts it back.").as_str())
    );

    let (_other, app_without_key) = app_with_a_trash_that_lists();
    let mut unbound =
        app_without_key.with_config(Ok(config_with("[keys]\nrestore_last_deletion = []\n")));
    moved(&mut unbound, 1);
    assert_eq!(
        unbound.notice_for_test(),
        Some("Moved 1 to the trash."),
        "a missing key left a hole in the sentence"
    );
}

#[test]
fn a_trash_the_app_cannot_list_gets_no_restore_promise() {
    let mut app = App::with_fixed_panels().with_trash(Arc::new(NoListTrash));
    moved(&mut app, 1);
    assert_eq!(app.notice_for_test(), None);
}

struct NoListTrash;

impl fs::Trash for NoListTrash {
    fn trash(&self, _path: &Path) -> Result<(), String> {
        Ok(())
    }

    fn reach(&self) -> fs::TrashReach {
        fs::TrashReach::FinderOnly
    }

    fn list(&self) -> Result<Vec<fs::TrashedItem>, String> {
        Ok(Vec::new())
    }

    fn restore(&self, _item: &fs::TrashedItem) -> Result<(), fs::RestoreError> {
        Err(fs::RestoreError::Failed(String::new()))
    }
}

#[test]
fn the_notice_after_a_restore_counts_what_came_back_and_what_stayed() {
    let (_folder, mut app) = app_with_a_trash_that_lists();
    app.ops.restoring = Some(Restoration {
        items: Vec::new(),
        restored: 2,
        skipped: 1,
        rule: None,
    });
    drop(app.end_restore());
    assert_eq!(
        app.notice_for_test(),
        Some("Restored 2 from the trash. 1 left in the trash.")
    );
    assert!(app.ops.restoring.is_none());
}

#[test]
fn a_failed_restore_keeps_its_error_instead_of_a_notice() {
    let (_folder, mut app) = app_with_a_trash_that_lists();
    app.ops.restoring = Some(Restoration {
        items: vec![fs::TrashedItem {
            original: PathBuf::from("/left/a.txt"),
            deleted_at: 0,
            key: "a".into(),
        }],
        restored: 1,
        skipped: 0,
        rule: None,
    });
    let generation = app.ops.generation;
    drop(app.update(Message::Trash(TrashMessage::RestoreRowDone {
        result: Err(fs::RestoreError::Failed("denied".to_string())),
        index: 0,
        generation,
    })));
    assert!(app
        .status_error()
        .is_some_and(|text| text.contains("a.txt")));
    assert_eq!(app.notice_for_test(), None);
}

#[test]
fn a_restore_the_sandbox_cannot_reach_says_so_and_offers_the_finder() {
    let (_folder, mut app) = app_with_a_trash_that_lists();
    app.ops.restoring = Some(Restoration {
        items: vec![fs::TrashedItem {
            original: PathBuf::from("/left/a.txt"),
            deleted_at: 0,
            key: "a".into(),
        }],
        restored: 0,
        skipped: 0,
        rule: None,
    });
    let generation = app.ops.generation;
    drop(app.update(Message::Trash(TrashMessage::RestoreRowDone {
        result: Err(fs::RestoreError::NotGranted),
        index: 0,
        generation,
    })));

    assert_eq!(
        app.status_error(),
        Some(
            "a.txt cannot be restored here: ncrs has no access to its folder. \
             Click to open the trash in the Finder"
        )
    );
    assert_eq!(
        app.job_error_press,
        Some(Message::Trash(TrashMessage::OpenInFinder))
    );

    app.set_status_error("something else");
    assert_eq!(app.job_error_press, None);
}

use fs::Trash as _;

struct RestoreScene {
    root: tempfile::TempDir,
    trash: fs::JournaledTrash<fs::FolderBackend>,
    item: fs::TrashedItem,
}

fn trashed_file_scene() -> RestoreScene {
    let root = tempfile::tempdir().unwrap();
    let trash_folder = root.path().join("trash");
    let origin = root.path().join("origin");
    std::fs::create_dir_all(&trash_folder).unwrap();
    std::fs::create_dir_all(&origin).unwrap();
    let original = origin.join("a.txt");
    std::fs::write(&original, "mine").unwrap();
    let trash = fs::JournaledTrash::new(
        root.path().join("journal.toml"),
        fs::FolderBackend(trash_folder),
    );
    trash.trash(&original).unwrap();
    let item = trash.list().unwrap().remove(0);
    RestoreScene { root, trash, item }
}

fn sandbox_reaching(reached: bool) -> fs::SandboxAccess {
    fs::SandboxAccess {
        active: true,
        grants_dir: Some(PathBuf::from("grants-of-this-test")),
        home_readable: || true,
        grant: |_, _| Ok(()),
        covers: if reached { |_, _| true } else { |_, _| false },
    }
}

#[test]
fn a_folder_out_of_reach_is_refused_before_anything_in_the_way_is_trashed() {
    let scene = trashed_file_scene();
    std::fs::write(&scene.item.original, "in the way").unwrap();

    let result = restore_one(&scene.trash, &sandbox_reaching(false), &scene.item, true);

    assert_eq!(result, Err(fs::RestoreError::NotGranted));
    assert_eq!(
        std::fs::read_to_string(&scene.item.original).unwrap(),
        "in the way"
    );
    assert!(
        Path::new(&scene.item.key).exists(),
        "the item left the trash"
    );
    assert_eq!(scene.trash.list().unwrap().len(), 1);
}

#[test]
fn a_folder_in_reach_gets_its_entry_back() {
    let scene = trashed_file_scene();

    let result = restore_one(&scene.trash, &sandbox_reaching(true), &scene.item, false);

    assert_eq!(result, Ok(()));
    assert_eq!(
        std::fs::read_to_string(&scene.item.original).unwrap(),
        "mine"
    );
    assert!(scene.root.path().join("origin").is_dir());
}

#[test]
fn outside_the_sandbox_every_folder_is_in_reach() {
    let scene = trashed_file_scene();

    let result = restore_one(
        &scene.trash,
        &fs::SandboxAccess::inactive(),
        &scene.item,
        false,
    );

    assert_eq!(result, Ok(()));
    assert!(scene.item.original.exists());
}

fn on_the_trash_view() -> PromptKeyState {
    PromptKeyState {
        trash: true,
        ..PromptKeyState::default()
    }
}

#[test]
fn the_trash_view_restores_on_enter_and_closes_on_escape() {
    let state = on_the_trash_view();
    let none = Modifiers::default();
    assert_eq!(
        route_key(&state, Key::Named(Named::Enter), none),
        Some(Message::Trash(TrashMessage::Restore))
    );
    assert_eq!(
        route_key(&state, Key::Named(Named::Escape), none),
        Some(Message::Trash(TrashMessage::Close))
    );
    assert_eq!(
        route_key(&state, Key::Named(Named::ArrowDown), none),
        Some(Message::Trash(TrashMessage::Move(1)))
    );
    assert_eq!(
        route_key(&state, Key::Named(Named::End), none),
        Some(Message::Trash(TrashMessage::Last))
    );
    assert_eq!(
        route_key(&state, Key::Named(Named::Insert), none),
        Some(Message::Trash(TrashMessage::ToggleTag))
    );
    assert_eq!(
        route_key(&state, Key::Named(Named::ArrowDown), Modifiers::SHIFT),
        Some(Message::Trash(TrashMessage::TagMove(1)))
    );
    let running = PromptKeyState {
        job_running: true,
        ..on_the_trash_view()
    };
    assert_eq!(
        route_key(&running, Key::Named(Named::Escape), none),
        Some(Message::AbortJob),
        "a restore in flight cannot be stopped"
    );
}

#[test]
fn the_trash_view_ignores_the_file_keys_and_typing_but_lets_a_few_through() {
    let state = on_the_trash_view();
    let none = Modifiers::default();
    for named in [Named::F5, Named::F6, Named::F7, Named::F8, Named::Backspace] {
        assert_eq!(
            route_key(&state, Key::Named(named), none),
            None,
            "{named:?} reached the panel behind the trash"
        );
    }
    assert_eq!(route_key(&state, character("a"), none), None);
    assert_eq!(
        route_key(&state, Key::Named(Named::Tab), none),
        Some(Message::Panel(PanelMessage::SwitchPanel))
    );
    assert_eq!(
        route_key(&state, character("z"), Modifiers::COMMAND),
        Some(Message::Trash(TrashMessage::RestoreLast))
    );
    assert_eq!(
        route_key(&state, character("q"), Modifiers::COMMAND),
        Some(Message::Quit)
    );
}

fn binding_key_of(typed: &str, physical: Code, modifiers: Modifiers) -> Key {
    binding_key(character(typed), Physical::Code(physical), modifiers)
}

#[test]
fn a_control_character_with_ctrl_becomes_its_letter() {
    assert_eq!(
        binding_key_of("\u{18}", Code::KeyX, Modifiers::CTRL),
        character("x")
    );
    assert_eq!(
        binding_key_of("\u{18}", Code::KeyQ, Modifiers::CTRL),
        character("x"),
        "the layout wins over the physical position"
    );
}

fn press_physical(typed: &str, physical: Code, modifiers: Modifiers) -> Option<Message> {
    route_press(
        &PromptKeyState::default(),
        character(typed),
        Physical::Code(physical),
        modifiers,
        false,
    )
}

#[test]
fn a_typed_star_with_ctrl_still_clears_the_tags() {
    assert_eq!(
        press_physical("*", Code::Digit8, Modifiers::CTRL | Modifiers::SHIFT),
        Some(Message::Panel(PanelMessage::ClearTags))
    );
}

#[test]
fn option_c_cedilla_still_copies_the_path() {
    assert_eq!(
        press_physical("ç", Code::KeyC, Modifiers::ALT | Modifiers::COMMAND),
        Some(Message::CopyToClipboard(ClipboardKind::Path))
    );
}

#[test]
fn the_sort_chord_routes_by_its_digit_or_function_key() {
    let binding = keymap::sort_binding(crate::fs::SortColumn::Name);
    let routed = match &binding.key {
        Key::Character(digit) => press_physical(digit, Code::Digit1, binding.modifiers),
        other => route_key(&PromptKeyState::default(), other.clone(), binding.modifiers),
    };
    assert_eq!(
        routed,
        Some(Message::Panel(PanelMessage::SortActive(
            crate::fs::SortColumn::Name
        )))
    );
}

#[test]
fn a_name_with_a_placeholder_in_it_is_shown_as_written() {
    let mut app = App::with_fixed_panels();
    app.ops.deleting = Some(Deletion {
        sources: vec![PathBuf::from("/left/{key}.txt")],
        permanent: false,
        side: PanelSide::Left,
    });
    let shown = app.with_ops_ctx(|ops, ctx| ncrs_ops::delete_failure(ops, ctx, 0, "{name}"));
    assert!(shown.contains("{key}.txt"), "{shown}");
    assert!(shown.contains("{name}"), "{shown}");

    let err = ConfigError::Invalid {
        path: PathBuf::from("/cfg/{reason}/config.toml"),
        line: Some(2),
        reason: "bad {line}".into(),
    };
    let config_shown = app.config_error_text(&ConfigProblem::Invalid(err));
    assert!(
        config_shown.contains("/cfg/{reason}/config.toml"),
        "{config_shown}"
    );
    assert!(config_shown.contains("bad {line}"), "{config_shown}");
}

#[test]
fn quit_on_alt_does_not_end_the_program_from_the_path_field() {
    let state = PromptKeyState {
        path_editing: true,
        ..state_for("[keys]\nquit = [\"alt+q\", \"primary+q\"]\n")
    };
    assert_eq!(route_key(&state, character("q"), Modifiers::ALT), None);
    assert_eq!(
        route_key(&state, character("q"), Modifiers::COMMAND),
        Some(Message::Quit)
    );
}

#[test]
fn without_ctrl_alt_or_super_a_typed_character_is_left_alone() {
    assert_eq!(
        binding_key_of("&", Code::Digit1, Modifiers::default()),
        character("&")
    );
    assert_eq!(
        binding_key_of("!", Code::Digit1, Modifiers::SHIFT),
        character("!")
    );
    assert_eq!(
        binding_key_of("¡", Code::Digit1, Modifiers::ALT),
        character("1")
    );
    assert_eq!(
        binding_key_of("*", Code::Digit8, Modifiers::CTRL | Modifiers::SHIFT),
        character("*")
    );
}
