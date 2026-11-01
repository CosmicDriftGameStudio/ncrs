use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn chord() -> Modifiers {
    Modifiers::COMMAND | Modifiers::SHIFT
}

fn palette_message(message: CommandPaletteMessage) -> Message {
    Message::CommandPalette(message)
}

fn open_palette(app: &mut App) {
    drop(app.update(palette_message(CommandPaletteMessage::Toggle)));
}

fn type_into_palette(app: &mut App, text: &str) {
    drop(
        app.update(palette_message(CommandPaletteMessage::QueryChanged(
            text.to_owned(),
        ))),
    );
}

fn route(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, modifiers)
}

fn app_with_files() -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.right.path = PathBuf::from("/right");
    app.panels.left.entries = (0..3)
        .map(|i| FileEntry {
            name: OsString::from(format!("f{i}.txt")),
            path: PathBuf::from("/left").join(format!("f{i}.txt")),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
            folder_size: None,
        })
        .collect();
    app.panels.left.selected = 0;
    app
}

fn app_with_keys(content: &str) -> App {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, content).unwrap();
    App::with_fixed_panels().with_config(Ok(config::load(&path).unwrap()))
}

#[test]
fn the_default_key_does_not_collide_with_another_default() {
    let keymap = Keymap::default();
    for scope in [KeyScope::Panel, KeyScope::Search] {
        assert_eq!(
            keymap.map_key(&Key::Character("p".into()), chord(), scope),
            Some(palette_message(CommandPaletteMessage::Toggle)),
            "{scope:?}"
        );
    }
}

#[test]
fn the_chord_opens_it_over_the_panels_and_over_the_search() {
    let key = || Key::Character("P".into());
    let panel = PromptKeyState::default();
    let search = PromptKeyState {
        search: true,
        ..PromptKeyState::default()
    };
    for state in [&panel, &search] {
        assert_eq!(
            route_key(state, Key::Character("p".into()), chord()),
            Some(palette_message(CommandPaletteMessage::Toggle))
        );
    }
    // Shift makes the letter a capital on most layouts.
    assert_eq!(
        route_key(&panel, key(), chord()),
        Some(palette_message(CommandPaletteMessage::Toggle))
    );
}

#[test]
fn opening_lists_the_actions_and_takes_the_keys() {
    let mut app = App::with_fixed_panels();
    open_palette(&mut app);
    let palette = app.command_palette_for_test().unwrap();
    assert!(palette.hits().next().is_some());
    assert!(app.prompt_key_state().command_palette);
    assert!(app.blocking_overlay_is_open());
}

#[test]
fn the_search_view_gets_the_actions_of_the_search() {
    let mut app = App::with_fixed_search();
    open_palette(&mut app);
    let palette = app.command_palette_for_test().unwrap();
    let regex = palette
        .hits()
        .filter_map(|hit| palette.entry(hit))
        .any(|entry| entry.command() == Some(keymap::KeyAction::SearchRegex));
    assert!(regex);
}

#[test]
fn the_arrows_move_and_the_other_bindings_are_dead_while_it_is_open() {
    let mut app = App::with_fixed_panels();
    open_palette(&mut app);
    let none = Modifiers::default();
    assert_eq!(
        route(&app, Key::Named(Named::ArrowDown), none),
        Some(palette_message(CommandPaletteMessage::Move(1)))
    );
    assert_eq!(
        route(&app, Key::Named(Named::ArrowUp), none),
        Some(palette_message(CommandPaletteMessage::Move(-1)))
    );
    assert_eq!(
        route(&app, Key::Named(Named::Enter), none),
        Some(palette_message(CommandPaletteMessage::Run))
    );
    assert_eq!(
        route(&app, Key::Named(Named::Escape), none),
        Some(palette_message(CommandPaletteMessage::Close))
    );
    for key in [Named::F5, Named::F7, Named::F8, Named::F9, Named::Backspace] {
        assert_eq!(route(&app, Key::Named(key), none), None, "{key:?}");
    }
    assert_eq!(route(&app, Key::Character("q".into()), none), None);
    assert_eq!(
        route(&app, Key::Named(Named::F10), none),
        Some(Message::Quit)
    );
}

#[test]
fn f8_with_the_palette_open_opens_no_delete_dialog() {
    let mut app = App::with_fixed_panels();
    open_palette(&mut app);
    let routed = route(&app, Key::Named(Named::F8), Modifiers::default());
    assert_eq!(routed, None);
    assert!(app.ops.delete_dialog.is_none());
}

#[test]
fn typing_filters_the_entries() {
    let mut app = App::with_fixed_panels();
    open_palette(&mut app);
    type_into_palette(&mut app, "filing status");
    let palette = app.command_palette_for_test().unwrap();
    assert_eq!(palette.hits().count(), 1);
    assert_eq!(palette.query(), "filing status");
}

#[test]
fn enter_on_filing_status_opens_the_page_and_closes_the_palette() {
    let mut app = App::with_fixed_panels().with_starting_hermes_for_test();
    open_palette(&mut app);
    type_into_palette(&mut app, "filing status");
    drop(app.update(palette_message(CommandPaletteMessage::Run)));
    assert!(app.command_palette_for_test().is_none());
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));
    assert!(!app.prompt_key_state().command_palette);
}

#[test]
fn a_click_on_an_entry_runs_it() {
    let mut app = App::with_fixed_panels().with_starting_hermes_for_test();
    open_palette(&mut app);
    type_into_palette(&mut app, "filing status");
    drop(app.update(palette_message(CommandPaletteMessage::Select(1))));
    assert!(app.command_palette_for_test().is_none());
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));
}

#[test]
fn run_without_a_match_keeps_the_palette_open() {
    let mut app = App::with_fixed_panels();
    open_palette(&mut app);
    type_into_palette(&mut app, "zzzzzz");
    drop(app.update(palette_message(CommandPaletteMessage::Run)));
    assert!(app.command_palette_for_test().is_some());
}

#[test]
fn a_menu_entry_does_what_the_same_entry_in_the_lab_does() {
    let mut lab = App::with_fixed_panels();
    open_palette(&mut lab);
    type_into_palette(&mut lab, "switch panel");
    drop(lab.update(palette_message(CommandPaletteMessage::Run)));

    let mut menu = App::with_fixed_panels();
    let message = crate::menu::command_message(KeyAction::SwitchPanel.name()).unwrap();
    drop(menu.update(message));

    assert_eq!(menu.panels.active, PanelSide::Right);
    assert_eq!(menu.panels.active, lab.panels.active);
    assert_eq!(menu.recent_commands, [KeyAction::SwitchPanel]);
    assert_eq!(menu.recent_commands, lab.recent_commands);
}

#[test]
fn the_menu_is_off_while_a_box_is_open_and_follows_the_search() {
    let mut app = App::with_fixed_panels();
    assert_eq!(app.menu_availability(), Availability::Open(KeyScope::Panel));
    open_palette(&mut app);
    let blocked = app.menu_availability();
    assert_eq!(blocked, Availability::Blocked);
    assert!(blocked.enables(KeyAction::Quit));
    assert!(!blocked.enables(KeyAction::Mkdir));
    drop(app.update(palette_message(CommandPaletteMessage::Close)));
    drop(app.update(Message::OpenSearch));
    assert_eq!(
        app.menu_availability(),
        Availability::Open(KeyScope::Search)
    );
}

#[test]
fn the_commands_run_last_head_the_list_next_time() {
    let mut app = App::with_fixed_panels();
    for query in ["switch panel", "tag all", "switch panel"] {
        open_palette(&mut app);
        type_into_palette(&mut app, query);
        drop(app.update(palette_message(CommandPaletteMessage::Run)));
    }
    open_palette(&mut app);
    let palette = app.command_palette_for_test().unwrap();
    let first_hits: Vec<keymap::KeyAction> = palette
        .rows()
        .iter()
        .skip(1)
        .take(2)
        .filter_map(|row| match row {
            ncrs_palette::command_list::Row::Hit(hit) => palette.entry(hit),
            ncrs_palette::command_list::Row::Header(_) => None,
        })
        .filter_map(|entry| entry.command())
        .collect();
    assert_eq!(
        first_hits,
        [keymap::KeyAction::SwitchPanel, keymap::KeyAction::TagAll]
    );
    assert_eq!(
        palette.selected_target(),
        Some(ncrs_palette::command_list::Target::Command(
            keymap::KeyAction::SwitchPanel
        ))
    );
}

#[test]
fn mkdir_from_the_palette_leaves_the_prompt_open() {
    let mut app = App::with_fixed_panels();
    open_palette(&mut app);
    type_into_palette(&mut app, "create folder");
    drop(app.update(palette_message(CommandPaletteMessage::Run)));
    assert!(app.command_palette_for_test().is_none());
    assert!(app.ops.prompt.is_some(), "the prompt did not open");
    assert!(app.prompt_key_state().open);
}

#[test]
fn a_rebound_key_shows_in_the_entry() {
    let mut app = app_with_keys("[keys]\nfiling_status = \"F11\"\n");
    open_palette(&mut app);
    let palette = app.command_palette_for_test().unwrap();
    let entry = palette
        .hits()
        .filter_map(|hit| palette.entry(hit))
        .find(|entry| entry.command() == Some(keymap::KeyAction::FilingStatus))
        .unwrap();
    assert_eq!(entry.shortcut.as_deref(), Some("F11"));
}

#[test]
fn an_action_without_a_key_has_no_key_in_its_entry() {
    let mut app = app_with_keys("[keys]\ndelete_permanently = []\n");
    open_palette(&mut app);
    let palette = app.command_palette_for_test().unwrap();
    let entry = palette
        .hits()
        .filter_map(|hit| palette.entry(hit))
        .find(|entry| entry.command() == Some(keymap::KeyAction::DeletePermanently))
        .unwrap();
    assert_eq!(entry.shortcut, None);
}

#[tokio::test]
async fn escape_closes_it_and_the_running_job_keeps_going() {
    let mut app = app_with_files();
    let _started = app.start_transfer(TransferKind::Copy);
    assert!(app.jobs.is_busy(), "no job to keep");
    drop(app.update(Message::TransferDialog(TransferDialogMessage::Background)));
    open_palette(&mut app);
    assert!(app.command_palette_for_test().is_some());

    let routed = route(&app, Key::Named(Named::Escape), Modifiers::default());
    assert_eq!(routed, Some(palette_message(CommandPaletteMessage::Close)));
    let _closed = app.update(routed.unwrap());

    assert!(app.command_palette_for_test().is_none());
    assert!(app.jobs.is_busy(), "Escape stopped the job");
}

#[test]
fn the_input_state_closes_it_on_escape_too() {
    let mut input = InputState {
        command_palette: true,
        job_running: true,
        ..InputState::default()
    };
    let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Escape),
        modified_key: Key::Named(Named::Escape),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::default(),
        repeat: false,
        text: None,
    });
    assert_eq!(
        input.handle(&escape, event::Status::Ignored),
        Some(palette_message(CommandPaletteMessage::Close))
    );
}
