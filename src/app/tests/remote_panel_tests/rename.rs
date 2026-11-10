use super::common::*;
use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_name_taken_on_the_server_keeps_the_input_open_with_the_taken_message() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/notes.txt"), "notes").unwrap();
    std::fs::write(fixture.server.local("/home/tester/taken.txt"), "taken").unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    let panel = app.panel_mut(PanelSide::Left);
    panel.selected = panel
        .entries
        .iter()
        .position(|entry| entry.name == "notes.txt")
        .unwrap();
    let _focus = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(Message::Panel(PanelMessage::RenameInput(
        "taken.txt".to_owned(),
    )));

    settle(&mut app, Message::Panel(PanelMessage::RenameSubmit)).await;

    let rename = app.panels.left.rename.clone().expect("input stays open");
    assert_eq!(
        rename.error.as_deref(),
        Some(app.lang.text(Msg::ErrorRenameTaken))
    );
    assert!(!rename.busy);
    assert_eq!(
        std::fs::read_to_string(fixture.server.local("/home/tester/taken.txt")).unwrap(),
        "taken"
    );
    assert!(fixture.server.local("/home/tester/notes.txt").exists());
}
