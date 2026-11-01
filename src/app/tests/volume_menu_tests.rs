use crate::app::*;

fn volume(name: &str, path: &str) -> fs::Volume {
    fs::Volume {
        name: name.to_string(),
        path: PathBuf::from(path),
    }
}

fn drives() -> Vec<fs::Volume> {
    vec![
        volume("Root", "/"),
        volume("Home", "/home/test"),
        volume("Stick", "/media/stick"),
    ]
}

fn route(named: Named, modifiers: Modifiers) -> Option<Message> {
    route_key(&PromptKeyState::default(), Key::Named(named), modifiers)
}

fn open_menu() -> PromptKeyState {
    PromptKeyState {
        volume_menu: true,
        ..PromptKeyState::default()
    }
}

/// On macOS the Option key arrives as the ALT modifier, and F1 stays the
/// named key: function keys have no character for Option to change.
#[test]
fn alt_f1_and_alt_f2_open_the_menu_for_their_panel() {
    let alt = Modifiers::ALT;
    assert!(alt.alt());
    assert_eq!(
        route(Named::F1, alt),
        Some(Message::VolumeMenu(PanelSide::Left))
    );
    assert_eq!(
        route(Named::F2, alt),
        Some(Message::VolumeMenu(PanelSide::Right))
    );
    assert_eq!(
        route(Named::F1, Modifiers::default()),
        Some(Message::CommandPalette(CommandPaletteMessage::Toggle))
    );
    assert_eq!(route(Named::F1, Modifiers::CTRL), None);
}

#[test]
fn an_open_menu_takes_the_movement_keys_and_nothing_else() {
    let menu = open_menu();
    let key = |named| route_key(&menu, Key::Named(named), Modifiers::default());
    assert_eq!(key(Named::ArrowDown), Some(Message::VolumeMenuMove(1)));
    assert_eq!(key(Named::ArrowUp), Some(Message::VolumeMenuMove(-1)));
    assert_eq!(key(Named::Home), Some(Message::VolumeMenuFirst));
    assert_eq!(key(Named::End), Some(Message::VolumeMenuLast));
    assert_eq!(key(Named::Enter), Some(Message::VolumeMenuActivate));
    assert_eq!(key(Named::Escape), Some(Message::VolumeMenuClose));
    assert_eq!(key(Named::Delete), Some(Message::VolumeMenuRemove));
    assert_eq!(key(Named::Backspace), Some(Message::VolumeMenuRemove));
    for ignored in [Named::F5, Named::F8, Named::Tab] {
        assert_eq!(key(ignored), None, "{ignored:?} reached the panels");
    }
}

#[test]
fn movement_repeats_but_choosing_does_not() {
    assert!(repeats(&Message::VolumeMenuMove(1)));
    assert!(repeats(&Message::VolumeMenuFirst));
    assert!(repeats(&Message::VolumeMenuLast));
    assert!(!repeats(&Message::VolumeMenuActivate));
    assert!(!repeats(&Message::VolumeMenu(PanelSide::Left)));
}

#[test]
fn the_menu_opens_on_the_longest_matching_drive() {
    let menu = |current: &str| VolumeMenu::new(PanelSide::Left, drives(), Path::new(current));
    assert_eq!(menu("/home/test/docs").selected, 1);
    assert_eq!(menu("/home/other").selected, 0);
    assert_eq!(menu("/media/stick").selected, 2);
    // `/home/tester` is not below `/home/test`.
    assert_eq!(menu("/home/tester").selected, 0);
    assert_eq!(menu("/elsewhere").selected, 0);
}

#[test]
fn the_highlight_stops_at_both_ends() {
    let mut menu = VolumeMenu::new(PanelSide::Left, drives(), Path::new("/"));
    menu.move_by(-1);
    assert_eq!(menu.selected, 0);
    menu.move_by(5);
    assert_eq!(menu.selected, 2);
}

#[test]
fn the_menu_opens_for_the_panel_that_asked() {
    let mut app = App::with_fixed_panels().with_volumes(drives);
    let _open = app.update(Message::VolumeMenu(PanelSide::Right));
    assert_eq!(
        app.panels.volume_menu.as_ref().unwrap().side,
        PanelSide::Right
    );
    // `/home/test/files` is the right panel's path.
    assert_eq!(app.volume_menu_selected(), Some(1));
    assert!(app.prompt_key_state().volume_menu);

    let _close = app.update(Message::VolumeMenuClose);
    assert!(!app.volume_menu_is_open());
    assert!(!app.prompt_key_state().volume_menu);
}

#[test]
fn a_dialog_keeps_the_menu_from_opening() {
    let mut app = App::with_fixed_panels().with_volumes(drives);
    app.open_delete_dialog_for_test(1, false);
    let _open = app.update(Message::VolumeMenu(PanelSide::Left));
    assert!(!app.volume_menu_is_open());
}

#[tokio::test]
async fn choosing_a_drive_loads_it_and_activates_its_panel() {
    let mut app = App::with_fixed_panels().with_volumes(drives);
    let _open = app.update(Message::VolumeMenu(PanelSide::Right));
    let _down = app.update(Message::VolumeMenuMove(1));
    let _enter = app.update(Message::VolumeMenuActivate);
    assert!(!app.volume_menu_is_open());
    assert_eq!(app.panels.active, PanelSide::Right);
    assert!(app.panel(PanelSide::Right).loading);
    assert!(!app.panel(PanelSide::Left).loading);
}
