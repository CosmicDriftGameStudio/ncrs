use crate::e2e_tests::*;
use iced::keyboard::key::Named;

/// Alt+F2 offers the drives for the right panel, Enter takes it to the
/// highlighted one, and the panel really lists that directory afterwards.
#[test]
fn alt_f2_then_arrows_and_enter_change_the_right_panel_drive() {
    let dir = scratch("drive-enter");
    write(&dir.path().join("files").join("inside.txt"), "x");
    let mut session = session_with_drives(dir.path());

    session.press_with(Key::Named(Named::F2), Modifiers::ALT);
    assert!(session.app.volume_menu_is_open(), "Alt+F2 opened nothing");
    assert_eq!(
        session.app.volume_menu_selected(),
        Some(0),
        "the drive the panel is on was not highlighted"
    );
    session.press(Key::Named(Named::ArrowDown));
    assert_eq!(session.app.volume_menu_selected(), Some(1));
    session.press(Key::Named(Named::Enter));

    assert!(!session.app.volume_menu_is_open(), "Enter left the menu up");
    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(
        session.app.panel(PanelSide::Right).path,
        canonical.join("files")
    );
    assert!(names(&session.app, PanelSide::Right).contains(&"inside.txt".to_string()));
    assert_eq!(
        session.app.panel(PanelSide::Left).path,
        canonical,
        "the other panel moved"
    );
}

#[test]
fn alt_f1_changes_the_left_panel_and_end_goes_to_the_last_drive() {
    let dir = scratch("drive-left");
    let mut session = session_with_drives(dir.path());

    session.press_with(Key::Named(Named::F1), Modifiers::ALT);
    session.press(Key::Named(Named::End));
    assert_eq!(
        session.app.volume_menu_selected(),
        Some(3),
        "the trash is the row after the drives"
    );
    session.press(Key::Named(Named::ArrowUp));
    assert_eq!(session.app.volume_menu_selected(), Some(2));
    session.press(Key::Named(Named::Home));
    assert_eq!(session.app.volume_menu_selected(), Some(0));
    session.press(Key::Named(Named::End));
    session.press(Key::Named(Named::ArrowUp));
    session.press(Key::Named(Named::Enter));

    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(
        session.app.panel(PanelSide::Left).path,
        canonical.join("links")
    );
}

#[test]
fn escape_closes_the_drive_menu_and_the_panel_stays() {
    let dir = scratch("drive-escape");
    let mut session = session_with_drives(dir.path());
    let before = selected(&session.app, PanelSide::Left);

    session.press_with(Key::Named(Named::F1), Modifiers::ALT);
    session.press(Key::Named(Named::ArrowDown));
    session.press(Key::Named(Named::Escape));

    assert!(!session.app.volume_menu_is_open());
    assert_eq!(
        session.app.panel(PanelSide::Left).path,
        std::fs::canonicalize(dir.path()).unwrap()
    );
    assert_eq!(
        selected(&session.app, PanelSide::Left),
        before,
        "the arrow moved the panel behind the menu"
    );
}

/// Held arrow keys repeat in the menu; Enter held down does not go twice.
#[test]
fn a_held_arrow_repeats_in_the_drive_menu() {
    let dir = scratch("drive-repeat");
    let mut session = session_with_drives(dir.path());

    session.press_with(Key::Named(Named::F1), Modifiers::ALT);
    session.press_repeat(Key::Named(Named::ArrowDown), Modifiers::default());
    assert_eq!(session.app.volume_menu_selected(), Some(1));
    session.press_repeat(Key::Named(Named::Enter), Modifiers::default());
    assert!(session.app.volume_menu_is_open(), "a held Enter chose");
}

#[test]
fn a_click_on_a_drive_chooses_it() {
    let dir = scratch("drive-click");
    let mut session = session_with_drives(dir.path());

    session.press_with(Key::Named(Named::F2), Modifiers::ALT);
    session.send(Message::Panel(PanelMessage::VolumeMenuClick(2)));

    assert!(!session.app.volume_menu_is_open());
    assert_eq!(
        session.app.panel(PanelSide::Right).path,
        std::fs::canonicalize(dir.path()).unwrap().join("links")
    );
}

/// Paging through the real key routing: PageDown, and Cmd+Down (Ctrl on
/// Linux and Windows), each move the cursor a page and keep it in view.
#[test]
fn page_down_and_cmd_down_turn_a_page() {
    let dir = scratch("paging");
    for i in 0..80 {
        write(&dir.path().join(format!("f{i:03}.txt")), "x");
    }
    let mut session = session_in(dir.path());
    let rows = crate::ui::layout::visible_rows(crate::ui::layout::INITIAL_WINDOW_SIZE, 0.0);
    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("..")
    );

    session.press(Key::Named(Named::PageDown));
    let first_page = session.app.panel(PanelSide::Left).selected;
    assert_eq!(first_page, rows, "PageDown did not move a page");

    session.press_with(Key::Named(Named::ArrowDown), Modifiers::COMMAND);
    let panel = session.app.panel(PanelSide::Left);
    assert_eq!(panel.selected, 2 * rows, "Cmd+Down did not move a page");
    assert!(panel.selected >= panel.scroll_offset && panel.selected < panel.scroll_offset + rows);

    session.press_with(Key::Named(Named::ArrowUp), Modifiers::COMMAND);
    session.press(Key::Named(Named::PageUp));
    assert_eq!(session.app.panel(PanelSide::Left).selected, 0);
}

#[test]
fn a_launcher_failure_shows_in_the_status_line() {
    let dir = scratch("open-fail");
    write(&dir.path().join("alpha.txt"), "a");
    let mut session =
        session_with_launcher(dir.path(), std::sync::Arc::new(MissingProgramLauncher));
    cursor_to(&mut session, "alpha.txt");

    session.press(Key::Named(Named::F4));

    let message = session.app.job_error_for_test().expect("no error shown");
    assert!(
        message.contains("alpha.txt") && message.contains("No such file"),
        "the error does not name the file and the reason: {message}"
    );
}
