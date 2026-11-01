use crate::app::*;
use iced::mouse::ScrollDelta;

/// The fixed app has 12 visible rows; the left panel gets 100 entries.
fn app_with_long_list() -> App {
    let mut app = App::with_fixed_panels();
    app.panels.left.entries = (0..100)
        .map(|i| fs::FileEntry {
            name: format!("f{i:03}").into(),
            path: PathBuf::from(format!("/left/f{i:03}")),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
            folder_size: None,
        })
        .collect();
    app
}

fn scroll(app: &mut App, side: PanelSide, delta: ScrollDelta) {
    let _task = app.update(Message::Panel(PanelMessage::PanelScrolled { side, delta }));
}

fn pixels(y: f32) -> ScrollDelta {
    ScrollDelta::Pixels { x: 0.0, y }
}

fn lines(y: f32) -> ScrollDelta {
    ScrollDelta::Lines { x: 0.0, y }
}

/// A slow trackpad gesture is many small events; none of them is a row
/// alone, together they are.
#[test]
fn small_pixel_events_add_up_to_whole_rows() {
    let mut app = app_with_long_list();
    // 22 px per row: 5 x 10 px is 2.27 rows.
    for _ in 0..5 {
        scroll(&mut app, PanelSide::Left, pixels(-10.0));
    }
    assert_eq!(app.panels.left.scroll_offset, 2);
    // The rest of each event is kept: 80 px in all is 3.6 rows.
    for _ in 0..3 {
        scroll(&mut app, PanelSide::Left, pixels(-10.0));
    }
    assert_eq!(app.panels.left.scroll_offset, 3);
    // And back up, towards the top of the list.
    scroll(&mut app, PanelSide::Left, pixels(88.0));
    assert_eq!(app.panels.left.scroll_offset, 0);
}

#[test]
fn wheel_lines_scroll_by_rows() {
    let mut app = app_with_long_list();
    scroll(&mut app, PanelSide::Left, lines(-3.0));
    assert_eq!(app.panels.left.scroll_offset, 3);
    scroll(&mut app, PanelSide::Left, lines(1.0));
    assert_eq!(app.panels.left.scroll_offset, 2);
}

#[test]
fn only_the_panel_under_the_pointer_scrolls_and_the_ends_hold() {
    let mut app = app_with_long_list();
    scroll(&mut app, PanelSide::Right, lines(-5.0));
    assert_eq!(app.panels.left.scroll_offset, 0);

    scroll(&mut app, PanelSide::Left, lines(500.0));
    assert_eq!(app.panels.left.scroll_offset, 0, "scrolled above the top");
    scroll(&mut app, PanelSide::Left, lines(-500.0));
    assert_eq!(app.panels.left.scroll_offset, 88, "scrolled past the end");
    // Scrolling up from the end moves at once; nothing was banked.
    scroll(&mut app, PanelSide::Left, lines(1.0));
    assert_eq!(app.panels.left.scroll_offset, 87);
}

#[test]
fn the_cursor_stays_while_visible_and_is_held_at_the_edge() {
    let mut app = app_with_long_list();
    app.panels.left.select(5, 12);
    scroll(&mut app, PanelSide::Left, lines(-2.0));
    assert_eq!(
        app.panels.left.selected, 5,
        "the cursor moved while in view"
    );

    scroll(&mut app, PanelSide::Left, lines(-10.0));
    assert_eq!(app.panels.left.scroll_offset, 12);
    assert_eq!(app.panels.left.selected, 12, "the cursor left the top");

    scroll(&mut app, PanelSide::Left, lines(20.0));
    assert_eq!(app.panels.left.scroll_offset, 0);
    assert_eq!(app.panels.left.selected, 11, "the cursor left the bottom");
}

#[test]
fn a_dialog_or_the_menu_keeps_the_panel_still() {
    let mut app = app_with_long_list();
    app.open_delete_dialog_for_test(1, false);
    scroll(&mut app, PanelSide::Left, lines(-4.0));
    assert_eq!(
        app.panels.left.scroll_offset, 0,
        "scrolled under the dialog"
    );

    let mut with_menu = app_with_long_list().with_volumes(|| {
        vec![fs::Volume {
            name: "Root".to_string(),
            path: PathBuf::from("/"),
        }]
    });
    let _open = with_menu.update(Message::VolumeMenu(PanelSide::Left));
    scroll(&mut with_menu, PanelSide::Left, lines(-4.0));
    assert_eq!(
        with_menu.panels.left.scroll_offset, 0,
        "scrolled under the menu"
    );

    let mut with_prompt = app_with_long_list();
    let _prompt = with_prompt.update(Message::CreateDirPrompt);
    scroll(&mut with_prompt, PanelSide::Left, lines(-4.0));
    assert_eq!(
        with_prompt.panels.left.scroll_offset, 0,
        "scrolled under the prompt"
    );
}

#[test]
fn page_keys_and_cmd_arrows_turn_a_page() {
    let idle = PromptKeyState::default();
    for (key, modifiers, message) in [
        (
            Named::PageDown,
            Modifiers::default(),
            Message::Panel(PanelMessage::PageDown),
        ),
        (
            Named::PageUp,
            Modifiers::default(),
            Message::Panel(PanelMessage::PageUp),
        ),
        (
            Named::ArrowDown,
            Modifiers::COMMAND,
            Message::Panel(PanelMessage::PageDown),
        ),
        (
            Named::ArrowUp,
            Modifiers::COMMAND,
            Message::Panel(PanelMessage::PageUp),
        ),
    ] {
        assert_eq!(
            route_key(&idle, Key::Named(key), modifiers),
            Some(message),
            "{key:?} {modifiers:?}"
        );
    }
    assert!(repeats(&Message::Panel(PanelMessage::PageDown)));
    assert!(repeats(&Message::Panel(PanelMessage::PageUp)));

    let mut app = app_with_long_list();
    let _down = app.update(Message::Panel(PanelMessage::PageDown));
    assert_eq!(app.panels.left.selected, 12);
    assert_eq!(app.panels.left.scroll_offset, 1);
    let _up = app.update(Message::Panel(PanelMessage::PageUp));
    assert_eq!(app.panels.left.selected, 0);
    assert_eq!(app.panels.left.scroll_offset, 0);
}
