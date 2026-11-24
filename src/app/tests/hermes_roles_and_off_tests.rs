use crate::app::*;
use crate::context_menu::{HermesAction, SubEntry};
use crate::messages::SettingsFeature;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_config::config::FilingLocation;

const LEFT: PanelSide = PanelSide::Left;
// Rows of the fixed left panel: `..`, then "Documents", a directory.
const DOCUMENTS: usize = 1;

/// Delivers `message` and everything the tasks it starts report.
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

fn ready_hermes_without_locations(config_dir: &tempfile::TempDir) -> App {
    let mut app = App::with_fixed_panels();
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    app.config_path = Some(config_path);
    // The fixed panels hold Unix paths, which are not absolute on Windows.
    let documents = config_dir.path().join("Documents");
    std::fs::create_dir(&documents).unwrap();
    app.panel_mut(LEFT).entries[DOCUMENTS].path = documents;
    app
}

fn submenu_items(app: &mut App) -> Vec<(HermesAction, bool)> {
    drop(app.update(Message::Files(MenuMessage::ContextMenuAt {
        side: LEFT,
        index: DOCUMENTS,
    })));
    let menu = app.panels.context_menu.as_mut().unwrap();
    menu.select_last();
    assert!(menu.selected_is_hermes());
    menu.open_submenu();
    menu.sub_entries()
        .iter()
        .filter_map(|entry| match entry {
            SubEntry::Item { action, enabled } => Some((*action, *enabled)),
            SubEntry::Separator => None,
        })
        .collect()
}

#[test]
fn a_folder_can_get_a_role_before_any_location_is_set() {
    let config = tempfile::tempdir().unwrap();
    let mut app = ready_hermes_without_locations(&config);

    let items = submenu_items(&mut app);

    let enabled = |wanted| items.iter().any(|&(action, on)| action == wanted && on);
    assert!(!enabled(HermesAction::FileOnce));
    assert!(!enabled(HermesAction::FileContinuously));
    assert!(enabled(HermesAction::SetFilingLocation));
    assert!(enabled(HermesAction::SetArchive));
    assert!(!enabled(HermesAction::ProcessAgain));
    assert!(!enabled(HermesAction::Exclude));
    assert!(!enabled(HermesAction::HandToInbox));
}

#[tokio::test]
async fn setting_the_filing_location_writes_it_to_the_config() {
    let config = tempfile::tempdir().unwrap();
    let mut app = ready_hermes_without_locations(&config);
    app.panel_mut(LEFT).selected = DOCUMENTS;

    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::SetRole(FolderRole::FilingLocation)),
    )
    .await;

    let [location] = app.hermes.filing_config.locations.as_slice() else {
        panic!(
            "one location expected: {:?}",
            app.hermes.filing_config.locations
        );
    };
    assert!(location.is_target);
    assert!(location.path.ends_with("Documents"));
    let written = std::fs::read_to_string(config.path().join("config.toml")).unwrap();
    assert!(written.contains("Documents"), "{written}");
}

#[tokio::test]
async fn an_inbox_needs_a_filing_location_first_and_the_sentence_stays() {
    let config = tempfile::tempdir().unwrap();
    let mut app = ready_hermes_without_locations(&config);
    app.panel_mut(LEFT).selected = DOCUMENTS;

    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Inbox)),
    )
    .await;

    assert_eq!(app.status_error(), Some("Set a target folder first."));
    assert!(app.hermes.filing_config.locations.is_empty());
}

async fn with_a_filing_location(config: &tempfile::TempDir) -> App {
    let mut app = ready_hermes_without_locations(config);
    app.panel_mut(LEFT).selected = DOCUMENTS;
    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::SetRole(FolderRole::FilingLocation)),
    )
    .await;
    app
}

#[tokio::test]
async fn the_chat_makes_a_folder_an_inbox_with_the_same_config_as_the_menu() {
    let by_menu_dir = tempfile::tempdir().unwrap();
    let mut by_menu = with_a_filing_location(&by_menu_dir).await;
    let inbox = by_menu_dir.path().join("Downloads");
    std::fs::create_dir(&inbox).unwrap();
    by_menu.panel_mut(LEFT).entries[DOCUMENTS].path = inbox.clone();
    settle(
        &mut by_menu,
        Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Inbox)),
    )
    .await;

    let by_chat_dir = tempfile::tempdir().unwrap();
    let mut by_chat = with_a_filing_location(&by_chat_dir).await;
    let chat_inbox = by_chat_dir.path().join("Downloads");
    std::fs::create_dir(&chat_inbox).unwrap();
    let task = by_chat.run_hermes_chat_action(ncrs_hermes_chat::Action::SetFolderRole {
        folder: chat_inbox.clone(),
        role: FolderRole::Inbox,
    });
    let mut queue = vec![task];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(next) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    queue.push(by_chat.update(reported));
                }
            }
        }
    }

    let watched = |app: &App, root: &std::path::Path| {
        app.hermes
            .filing_config
            .locations
            .iter()
            .filter(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.strip_prefix(root).map(ToOwned::to_owned))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        watched(&by_chat, by_chat_dir.path()),
        watched(&by_menu, by_menu_dir.path())
    );
    assert_eq!(watched(&by_chat, by_chat_dir.path()).len(), 1);
}

#[test]
fn a_hermes_command_while_hermes_is_off_opens_the_hermes_settings_with_the_reason() {
    let mut app = App::with_fixed_panels();
    assert!(app.hermes.filing.is_none());

    drop(app.update(Message::QuickFix(QuickFixMessage::Open)));

    assert!(app.settings_for_test().is_some(), "the settings are up");
    assert_eq!(
        app.status_error(),
        Some(app.lang.text(Msg::StatusHermesIsOff))
    );
}

#[test]
fn a_hermes_command_while_hermes_restarts_says_so_and_opens_nothing() {
    let mut app = App::with_fixed_panels();
    app.hermes.filing_restart = Some((1, Filing::default()));
    assert!(!app.hermes.is_off());

    drop(app.update(Message::QuickFix(QuickFixMessage::Open)));

    assert!(app.settings_for_test().is_none());
    assert_eq!(app.notice_for_test(), Some("Hermes restarting …"));
}

#[test]
fn with_the_settings_already_open_the_command_still_explains_why_it_cannot_run() {
    let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());

    drop(app.update(Message::QuickFix(QuickFixMessage::Open)));

    assert!(app.status_error().is_some());
}

fn active_hermes_with(locations: Vec<FilingLocation>) -> App {
    let mut app = App::with_fixed_panels();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    app.hermes.filing_config.locations = locations;
    app.panel_mut(LEFT).selected = DOCUMENTS;
    assert!(app.hermes.is_active());
    app
}

fn location_at(path: &Path, mode: LocationMode) -> FilingLocation {
    FilingLocation {
        path: path.to_path_buf(),
        mode,
        images: false,
        is_target: mode == LocationMode::Archive,
        transfer: Default::default(),
        continuous: false,
    }
}

#[tokio::test]
async fn handing_the_cursor_row_to_the_inbox_opens_the_move_dialog_aimed_at_it() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("Downloads");
    let mut app = active_hermes_with(vec![location_at(&inbox, LocationMode::Inbox)]);
    app.panel_mut(LEFT).entries[DOCUMENTS].path = dir.path().join("Scans");

    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::HandToInbox),
    )
    .await;

    let dialog = app.ops.transfer_dialog.as_ref().expect("the move dialog");
    assert_eq!(dialog.target_path(), inbox);
}

#[tokio::test]
async fn handing_to_the_inbox_without_one_says_so() {
    let mut app = active_hermes_with(Vec::new());

    drop(app.update(Message::HermesFile(HermesFileMessage::HandToInbox)));

    assert!(app.ops.transfer_dialog.is_none());
    assert_eq!(
        app.notice_for_test(),
        Some(app.lang.text(Msg::StatusHermesNoInbox))
    );
}
/// An app with a write folder and a second folder under the cursor.
async fn with_a_folder_to_file(config: &tempfile::TempDir) -> (App, std::path::PathBuf) {
    let mut app = with_a_filing_location(config).await;
    let created = config.path().join("Scans");
    std::fs::create_dir(&created).unwrap();
    let source = ncrs_intake::canonical_path(&created).unwrap();
    app.panel_mut(LEFT).entries[DOCUMENTS].path = source.clone();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    (app, source)
}

fn finished_run(source: &std::path::Path, filed: usize) -> ncrs_filing_view::Action {
    ncrs_filing_view::Action::Finished {
        source: source.to_path_buf(),
        transfer: ncrs_index::Transfer::Move,
        filed,
        left: Vec::new(),
        batch: ncrs_index::BatchId::from_number(1),
        then_continuous: true,
    }
}

fn continuous_folders(app: &App) -> Vec<(std::path::PathBuf, LocationMode)> {
    app.hermes
        .filing_config
        .locations
        .iter()
        .filter(|location| location.continuous)
        .map(|location| (location.path.clone(), location.mode))
        .collect()
}

#[tokio::test]
async fn filing_continuously_changes_nothing_until_the_run_is_filed() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config).await;

    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::FileContinuously),
    )
    .await;

    let session = app.filing_view.session.as_ref().unwrap();
    assert!(session.then_continuous);
    assert_eq!(session.source, source);
    assert!(continuous_folders(&app).is_empty());
    assert!(app
        .hermes
        .filing_config
        .locations
        .iter()
        .all(|location| location.path != source));
}

#[tokio::test]
async fn filing_once_opens_the_tab_without_the_continuous_mark() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, _) = with_a_folder_to_file(&config).await;

    settle(&mut app, Message::HermesFile(HermesFileMessage::FileOnce)).await;

    assert!(!app.filing_view.session.as_ref().unwrap().then_continuous);
}

#[tokio::test]
async fn the_panels_show_the_folder_without_the_moved_files_after_a_run() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config).await;
    let moved = source.join("scan.pdf");
    std::fs::write(&moved, b"scan").unwrap();
    let opened = app.load(LEFT, source.clone(), None);
    drive(&mut app, opened).await;
    let shows_scan = |shown: &App| {
        shown
            .panel(LEFT)
            .entries
            .iter()
            .any(|entry| entry.name == "scan.pdf")
    };
    assert!(shows_scan(&app));

    std::fs::remove_file(&moved).unwrap();
    let mut run = finished_run(&source, 1);
    if let ncrs_filing_view::Action::Finished {
        then_continuous, ..
    } = &mut run
    {
        *then_continuous = false;
    }
    let finished = app.run_filing_action(run);
    drive(&mut app, finished).await;

    assert!(!shows_scan(&app));
}

/// Runs `first` and delivers everything it and the tasks after it report.
async fn drive(app: &mut App, first: Task<Message>) {
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

#[tokio::test]
async fn the_folder_files_by_itself_only_after_a_run_was_filed() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config).await;

    let nothing = app.run_filing_action(finished_run(&source, 0));
    assert!(
        task::into_stream(nothing).is_none(),
        "nothing filed, nothing set"
    );
    assert!(continuous_folders(&app).is_empty());

    let filed_run = app.run_filing_action(finished_run(&source, 3));
    let mut queue = vec![filed_run];
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

    assert_eq!(
        continuous_folders(&app),
        vec![(source, LocationMode::Inbox)]
    );
}

#[tokio::test]
async fn stopping_continuous_filing_keeps_the_role() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config).await;
    app.hermes
        .filing_config
        .locations
        .push(ncrs_config::config::FilingLocation {
            path: source.clone(),
            mode: LocationMode::Inbox,
            images: false,
            is_target: false,
            transfer: Default::default(),
            continuous: true,
        });

    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::StopContinuous),
    )
    .await;

    assert!(continuous_folders(&app).is_empty());
    assert!(app
        .hermes
        .filing_config
        .locations
        .iter()
        .any(|location| location.path == source && location.mode == LocationMode::Inbox));
}

#[tokio::test]
async fn always_in_the_chat_sets_no_role_before_the_run_in_the_tab_is_filed() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config).await;
    let target = app.hermes.filing_config.locations[0].path.clone();
    let before = app.hermes.filing_config.locations.clone();

    drop(
        app.run_hermes_chat_action(ncrs_hermes_chat::Action::OpenFiling {
            source,
            target,
            watch_source: true,
        }),
    );

    assert_eq!(app.hermes.filing_config.locations, before);
    assert!(app.filing_view.session.as_ref().unwrap().then_continuous);
}
