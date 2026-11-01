use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

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
    app
}

fn feed(app: &mut App, event: event::Event) -> Option<Message> {
    let emitted = app.input.lock().handle(&event, event::Status::Ignored);
    if let Some(message) = emitted.clone() {
        drop(app.update(message));
    }
    emitted
}

fn pointer_at(x: f32, y: f32) -> event::Event {
    event::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: Point::new(x, y),
    })
}

fn left_released() -> event::Event {
    event::Event::Mouse(iced::mouse::Event::ButtonReleased(
        iced::mouse::Button::Left,
    ))
}

fn press_row_one(app: &mut App) {
    feed(app, pointer_at(50.0, 50.0));
    drop(app.update(Message::Panel(PanelMessage::RowClicked {
        side: PanelSide::Left,
        index: 1,
        on_tag: false,
    })));
}

fn move_modifier() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::COMMAND
    } else {
        Modifiers::SHIFT
    }
}

#[test]
fn the_input_reports_no_drag_while_no_row_is_held() {
    let mut app = app_with_files();

    assert_eq!(feed(&mut app, pointer_at(20.0, 20.0)), None);
    assert_eq!(feed(&mut app, left_released()), None);
}

#[test]
fn a_click_without_movement_only_selects() {
    let mut app = app_with_files();
    press_row_one(&mut app);
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Right))));

    feed(&mut app, pointer_at(52.0, 50.0));
    feed(&mut app, left_released());

    assert!(app.ops.transfer_dialog.is_none());
    assert_eq!(app.panels.left.selected, 1);
    assert!(!app.panels.left.selection.any_tagged());
    assert!(app.panels.drag.is_none());
}

#[tokio::test]
async fn dragging_a_row_onto_the_other_panel_asks_the_copy_question() {
    let mut app = app_with_files();
    press_row_one(&mut app);

    feed(&mut app, pointer_at(70.0, 50.0));
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Right))));
    feed(&mut app, left_released());

    let dialog = app.ops.transfer_dialog.as_ref().expect("the dialog opens");
    assert_eq!(dialog.kind(), TransferKind::Copy);
    assert_eq!(dialog.target_path(), PathBuf::from("/right"));
    assert_eq!(dialog.sources(), [PathBuf::from("/left/f1.txt")]);
}

#[tokio::test]
async fn the_move_modifier_at_the_drop_asks_the_move_question() {
    let mut app = app_with_files();
    press_row_one(&mut app);

    feed(&mut app, pointer_at(70.0, 50.0));
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Right))));
    drop(app.update(Message::ModifiersChanged(move_modifier())));
    feed(&mut app, left_released());

    let dialog = app.ops.transfer_dialog.as_ref().expect("the dialog opens");
    assert_eq!(dialog.kind(), TransferKind::Move);
}

#[test]
fn the_other_panel_is_outlined_while_a_drag_hovers_it() {
    let mut app = app_with_files();
    press_row_one(&mut app);
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Right))));
    assert!(!app.is_drop_target(PanelSide::Right));

    feed(&mut app, pointer_at(70.0, 50.0));

    assert!(app.is_drop_target(PanelSide::Right));
    assert!(!app.is_drop_target(PanelSide::Left));
}

#[test]
fn the_cursor_shows_what_dropping_the_dragged_rows_would_do() {
    let mut app = app_with_files();
    assert_eq!(app.drag_cursor(PanelSide::Right), None);
    press_row_one(&mut app);
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Right))));
    assert_eq!(app.drag_cursor(PanelSide::Right), None);

    feed(&mut app, pointer_at(70.0, 50.0));
    assert_eq!(
        app.drag_cursor(PanelSide::Right),
        Some(mouse::Interaction::Copy)
    );
    assert_eq!(app.drag_cursor(PanelSide::Left), None);

    drop(app.update(Message::ModifiersChanged(move_modifier())));
    assert_eq!(
        app.drag_cursor(PanelSide::Right),
        Some(mouse::Interaction::Move)
    );

    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Left))));
    assert_eq!(
        app.drag_cursor(PanelSide::Left),
        Some(mouse::Interaction::NoDrop)
    );
}

#[test]
fn files_from_another_app_keep_the_os_cursor() {
    let mut app = app_with_files();
    app.ops.drop_hover = true;
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Left))));

    assert_eq!(app.drag_cursor(PanelSide::Left), None);
    assert_eq!(app.drag_cursor(PanelSide::Right), None);
}

#[test]
fn dropping_a_row_on_its_own_panel_does_nothing() {
    let mut app = app_with_files();
    press_row_one(&mut app);

    feed(&mut app, pointer_at(70.0, 50.0));
    drop(app.update(Message::Panel(PanelMessage::PanelEntered(PanelSide::Left))));
    feed(&mut app, left_released());

    assert!(app.ops.transfer_dialog.is_none());
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn dragging_a_server_file_out_of_the_window_shows_the_local_only_notice() {
    let mut app = app_with_files();
    app.panels.left.entries[1].path =
        RemotePath::new(ServerId::new("user", "files.invalid", 22), "/f1.txt").to_path_buf();
    press_row_one(&mut app);

    feed(&mut app, pointer_at(70.0, 50.0));
    feed(&mut app, pointer_at(-30.0, 50.0));

    assert_eq!(
        app.notice.as_deref(),
        Some(app.lang.text(Msg::StatusDragOutLocalOnly))
    );
    assert!(app.panels.drag.is_none());
    assert!(!app.input.lock().panel_drag_armed);
}

#[tokio::test]
async fn a_double_click_on_a_folder_opens_it_like_enter() {
    let base = tempfile::tempdir().unwrap();
    std::fs::create_dir(base.path().join("sub")).unwrap();
    let mut app = App::with_fixed_panels();
    app.panels.left = PanelState::new(base.path().to_path_buf());
    app.panels.active = PanelSide::Right;
    let mut stream =
        iced_test::runtime::task::into_stream(app.load(PanelSide::Left, base.path().into(), None))
            .unwrap();
    while let Some(action) = futures::StreamExt::next(&mut stream).await {
        if let iced_test::runtime::Action::Output(reported) = action {
            drop(app.update(reported));
        }
    }
    let index = app
        .panels
        .left
        .entries
        .iter()
        .position(|entry| entry.name == "sub")
        .unwrap();

    let opening = app.update(Message::Panel(PanelMessage::RowActivated {
        side: PanelSide::Left,
        index,
    }));
    let mut opened = iced_test::runtime::task::into_stream(opening).unwrap();
    while let Some(action) = futures::StreamExt::next(&mut opened).await {
        if let iced_test::runtime::Action::Output(reported) = action {
            drop(app.update(reported));
        }
    }

    assert_eq!(app.panels.active, PanelSide::Left);
    assert_eq!(app.panels.left.path, base.path().join("sub"));
}

/// A click on the star tags; a click anywhere else does not. Without the
/// first, tagging is keyboard-only and the column of stars cannot be used.
#[test]
fn a_click_on_the_star_tags_the_row() {
    let mut app = app_with_files();

    let _task = app.update(Message::Panel(PanelMessage::RowClicked {
        side: PanelSide::Left,
        index: 1,
        on_tag: true,
    }));

    let name = app
        .panels
        .left
        .entries
        .get(1)
        .expect("the fixture has three files")
        .path
        .clone();
    assert!(
        app.panels.left.selection.is_tagged(&name),
        "clicking the star did not tag the row"
    );
}

/// Clicking the row moves the cursor and leaves the tags alone — otherwise
/// selecting files to copy would tag them as a side effect.
#[test]
fn a_click_on_the_row_only_moves_the_cursor() {
    let mut app = app_with_files();

    let _task = app.update(Message::Panel(PanelMessage::RowClicked {
        side: PanelSide::Left,
        index: 2,
        on_tag: false,
    }));

    assert_eq!(app.panels.left.selected, 2);
    assert!(
        app.panels.left.selection.is_empty(),
        "clicking a row tagged it"
    );
}

/// The star toggles: clicking it again untags.
#[test]
fn a_second_click_on_the_star_untags() {
    let mut app = app_with_files();
    for _ in 0..2 {
        let _task = app.update(Message::Panel(PanelMessage::RowClicked {
            side: PanelSide::Left,
            index: 1,
            on_tag: true,
        }));
    }
    assert!(app.panels.left.selection.is_empty());
}

/// `..` has no star to click, and must not be taggable if it somehow is.
#[test]
fn the_parent_entry_cannot_be_tagged_by_click() {
    let mut app = app_with_files();
    app.panels
        .left
        .entries
        .get_mut(0)
        .expect("the fixture has three files")
        .is_parent = true;

    let _task = app.update(Message::Panel(PanelMessage::RowClicked {
        side: PanelSide::Left,
        index: 0,
        on_tag: true,
    }));

    assert!(app.panels.left.selection.is_empty());
}

/// Clicking a row in the other panel makes that panel the source of F5.
#[test]
fn clicking_a_row_activates_its_panel() {
    let mut app = app_with_files();
    app.panels.right.entries = app.panels.left.entries.clone();

    let _task = app.update(Message::Panel(PanelMessage::RowClicked {
        side: PanelSide::Right,
        index: 0,
        on_tag: false,
    }));

    assert_eq!(app.panels.active, PanelSide::Right);
}
