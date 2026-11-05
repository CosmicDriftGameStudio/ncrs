//! The System page shows this computer, also while a panel is on a server:
//! the jumps to a folder leave the server for the local disk.

use super::common::*;
use super::*;
use ncrs_monitor::MonitorMessage;

/// A panel on the server and the System page open with a measurement on it.
async fn page_over_a_server_panel(fixture: &Fixture) -> App {
    let mut app = app_over(fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    assert_eq!(app.panel(PanelSide::Left).path, url("/home/tester"));

    settle(&mut app, Message::Monitor(MonitorMessage::Toggle)).await;
    app
}

/// Puts the cursor on this test's own process and says where its program is.
/// The App Store build measures no tasks.
#[cfg(not(feature = "sandbox"))]
async fn cursor_on_own_process(app: &mut App) -> std::path::PathBuf {
    let own = std::process::id();
    settle(app, Message::Monitor(MonitorMessage::SelectRow(own))).await;
    app.monitor_for_test()
        .snapshot()
        .and_then(|snapshot| snapshot.tasks.iter().find(|task| task.pid == own))
        .and_then(|task| task.program.clone())
        .expect("the measurement knows this process and where its program is")
}

#[cfg(not(feature = "sandbox"))]
fn assert_landed_locally(app: &App, program: &std::path::Path) {
    let folder = program.parent().expect("the folder of the program");
    let file = program.file_name().expect("the program's file name");
    assert!(!app.monitor_for_test().is_open(), "the page stayed open");
    assert_eq!(app.panel(PanelSide::Left).path, folder);
    assert_eq!(
        app.panel(PanelSide::Left)
            .selected_entry()
            .map(|entry| entry.name.clone()),
        Some(file.to_owned()),
        "the cursor is not on the program"
    );
}

#[cfg(not(feature = "sandbox"))]
async fn press(app: &mut App, key: Named, modifiers: Modifiers) {
    let message = route_key(&app.prompt_key_state(), Key::Named(key), modifiers)
        .expect("the key means something on the page");
    settle(app, message).await;
}

#[cfg(not(feature = "sandbox"))]
#[tokio::test(flavor = "multi_thread")]
async fn go_to_folder_by_mouse_leaves_the_server() {
    let fixture = Fixture::connected().await;
    let mut app = page_over_a_server_panel(&fixture).await;
    let program = cursor_on_own_process(&mut app).await;
    let own = std::process::id();

    settle(&mut app, Message::Monitor(MonitorMessage::OpenMenu(own))).await;
    assert!(app.monitor_for_test().menu_is_open());
    // Kill, Copy command line, Go to folder.
    settle(&mut app, Message::Monitor(MonitorMessage::MenuChoose(2))).await;

    assert_landed_locally(&app, &program);
}

#[cfg(not(feature = "sandbox"))]
#[tokio::test(flavor = "multi_thread")]
async fn go_to_folder_by_key_leaves_the_server() {
    let fixture = Fixture::connected().await;
    let mut app = page_over_a_server_panel(&fixture).await;
    let program = cursor_on_own_process(&mut app).await;

    press(&mut app, Named::F10, Modifiers::SHIFT).await;
    assert!(app.monitor_for_test().menu_is_open());
    press(&mut app, Named::ArrowDown, Modifiers::empty()).await;
    press(&mut app, Named::ArrowDown, Modifiers::empty()).await;
    press(&mut app, Named::Enter, Modifiers::empty()).await;

    assert_landed_locally(&app, &program);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_click_on_a_drive_leaves_the_server() {
    let fixture = Fixture::connected().await;
    let mut app = page_over_a_server_panel(&fixture).await;
    let mount = std::env::temp_dir();

    settle(
        &mut app,
        Message::Monitor(MonitorMessage::OpenDrive(mount.clone())),
    )
    .await;

    assert_eq!(app.panel(PanelSide::Left).path, mount);
    assert!(!app.monitor_for_test().is_open());
}
