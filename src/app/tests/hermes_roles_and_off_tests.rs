use crate::app::*;
use crate::context_menu::{HermesAction, SubEntry};
use crate::messages::SettingsFeature;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

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
    drop(app.update(Message::ContextMenuAt {
        side: LEFT,
        index: DOCUMENTS,
    }));
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
    assert!(enabled(HermesAction::SetInbox));
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

    assert_eq!(app.status_error(), Some("Set a filing location first."));
    assert!(app.hermes.filing_config.locations.is_empty());
}

#[test]
fn a_hermes_command_while_hermes_is_off_opens_the_overview_with_the_reason() {
    let mut app = App::with_fixed_panels();
    assert!(app.hermes.filing.is_none());

    drop(app.update(Message::ReviewList(ReviewListMessage::Open)));

    assert!(app.review_list_for_test().is_none());
    assert!(app.hermes.overview.is_some(), "the overview is up");
    assert_eq!(
        app.status_error(),
        Some("Filing is off. Switch it on here to sort your documents.")
    );
}

#[test]
fn a_hermes_command_while_hermes_restarts_says_so_and_opens_nothing() {
    let mut app = App::with_fixed_panels();
    app.hermes.filing_restart = Some((1, Filing::default()));
    assert!(!app.hermes.is_off());

    drop(app.update(Message::ReviewList(ReviewListMessage::Open)));

    assert!(app.review_list_for_test().is_none());
    assert!(app.settings_for_test().is_none());
    assert_eq!(app.notice_for_test(), Some("Filing restarting …"));
}

#[test]
fn with_the_settings_already_open_the_command_still_opens_the_overview() {
    let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());

    drop(app.update(Message::QuickFix(QuickFixMessage::Open)));

    assert!(app.hermes.overview.is_some());
    assert!(app.status_error().is_some());
}
