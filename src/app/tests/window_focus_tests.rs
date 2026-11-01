use crate::app::*;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

async fn deliver(app: &mut App, message: Message) {
    let mut queue = vec![message];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(app.update(next)) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                queue.push(reported);
            }
        }
    }
}

fn size_of(app: &App, name: &str) -> u64 {
    app.panels
        .left
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
        .size
}

#[tokio::test]
async fn focus_returning_rereads_local_panels_and_keeps_cursor_and_tags() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    let mut app = App::with_fixed_panels();
    app.panels.left = PanelState::new(dir.path().to_path_buf());
    let first = app.load(PanelSide::Left, dir.path().to_path_buf(), None);
    let mut stream = task::into_stream(first).unwrap();
    while let Some(action) = stream.next().await {
        if let Action::Output(reported) = action {
            deliver(&mut app, reported).await;
        }
    }
    let rows = app.visible_rows;
    let index = app
        .panels
        .left
        .entries
        .iter()
        .position(|entry| entry.name == "b.txt")
        .unwrap();
    app.panels.left.select(index, rows);
    app.panels.left.selection.toggle(&dir.path().join("c.txt"));
    assert_eq!(size_of(&app, "b.txt"), 0);

    std::fs::write(dir.path().join("b.txt"), "edited").unwrap();
    deliver(&mut app, Message::WindowFocused).await;

    assert_eq!(size_of(&app, "b.txt"), 6);
    assert_eq!(app.panels.left.selected_entry().unwrap().name, "b.txt");
    assert!(app
        .panels
        .left
        .selection
        .is_tagged(&dir.path().join("c.txt")));
}

#[tokio::test]
async fn focus_returning_leaves_a_panel_inside_an_archive_alone() {
    let mut app = App::with_fixed_panels();
    app.panels.left.archive = Some(PathBuf::from("/home/test/a.zip"));
    let request_before = app.panels.left.request_id;
    drop(app.update(Message::WindowFocused));
    assert_eq!(app.panels.left.request_id, request_before);
    assert_ne!(app.panels.right.request_id, 0);
}
