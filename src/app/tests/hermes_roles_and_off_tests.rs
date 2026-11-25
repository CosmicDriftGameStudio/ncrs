use crate::app::*;
use crate::context_menu::{HermesAction, SubEntry};
use crate::messages::SettingsFeature;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_config::config::TargetSetting;
use ncrs_hermes_ui::suggest::SuggestFor;
use ncrs_index::TargetId;
use ncrs_intake::SortTarget;

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
fn a_folder_becomes_a_source_or_a_target_before_any_is_set() {
    let config = tempfile::tempdir().unwrap();
    let mut app = ready_hermes_without_locations(&config);

    let items = submenu_items(&mut app);

    let enabled = |wanted| items.iter().any(|&(action, on)| action == wanted && on);
    assert!(!enabled(HermesAction::FileOnce));
    assert!(enabled(HermesAction::WatchSource));
    assert!(enabled(HermesAction::AddTarget));
    assert!(!enabled(HermesAction::ProcessAgain));
    assert!(!enabled(HermesAction::Exclude));
}

#[tokio::test]
async fn adding_a_target_opens_its_dialog_with_the_folder_and_writes_nothing() {
    let config = tempfile::tempdir().unwrap();
    let mut app = ready_hermes_without_locations(&config);
    app.panel_mut(LEFT).selected = DOCUMENTS;

    settle(&mut app, Message::HermesFile(HermesFileMessage::AddTarget)).await;

    let dialog = app
        .hermes
        .target_dialog
        .as_ref()
        .expect("the target dialog");
    assert!(dialog.folder.ends_with("Documents"), "{}", dialog.folder);
    assert!(dialog.replacing.is_none());
    assert!(app.hermes.filing_config.targets.is_empty());
}

#[tokio::test]
async fn a_hermes_command_while_hermes_is_off_opens_the_hermes_settings_with_the_reason() {
    let mut app = App::with_fixed_panels();
    assert!(app.hermes.filing.is_none());

    drop(app.update(Message::HermesFile(HermesFileMessage::WatchSource)));

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

    drop(app.update(Message::HermesFile(HermesFileMessage::WatchSource)));

    assert!(app.settings_for_test().is_none());
    assert_eq!(app.notice_for_test(), Some("Hermes restarting …"));
}

#[tokio::test]
async fn with_the_settings_already_open_the_command_still_explains_why_it_cannot_run() {
    let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());

    drop(app.update(Message::HermesFile(HermesFileMessage::WatchSource)));

    assert!(app.status_error().is_some());
}

fn target_at(path: &Path) -> TargetSetting {
    TargetSetting {
        target: SortTarget {
            id: TargetId::main(),
            name: "Documents".to_owned(),
            path: path.to_path_buf(),
            own_volume: None,
        },
        paused: None,
    }
}

/// An app with a target and a second folder under the cursor.
fn with_a_folder_to_file(config: &tempfile::TempDir) -> (App, std::path::PathBuf) {
    let mut app = ready_hermes_without_locations(config);
    app.panel_mut(LEFT).selected = DOCUMENTS;
    app.hermes.filing_config.targets = vec![target_at(&config.path().join("Documents"))];
    let created = config.path().join("Scans");
    std::fs::create_dir(&created).unwrap();
    let source = ncrs_intake::canonical_path(&created).unwrap();
    app.panel_mut(LEFT).entries[DOCUMENTS].path = source.clone();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    (app, source)
}

fn moved_run(source: &std::path::Path, filed: usize) -> ncrs_filing_view::Action {
    ncrs_filing_view::Action::Filed {
        source: source.to_path_buf(),
        transfer: ncrs_intake::LocationTransfer::Move,
        filed,
        left: 0,
        batch: Some(ncrs_index::BatchId::from_number(1)),
    }
}

/// The kind and the folder of the open source dialog.
fn open_source_dialog(app: &App) -> Option<(ncrs_hermes_ui::SourceDialogKind, String)> {
    app.hermes
        .source_dialog
        .as_ref()
        .map(|dialog| (dialog.kind, dialog.folder.clone()))
}

#[tokio::test]
async fn watching_a_folder_asks_for_the_source_and_changes_nothing() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config);
    let before = app.hermes.filing_config.clone();

    settle(
        &mut app,
        Message::HermesFile(HermesFileMessage::WatchSource),
    )
    .await;

    assert_eq!(
        open_source_dialog(&app),
        Some((
            ncrs_hermes_ui::SourceDialogKind::Watched,
            source.display().to_string()
        ))
    );
    assert!(app.filing_view.session.is_none());
    assert_eq!(app.hermes.filing_config, before);
}

#[tokio::test]
async fn filing_once_asks_for_the_source() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config);

    settle(&mut app, Message::HermesFile(HermesFileMessage::FileOnce)).await;

    assert_eq!(
        open_source_dialog(&app),
        Some((
            ncrs_hermes_ui::SourceDialogKind::Once,
            source.display().to_string()
        ))
    );
}

#[tokio::test]
async fn the_panels_show_the_folder_without_the_moved_files_after_a_run() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config);
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
    let finished = app.run_filing_action(moved_run(&source, 1));
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

fn suggested_in_the_chat(folder: &Path) -> ncrs_hermes_chat::Action {
    ncrs_hermes_chat::Action::SuggestRules {
        folder: folder.to_path_buf(),
        pattern: Some("{year}/{kind}/".to_owned()),
    }
}

#[tokio::test]
async fn rules_for_a_folder_that_is_a_target_are_suggested_at_once() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, _) = with_a_folder_to_file(&config);

    drop(app.run_hermes_chat_action(suggested_in_the_chat(&config.path().join("Documents"))));

    let suggest = app.hermes.suggest.as_ref().expect("the suggestions");
    assert_eq!(suggest.subject, SuggestFor::Target(TargetId::main()));
    assert_eq!(suggest.wish.as_deref(), Some("{year}/{kind}/"));
    assert!(app.hermes.target_dialog.is_none());
}

#[tokio::test]
async fn a_folder_that_is_no_target_becomes_one_before_rules_are_suggested() {
    let config = tempfile::tempdir().unwrap();
    let (mut app, source) = with_a_folder_to_file(&config);

    drop(app.run_hermes_chat_action(suggested_in_the_chat(&source)));

    assert!(app.hermes.suggest.is_none());
    let dialog = app
        .hermes
        .target_dialog
        .as_ref()
        .expect("the target dialog");
    assert_eq!(Path::new(&dialog.folder), source);
    let after = dialog
        .then_suggest
        .as_ref()
        .expect("the suggestions follow");
    assert_eq!(after.wish.as_deref(), Some("{year}/{kind}/"));
    assert_eq!(app.hermes.filing_config.targets.len(), 1);
}
