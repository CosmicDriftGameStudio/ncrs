use super::settings_pages_tests::settle;
use crate::app::*;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

async fn app_in(dir: &Path) -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = dir.to_path_buf();
    let mut stream =
        task::into_stream(app.load(PanelSide::Left, dir.to_path_buf(), None)).expect("a load task");
    let mut reports = Vec::new();
    while let Some(action) = stream.next().await {
        if let Action::Output(reported) = action {
            reports.push(reported);
        }
    }
    for reported in reports {
        settle(&mut app, reported).await;
    }
    app
}

fn put_cursor_on(app: &mut App, name: &str) {
    app.panels.left.selected = app
        .panels
        .left
        .entries
        .iter()
        .position(|entry| entry.name == name)
        .expect("entry listed");
}

fn cursor_name(app: &App) -> String {
    app.panels
        .left
        .selected_entry()
        .unwrap()
        .name
        .to_string_lossy()
        .into_owned()
}

fn type_name(name: &str) -> Message {
    Message::Panel(PanelMessage::RenameInput(name.to_owned()))
}

fn scratch() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    std::fs::write(dir.path().join("c.txt"), "c").unwrap();
    dir
}

#[tokio::test]
async fn shift_f6_opens_the_input_with_the_name_of_the_cursor_entry() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");

    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));

    let rename = app.panels.left.rename.clone().expect("input open");
    assert_eq!(rename.draft, "b.txt");
    assert!(app.panels.renaming_side().is_some());
}

#[tokio::test]
async fn escape_cancels_and_changes_nothing() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(type_name("zzz.txt"));

    settle(&mut app, Message::PathFieldCancel).await;

    assert!(app.panels.left.rename.is_none());
    assert!(dir.path().join("b.txt").exists());
    assert!(!dir.path().join("zzz.txt").exists());
}

#[tokio::test]
async fn a_click_elsewhere_cancels() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));

    settle(&mut app, Message::ClickedOutside).await;

    assert!(app.panels.left.rename.is_none());
    assert!(dir.path().join("b.txt").exists());
}

#[tokio::test]
async fn enter_renames_and_the_cursor_stays_on_the_renamed_entry() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(type_name("x.txt"));

    settle(&mut app, Message::Panel(PanelMessage::RenameSubmit)).await;

    assert!(app.panels.left.rename.is_none());
    assert!(!dir.path().join("b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("x.txt")).unwrap(),
        "b"
    );
    assert_eq!(cursor_name(&app), "x.txt");
}

#[tokio::test]
async fn a_taken_name_stays_in_the_input_with_a_message_and_changes_nothing() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(type_name("c.txt"));

    settle(&mut app, Message::Panel(PanelMessage::RenameSubmit)).await;

    let rename = app.panels.left.rename.clone().expect("input stays open");
    assert!(rename.error.is_some());
    assert!(!rename.busy);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "b"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("c.txt")).unwrap(),
        "c"
    );
}

#[tokio::test]
async fn an_empty_name_and_a_path_are_refused_before_the_file_system() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    for bad in ["", "..", "sub/x.txt"] {
        let _typed = app.update(type_name(bad));
        settle(&mut app, Message::Panel(PanelMessage::RenameSubmit)).await;
        let rename = app.panels.left.rename.clone().expect("input stays open");
        assert!(rename.error.is_some(), "{bad:?} was accepted");
    }
    assert!(dir.path().join("b.txt").exists());
}

#[tokio::test]
async fn a_change_of_case_only_renames() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(type_name("B.txt"));

    settle(&mut app, Message::Panel(PanelMessage::RenameSubmit)).await;

    assert!(app.panels.left.rename.is_none());
    assert_eq!(cursor_name(&app), "B.txt");
}

#[tokio::test]
async fn the_parent_row_cannot_be_renamed() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    app.panels.left.selected = app
        .panels
        .left
        .entries
        .iter()
        .position(|entry| entry.is_parent)
        .expect("a parent row");

    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));

    assert!(app.panels.left.rename.is_none());
}

#[tokio::test]
async fn a_late_answer_for_another_entry_leaves_the_open_input_alone() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "b.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let request_id = app.ops.rename_request_id;
    let other = dir.path().join("a.txt");
    let answers = [
        Ok(dir.path().join("x.txt")),
        Err(RowFailure::Conflict("taken".to_owned())),
        Err(RowFailure::Other("disk".to_owned())),
    ];

    for result in answers {
        let _task = app.update(Message::Renamed {
            request_id,
            side: PanelSide::Left,
            from: other.clone(),
            result,
        });
        let rename = app.panels.left.rename.clone().expect("input stays open");
        assert_eq!(rename.path, dir.path().join("b.txt"));
        assert_eq!(rename.error, None);
    }
}

#[tokio::test]
async fn the_answer_of_a_replaced_rename_still_reloads_the_panel() {
    let dir = scratch();
    let mut app = app_in(dir.path()).await;
    put_cursor_on(&mut app, "a.txt");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let stale_id = app.ops.rename_request_id.wrapping_sub(1);
    let from = dir.path().join("a.txt");
    let to = dir.path().join("z.txt");
    std::fs::rename(&from, &to).unwrap();

    settle(
        &mut app,
        Message::Renamed {
            request_id: stale_id,
            side: PanelSide::Left,
            from,
            result: Ok(to),
        },
    )
    .await;

    assert!(app
        .panels
        .left
        .entries
        .iter()
        .any(|entry| entry.name == "z.txt"));
}

#[tokio::test]
async fn the_other_panel_inside_a_renamed_folder_follows_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("photos").join("2026")).unwrap();
    let mut app = app_in(dir.path()).await;
    let inside = dir.path().join("photos").join("2026");
    app.panels.right.path = inside.clone();
    put_cursor_on(&mut app, "photos");
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(type_name("pictures"));

    settle(&mut app, Message::Panel(PanelMessage::RenameSubmit)).await;

    assert!(dir.path().join("pictures").join("2026").is_dir());
    assert_eq!(
        app.panels.right.path,
        dir.path().join("pictures").join("2026")
    );
}
