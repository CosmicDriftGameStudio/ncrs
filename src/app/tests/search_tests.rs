use crate::app::*;
use crate::fs::search::{LineMatch, SearchEvent};

fn route(search_open: bool, key: Key, modifiers: Modifiers) -> Option<Message> {
    let state = PromptKeyState {
        search: search_open,
        ..PromptKeyState::default()
    };
    route_key(&state, key, modifiers)
}

fn named(named: Named) -> Key {
    Key::Named(named)
}

fn character(typed: &str) -> Key {
    Key::Character(typed.into())
}

fn found(path: &str, lines: &[u64]) -> SearchEvent {
    SearchEvent::File {
        path: PathBuf::from(path),
        lines: lines
            .iter()
            .map(|&number| LineMatch {
                number,
                text: format!("line {number}"),
                ranges: std::iter::once(0..4).collect(),
            })
            .collect(),
        size: 0,
        modified: None,
    }
}

#[test]
fn the_status_bar_names_the_cursor_file_only_while_the_search_has_the_keys() {
    let mut app = App::with_fixed_search();
    let shown = app.search_status().unwrap();
    assert!(!shown.is_empty() && !shown.starts_with('/'), "{shown}");

    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    assert_eq!(app.search_status(), None);
}

fn app_with_results(files: &[(&str, &[u64])]) -> App {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    let generation = app.search_for_test().unwrap().generation;
    let events = files
        .iter()
        .map(|(path, lines)| found(path, lines))
        .collect();
    drop(app.update(Message::Search(SearchMessage::Events {
        generation,
        events,
    })));
    app
}

#[derive(Default)]
struct RecordingLauncher(Mutex<Vec<fs::OpenCommand>>);

impl fs::Launcher for RecordingLauncher {
    fn launch(&self, command: &fs::OpenCommand) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|poisoned| poisoned.to_string())?
            .push(command.clone());
        Ok(())
    }
}

#[test]
fn the_cursor_keys_move_in_the_results_while_the_search_is_up() {
    let search = |message| Some(Message::Search(message));
    let none = Modifiers::default();
    assert_eq!(
        route(true, named(Named::ArrowDown), none),
        search(SearchMessage::Move(1))
    );
    assert_eq!(
        route(true, named(Named::ArrowUp), none),
        search(SearchMessage::Move(-1))
    );
    assert_eq!(
        route(true, named(Named::PageDown), none),
        search(SearchMessage::Page(1))
    );
    assert_eq!(
        route(true, named(Named::Home), none),
        search(SearchMessage::First)
    );
    assert_eq!(
        route(true, named(Named::End), none),
        search(SearchMessage::Last)
    );
    assert_eq!(
        route(true, named(Named::Enter), none),
        search(SearchMessage::Activate)
    );
    assert_eq!(
        route(false, named(Named::ArrowDown), none),
        Some(Message::Panel(PanelMessage::MoveSelection(1))),
        "without the search the arrows are the panel's"
    );
}

#[test]
fn a_printable_key_without_a_focused_field_goes_to_the_query() {
    assert_eq!(
        route(true, character("a"), Modifiers::default()),
        Some(Message::Search(SearchMessage::AppendToQuery(
            "a".to_string()
        )))
    );
    assert_eq!(
        route(true, character("A"), Modifiers::SHIFT),
        Some(Message::Search(SearchMessage::AppendToQuery(
            "A".to_string()
        )))
    );
}

#[test]
fn the_keys_that_would_act_on_the_hidden_panel_do_nothing() {
    let none = Modifiers::default();
    for key in [
        named(Named::Backspace),
        named(Named::Tab),
        named(Named::Escape),
        named(Named::F7),
    ] {
        assert_eq!(route(true, key.clone(), none), None, "{key:?}");
    }
}

#[test]
fn the_file_actions_and_quit_still_work_over_the_search() {
    let none = Modifiers::default();
    assert_eq!(
        route(true, named(Named::F5), none),
        Some(Message::Transfer(TransferKind::Copy))
    );
    assert_eq!(
        route(true, named(Named::F3), none),
        Some(Message::OpenExternal(fs::OpenKind::View))
    );
    assert_eq!(
        route(true, named(Named::F4), none),
        Some(Message::OpenExternal(fs::OpenKind::Edit))
    );
    assert_eq!(route(true, named(Named::F10), none), Some(Message::Quit));
}

#[test]
fn alt_f7_and_the_find_chord_open_the_search_also_over_it() {
    let find = Modifiers::COMMAND | Modifiers::SHIFT;
    for open in [false, true] {
        assert_eq!(
            route(open, named(Named::F7), Modifiers::ALT),
            Some(Message::OpenSearch)
        );
        assert_eq!(route(open, character("F"), find), Some(Message::OpenSearch));
        assert_eq!(route(open, character("f"), find), Some(Message::OpenSearch));
    }
}

#[test]
fn opening_the_search_covers_the_active_panel_and_gives_it_the_keys() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    let view = app.search_for_test().unwrap();
    assert_eq!(view.root, app.panel(PanelSide::Left).path);
    assert!(app.search_is_open_for_test());
    assert!(app.prompt_key_state().search);
}

#[test]
fn closing_keeps_the_fields_and_results_and_stops_taking_batches() {
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
    drop(app.update(Message::Search(SearchMessage::QueryChanged(
        "line".to_string(),
    ))));
    let generation = app.search_for_test().unwrap().generation;
    drop(app.update(Message::Search(SearchMessage::Close)));

    assert!(!app.search_is_open_for_test());
    assert!(!app.prompt_key_state().search);
    let view = app.search_for_test().unwrap();
    assert_eq!(view.query, "line");
    assert_eq!(view.results.len(), 1);

    drop(app.update(Message::Search(SearchMessage::Events {
        generation,
        events: vec![found("/home/test/links/late.txt", &[1])],
    })));
    assert_eq!(app.search_for_test().unwrap().results.len(), 1);
}

#[test]
fn a_batch_of_an_older_search_is_dropped() {
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
    let current = app.search_for_test().unwrap().generation;
    drop(app.update(Message::Search(SearchMessage::Events {
        generation: current + 7,
        events: vec![found("/home/test/links/b.txt", &[1])],
    })));
    assert_eq!(app.search_for_test().unwrap().results.len(), 1);
}

#[test]
fn the_visible_rows_follow_the_row_height_of_the_kind_of_search() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    let rows_of =
        |current: &App, height| layout::search_visible_rows(current.window_size, false, height);
    assert_eq!(
        app.search_rows_for_test(),
        rows_of(&app, layout::HIT_LIST_ROW_HEIGHT),
        "file names only: two lines"
    );

    drop(app.update(Message::Search(SearchMessage::QueryChanged(
        "x".to_string(),
    ))));
    let generation = app.search_for_test().unwrap().generation;
    drop(app.update(Message::Search(SearchMessage::Debounced(generation))));

    assert!(app.search_for_test().unwrap().third_line);
    assert_eq!(
        app.search_rows_for_test(),
        rows_of(&app, layout::SEARCH_CONTENT_ROW_HEIGHT),
        "matching lines: three lines"
    );
}

#[test]
fn a_pause_that_a_later_keystroke_overtook_starts_nothing() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    drop(app.update(Message::Search(SearchMessage::QueryChanged(
        "h".to_string(),
    ))));
    let first = app.search_for_test().unwrap().generation;
    drop(app.update(Message::Search(SearchMessage::QueryChanged(
        "he".to_string(),
    ))));
    assert_ne!(app.search_for_test().unwrap().generation, first);

    drop(app.update(Message::Search(SearchMessage::Debounced(first))));
    assert!(
        !app.search_for_test().unwrap().running,
        "the old timer started a search"
    );
}

#[test]
fn the_right_click_menu_does_not_open_over_the_search() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    drop(app.update(Message::ContextMenuAt {
        side: PanelSide::Left,
        index: 1,
    }));
    assert!(app.context_menu_for_test().is_none());
}

fn search_message(message: SearchMessage) -> Message {
    Message::Search(message)
}

#[test]
fn file_actions_take_the_tagged_files_else_the_cursors_file() {
    let mut app = app_with_results(&[
        ("/home/test/links/b.txt", &[2]),
        ("/home/test/links/a.txt", &[1, 3]),
        ("/home/test/links/c.txt", &[]),
    ]);
    let a = PathBuf::from("/home/test/links/a.txt");
    let c = PathBuf::from("/home/test/links/c.txt");
    assert_eq!(
        app.action_sources(),
        std::slice::from_ref(&a),
        "not every file found"
    );

    // Without tags the file under the cursor is the source.
    let b = PathBuf::from("/home/test/links/b.txt");
    drop(app.update(search_message(SearchMessage::Move(1))));
    assert_eq!(app.action_sources(), [b]);

    // Tagged files win over the cursor, in display order.
    drop(app.update(search_message(SearchMessage::Last)));
    drop(app.update(search_message(SearchMessage::ToggleTag)));
    drop(app.update(search_message(SearchMessage::First)));
    drop(app.update(search_message(SearchMessage::ToggleTag)));
    assert_eq!(app.action_sources(), [a, c]);

    drop(app.update(Message::Search(SearchMessage::Close)));
    assert_ne!(app.action_sources().len(), 3);
}

#[test]
fn without_a_cursor_and_tags_f8_opens_no_dialog() {
    let mut app = app_with_results(&[]);
    assert!(app.action_sources().is_empty());
    drop(app.update(Message::Delete { permanent: false }));
    assert!(app.ops.delete_dialog.is_none());
    drop(app.update(Message::Delete { permanent: true }));
    assert!(app.ops.delete_dialog.is_none());
}

#[test]
fn f8_over_tagged_results_asks_about_exactly_those() {
    let mut app = app_with_results(&[
        ("/home/test/links/a.txt", &[]),
        ("/home/test/links/b.txt", &[]),
        ("/home/test/links/c.txt", &[]),
    ]);
    drop(app.update(search_message(SearchMessage::ToggleTag)));
    drop(app.update(search_message(SearchMessage::ToggleTag)));
    drop(app.update(Message::Delete { permanent: false }));
    assert_eq!(app.ops.delete_dialog.as_ref().unwrap().sources.len(), 2);
}

#[test]
fn tagging_in_the_results_moves_on_and_a_new_search_forgets_the_tags() {
    let mut app = app_with_results(&[
        ("/home/test/links/a.txt", &[]),
        ("/home/test/links/b.txt", &[]),
    ]);
    drop(app.update(search_message(SearchMessage::ToggleTag)));
    let view = app.search_for_test().unwrap();
    assert!(view.is_tagged(0));
    assert_eq!(view.cursor, Some(1));

    drop(app.update(search_message(SearchMessage::TagAll)));
    assert_eq!(app.search_for_test().unwrap().tagged.len(), 2);
    drop(app.update(search_message(SearchMessage::ClearTags)));
    assert!(app.search_for_test().unwrap().tagged.is_empty());

    drop(app.update(search_message(SearchMessage::TagAll)));
    drop(app.update(search_message(SearchMessage::QueryChanged("x".to_string()))));
    let generation = app.search_for_test().unwrap().generation;
    drop(app.update(search_message(SearchMessage::Debounced(generation))));
    assert!(app.search_for_test().unwrap().tagged.is_empty());
}

#[test]
fn the_tag_keys_tag_in_the_results_instead_of_typing() {
    let none = Modifiers::default();
    let search = |message| Some(search_message(message));
    assert_eq!(
        route(true, named(Named::Space), none),
        search(SearchMessage::ToggleTag)
    );
    assert_eq!(
        route(true, named(Named::Insert), none),
        search(SearchMessage::ToggleTag)
    );
    assert_eq!(
        route(true, named(Named::ArrowDown), Modifiers::SHIFT),
        search(SearchMessage::TagMove(1))
    );
    assert_eq!(
        route(true, named(Named::ArrowUp), Modifiers::SHIFT),
        search(SearchMessage::TagMove(-1))
    );
    assert_eq!(
        route(true, character("*"), Modifiers::SHIFT),
        search(SearchMessage::TagAll)
    );
    assert_eq!(
        route(true, character("*"), Modifiers::CTRL | Modifiers::SHIFT),
        search(SearchMessage::ClearTags)
    );
    assert_eq!(
        route(true, named(Named::ArrowDown), none),
        search(SearchMessage::Move(1)),
        "plain arrows still move"
    );
}

fn press(
    search: bool,
    typed: &str,
    physical: keyboard::key::Code,
    modifiers: Modifiers,
) -> Option<Message> {
    let state = PromptKeyState {
        search,
        ..PromptKeyState::default()
    };
    route_press(
        &state,
        character(typed),
        keyboard::key::Physical::Code(physical),
        modifiers,
        false,
    )
}

#[test]
fn alt_c_w_r_toggle_the_options_while_the_results_have_the_keys() {
    use keyboard::key::Code;
    // "ç" is what Option+C types on a Mac.
    assert_eq!(
        press(true, "ç", Code::KeyC, Modifiers::ALT),
        Some(search_message(SearchMessage::ToggleCase))
    );
    assert_eq!(
        press(true, "w", Code::KeyW, Modifiers::ALT),
        Some(search_message(SearchMessage::ToggleWholeWord))
    );
    assert_eq!(
        press(true, "®", Code::KeyR, Modifiers::ALT),
        Some(search_message(SearchMessage::ToggleRegex))
    );
}

#[test]
fn ctrl_alt_c_is_altgr_on_windows_and_no_toggle() {
    use keyboard::key::Code;
    let altgr = Modifiers::CTRL | Modifiers::ALT;
    for typed in ["c", "ć"] {
        assert_ne!(
            press(true, typed, Code::KeyC, altgr),
            Some(search_message(SearchMessage::ToggleCase)),
            "{typed}"
        );
    }
}

#[test]
fn alt_c_w_r_are_not_swallowed_in_panel_mode() {
    use keyboard::key::Code;
    for (typed, code) in [("ç", Code::KeyC), ("w", Code::KeyW), ("®", Code::KeyR)] {
        assert_eq!(press(false, typed, code, Modifiers::ALT), None, "{typed}");
    }
}

#[test]
fn a_panel_action_on_alt_c_and_the_case_toggle_each_fire_in_their_own_area() {
    use keyboard::key::{Code, Physical};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[keys]\nmkdir = \"alt+c\"\n").unwrap();
    let keymap = crate::config::load(&path).unwrap().keymap;
    let route = |search, typed| {
        let state = PromptKeyState {
            keymap: Arc::clone(&keymap),
            search,
            ..PromptKeyState::default()
        };
        route_press(
            &state,
            character(typed),
            Physical::Code(Code::KeyC),
            Modifiers::ALT,
            false,
        )
    };
    // Option+C types `ç` on a Mac; the physical key decides.
    for typed in ["c", "ç"] {
        assert_eq!(route(false, typed), Some(Message::CreateDirPrompt));
        assert_eq!(
            route(true, typed),
            Some(search_message(SearchMessage::ToggleCase))
        );
    }
    assert_eq!(
        search_toggle_for_press(
            &keymap,
            character("c"),
            Physical::Code(Code::KeyC),
            Modifiers::ALT
        ),
        Some(SearchMessage::ToggleCase)
    );
}

#[test]
fn the_toggle_lookup_of_a_field_is_the_routings_and_leaves_option_characters_alone() {
    use keyboard::key::{Code, Physical};
    let keymap = Keymap::built_in();
    assert_eq!(
        search_toggle_for_press(
            &keymap,
            character("ç"),
            Physical::Code(Code::KeyC),
            Modifiers::ALT
        ),
        Some(SearchMessage::ToggleCase)
    );
    // Option+5 types "[" on a German Mac, and a regex needs it.
    assert_eq!(
        search_toggle_for_press(
            &keymap,
            character("["),
            Physical::Code(Code::Digit5),
            Modifiers::ALT
        ),
        None
    );
    assert_eq!(
        search_toggle_for_press(
            &keymap,
            character("c"),
            Physical::Code(Code::KeyC),
            Modifiers::default()
        ),
        None
    );
}

#[test]
fn the_function_key_bar_of_the_search_drops_what_does_nothing_there() {
    let app = App::with_fixed_panels();
    assert!(app.function_keys[6].is_some());
    for slot in [2, 3, 4, 5, 7, 8, 9] {
        assert!(app.search_function_keys[slot].is_some(), "F{}", slot + 1);
    }
    assert_eq!(app.search_function_keys[6], None, "F7 makes no directory");
}

#[test]
fn after_a_switch_to_the_other_panel_the_keys_and_actions_are_the_panels() {
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));

    assert!(!app.prompt_key_state().search);
    assert_eq!(
        route_key(
            &app.prompt_key_state(),
            named(Named::ArrowDown),
            Modifiers::default()
        ),
        Some(Message::Panel(PanelMessage::MoveSelection(1)))
    );
    let other_cursor = app
        .panel(PanelSide::Right)
        .selected_entry()
        .unwrap()
        .path
        .clone();
    assert!(
        app.panel(PanelSide::Right)
            .selected_entry()
            .unwrap()
            .is_parent
    );
    app.panel_mut_for_test(PanelSide::Right).selected = 1;
    let expected = app
        .panel(PanelSide::Right)
        .selected_entry()
        .unwrap()
        .path
        .clone();
    assert_ne!(other_cursor, expected);
    assert_eq!(app.action_sources(), [expected]);

    drop(app.update(Message::ContextMenuAt {
        side: PanelSide::Right,
        index: 1,
    }));
    assert!(app.context_menu_for_test().is_some());
    assert!(app.search_is_open_for_test(), "the search stays up");
}

#[test]
fn opening_the_search_again_brings_the_keys_back_without_changing_the_field() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    drop(app.update(Message::OpenSearch));
    assert_eq!(app.search_for_test().unwrap().focused, SearchField::Include);
    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    assert!(!app.prompt_key_state().search);

    drop(app.update(Message::OpenSearch));

    assert!(app.prompt_key_state().search);
    assert_eq!(app.search_for_test().unwrap().focused, SearchField::Include);
}

#[test]
fn using_the_search_makes_its_panel_active_again() {
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    drop(app.update(Message::Search(SearchMessage::RowClicked(0))));
    assert!(app.prompt_key_state().search);

    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    drop(app.update(Message::Search(SearchMessage::ToggleCase)));
    assert!(app.prompt_key_state().search);

    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    let generation = app.search_for_test().unwrap().generation;
    drop(app.update(Message::Search(SearchMessage::Debounced(generation))));
    assert!(
        !app.prompt_key_state().search,
        "a timer is no use of the view"
    );
}

#[test]
fn pressing_the_search_key_again_moves_to_the_next_field() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));
    assert_eq!(app.search_for_test().unwrap().focused, SearchField::Query);
    drop(app.update(Message::OpenSearch));
    assert_eq!(app.search_for_test().unwrap().focused, SearchField::Include);
    drop(app.update(Message::OpenSearch));
    drop(app.update(Message::OpenSearch));
    assert_eq!(app.search_for_test().unwrap().focused, SearchField::Query);
}

#[test]
fn reopening_searches_again_with_the_same_fields() {
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
    drop(app.update(Message::Search(SearchMessage::QueryChanged(
        "line".to_string(),
    ))));
    drop(app.update(Message::Search(SearchMessage::Close)));
    drop(app.update(Message::OpenSearch));
    let view = app.search_for_test().unwrap();
    assert_eq!(view.query, "line");
    assert!(view.running, "the old results are not trusted");
    assert!(view.results.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn opening_the_privacy_settings_launches_the_full_disk_access_page() {
    let launcher = Arc::new(RecordingLauncher::default());
    let mut app =
        App::with_fixed_panels().with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);

    drop(app.update(Message::OpenPrivacySettings));

    let calls = launcher.0.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "/usr/bin/open");
    assert_eq!(
        calls[0].args,
        ["x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"]
    );
}

#[test]
fn f4_edits_the_cursors_file_at_its_first_match() {
    let launcher = Arc::new(RecordingLauncher::default());
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[7, 9])])
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    app.open_programs = config::OpenPrograms {
        edit: Some(config::ProgramLine::new("editor", &["+{line}"])),
        ..config::OpenPrograms::default()
    };

    drop(app.update(Message::OpenExternal(fs::OpenKind::Edit)));

    let calls = launcher.0.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "editor");
    assert_eq!(calls[0].args[0], "+7");
    assert_eq!(calls[0].args.last().unwrap(), "/home/test/links/a.txt");
}

#[test]
fn enter_shows_the_file_in_the_other_panel_and_opens_no_editor() {
    let launcher = Arc::new(RecordingLauncher::default());
    let mut app = app_with_results(&[("/home/test/links/a.txt", &[7])])
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    let other = app.search.side.other();
    let before = app.panel(other).request_id;

    drop(app.update(Message::Search(SearchMessage::Activate)));

    assert!(launcher.0.lock().unwrap().is_empty());
    assert_ne!(app.panel(other).request_id, before);
    assert!(app.panel(other).loading);
}

#[test]
fn a_click_selects_the_file_and_the_star_tags_it() {
    let mut app = app_with_results(&[
        ("/home/test/links/a.txt", &[1, 2]),
        ("/home/test/links/b.txt", &[3]),
    ]);
    drop(app.update(Message::Search(SearchMessage::RowClicked(1))));
    assert_eq!(app.search_for_test().unwrap().cursor, Some(1));

    drop(app.update(Message::Search(SearchMessage::ToggleTagAt(0))));

    let view = app.search_for_test().unwrap();
    assert_eq!(view.cursor, Some(0));
    assert!(view.is_tagged(0) && !view.is_tagged(1));
}

fn menu_actions(app: &App) -> Vec<ContextAction> {
    app.context_menu_for_test()
        .unwrap()
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            crate::context_menu::MenuEntry::Item { action, .. } => Some(*action),
            _ => None,
        })
        .collect()
}

fn app_with_two_results() -> App {
    app_with_results(&[
        ("/home/test/links/a.txt", &[1]),
        ("/home/test/links/sub/b.txt", &[3]),
    ])
}

#[test]
fn a_right_click_opens_the_hit_list_menu_without_remove_from_list() {
    let mut app = app_with_two_results();
    app.set_pointer_for_test(Point::new(200.0, 150.0));

    drop(app.update(Message::Search(SearchMessage::ContextAt(1))));

    assert_eq!(app.search_for_test().unwrap().cursor, Some(1));
    let actions = menu_actions(&app);
    for expected in [
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::ShowOnOtherSide,
        ContextAction::CopyPath,
        ContextAction::CopyName,
        ContextAction::Trash,
        ContextAction::DeletePermanently,
    ] {
        assert!(actions.contains(&expected), "{expected:?} is missing");
    }
    assert!(!actions.contains(&ContextAction::RemoveFromList));
    assert!(!actions.contains(&ContextAction::NewFolder));
}

#[test]
fn a_right_click_outside_the_tags_drops_them_and_on_a_tagged_file_keeps_them() {
    let mut app = app_with_two_results();
    drop(app.update(Message::Search(SearchMessage::ToggleTagAt(0))));

    drop(app.update(Message::Search(SearchMessage::ContextAt(0))));
    assert!(app.search_for_test().unwrap().is_tagged(0));

    drop(app.update(Message::Search(SearchMessage::ContextAt(1))));
    let view = app.search_for_test().unwrap();
    assert!(!view.is_tagged(0) && !view.is_tagged(1));
    assert_eq!(view.cursor, Some(1));
}

#[test]
fn open_in_the_menu_shows_the_file_in_the_other_panel() {
    let mut app = app_with_two_results();
    let other = app.search.side.other();
    let before = app.panel(other).request_id;
    drop(app.update(Message::Search(SearchMessage::ContextAt(0))));
    assert_eq!(
        app.context_menu_selected_action(),
        Some(ContextAction::Open)
    );

    drop(app.update(Message::ContextMenuActivate));

    assert!(!app.context_menu_is_open());
    assert_ne!(app.panel(other).request_id, before);
}

#[test]
fn show_on_the_other_side_loads_the_results_folder_and_keeps_the_search() {
    let mut app = app_with_two_results();
    let other = app.search.side.other();
    drop(app.update(Message::Search(SearchMessage::ContextAt(1))));

    drop(app.update(Message::HitList(HitListMessage::ShowOnOtherSide)));

    assert!(app.panel(other).loading);
    assert_eq!(app.panels.active, other);
    assert!(app.search_is_open_for_test());
}

#[test]
fn trash_in_the_menu_deletes_the_result_under_the_pointer_not_the_panel_row() {
    let mut app = app_with_two_results();
    drop(app.update(Message::Search(SearchMessage::ContextAt(1))));
    let trash = app
        .context_menu_for_test()
        .unwrap()
        .entries()
        .iter()
        .position(|entry| {
            matches!(
                entry,
                crate::context_menu::MenuEntry::Item {
                    action: ContextAction::Trash,
                    ..
                }
            )
        })
        .unwrap();

    drop(app.update(Message::ContextMenuClick(trash)));

    let sources = &app.ops.delete_dialog.as_ref().unwrap().sources;
    let names: Vec<_> = sources.iter().filter_map(|path| path.file_name()).collect();
    assert_eq!(names, [std::ffi::OsStr::new("b.txt")]);
}
