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

fn no_offer() -> SetupOffer {
    SetupOffer {
        existing_index: None,
        model_download: false,
        hermes_was_off: false,
    }
}

fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
    FilingLocation {
        path: PathBuf::from(path),
        mode,
        images: false,
        is_target,
    }
}

fn configured() -> Filing {
    Filing {
        enabled: true,
        locations: vec![
            location("/home/test/inbox", LocationMode::Inbox, false),
            location("/home/test/target", LocationMode::Archive, true),
        ],
        ..Filing::default()
    }
}

fn overview_with_config_file(config_dir: &TempDir) -> App {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    app.config_path = Some(config_path);
    app
}

fn edit_target(edit: TargetEdit) -> Message {
    Message::HermesOverview(HermesOverviewMessage::TargetEdited(edit))
}

fn with_default_pattern(config_dir: &TempDir, pattern: &str) -> App {
    let mut app = App::with_hermes_overview(
        Some(Phase::Ready),
        Filing {
            default_pattern: Some(pattern.to_owned()),
            ..configured()
        },
        None,
    );
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!("[modules.filing]\ndefault_pattern = \"{pattern}\"\n"),
    )
    .unwrap();
    app.config_path = Some(config_path);
    app
}

#[tokio::test]
async fn a_change_of_the_standard_target_is_written_at_once_and_an_unusable_one_is_not() {
    let config_dir = TempDir::new().unwrap();
    let mut app = overview_with_config_file(&config_dir);

    settle(&mut app, edit_target(TargetEdit::Remove(0))).await;
    let written = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(written.contains("{year}/{sender}/"), "{written}");
    assert_eq!(
        app.hermes.filing_config.default_pattern.as_deref(),
        Some("{year}/{sender}/")
    );

    settle(&mut app, edit_target(TargetEdit::Remove(0))).await;
    settle(&mut app, edit_target(TargetEdit::Remove(0))).await;
    let unchanged = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(unchanged.contains("{sender}/"), "{unchanged}");
    assert_eq!(
        app.hermes.filing_config.default_pattern.as_deref(),
        Some("{sender}/")
    );
    settle(&mut app, edit_target(TargetEdit::Remove(0))).await;
    let still = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert_eq!(still, unchanged, "an empty target is not stored");
}

#[tokio::test]
async fn getting_back_to_the_built_in_target_removes_the_key() {
    let config_dir = TempDir::new().unwrap();
    let mut app = with_default_pattern(&config_dir, "{recipient}/{year}/");

    settle(
        &mut app,
        edit_target(TargetEdit::AddField(
            crate::filing_pieces::FolderField::Sender,
        )),
    )
    .await;

    let written = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(!written.contains("default_pattern"), "{written}");
    assert_eq!(app.hermes.filing_config.default_pattern, None);
}

#[tokio::test]
async fn changing_the_folders_leaves_the_overview_for_the_card() {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);

    settle(
        &mut app,
        Message::HermesOverview(HermesOverviewMessage::ChangeFolders),
    )
    .await;

    assert!(app.hermes.overview.is_none());
    assert!(app.hermes.filing_setup.is_some());
}

#[tokio::test]
async fn load_starts_the_download_on_top_of_the_overview() {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);

    drop(app.update(Message::HermesOverview(HermesOverviewMessage::LoadAiModel)));

    assert!(app.hermes.overview.is_some());
    assert!(app.settings.ai_model.is_some());
}

#[tokio::test]
async fn closing_the_card_of_a_running_download_shows_the_overview_and_keeps_the_download() {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);
    app.hermes.index_path = Some(PathBuf::from("/example/data/index.db"));
    drop(app.update(Message::HermesOverview(HermesOverviewMessage::LoadAiModel)));
    assert!(app.settings.ai_card_is_shown());

    drop(app.update(Message::AiModel(AiModelMessage::Close)));

    assert!(app.hermes.overview.is_some());
    assert!(!app.settings.ai_card_is_shown());
    assert!(app.settings.ai_model_download_runs());
    assert!(!app.overlay_apart_from_overview_is_open());
}

#[tokio::test]
async fn closing_the_folders_card_returns_to_the_overview() {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);

    drop(app.update(Message::HermesOverview(
        HermesOverviewMessage::ChangeFolders,
    )));
    assert!(app.hermes.filing_setup.is_some());
    assert!(app.hermes.overview.is_none());

    drop(app.update(Message::FilingSetup(FilingSetupMessage::Close)));
    assert!(app.hermes.filing_setup.is_none());
    assert!(app.hermes.overview.is_some());
}

#[tokio::test]
async fn closing_the_setup_card_opened_from_the_overview_returns_to_it() {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);

    drop(app.update(Message::HermesOverview(
        HermesOverviewMessage::ContinueSetup,
    )));
    assert!(app.hermes.hermes_setup.is_some());
    assert!(app.hermes.overview.is_none());

    drop(app.update(Message::HermesSetup(HermesSetupMessage::Later)));
    assert!(app.hermes.overview.is_some());
}

#[tokio::test]
async fn switching_hermes_on_without_folders_opens_the_setup() {
    let config_dir = TempDir::new().unwrap();
    let mut app = App::with_hermes_overview(None, Filing::default(), None);
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    app.config_path = Some(config_path);

    settle(
        &mut app,
        Message::HermesOverview(HermesOverviewMessage::SwitchHermesOn),
    )
    .await;

    assert!(app.hermes.hermes_setup.is_some());
}

#[tokio::test]
async fn opening_the_overview_asks_for_the_counts_and_the_answer_arrives() {
    let mut app = App::with_fixed_filing(Phase::Ready);
    assert!(app.hermes.filing_status.hermes_counts().is_none());

    let mut stream = task::into_stream(app.open_hermes_overview()).expect("a task");
    let mut reported = Vec::new();
    while let Some(action) = stream.next().await {
        if let Action::Output(message) = action {
            reported.push(message);
        }
    }

    assert!(app.hermes.overview.is_some());
    assert!(reported.iter().any(|message| matches!(
        message,
        Message::FilingStatus(FilingStatusMessage::HermesCounts(_))
    )));
}

fn overview_keys(editing: bool, sidebar: bool) -> impl Fn(Key) -> Option<Message> {
    let state = PromptKeyState {
        hermes_overview: true,
        hermes_overview_editing: editing,
        hermes_sidebar: sidebar,
        ..PromptKeyState::default()
    };
    move |key| route_key(&state, key, Modifiers::default())
}

fn overview(message: HermesOverviewMessage) -> Option<Message> {
    Some(Message::HermesOverview(message))
}

#[test]
fn enter_opens_the_review_list_only_when_something_waits() {
    let counts = ncrs_index::HermesCounts {
        open_reviews: 5,
        ..ncrs_index::HermesCounts::default()
    };
    let mut waiting =
        App::with_hermes_overview(Some(Phase::Ready), config::Filing::default(), Some(counts));
    drop(waiting.update(Message::HermesOverview(HermesOverviewMessage::ShowReviews)));
    assert!(waiting.hermes.review_list.is_some());
    assert!(waiting.hermes.overview.is_none());

    let mut clear = App::with_hermes_overview(Some(Phase::Ready), config::Filing::default(), None);
    drop(clear.update(Message::HermesOverview(HermesOverviewMessage::ShowReviews)));
    assert!(clear.hermes.review_list.is_none());
    assert!(clear.hermes.overview.is_some());
}

#[test]
fn without_the_field_the_page_has_the_letters_the_sidebar_and_enter() {
    let route = overview_keys(false, false);
    assert_eq!(
        route(Key::Named(Named::Escape)),
        overview(HermesOverviewMessage::Close)
    );
    assert_eq!(
        route(Key::Character("p".into())),
        Some(Message::FilingStatus(FilingStatusMessage::TogglePause))
    );
    assert_eq!(
        route(Key::Named(Named::Enter)),
        overview(HermesOverviewMessage::ShowReviews)
    );
    assert_eq!(route(Key::Character("2".into())), None);
    assert_eq!(route(Key::Character("e".into())), None);
    assert_eq!(
        route(Key::Named(Named::ArrowLeft)),
        Some(Message::HermesFocus(HermesFocus::Sidebar))
    );
    let in_sidebar = overview_keys(false, true);
    assert_eq!(
        in_sidebar(Key::Named(Named::ArrowDown)),
        Some(Message::HermesStep(1))
    );
    assert_eq!(
        in_sidebar(Key::Named(Named::Enter)),
        Some(Message::HermesFocus(HermesFocus::Content))
    );
}

#[test]
fn with_the_field_focused_the_letters_and_arrows_are_the_fields() {
    let route = overview_keys(true, false);
    for typed in ["p", "1", "+", "e"] {
        assert_eq!(route(Key::Character(typed.into())), None, "{typed}");
    }
    assert_eq!(route(Key::Named(Named::ArrowLeft)), None);
    assert_eq!(
        route(Key::Named(Named::Enter)),
        overview(HermesOverviewMessage::TargetEdited(
            TargetEdit::OwnNameSubmit
        ))
    );
    assert_eq!(
        route(Key::Named(Named::Escape)),
        overview(HermesOverviewMessage::TargetEdited(
            TargetEdit::OwnNameCancel
        ))
    );
}

#[test]
fn typing_an_own_folder_name_and_any_other_click_decide_who_has_the_letters() {
    let mut app = App::with_hermes_overview(Some(Phase::Ready), configured(), None);
    assert!(!app.prompt_key_state().hermes_overview_editing);

    drop(app.update(edit_target(TargetEdit::StartOwnName)));
    assert!(app.prompt_key_state().hermes_overview_editing);

    drop(app.update(Message::HermesOverview(
        HermesOverviewMessage::SetAiEnabled(false),
    )));
    assert!(!app.prompt_key_state().hermes_overview_editing);
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
async fn a_folder_picked_in_the_old_card_is_granted_and_a_refusal_is_shown() {
    let mut granted = sandboxed(App::with_filing_setup(&[], false), recording_grant);
    let picked = PathBuf::from("/home/test/picked-for-the-old-card");
    settle(
        &mut granted,
        Message::FilingSetup(FilingSetupMessage::FolderPicked {
            mode: LocationMode::Inbox,
            folder: Some(picked.clone()),
        }),
    )
    .await;
    assert!(GRANTED.lock().unwrap().contains(&picked));

    let mut refused = sandboxed(App::with_filing_setup(&[], false), refusing_grant);
    settle(
        &mut refused,
        Message::FilingSetup(FilingSetupMessage::FolderPicked {
            mode: LocationMode::Inbox,
            folder: Some(picked.clone()),
        }),
    )
    .await;
    assert!(matches!(
        refused.hermes.filing_setup.as_ref().unwrap().note(),
        Some(SetupNote::GrantFailed { .. })
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

/// The overview over a module that really opened the index at `db`.
fn overview_over_index(db: &Path, store: Arc<dyn KeyStore + Send + Sync>) -> App {
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
    let mut app = App::with_hermes_overview(None, Filing::default(), None);
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
    app.overview_facts(app.hermes.overview.as_ref().unwrap().ocr_models_present())
}

#[test]
fn encrypting_is_offered_for_a_plain_index_with_a_credential_store_only() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);

    let with_store = overview_over_index(&db, Arc::new(ncrs_index::InMemoryKeyStore::empty()));
    assert!(facts_of(&with_store).can_encrypt);

    let without_store = overview_over_index(&db, Arc::new(NoCredentialStore));
    assert!(!facts_of(&without_store).can_encrypt);
    assert!(facts_of(&without_store).can_start_over);
}

#[tokio::test]
async fn encrypting_shows_the_recovery_key_on_a_card_and_returns_to_the_overview_after() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);
    let store = Arc::new(ncrs_index::InMemoryKeyStore::empty());
    let mut app = overview_over_index(&db, Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>);

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::EncryptNow),
    )
    .await;

    let module = app.hermes.filing.as_ref().unwrap();
    assert!(matches!(module.phase(), Phase::ShowRecoveryKey { .. }));
    assert!(!ncrs_index::Index::file_is_plain(&db).unwrap());
    assert!(store.snapshot().is_some());
    assert!(
        app.hermes.overview.is_none(),
        "the card must not hide under the overview"
    );
    assert!(app.hermes.return_to_overview);
}

#[tokio::test]
async fn a_failed_encryption_keeps_the_plain_index_open_and_says_why_on_the_overview() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);
    let mut app = overview_over_index(&db, Arc::new(RefusingStore));

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::EncryptNow),
    )
    .await;

    let module = app.hermes.filing.as_ref().unwrap();
    assert_eq!(module.phase(), &Phase::Ready);
    assert!(module.protection_note().is_some());
    assert!(ncrs_index::Index::file_is_plain(&db).unwrap());
    assert!(app.hermes.overview.is_some());
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
    let mut app = overview_over_index(&db, Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>);

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
fn escape_cancels_the_question_before_anything_else_in_the_overview() {
    let prompt = PromptKeyState {
        start_over_dialog: true,
        hermes_overview: true,
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
    let mut app = overview_over_index(&db, Arc::clone(&store) as Arc<dyn KeyStore + Send + Sync>);
    app.hermes.filing_config = configured();

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
    assert!(app.hermes.overview.is_none());
    assert!(app.hermes.return_to_overview);
}

#[tokio::test]
async fn confirming_without_the_question_does_nothing() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    plain_index(&db);
    let mut app = overview_over_index(&db, Arc::new(ncrs_index::InMemoryKeyStore::empty()));

    settle(
        &mut app,
        overview_message(HermesOverviewMessage::ConfirmStartOver),
    )
    .await;

    assert!(db.exists());
    assert_eq!(app.hermes.filing.as_ref().unwrap().phase(), &Phase::Ready);
}

#[tokio::test]
async fn a_dismissed_key_card_shows_as_a_missing_key_on_the_overview() {
    let dir = TempDir::new().unwrap();
    let db = index_in(&dir);
    encrypted_index_with_sidecars(&db, &ncrs_index::InMemoryKeyStore::empty());
    let mut app = overview_over_index(&db, Arc::new(ncrs_index::InMemoryKeyStore::empty()));
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
    assert!(app.hermes.overview.is_none());
}
