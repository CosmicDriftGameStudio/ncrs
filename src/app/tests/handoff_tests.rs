use crate::app::*;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_handoff::Request;

async fn settle(app: &mut App, message: Message) {
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

fn scratch_with_file() -> (tempfile::TempDir, PathBuf) {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("album");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), "a").unwrap();
    std::fs::write(folder.join("b.txt"), "b").unwrap();
    (scratch, folder)
}

#[tokio::test]
async fn a_handed_over_folder_opens_in_the_active_panel() {
    let (scratch, folder) = scratch_with_file();
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.active = PanelSide::Right;
    settle(
        &mut app,
        Message::HandedOver(vec![Request::Open(folder.clone())]),
    )
    .await;
    assert_eq!(app.panels.right.path, folder);
    assert_ne!(app.panels.left.path, folder);
    drop(scratch);
}

#[tokio::test]
async fn a_handed_over_file_opens_its_folder_with_the_cursor_on_the_file() {
    let (scratch, folder) = scratch_with_file();
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.active = PanelSide::Left;
    settle(
        &mut app,
        Message::HandedOver(vec![Request::Open(folder.join("b.txt"))]),
    )
    .await;
    assert_eq!(app.panels.left.path, folder);
    assert_eq!(
        app.panels.left.selected_entry().unwrap().name,
        std::ffi::OsString::from("b.txt")
    );
    drop(scratch);
}

#[tokio::test]
async fn a_handed_over_path_that_is_gone_changes_nothing() {
    let (scratch, folder) = scratch_with_file();
    let mut app = App::with_environment(Environment::isolated()).0;
    let before = app.panels.left.path.clone();
    settle(
        &mut app,
        Message::HandedOver(vec![Request::Open(folder.join("gone"))]),
    )
    .await;
    assert_eq!(app.panels.left.path, before);
    drop(scratch);
}

#[tokio::test]
async fn the_inbox_speaks_only_when_something_was_handed_over() {
    let (scratch, folder) = scratch_with_file();
    let mut looks = std::pin::pin!(crate::app::handoff::found_in(Some(
        scratch.path().to_path_buf()
    )));
    let nothing = tokio::time::timeout(std::time::Duration::from_millis(1200), looks.next()).await;
    assert!(nothing.is_err());
    ncrs_handoff::send(scratch.path(), &[Request::Open(folder.clone())]).unwrap();
    let Some(Message::HandedOver(found)) = looks.next().await else {
        panic!("the request was not found");
    };
    assert_eq!(found, [Request::Open(folder)]);
}
