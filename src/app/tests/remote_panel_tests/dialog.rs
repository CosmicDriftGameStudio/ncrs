use super::common::*;
use super::*;

#[test]
fn every_action_is_refused_on_a_server_exactly_when_it_is_not_allowed() {
    let panel = PanelState::new(url("/home"));
    assert!(panel.remote().is_some());
    let refused = |message: &Message| remote_refusal(&panel, message).is_some();
    for action in KeyAction::ALL {
        assert_eq!(
            refused(&action.message()),
            !action.allowed_on_server(),
            "{action:?}"
        );
    }
    for action in ContextAction::ALL {
        assert_eq!(
            refused(&action.message()),
            !action.allowed_on_server(),
            "{action:?}"
        );
    }
    for action in HermesAction::ALL {
        assert_eq!(
            refused(&action.message()),
            !action.allowed_on_server(),
            "{action:?}"
        );
    }
}

#[test]
fn a_local_panel_refuses_nothing_for_being_local() {
    let panel = PanelState::new(std::env::temp_dir());
    for action in KeyAction::ALL {
        assert_eq!(
            remote_refusal(&panel, &action.message()),
            None,
            "{action:?}"
        );
    }
}

#[test]
fn the_refusal_says_what_works_and_why_the_command_line_does_not() {
    let panel = PanelState::new(url("/"));
    assert_eq!(
        remote_refusal(&panel, &Message::Delete { permanent: false }),
        Some(Msg::StatusRemoteReadOnly)
    );
    assert_eq!(
        remote_refusal(&panel, &Message::CommandLine(CommandLineMessage::Focus)),
        Some(Msg::StatusRemoteNoShell)
    );
}

#[test]
fn a_refused_key_on_a_server_panel_shows_the_notice_and_does_nothing() {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = url("/home");

    drop(app.update(Message::Delete { permanent: true }));

    assert!(!app.delete_dialog_is_open());
    assert_eq!(
        app.status_error(),
        Some("Files on the server can only be browsed, viewed and copied for now.")
    );
}

#[test]
fn a_server_panel_is_remembered_as_home_and_history_drops_server_paths() {
    let mut app = App::with_environment(Environment::isolated()).0;
    let local = std::env::temp_dir();
    app.panels.left.path = url("/home/tester");
    app.panels.left.history.visit(local.clone());
    app.panels.left.history.visit(url("/etc"));
    app.panels.left.history.visit(url("/home"));
    app.panels.right.path = url("/srv");
    app.panels.favourites = vec![local.clone()];

    let state = app.session_state();

    assert_eq!(state.left.path, session_state::record_path(&app.home));
    assert_eq!(state.left.cursor, None);
    assert_eq!(state.right.path, session_state::record_path(&app.home));
    assert_eq!(
        state.left.back,
        [session_state::record_path(&local).unwrap()]
    );
    let text = format!("{state:?}");
    assert!(!text.contains("sftp"), "{text}");
}

#[tokio::test]
async fn typing_an_sftp_address_opens_the_connect_dialog_without_touching_the_disk() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    drop(app.update(Message::PathFieldInput(
        "sftp://example.org:2222/srv/files".into(),
    )));

    let reported = reports(&mut app, Message::PathFieldSubmit).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(dialog.value(ConnectField::Host), "example.org");
    assert_eq!(dialog.value(ConnectField::Port), "2222");
    assert_eq!(dialog.value(ConnectField::User), "");
    assert_eq!(dialog.value(ConnectField::StartPath), "/srv/files");
    assert_eq!(app.path_field_for_test(), None);
    assert!(
        !reported
            .iter()
            .any(|message| matches!(message, Message::PathChecked { .. })),
        "no look at the disk was started: {reported:?}"
    );
}

#[tokio::test]
async fn a_bad_sftp_address_is_reported_and_leaves_the_field() {
    let mut app = App::with_environment(Environment::isolated()).0;
    drop(app.update(Message::PathFieldInput("sftp://host:99999/x".into())));

    settle(&mut app, Message::PathFieldSubmit).await;

    assert_eq!(app.status_error(), Some("Not a valid sftp:// address"));
    assert!(app.remote_ui.dialog.is_none());
    assert!(app.path_field_for_test().is_some());
}

#[tokio::test]
async fn loading_a_disconnected_server_asks_for_the_password() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);

    open(
        &mut app,
        PanelSide::Left,
        url("/home/tester"),
        Some("x".into()),
    )
    .await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(dialog.value(ConnectField::Host), HOST);
    assert_eq!(dialog.value(ConnectField::User), USER);
    assert_eq!(dialog.value(ConnectField::Password), "");
    assert!(app.panels.left.loading, "the panel waits for the answer");
    assert!(!fixture.pool.is_connected(&Fixture::id()));
    assert!(app.prompt_key_state().connect_dialog);
    assert!(app.blocking_overlay_is_open());
}

#[tokio::test]
async fn a_second_folder_while_a_dialog_is_up_is_told_there_is_no_connection() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;

    open(&mut app, PanelSide::Right, url("/home"), None).await;

    assert!(matches!(
        app.panels.right.error,
        Some(fs::ReadError::RemoteDisconnected { .. })
    ));
    assert_eq!(app.panels.right.path, url("/home"));
}

#[tokio::test]
async fn stored_password_reconnects_without_asking() {
    let fixture = Fixture::trusted();
    std::fs::write(fixture.server.local("/home/tester/a.txt"), "a").unwrap();
    let mut app = app_over(&fixture)
        .with_passwords(Arc::new(InMemoryPasswords::with(&Fixture::id(), PASSWORD)));

    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;

    assert!(app.remote_ui.dialog.is_none());
    assert!(fixture.pool.is_connected(&Fixture::id()));
    assert_eq!(app.panels.left.path, url("/home/tester"));
    assert_eq!(names(&app, PanelSide::Left), ["..", "a.txt"]);
    assert_eq!(app.panels.left.error, None);
}

#[tokio::test]
async fn a_stored_password_that_no_longer_fits_is_forgotten_and_asked_for_again() {
    let fixture = Fixture::trusted();
    let passwords = Arc::new(InMemoryPasswords::with(&Fixture::id(), "old password"));
    let mut app = app_over(&fixture).with_passwords(Arc::<InMemoryPasswords>::clone(&passwords));

    open(&mut app, PanelSide::Left, url("/home"), None).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(dialog.error(), Some("User name or password is wrong"));
    assert_eq!(dialog.value(ConnectField::Password), "");
    assert_eq!(
        passwords.get(&Fixture::id()).unwrap(),
        None,
        "the keychain no longer holds the old password"
    );
}

#[tokio::test]
async fn a_stored_password_is_kept_when_the_trouble_is_not_the_password() {
    // A server that is down says nothing about the password.
    let fixture = Fixture::untrusted();
    let passwords = Arc::new(InMemoryPasswords::with(&Fixture::id(), PASSWORD));
    let mut app = app_over(&fixture).with_passwords(Arc::<InMemoryPasswords>::clone(&passwords));

    open(&mut app, PanelSide::Left, url("/home"), None).await;

    assert!(matches!(
        app.remote_ui.dialog.as_ref().unwrap().stage(),
        remote_dialog::Stage::HostKey { .. }
    ));
    assert!(passwords.get(&Fixture::id()).unwrap().is_some());
}

#[tokio::test]
async fn a_key_of_another_type_is_refused_in_the_dialog_with_no_way_to_trust() {
    let fixture = Fixture::untrusted();
    std::fs::write(
        fixture.known.ssh.as_deref().unwrap(),
        format!(
            "test.invalid {}\n",
            crate::fs::remote::test_server::ECDSA_PUBLIC
        ),
    )
    .unwrap();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;
    settle(&mut app, Message::Remote(RemoteMessage::TrustHostKey)).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(*dialog.stage(), remote_dialog::Stage::Editing);
    let error = dialog.error().unwrap();
    assert!(
        error.contains("offers a different key type") && error.contains("Refused"),
        "{error}"
    );
    assert!(!fixture.server.control.events().contains(&"password"));
}

#[tokio::test]
async fn an_unreadable_host_key_file_is_named_and_cannot_be_trusted_past() {
    let fixture = Fixture::untrusted();
    std::fs::create_dir(fixture.known.ssh.as_deref().unwrap()).unwrap();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(*dialog.stage(), remote_dialog::Stage::Editing);
    let error = dialog.error().unwrap();
    assert!(
        error.contains("ssh_known_hosts")
            && error.contains("could not be read, so the server cannot be checked"),
        "{error}"
    );
}

#[tokio::test]
async fn a_server_without_password_login_says_so() {
    let fixture =
        Fixture::over(crate::fs::remote::test_server::TestServer::without_password_login());
    std::fs::write(
        fixture.known.own.as_deref().unwrap(),
        format!(
            "test.invalid {}\n",
            crate::fs::remote::test_server::public_key_text()
        ),
    )
    .unwrap();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert_eq!(
        app.remote_ui.dialog.as_ref().unwrap().error(),
        Some("test.invalid does not accept password sign-in")
    );
}

#[tokio::test]
async fn the_password_is_in_no_debug_text_of_a_message_either() {
    let secret = Secret::new("hunter2-the-password".to_owned());
    let fixture = Fixture::trusted();
    let failed = fixture.pool.connect(&Fixture::id(), &secret).await;
    assert_eq!(failed, Err(ConnectFailure::AuthFailed));
    let unknown = Fixture::untrusted()
        .pool
        .connect(&Fixture::id(), &secret)
        .await;
    let mut shown = String::new();
    for result in [failed, unknown] {
        shown.push_str(&format!(
            "{:?}{:?}{:?}",
            Message::Remote(RemoteMessage::Connected {
                attempt: 1,
                result: result.clone(),
            }),
            Message::Remote(RemoteMessage::AutoConnected {
                side: PanelSide::Left,
                request_id: 1,
                remote: RemotePath::new(Fixture::id(), "/"),
                select: None,
                password: secret.clone(),
                result,
            }),
            Message::Remote(RemoteMessage::Password(secret.clone())),
        ));
    }
    assert!(
        !shown.contains("hunter2"),
        "the password leaked into {shown}"
    );
    assert!(shown.contains("***"));
}

#[test]
fn no_text_of_a_failure_carries_the_password_and_each_names_its_host() {
    let app = App::with_environment(Environment::isolated()).0;
    let secret = "hunter2-the-password";
    let failures = [
        ConnectFailure::AuthFailed,
        ConnectFailure::Timeout,
        ConnectFailure::HostKeyRevoked,
        ConnectFailure::NoCommonAlgorithm,
        ConnectFailure::PasswordNotOffered,
        ConnectFailure::Unreachable("refused".to_owned()),
        ConnectFailure::Keychain("locked".to_owned()),
        ConnectFailure::HostKeyChanged {
            fingerprint: "SHA256:x".to_owned(),
            file: PathBuf::from("known_hosts"),
            line: 3,
        },
        ConnectFailure::HostKeyOtherType {
            fingerprint: "SHA256:x".to_owned(),
            file: PathBuf::from("known_hosts"),
            line: 4,
        },
        ConnectFailure::KnownHostsUnreadable {
            file: PathBuf::from("known_hosts"),
            reason: "denied".to_owned(),
        },
    ];
    for failure in failures {
        for lang in [Language::English, Language::German] {
            let mut speaking = App::with_environment(Environment::isolated()).0;
            speaking.lang = lang;
            let text = ncrs_remote::update::connect_failure_text(
                speaking.lang,
                &failure,
                "files.example.org",
            );
            assert!(
                !text.contains(secret) && !text.is_empty(),
                "{failure:?}: {text}"
            );
            let names_host = matches!(
                failure,
                ConnectFailure::AuthFailed
                    | ConnectFailure::Keychain(_)
                    | ConnectFailure::KnownHostsUnreadable { .. }
            ) || text.contains("files.example.org");
            assert!(names_host, "{failure:?}: {text}");
        }
    }
    drop(app);
}

#[tokio::test]
async fn the_dialog_connects_opens_the_home_folder_and_remembers_the_password() {
    let fixture = Fixture::trusted();
    let passwords = Arc::new(InMemoryPasswords::default());
    let mut app = app_over(&fixture).with_passwords(Arc::<InMemoryPasswords>::clone(&passwords));
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, "Test.Invalid", USER, PASSWORD).await;
    settle(&mut app, Message::Remote(RemoteMessage::ToggleRemember)).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert!(app.remote_ui.dialog.is_none());
    assert_eq!(app.panels.left.path, url(HOME));
    assert_eq!(app.panels.left.error, None);
    assert_eq!(
        app.notice_for_test(),
        Some("Connected to tester@test.invalid")
    );
    let stored = passwords.get(&Fixture::id()).unwrap().unwrap();
    assert_eq!(stored.expose(), PASSWORD);
}

#[tokio::test]
async fn without_the_tick_the_password_is_not_stored() {
    let fixture = Fixture::trusted();
    let passwords = Arc::new(InMemoryPasswords::default());
    let mut app = app_over(&fixture).with_passwords(Arc::<InMemoryPasswords>::clone(&passwords));
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert!(fixture.pool.is_connected(&Fixture::id()));
    assert_eq!(passwords.get(&Fixture::id()).unwrap(), None);
}

#[tokio::test]
async fn a_start_folder_typed_in_the_dialog_is_opened() {
    let fixture = Fixture::trusted();
    std::fs::create_dir_all(fixture.server.local("/srv/files")).unwrap();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;
    settle(
        &mut app,
        Message::Remote(RemoteMessage::Field(
            ConnectField::StartPath,
            "/srv/files".into(),
        )),
    )
    .await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert_eq!(app.panels.left.path, url("/srv/files"));
}

#[tokio::test]
async fn an_unknown_server_asks_before_the_password_goes_and_trusts_on_request() {
    let fixture = Fixture::untrusted();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    let remote_dialog::Stage::HostKey { fingerprint, .. } = dialog.stage() else {
        panic!("{:?}", dialog.stage());
    };
    assert!(fingerprint.starts_with("SHA256:"));
    assert!(!fixture.server.control.events().contains(&"password"));
    assert!(!fixture.pool.is_connected(&Fixture::id()));

    settle(&mut app, Message::Remote(RemoteMessage::TrustHostKey)).await;

    assert!(app.remote_ui.dialog.is_none());
    assert!(fixture.pool.is_connected(&Fixture::id()));
    let learned = std::fs::read_to_string(fixture.known.own.as_deref().unwrap()).unwrap();
    assert!(learned.contains("\ntest.invalid ssh-ed25519 "), "{learned}");
}

#[tokio::test]
async fn a_wrong_password_stays_in_the_dialog_with_the_reason() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, "nope").await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(*dialog.stage(), remote_dialog::Stage::Editing);
    assert_eq!(dialog.error(), Some("User name or password is wrong"));
    assert!(!format!("{dialog:?}").contains("nope"));
}

#[tokio::test]
async fn a_changed_host_key_is_refused_with_no_way_to_trust_it() {
    let fixture = Fixture::over(crate::fs::remote::test_server::TestServer::with_other_key());
    std::fs::write(
        fixture.known.own.as_deref().unwrap(),
        format!(
            "test.invalid {}\n",
            crate::fs::remote::test_server::public_key_text()
        ),
    )
    .unwrap();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    type_into(&mut app, HOST, USER, PASSWORD).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;
    settle(&mut app, Message::Remote(RemoteMessage::TrustHostKey)).await;

    let dialog = app.remote_ui.dialog.as_ref().unwrap();
    assert_eq!(*dialog.stage(), remote_dialog::Stage::Editing);
    let error = dialog.error().unwrap();
    assert!(
        error.contains("has changed") && error.contains("line 1"),
        "{error}"
    );
    assert!(!fixture.server.control.events().contains(&"password"));
}

#[tokio::test]
async fn escape_closes_the_dialog_and_the_panel_stops_waiting() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home"), None).await;
    assert!(app.panels.left.loading);

    let route = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Escape),
        Modifiers::default(),
    );
    assert_eq!(route, Some(Message::Remote(RemoteMessage::Cancel)));
    settle(&mut app, Message::Remote(RemoteMessage::Cancel)).await;

    assert!(app.remote_ui.dialog.is_none());
    assert!(!app.panels.left.loading);
}

#[tokio::test]
async fn enter_in_the_dialog_connects_and_tab_walks_the_fields() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;
    let state = app.prompt_key_state();
    let key = |named, modifiers| route_key(&state, Key::Named(named), modifiers);

    assert_eq!(
        key(Named::Enter, Modifiers::default()),
        Some(Message::Remote(RemoteMessage::Submit))
    );
    assert_eq!(
        key(Named::Tab, Modifiers::default()),
        Some(Message::Remote(RemoteMessage::FocusField {
            backwards: false
        }))
    );
    assert_eq!(
        key(Named::Tab, Modifiers::SHIFT),
        Some(Message::Remote(RemoteMessage::FocusField {
            backwards: true
        }))
    );
}

#[tokio::test]
async fn an_incomplete_dialog_does_not_connect() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    settle(&mut app, Message::Remote(RemoteMessage::OpenConnect)).await;

    settle(&mut app, Message::Remote(RemoteMessage::Submit)).await;

    assert_eq!(
        *app.remote_ui.dialog.as_ref().unwrap().stage(),
        remote_dialog::Stage::Editing
    );
    assert_eq!(fixture.server.control.dials(), 0);
}

#[tokio::test]
async fn going_up_on_a_server_cuts_the_address_as_text() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    assert_eq!(app.panels.left.path, url("/home/tester"));

    settle(&mut app, Message::Panel(PanelMessage::GoUp)).await;
    assert_eq!(app.panels.left.path, url("/home"));
    assert_eq!(app.panels.left.selected_entry().unwrap().name, "tester");

    settle(&mut app, Message::Panel(PanelMessage::GoUp)).await;
    assert_eq!(app.panels.left.path, url("/"));
    assert!(app.panels.left.entries.iter().all(|entry| !entry.is_parent));

    let before = app.panels.left.request_id;
    settle(&mut app, Message::Panel(PanelMessage::GoUp)).await;
    assert_eq!(app.panels.left.request_id, before, "the root has no parent");
    assert_eq!(app.panels.left.path, url("/"));
}

#[tokio::test]
async fn enter_on_a_folder_and_on_dot_dot_walk_a_server() {
    let fixture = Fixture::connected().await;
    std::fs::create_dir_all(fixture.server.local("/home/tester/docs")).unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    let docs = app
        .panels
        .left
        .entries
        .iter()
        .position(|e| e.name == "docs")
        .unwrap();
    app.panels.left.selected = docs;

    settle(&mut app, Message::Panel(PanelMessage::OpenSelected)).await;
    assert_eq!(app.panels.left.path, url("/home/tester/docs"));

    app.panels.left.selected = 0;
    settle(&mut app, Message::Panel(PanelMessage::OpenSelected)).await;
    assert_eq!(app.panels.left.path, url("/home/tester"));
    assert_eq!(app.panels.left.selected_entry().unwrap().name, "docs");
}

#[tokio::test]
async fn an_archive_on_a_server_is_not_opened_like_a_folder() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/pack.zip"), "not a zip").unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    app.panels.left.selected = app
        .panels
        .left
        .entries
        .iter()
        .position(|e| e.name == "pack.zip")
        .unwrap();
    let before = app.panels.left.request_id;

    settle(&mut app, Message::Panel(PanelMessage::OpenSelected)).await;

    assert_eq!(app.panels.left.request_id, before);
    assert_eq!(app.panels.left.path, url("/home/tester"));
}

#[tokio::test]
async fn history_walks_server_paths_back_and_forward() {
    let fixture = Fixture::connected().await;
    std::fs::create_dir_all(fixture.server.local("/home/tester/docs")).unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    open(&mut app, PanelSide::Left, url("/home/tester/docs"), None).await;

    settle(&mut app, Message::Panel(PanelMessage::HistoryBack)).await;
    assert_eq!(app.panels.left.path, url("/home/tester"));
    assert_eq!(app.panels.left.selected_entry().unwrap().name, "docs");

    settle(&mut app, Message::Panel(PanelMessage::HistoryForward)).await;
    assert_eq!(app.panels.left.path, url("/home/tester/docs"));
}

#[tokio::test]
async fn a_path_typed_over_a_server_folder_is_read_on_the_server() {
    let fixture = Fixture::connected().await;
    std::fs::create_dir_all(fixture.server.local("/home/tester/docs")).unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;

    for (typed, expected) in [
        ("docs", "/home/tester/docs"),
        ("..", "/home/tester"),
        ("~", HOME),
        ("/", "/"),
    ] {
        drop(app.update(Message::PathFieldInput(typed.into())));
        settle(&mut app, Message::PathFieldSubmit).await;
        assert_eq!(app.panels.left.path, url(expected), "{typed}");
        assert_eq!(app.path_field_for_test(), None);
    }
}

#[tokio::test]
async fn the_command_line_does_not_run_on_a_server() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home"), None).await;
    drop(app.update(Message::CommandLine(CommandLineMessage::Input(
        "touch x".into(),
    ))));

    settle(&mut app, Message::CommandLine(CommandLineMessage::Submit)).await;

    assert_eq!(
        app.status_error(),
        Some("The command line does not run on a server")
    );
    assert!(!app.jobs.is_busy());
}

#[tokio::test]
async fn copy_path_on_a_server_is_the_address() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/a.txt"), "a").unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    app.panels.left.selected = app
        .panels
        .left
        .entries
        .iter()
        .position(|e| e.name == "a.txt")
        .unwrap();

    let copied =
        ncrs_ops::clipboard_text(rows_of(app.panel(PanelSide::Left)), ClipboardKind::Path).unwrap();

    assert_eq!(copied.text, "sftp://tester@test.invalid/home/tester/a.txt");
}

#[tokio::test]
async fn the_drive_menu_lists_connected_servers_and_delete_disconnects() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture).with_volumes(Vec::new);
    open(&mut app, PanelSide::Left, url("/home"), None).await;

    drop(app.update(Message::VolumeMenu(PanelSide::Left)));
    let menu = app.panels.volume_menu.as_ref().unwrap();
    assert_eq!(menu.servers.len(), 1);
    assert_eq!(menu.servers[0].label, "tester@test.invalid");
    assert!(menu.servers[0].connected);
    assert_eq!(menu.selected, 0, "a panel on a server opens the menu on it");

    settle(&mut app, Message::VolumeMenuRemove).await;

    assert!(!fixture.pool.is_connected(&Fixture::id()));
    assert!(
        app.panels
            .volume_menu
            .as_ref()
            .is_some_and(|open| open.servers.is_empty()),
        "the trash row keeps the menu open"
    );
    assert_eq!(app.panels.left.path, app.home, "the panel went home");
    assert!(app
        .notice_for_test()
        .unwrap()
        .starts_with("Disconnected from"));
}

#[tokio::test]
async fn choosing_a_server_in_the_drive_menu_opens_its_root() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture).with_volumes(Vec::new);
    drop(app.update(Message::VolumeMenu(PanelSide::Right)));

    settle(&mut app, Message::VolumeMenuActivate).await;

    assert_eq!(app.panels.right.path, url("/"));
    assert_eq!(app.panels.right.error, None);
}

#[tokio::test]
async fn the_disconnect_command_without_a_server_says_so() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);

    settle(&mut app, KeyAction::DisconnectServer.message()).await;

    assert_eq!(app.notice_for_test(), Some("This panel is not on a server"));
}

#[tokio::test]
async fn the_disconnect_command_closes_the_connection_of_the_active_panel() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home"), None).await;

    settle(&mut app, KeyAction::DisconnectServer.message()).await;

    assert!(!fixture.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn quit_closes_all_connections() {
    let fixture = Fixture::connected().await;
    let mut app = app_over(&fixture);

    drop(app.update(Message::Quit));

    assert!(!fixture.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn the_connect_commands_are_in_the_command_list() {
    let listed: Vec<KeyAction> = crate::commands::all()
        .map(|command| command.action)
        .collect();
    assert!(listed.contains(&KeyAction::ConnectServer));
    assert!(listed.contains(&KeyAction::DisconnectServer));
}

#[tokio::test]
async fn the_connect_command_opens_the_dialog_and_is_blocked_by_another_one() {
    let fixture = Fixture::trusted();
    let mut app = app_over(&fixture);
    app.open_delete_dialog_for_test(1, false);

    settle(&mut app, KeyAction::ConnectServer.message()).await;
    assert!(app.remote_ui.dialog.is_none());

    app.ops.delete_dialog = None;
    settle(&mut app, KeyAction::ConnectServer.message()).await;
    assert!(app.remote_ui.dialog.is_some());
}
