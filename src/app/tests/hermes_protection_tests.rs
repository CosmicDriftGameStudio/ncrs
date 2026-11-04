use std::sync::Mutex;

use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_intake::LocationMode;
use tempfile::TempDir;

use crate::app::*;
use crate::config::FilingLocation;
use crate::filing_setup::SetupNote;
use crate::hermes_setup::{FolderSlot, SetupOffer};

static GRANTED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn recording_grant(_: &Path, folder: &Path) -> Result<(), String> {
    let mut recorded = GRANTED.lock().map_err(|err| err.to_string())?;
    recorded.push(folder.to_path_buf());
    Ok(())
}

fn refusing_grant(_: &Path, _: &Path) -> Result<(), String> {
    Err("bookmark refused".to_owned())
}

fn sandboxed(mut app: App, grant: fn(&Path, &Path) -> Result<(), String>) -> App {
    app.sandbox = fs::SandboxAccess {
        active: true,
        grants_dir: Some(PathBuf::from("grants-of-this-test")),
        home_readable: || true,
        grant,
        covers: |_, _| true,
    };
    app
}

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

fn no_offer() -> SetupOffer {
    SetupOffer {
        existing_index: None,
        model_download: false,
        hermes_was_off: false,
    }
}

#[tokio::test]
async fn a_folder_picked_in_the_setup_card_is_granted_in_the_sandbox() {
    let mut app = sandboxed(App::with_hermes_setup(&[], no_offer()), recording_grant);
    let picked = PathBuf::from("/home/test/picked-for-the-setup");

    settle(
        &mut app,
        Message::HermesSetup(HermesSetupMessage::FolderPicked {
            slot: FolderSlot::Inbox,
            folder: Some(picked.clone()),
        }),
    )
    .await;

    assert!(GRANTED.lock().unwrap().contains(&picked));
}

#[tokio::test]
async fn a_refused_grant_stays_as_a_sentence_in_the_setup_card() {
    let mut app = sandboxed(App::with_hermes_setup(&[], no_offer()), refusing_grant);
    let picked = PathBuf::from("/home/test/refused-for-the-setup");

    settle(
        &mut app,
        Message::HermesSetup(HermesSetupMessage::FolderPicked {
            slot: FolderSlot::Inbox,
            folder: Some(picked.clone()),
        }),
    )
    .await;

    assert!(matches!(
        app.hermes.hermes_setup.as_ref().unwrap().note(),
        Some(SetupNote::GrantFailed { folder, reason })
            if *folder == picked && reason == "bookmark refused"
    ));
}

#[tokio::test]
async fn without_a_grants_folder_nothing_is_granted() {
    let mut app = App::with_hermes_setup(&[], no_offer());
    app.sandbox.grants_dir = None;
    settle(
        &mut app,
        Message::HermesSetup(HermesSetupMessage::FolderPicked {
            slot: FolderSlot::Inbox,
            folder: Some(PathBuf::from("/home/test/not-granted")),
        }),
    )
    .await;
    assert!(!GRANTED
        .lock()
        .unwrap()
        .contains(&PathBuf::from("/home/test/not-granted")));
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
    assert!(app.hermes.hermes_setup.is_none());

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
async fn confirming_removes_the_index_files_and_the_key_and_opens_the_setup_filled() {
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
    assert!(!db.exists());
    assert!(!db.with_file_name("index.db.encrypting").exists());
    assert!(db.with_file_name("index.db.bak").exists());
    assert_eq!(store.snapshot(), None);
    assert!(app.hermes.filing.as_ref().unwrap().asks_encryption());
    let setup = app
        .hermes
        .hermes_setup
        .as_ref()
        .expect("the setup card opens");
    assert!(setup.can_start(), "the folders are still filled in");
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

