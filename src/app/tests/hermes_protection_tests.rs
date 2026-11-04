use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_intake::LocationMode;
use tempfile::TempDir;

use crate::app::*;
use crate::config::FilingLocation;

async fn settle(app: &mut App, message: Message) {
    let mut queue = vec![message];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(app.update(next)) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    queue.push(reported);
                }
            }
        }
    }
}

fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
    FilingLocation {
        path: PathBuf::from(path),
        mode,
        images: false,
        is_target,
        transfer: Default::default(),
    }
}

#[derive(Debug)]
struct NoCredentialStore;

impl KeyStore for NoCredentialStore {
    fn load(&self) -> Result<Option<Vec<u8>>, ncrs_index::KeyStoreError> {
        Err(ncrs_index::KeyStoreError::Unavailable("none".to_owned()))
    }

    fn store_new(&self, _key: &[u8]) -> Result<(), ncrs_index::KeyStoreError> {
        Err(ncrs_index::KeyStoreError::Unavailable("none".to_owned()))
    }

    fn replace(&self, _key: &[u8]) -> Result<(), ncrs_index::KeyStoreError> {
        Err(ncrs_index::KeyStoreError::Unavailable("none".to_owned()))
    }

    fn delete(&self) -> Result<(), ncrs_index::KeyStoreError> {
        Ok(())
    }
}

/// Finds nothing, and cannot keep a new key.
#[derive(Debug)]
struct RefusingStore;

impl KeyStore for RefusingStore {
    fn load(&self) -> Result<Option<Vec<u8>>, ncrs_index::KeyStoreError> {
        Ok(None)
    }

    fn store_new(&self, _key: &[u8]) -> Result<(), ncrs_index::KeyStoreError> {
        Err(ncrs_index::KeyStoreError::Unavailable("locked".to_owned()))
    }

    fn replace(&self, _key: &[u8]) -> Result<(), ncrs_index::KeyStoreError> {
        Err(ncrs_index::KeyStoreError::Unavailable("locked".to_owned()))
    }

    fn delete(&self) -> Result<(), ncrs_index::KeyStoreError> {
        Ok(())
    }
}

fn index_in(dir: &TempDir) -> PathBuf {
    dir.path().join("data").join("index.db")
}

/// Hermes over a module that really opened the index at `db`.
fn app_over_index(db: &Path, store: Arc<dyn KeyStore + Send + Sync>) -> App {
    let (mut module, start) = FilingModule::start(
        &Filing {
            enabled: true,
            ..Filing::default()
        },
        Some(db.to_path_buf()),
        Arc::clone(&store),
    )
    .unwrap();
    if let Some(opening) = start {
        drop(module.update(opening.run()));
    }
    let mut app = App::with_fixed_panels();
    app.hermes.filing = Some(module);
    app.hermes.index_path = Some(db.to_path_buf());
    app.hermes.key_store = Some(ncrs_hermes_ui::KeyStoreHandle(store));
    app
}

fn plain_index(db: &Path) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    drop(ncrs_index::Index::create_plain(db).unwrap());
}

fn overview_message(message: HermesOverviewMessage) -> Message {
    Message::HermesOverview(message)
}

fn facts_of(app: &App) -> OverviewFacts {
    app.overview_facts(false)
}

#[test]
fn encrypting_is_offered_for_a_plain_index_with_a_credential_store_only() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);

    let with_store = app_over_index(&db, Arc::new(ncrs_index::InMemoryKeyStore::empty()));
    assert!(facts_of(&with_store).can_encrypt);

    let without_store = app_over_index(&db, Arc::new(NoCredentialStore));
    assert!(!facts_of(&without_store).can_encrypt);
    assert!(facts_of(&without_store).can_start_over);
}

#[tokio::test]
async fn encrypting_shows_the_recovery_key_on_a_card() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);
    let store = Arc::new(ncrs_index::InMemoryKeyStore::empty());
    let mut app = app_over_index(&db, Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>);

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::EncryptNow),
    )
    .await;

    let module = app.hermes.filing.as_ref().unwrap();
    assert!(matches!(module.phase(), Phase::ShowRecoveryKey { .. }));
    assert!(!ncrs_index::Index::file_is_plain(&db).unwrap());
    assert!(store.snapshot().is_some());
}

#[tokio::test]
async fn a_failed_encryption_keeps_the_plain_index_open_and_says_why() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);
    let mut app = app_over_index(&db, Arc::new(RefusingStore));

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::EncryptNow),
    )
    .await;

    let module = app.hermes.filing.as_ref().unwrap();
    assert_eq!(module.phase(), &Phase::Ready);
    assert!(module.protection_note().is_some());
    assert!(ncrs_index::Index::file_is_plain(&db).unwrap());
    assert!(facts_of(&app).protection_note.is_some());
    assert!(facts_of(&app).can_encrypt, "the user may try again");
}

fn encrypted_index_with_sidecars(db: &Path, store: &ncrs_index::InMemoryKeyStore) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    drop(ncrs_index::Index::create_encrypted(db, store).unwrap());
    std::fs::write(db.with_file_name("index.db.encrypting"), b"half").unwrap();
    std::fs::write(db.with_file_name("index.db.bak"), b"mine").unwrap();
}

#[tokio::test]
async fn starting_over_asks_first_and_cancelling_removes_nothing() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    let store = Arc::new(ncrs_index::InMemoryKeyStore::empty());
    encrypted_index_with_sidecars(&db, &store);
    let mut app = app_over_index(&db, Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>);

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::AskStartOver),
    )
    .await;
    assert!(app.start_over_dialog);
    assert_eq!(app.dialog_button_count(), 2);
    assert_eq!(
        app.dialog_focus,
        crate::ui::hermes_overview::START_OVER_CANCEL_FOCUS
    );
    settle(
        &mut app,
        overview_message(HermesOverviewMessage::CancelStartOver),
    )
    .await;

    assert!(!app.start_over_dialog);
    assert!(db.exists() && db.with_file_name("index.db.encrypting").exists());
    assert!(store.snapshot().is_some());

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::AskStartOver),
    )
    .await;
    drop(app.activate_dialog_button());
    assert!(
        !app.start_over_dialog,
        "Enter on the default button cancels"
    );
    assert!(db.exists());
}

#[test]
fn escape_cancels_the_question_before_anything_else() {
    let prompt = PromptKeyState {
        start_over_dialog: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        card_close_message(&prompt),
        Some(overview_message(HermesOverviewMessage::CancelStartOver))
    );
}

#[tokio::test]
async fn confirming_removes_the_index_files_and_the_key_and_starts_afresh_in_the_chat() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    let store = Arc::new(ncrs_index::InMemoryKeyStore::empty());
    encrypted_index_with_sidecars(&db, &store);
    let mut app = app_over_index(&db, Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>);
    app.hermes.filing_config = Filing {
        enabled: true,
        locations: vec![
            location("/home/test/inbox", LocationMode::Inbox, false),
            location("/home/test/target", LocationMode::Archive, true),
        ],
        ..Filing::default()
    };

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::AskStartOver),
    )
    .await;
    settle(
        &mut app,
        overview_message(HermesOverviewMessage::ConfirmStartOver),
    )
    .await;

    assert!(!app.start_over_dialog);
    assert!(!db.with_file_name("index.db.encrypting").exists());
    assert!(db.with_file_name("index.db.bak").exists());
    assert!(app.hermes_chat.open, "the chat opens");
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::ShowRecoveryKey { .. }
    ));
    assert!(store.snapshot().is_some(), "a new key was kept");
    assert_eq!(
        app.hermes.filing_config.locations.len(),
        2,
        "the config locations stay"
    );
}

#[tokio::test]
async fn confirming_without_the_question_does_nothing() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);
    let mut app = app_over_index(&db, Arc::new(ncrs_index::InMemoryKeyStore::empty()));

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::ConfirmStartOver),
    )
    .await;

    assert!(db.exists());
    assert_eq!(app.hermes.filing.as_ref().unwrap().phase(), &Phase::Ready);
}

#[tokio::test]
async fn a_dismissed_key_card_shows_as_a_missing_key() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    encrypted_index_with_sidecars(&db, &ncrs_index::InMemoryKeyStore::empty());
    let mut app = app_over_index(&db, Arc::new(ncrs_index::InMemoryKeyStore::empty()));
    drop(app.update(Message::Filing(FilingMessage::Dismiss)));

    let facts = facts_of(&app);
    assert_eq!(facts.module, ModuleState::KeyMissing);
    assert_eq!(facts.protection, Protection::Encrypted);
    assert!(facts.can_start_over);

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::EnterRecoveryKey),
    )
    .await;
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::AskRecoveryKey { .. }
    ));
}

async fn settle_task(app: &mut App, first: Task<Message>) {
    let mut queue = vec![first];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(next) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    queue.push(app.update(reported));
                }
            }
        }
    }
}

fn environment_in(dir: &TempDir, store: Arc<dyn KeyStore + Send + Sync>) -> Environment {
    Environment {
        config_path: Some(dir.path().join("config.toml")),
        index_path: Some(index_in(dir)),
        key_store: store,
        ..Environment::isolated()
    }
}

async fn started_in(environment: Environment) -> App {
    let (mut app, start) = App::restored(environment, SessionState::default());
    settle_task(&mut app, start).await;
    app
}

fn config_in(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join("config.toml")).unwrap_or_default()
}

fn phase_of(app: &App) -> &Phase {
    app.hermes.filing.as_ref().unwrap().phase()
}

#[tokio::test]
async fn the_first_start_turns_hermes_on_and_makes_an_encrypted_index() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(ncrs_index::InMemoryKeyStore::empty());
    let app = started_in(environment_in(
        &dir,
        Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>,
    ))
    .await;

    assert!(
        config_in(&dir).contains("enabled = true"),
        "{}",
        config_in(&dir)
    );
    assert!(!ncrs_index::Index::file_is_plain(&index_in(&dir)).unwrap());
    assert!(store.snapshot().is_some());
    assert!(app.hermes.filing_config.locations.is_empty());
    assert!(
        matches!(phase_of(&app), Phase::ShowRecoveryKey { .. }),
        "the new key is shown once"
    );
}

#[tokio::test]
async fn the_chat_greets_while_hermes_starts_and_the_first_folder_becomes_a_location() {
    let dir = TempDir::new().unwrap();
    let mut app = started_in(environment_in(
        &dir,
        Arc::new(ncrs_index::InMemoryKeyStore::empty()),
    ))
    .await;
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    assert!(app.hermes_chat.open);
    assert!(app.hermes_chat.waiting_for_hermes);
    assert_eq!(app.hermes_chat.thread().len(), 1);
    assert!(
        app.hermes_chat
            .chips()
            .iter()
            .any(|chip| chip.reply == ncrs_hermes_chat::ChipReply::PickFolder),
        "{:?}",
        app.hermes_chat.thread()
    );
    assert!(app
        .hermes_chat
        .chips()
        .iter()
        .all(|chip| chip.reply != ncrs_hermes_chat::ChipReply::TurnOn));

    let Phase::ShowRecoveryKey {
        group: shown_group, ..
    } = phase_of(&app)
    else {
        panic!("the recovery key is not shown");
    };
    let group = *shown_group;
    let shown = app
        .hermes
        .filing
        .as_ref()
        .unwrap()
        .recovery_key_text()
        .unwrap();
    let part = shown.split('-').nth(group).unwrap().to_owned();
    settle(
        &mut app,
        Message::Filing(FilingMessage::GroupInput(part.as_str().into())),
    )
    .await;
    settle(&mut app, Message::Filing(FilingMessage::GroupSubmit)).await;
    assert_eq!(phase_of(&app), &Phase::Ready);

    let documents = dir.path().join("Documents");
    std::fs::create_dir(&documents).unwrap();
    let task = app.run_hermes_chat_action(ncrs_hermes_chat::Action::SetFolderRole {
        folder: documents,
        role: FolderRole::FilingLocation,
    });
    settle_task(&mut app, task).await;
    let [location] = app.hermes.filing_config.locations.as_slice() else {
        panic!(
            "one location expected: {:?}",
            app.hermes.filing_config.locations
        );
    };
    assert!(location.is_target && location.path.ends_with("Documents"));
    assert!(config_in(&dir).contains("Documents"), "{}", config_in(&dir));
}

#[tokio::test]
async fn hermes_switched_off_stays_off_and_the_chat_offers_to_turn_it_on() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[modules.filing]\nenabled = false\n",
    )
    .unwrap();
    let mut app = started_in(environment_in(
        &dir,
        Arc::new(ncrs_index::InMemoryKeyStore::empty()),
    ))
    .await;
    assert!(app.hermes.filing.is_none());
    assert!(config_in(&dir).contains("enabled = false"));
    assert!(!index_in(&dir).exists());

    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    let turn_on = app
        .hermes_chat
        .chips()
        .iter()
        .find(|chip| chip.reply == ncrs_hermes_chat::ChipReply::TurnOn)
        .map(|chip| {
            Message::HermesChat(HermesChatMessage::ChipPressed {
                label: chip.label.clone(),
                reply: chip.reply.clone(),
            })
        })
        .expect("a chip turns Hermes on");
    settle(&mut app, turn_on).await;

    assert!(
        config_in(&dir).contains("enabled = true"),
        "{}",
        config_in(&dir)
    );
    assert!(app.hermes.filing.is_some());
}

#[tokio::test]
async fn a_restart_keeps_a_plain_index_plain() {
    let dir = TempDir::new().unwrap();
    plain_index(&index_in(&dir));
    std::fs::write(
        dir.path().join("config.toml"),
        "[modules.filing]\nenabled = true\n",
    )
    .unwrap();
    let app = started_in(environment_in(
        &dir,
        Arc::new(ncrs_index::InMemoryKeyStore::empty()),
    ))
    .await;

    assert_eq!(phase_of(&app), &Phase::Ready);
    assert!(ncrs_index::Index::file_is_plain(&index_in(&dir)).unwrap());
    assert_eq!(facts_of(&app).protection, Protection::Plain);
}

#[tokio::test]
async fn an_index_whose_key_is_missing_asks_for_it_by_itself() {
    let dir = TempDir::new().unwrap();
    encrypted_index_with_sidecars(&index_in(&dir), &ncrs_index::InMemoryKeyStore::empty());
    std::fs::write(
        dir.path().join("config.toml"),
        "[modules.filing]\nenabled = true\n",
    )
    .unwrap();
    let app = started_in(environment_in(
        &dir,
        Arc::new(ncrs_index::InMemoryKeyStore::empty()),
    ))
    .await;

    assert!(matches!(phase_of(&app), Phase::AskRecoveryKey { .. }));
    assert!(app.hermes.filing.as_ref().unwrap().has_card());
}

#[tokio::test]
async fn a_key_store_that_refuses_ends_in_the_error_card_not_in_a_plain_index() {
    let dir = TempDir::new().unwrap();
    let app = started_in(environment_in(&dir, Arc::new(RefusingStore))).await;

    assert!(matches!(phase_of(&app), Phase::Failed(_)));
    assert!(app.hermes.filing.as_ref().unwrap().has_card());
    assert!(
        !index_in(&dir).exists() || !ncrs_index::Index::file_is_plain(&index_in(&dir)).unwrap()
    );
}
