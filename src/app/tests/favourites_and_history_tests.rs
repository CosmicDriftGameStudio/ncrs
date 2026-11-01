use crate::app::*;
use crate::ui::panel_history::PanelHistory;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

/// Delivers `message` and everything the tasks it starts report, until
/// nothing is left. Only for apps without a state path: the debounce timer
/// of a scheduled write would be waited for.
async fn settle(app: &mut App, message: Message) {
    let mut queue = vec![message];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(app.update(next)) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                queue.push(reported);
            }
        }
    }
}

/// The panel opens `path`, as a typed path or a double click does.
async fn go(app: &mut App, path: &Path) {
    let mut stream = task::into_stream(app.load(PanelSide::Left, path.to_path_buf(), None))
        .expect("a load task");
    let mut reports = Vec::new();
    while let Some(action) = stream.next().await {
        if let Action::Output(reported) = action {
            reports.push(reported);
        }
    }
    for reported in reports {
        settle(app, reported).await;
    }
}

fn started_in(base: &Path) -> App {
    let saved = SessionState {
        left: PanelRecord {
            path: session_state::record_path(base),
            ..PanelRecord::default()
        },
        ..SessionState::default()
    };
    App::restored(Environment::isolated(), saved).0
}

fn cursor_name(app: &App) -> String {
    app.panels
        .left
        .selected_entry()
        .unwrap()
        .name
        .to_string_lossy()
        .into_owned()
}

fn tree() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(scratch.path().join("a").join("b")).unwrap();
    std::fs::create_dir(scratch.path().join("c")).unwrap();
    scratch
}

#[tokio::test]
async fn back_returns_to_the_folder_with_the_cursor_on_where_it_came_from() {
    let scratch = tree();
    let base = scratch.path();
    let mut app = started_in(base);
    go(&mut app, base).await;
    go(&mut app, &base.join("a")).await;
    go(&mut app, &base.join("a").join("b")).await;

    settle(&mut app, Message::Panel(PanelMessage::HistoryBack)).await;
    assert_eq!(app.panels.left.path, base.join("a"));
    assert_eq!(cursor_name(&app), "b");

    settle(&mut app, Message::Panel(PanelMessage::HistoryBack)).await;
    assert_eq!(app.panels.left.path, base);
    assert_eq!(cursor_name(&app), "a");

    settle(&mut app, Message::Panel(PanelMessage::HistoryForward)).await;
    assert_eq!(app.panels.left.path, base.join("a"));
    settle(&mut app, Message::Panel(PanelMessage::HistoryForward)).await;
    assert_eq!(app.panels.left.path, base.join("a").join("b"));
}

#[tokio::test]
async fn a_reload_of_the_same_folder_records_nothing() {
    let scratch = tree();
    let mut app = started_in(scratch.path());
    go(&mut app, scratch.path()).await;
    go(&mut app, scratch.path()).await;
    assert!(app.panels.left.history.back().is_empty());
}

#[tokio::test]
async fn a_new_visit_after_back_clears_forward() {
    let scratch = tree();
    let base = scratch.path();
    let mut app = started_in(base);
    go(&mut app, &base.join("a")).await;
    settle(&mut app, Message::Panel(PanelMessage::HistoryBack)).await;
    assert_eq!(app.panels.left.history.forward(), [base.join("a")]);

    go(&mut app, &base.join("c")).await;

    assert!(app.panels.left.history.forward().is_empty());
}

#[tokio::test]
async fn a_step_into_a_deleted_folder_lands_on_its_nearest_existing_parent() {
    let scratch = tree();
    let base = scratch.path();
    let mut app = started_in(base);
    go(&mut app, &base.join("a")).await;
    go(&mut app, &base.join("a").join("b")).await;
    std::fs::remove_dir_all(base.join("a")).unwrap();

    settle(&mut app, Message::Panel(PanelMessage::HistoryBack)).await;

    assert_eq!(app.panels.left.path, base);
}

#[test]
fn history_and_favourites_survive_a_restart() {
    let scratch = tree();
    let base = scratch.path();
    let file = scratch.path().join("state.toml");
    let mut app = started_in(base);
    app.state_path = Some(file.clone());
    app.panels.left.history = PanelHistory::restored(
        vec![base.join("a"), base.join("c")],
        vec![base.join("a").join("b")],
    );
    app.panels.favourites = vec![base.join("c"), base.join("a")];

    drop(app.update(Message::Quit));
    let environment = Environment {
        state_path: Some(file.clone()),
        ..Environment::isolated()
    };
    let restored = App::restored(environment, session_state::load(&file)).0;

    assert_eq!(restored.panels.left.history, app.panels.left.history);
    assert_eq!(restored.panels.favourites, app.panels.favourites);
}

#[test]
fn adding_a_favourite_twice_keeps_one_and_says_so() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::AddFavourite));
    assert_eq!(
        app.notice.as_deref(),
        Some("/home/test/links added to favourites")
    );
    drop(app.update(Message::AddFavourite));
    assert_eq!(
        app.notice.as_deref(),
        Some("/home/test/links is already a favourite")
    );
    assert_eq!(app.panels.favourites, [PathBuf::from("/home/test/links")]);
}

#[test]
fn removing_a_favourite_takes_the_active_folder_out() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::RemoveFavourite));
    assert_eq!(
        app.notice.as_deref(),
        Some("/home/test/links is not a favourite")
    );
    drop(app.update(Message::AddFavourite));
    drop(app.update(Message::RemoveFavourite));
    assert!(app.panels.favourites.is_empty());
    assert_eq!(
        app.notice.as_deref(),
        Some("/home/test/links removed from favourites")
    );
}

fn two_drives() -> Vec<fs::Volume> {
    ["one", "two"]
        .iter()
        .map(|name| fs::Volume {
            name: (*name).to_string(),
            path: PathBuf::from("/").join(name),
        })
        .collect()
}

#[test]
fn the_favourites_action_needs_a_favourite_and_opens_on_the_first() {
    let mut app = App::with_fixed_panels().with_volumes(two_drives);
    drop(app.update(Message::OpenFavourites));
    assert_eq!(app.notice.as_deref(), Some("No favourites yet"));
    assert!(!app.volume_menu_is_open());

    app.panels.favourites = vec![PathBuf::from("/nowhere/at/all")];
    drop(app.update(Message::OpenFavourites));
    assert_eq!(app.volume_menu_selected(), Some(2));
}

#[tokio::test]
async fn a_click_on_a_favourite_row_opens_its_folder() {
    let scratch = tree();
    let favourite = scratch.path().join("c");
    let mut app = started_in(scratch.path()).with_volumes(two_drives);
    app.panels.favourites = vec![favourite.clone()];
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));

    settle(&mut app, Message::VolumeMenuClick(2)).await;

    assert!(!app.volume_menu_is_open());
    assert_eq!(app.panels.left.path, favourite);
}

#[test]
fn the_menu_opens_with_a_favourite_and_no_drives() {
    let mut app = App::with_fixed_panels().with_volumes(Vec::new);
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));
    assert!(!app.volume_menu_is_open());

    app.panels.favourites = vec![PathBuf::from("/nowhere/at/all")];
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));
    assert!(app.volume_menu_is_open());
}

#[test]
fn a_missing_favourite_reports_and_stays_in_the_list() {
    let mut app = App::with_fixed_panels().with_volumes(two_drives);
    app.panels.favourites = vec![PathBuf::from("/nowhere/at/all")];
    drop(app.update(Message::OpenFavourites));

    drop(app.update(Message::VolumeMenuActivate));

    assert!(!app.volume_menu_is_open());
    assert_eq!(
        app.job_error.as_deref(),
        Some("/nowhere/at/all no longer exists")
    );
    assert_eq!(app.panels.favourites, [PathBuf::from("/nowhere/at/all")]);
}

#[test]
fn delete_removes_a_favourite_row_and_ignores_a_drive_row() {
    let mut app = App::with_fixed_panels().with_volumes(two_drives);
    app.panels.favourites = vec![PathBuf::from("/x/one"), PathBuf::from("/x/two")];
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));
    drop(app.update(Message::VolumeMenuFirst));

    drop(app.update(Message::VolumeMenuRemove));
    assert_eq!(
        app.panels.favourites.len(),
        2,
        "a drive row is not removable"
    );

    drop(app.update(Message::VolumeMenuLast));
    drop(app.update(Message::VolumeMenuRemove));
    assert_eq!(
        app.panels.favourites.len(),
        2,
        "the trash row is not removable"
    );
    if let Some(menu) = app.panels.volume_menu.as_mut() {
        menu.selected = menu.last_row().saturating_sub(1);
    }
    drop(app.update(Message::VolumeMenuRemove));
    assert_eq!(app.panels.favourites, [PathBuf::from("/x/one")]);
    assert!(app.volume_menu_is_open(), "the menu stays open");
    assert_eq!(
        app.volume_menu_selected(),
        Some(3),
        "the highlight lands on the trash row, the last one left"
    );
}

#[test]
fn removing_the_last_favourite_without_drives_leaves_the_trash_row_open() {
    let mut app = App::with_fixed_panels().with_volumes(Vec::new);
    app.panels.favourites = vec![PathBuf::from("/x/one")];
    drop(app.update(Message::VolumeMenu(PanelSide::Left)));

    drop(app.update(Message::VolumeMenuRemove));

    assert!(app.panels.favourites.is_empty());
    assert!(app.volume_menu_is_open(), "the trash row is still there");
}

#[test]
fn delete_and_backspace_in_the_menu_remove_and_alt_left_is_the_panels() {
    let in_menu = PromptKeyState {
        volume_menu: true,
        ..PromptKeyState::default()
    };
    for key in [Named::Delete, Named::Backspace] {
        assert_eq!(
            route_key(&in_menu, Key::Named(key), Modifiers::default()),
            Some(Message::VolumeMenuRemove)
        );
    }
    let panels = PromptKeyState::default();
    assert_eq!(
        route_key(&panels, Key::Named(Named::ArrowLeft), Modifiers::ALT),
        Some(Message::Panel(PanelMessage::HistoryBack))
    );
    assert_eq!(
        route_key(&panels, Key::Named(Named::ArrowRight), Modifiers::ALT),
        Some(Message::Panel(PanelMessage::HistoryForward))
    );
}

/// The text fields have the arrows with Alt (a word jump): the keys must
/// not also walk the history behind them.
#[test]
fn alt_left_does_not_reach_the_panel_while_a_text_field_has_the_keyboard() {
    let fields = [
        PromptKeyState {
            command_line: true,
            ..PromptKeyState::default()
        },
        PromptKeyState {
            path_editing: true,
            ..PromptKeyState::default()
        },
        PromptKeyState {
            open: true,
            ..PromptKeyState::default()
        },
        PromptKeyState {
            quick_open: true,
            ..PromptKeyState::default()
        },
        PromptKeyState {
            command_palette: true,
            ..PromptKeyState::default()
        },
        PromptKeyState {
            search: true,
            ..PromptKeyState::default()
        },
    ];
    for state in fields {
        for key in [Named::ArrowLeft, Named::ArrowRight] {
            let routed = route_key(&state, Key::Named(key), Modifiers::ALT);
            assert_ne!(routed, Some(Message::Panel(PanelMessage::HistoryBack)));
            assert_ne!(routed, Some(Message::Panel(PanelMessage::HistoryForward)));
        }
    }
}
