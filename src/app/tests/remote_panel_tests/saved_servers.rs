use super::common::*;
use super::*;

fn saved(name: Option<&str>, host: &str, path: Option<&str>) -> ServerEntry {
    ServerEntry {
        name: name.map(str::to_owned),
        host: host.to_owned(),
        port: 22,
        user: USER.to_owned(),
        path: path.map(str::to_owned),
    }
}

#[tokio::test]
async fn drive_menu_lists_saved_servers_and_delete_disconnects() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture).with_volumes(Vec::new);
    app.saved_servers = vec![
        saved(Some("Work"), HOST, Some("/home")),
        saved(None, "elsewhere.invalid", None),
    ];
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));

    let menu = app.panels.volume_menu.as_ref().unwrap();
    let shown: Vec<(&str, bool)> = menu
        .servers
        .iter()
        .map(|row| (row.label.as_str(), row.connected))
        .collect();
    assert_eq!(shown, [("Work", true), ("tester@elsewhere.invalid", false)]);
    assert_eq!(menu.servers[0].url, url("/home"));

    settle(&mut app, Message::VolumeMenuRemove).await;

    assert!(!fixture.pool.is_connected(&Fixture::id()));
    let kept = app
        .panels
        .volume_menu
        .as_ref()
        .expect("a saved server stays listed");
    assert!(!kept.servers[0].connected);
    assert_eq!(kept.servers.len(), 2);
}

#[tokio::test]
async fn choosing_a_saved_server_that_is_not_connected_asks_for_its_password() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture).with_volumes(Vec::new);
    app.saved_servers = vec![saved(Some("Work"), HOST, Some("/home"))];
    drop(app.update(Message::VolumeMenu(PanelSide::Right)));

    settle(&mut app, Message::VolumeMenuActivate).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(dialog.value(ConnectField::Host), HOST);
    assert_eq!(dialog.value(ConnectField::User), USER);
    assert_eq!(
        dialog.then().map(|then| then.path.clone()),
        Some(url("/home"))
    );
}

#[tokio::test]
async fn a_saved_server_opens_from_the_settings_in_the_active_panel() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture);
    app.panels.active = PanelSide::Right;

    settle(
        &mut app,
        Message::Remote(RemoteMessage::OpenServer(url("/home"))),
    )
    .await;

    assert_eq!(app.panels.right.path, url("/home"));
    assert_eq!(app.panels.right.error, None);
}

#[tokio::test]
async fn adding_from_the_settings_opens_the_dialog_with_the_save_box_ticked() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);

    settle(&mut app, Message::Remote(RemoteMessage::OpenAdd)).await;

    assert!(app.remote_ui.dialog.as_ref().unwrap().save());
    assert!(!app.remote_ui.dialog.as_ref().unwrap().remember());
}

#[tokio::test]
async fn saving_through_the_dialog_writes_the_server_and_never_the_password() {
    let fixture = Fixture::trusted();
    let config = tempfile::tempdir().unwrap();
    let config_path = config.path().join("config.toml");
    std::fs::write(&config_path, "# mine\nlang = \"en\"\n").unwrap();
    let mut app = app_over(&fixture);
    app.config_path = Some(config_path.clone());
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;
    settle(&mut app, Message::Remote(RemoteMessage::ToggleSave)).await;
    settle(
        &mut app,
        Message::Remote(RemoteMessage::Field(
            ConnectField::StartPath,
            "/home".into(),
        )),
    )
    .await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    let text = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        text.contains("# mine") && text.contains("[[servers]]"),
        "{text}"
    );
    assert!(text.contains("host = \"test.invalid\"") && text.contains("path = \"/home\""));
    assert!(!text.contains(PASSWORD), "{text}");
    assert_eq!(app.saved_servers, [saved(None, HOST, Some("/home"))]);
    assert_eq!(app.panels.left.path, url("/home"));
}

#[tokio::test]
async fn saving_a_server_that_is_saved_already_does_not_list_it_twice() {
    let fixture = Fixture::trusted();
    let config = tempfile::tempdir().unwrap();
    let config_path = config.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[[servers]]\nname = \"Work\"\nhost = \"test.invalid\"\nuser = \"tester\"\n",
    )
    .unwrap();
    let mut app = app_over(&fixture);
    app.config_path = Some(config_path.clone());
    app.saved_servers = vec![saved(Some("Work"), HOST, None)];
    settle(&mut app, Message::Remote(RemoteMessage::OpenAdd)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert_eq!(app.saved_servers, [saved(Some("Work"), HOST, None)]);
    let text = std::fs::read_to_string(&config_path).unwrap();
    assert_eq!(text.matches("[[servers]]").count(), 1, "{text}");
}

#[tokio::test]
async fn saving_without_a_config_file_says_so_and_still_connects() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenAdd)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert!(fixture.pool.is_connected(&Fixture::id()));
    assert!(app.saved_servers.is_empty());
    assert!(app.status_error().is_some());
}

#[tokio::test]
async fn removing_a_server_in_the_settings_forgets_its_password_too() {
    let fixture = Fixture::trusted();
    let config = tempfile::tempdir().unwrap();
    let config_path = config.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[[servers]]\nhost = \"test.invalid\"\nuser = \"tester\"\n\n[[servers]]\nhost = \"keep.invalid\"\nuser = \"tester\"\n",
    )
    .unwrap();
    let passwords = Arc::new(InMemoryPasswords::with(&Fixture::id(), PASSWORD));
    let mut app = app_over(&fixture).with_passwords(Arc::<InMemoryPasswords>::clone(&passwords));
    app.config_path = Some(config_path.clone());
    app.saved_servers = vec![saved(None, HOST, None), saved(None, "keep.invalid", None)];
    let kept = ServerId::new(USER, "keep.invalid", 22);
    passwords
        .set(&kept, &Secret::new("other".to_owned()))
        .unwrap();

    settle(
        &mut app,
        Message::Settings(SettingsMessage::Change(SettingChange::Servers(vec![
            saved(None, "keep.invalid", None),
        ]))),
    )
    .await;

    assert_eq!(app.saved_servers, [saved(None, "keep.invalid", None)]);
    assert_eq!(passwords.get(&Fixture::id()).unwrap(), None);
    assert!(passwords.get(&kept).unwrap().is_some());
    let text = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        !text.contains("test.invalid") && text.contains("keep.invalid"),
        "{text}"
    );
}

#[tokio::test]
async fn the_servers_of_the_config_are_there_at_the_start() {
    let config = tempfile::tempdir().unwrap();
    let config_path = config.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[[servers]]\nname = \"Work\"\nhost = \"files.example.org\"\nuser = \"marc\"\n",
    )
    .unwrap();
    let loaded = config::load(&config_path);

    let app = App::with_environment(Environment::isolated())
        .0
        .with_config(loaded);

    assert_eq!(app.saved_servers.len(), 1);
    assert_eq!(app.saved_servers[0].label(), "Work");
}
