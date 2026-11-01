use crate::app::*;
use crate::fs::{SortColumn, SortKey};

fn environment_writing_to(file: &Path) -> Environment {
    Environment {
        state_path: Some(file.to_path_buf()),
        ..Environment::isolated()
    }
}

fn saved_for(left: &Path, right: &Path) -> SessionState {
    SessionState {
        version: session_state::STATE_VERSION,
        active: Some(PanelSide::Right),
        window: Some(WindowSize {
            width: 1000.0,
            height: 700.0,
        }),
        left: PanelRecord {
            path: session_state::record_path(left),
            sort: SortColumn::Size,
            descending: true,
            ..PanelRecord::default()
        },
        right: PanelRecord {
            path: session_state::record_path(right),
            sort: SortColumn::Modified,
            descending: false,
            ..PanelRecord::default()
        },
        ..SessionState::default()
    }
}

fn started(scratch: &tempfile::TempDir) -> App {
    let left = scratch.path().join("left");
    let right = scratch.path().join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    let file = scratch.path().join("state.toml");
    App::restored(environment_writing_to(&file), saved_for(&left, &right)).0
}

#[test]
fn a_saved_state_is_restored() {
    let scratch = tempfile::tempdir().unwrap();
    let app = started(&scratch);

    assert_eq!(app.panels.left.path, scratch.path().join("left"));
    assert_eq!(app.panels.right.path, scratch.path().join("right"));
    assert_eq!(
        app.panels.left.sort,
        SortKey {
            column: SortColumn::Size,
            descending: true
        }
    );
    assert_eq!(app.panels.right.sort.column, SortColumn::Modified);
    assert_eq!(app.panels.active, PanelSide::Right);
    assert_eq!(app.window_size, Size::new(1000.0, 700.0));
}

/// Starts from `saved` and delivers the first listings, as the running
/// app would. Without a state path, so no write timer is waited for.
async fn started_and_listed(saved: SessionState) -> App {
    use iced_test::futures::futures::StreamExt as _;
    use iced_test::runtime::{task, Action};

    let (mut app, start) = App::restored(Environment::isolated(), saved);
    let mut stream = task::into_stream(start).expect("the first loads");
    let mut reports = Vec::new();
    while let Some(action) = stream.next().await {
        if let Action::Output(reported) = action {
            reports.push(reported);
        }
    }
    for reported in reports {
        drop(app.update(reported));
    }
    app
}

fn cursor_name(panel: &PanelState) -> Option<String> {
    panel
        .selected_entry()
        .map(|entry| entry.name.to_string_lossy().into_owned())
}

fn folder_with_files(scratch: &tempfile::TempDir, name: &str) -> PathBuf {
    let folder = scratch.path().join(name);
    std::fs::create_dir_all(&folder).unwrap();
    for file in ["alpha.txt", "beta.txt", "gamma.txt"] {
        std::fs::write(folder.join(file), "x").unwrap();
    }
    folder
}

#[tokio::test]
async fn the_cursor_row_comes_back_and_is_in_view() {
    let scratch = tempfile::tempdir().unwrap();
    let left = folder_with_files(&scratch, "left");
    let right = folder_with_files(&scratch, "right");
    let mut saved = saved_for(&left, &right);
    saved.left.cursor = Some("gamma.txt".to_string());
    saved.window = None;

    let mut app = started_and_listed(saved).await;
    assert_eq!(cursor_name(&app.panels.left).as_deref(), Some("gamma.txt"));
    assert_eq!(cursor_name(&app.panels.right).as_deref(), Some(".."));
    assert!(app.panels.left.selected >= app.panels.left.scroll_offset);

    assert_eq!(
        app.session_state().left.cursor.as_deref(),
        Some("gamma.txt")
    );
    app.panels.left.select(0, app.visible_rows);
    assert_eq!(
        app.session_state().left.cursor,
        None,
        "`..` is not recorded"
    );
}

#[tokio::test]
async fn a_cursor_entry_that_is_gone_leaves_the_cursor_on_the_first_row() {
    let scratch = tempfile::tempdir().unwrap();
    let left = folder_with_files(&scratch, "left");
    let right = folder_with_files(&scratch, "right");
    let mut saved = saved_for(&left, &right);
    saved.left.cursor = Some("deleted.txt".to_string());

    let app = started_and_listed(saved).await;

    assert_eq!(app.panels.left.selected, 0);
}

#[tokio::test]
async fn the_cursor_is_not_carried_over_to_the_parent_of_a_deleted_folder() {
    let scratch = tempfile::tempdir().unwrap();
    let right = folder_with_files(&scratch, "right");
    std::fs::write(scratch.path().join("left-file"), "x").unwrap();
    let gone = scratch.path().join("gone");
    let mut saved = saved_for(&gone, &right);
    saved.left.cursor = Some("left-file".to_string());

    let app = started_and_listed(saved).await;

    assert_eq!(app.panels.left.path, scratch.path());
    assert_eq!(app.panels.left.selected, 0);
}

#[tokio::test]
async fn the_active_panel_is_still_the_saved_one_once_the_listings_are_in() {
    let scratch = tempfile::tempdir().unwrap();
    let left = folder_with_files(&scratch, "left");
    let right = folder_with_files(&scratch, "right");
    let saved = saved_for(&left, &right);
    assert_eq!(saved.active, Some(PanelSide::Right));

    let app = started_and_listed(saved).await;

    assert_eq!(app.panels.active, PanelSide::Right);
}

#[test]
fn switching_panels_schedules_a_write_of_the_new_active_panel() {
    let scratch = tempfile::tempdir().unwrap();
    let mut app = started(&scratch);
    assert_eq!(app.panels.active, PanelSide::Right);

    drop(app.update(Message::Panel(PanelMessage::SwitchPanel)));
    assert_eq!(app.session_save_generation, 1);
    let (_, after_switch) = app.due_session_state(1).unwrap();
    assert_eq!(after_switch.active, Some(PanelSide::Left));

    drop(app.update(Message::Panel(PanelMessage::RowClicked {
        side: PanelSide::Right,
        index: 0,
        on_tag: false,
    })));
    assert_eq!(app.session_save_generation, 2);
    let (_, after_click) = app.due_session_state(2).unwrap();
    assert_eq!(after_click.active, Some(PanelSide::Right));
}

#[test]
fn an_empty_panel_records_no_cursor() {
    let scratch = tempfile::tempdir().unwrap();
    let app = started(&scratch);
    assert_eq!(app.session_state().left.cursor, None);
}

#[test]
fn a_deleted_folder_opens_its_nearest_existing_parent() {
    let scratch = tempfile::tempdir().unwrap();
    let gone = scratch.path().join("gone").join("deeper");
    let saved = saved_for(&gone, scratch.path());

    let app = App::restored(Environment::isolated(), saved).0;

    assert_eq!(app.panels.left.path, scratch.path());
}

#[test]
fn starting_schedules_no_write() {
    let scratch = tempfile::tempdir().unwrap();
    let app = started(&scratch);
    assert_eq!(app.session_state(), app.session_scheduled);
    assert_eq!(app.session_save_generation, 0);
}

#[test]
fn quitting_writes_what_the_next_start_restores() {
    let scratch = tempfile::tempdir().unwrap();
    let mut app = started(&scratch);

    drop(app.update(Message::Quit));

    let written = session_state::load(&scratch.path().join("state.toml"));
    assert_eq!(
        written.left.path.as_deref(),
        scratch.path().join("left").to_str()
    );
    assert_eq!(
        written.right.path.as_deref(),
        scratch.path().join("right").to_str()
    );
    assert_eq!(written.left.sort, SortColumn::Size);
    assert!(written.left.descending);
    assert_eq!(written.right.sort, SortColumn::Modified);
    assert_eq!(written.active, Some(PanelSide::Right));
}

#[test]
fn a_sort_change_is_written_once_for_the_newest_generation() {
    let scratch = tempfile::tempdir().unwrap();
    let mut app = started(&scratch);

    drop(app.update(Message::Panel(PanelMessage::SortBy {
        side: PanelSide::Left,
        column: SortColumn::Name,
    })));
    let first = app.session_save_generation;
    assert_eq!(first, 1);
    drop(app.update(Message::Panel(PanelMessage::SortBy {
        side: PanelSide::Right,
        column: SortColumn::Name,
    })));

    assert_eq!(app.session_save_generation, 2);
    assert!(app.due_session_state(first).is_none());
    let (path, state) = app.due_session_state(2).unwrap();
    assert_eq!(path, scratch.path().join("state.toml"));
    assert_eq!(state.left.sort, SortColumn::Name);
    assert_eq!(state.right.sort, SortColumn::Name);
}

#[test]
fn a_message_that_changes_nothing_persistent_schedules_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let mut app = started(&scratch);

    drop(app.update(Message::Panel(PanelMessage::MoveSelection(1))));

    assert_eq!(app.session_save_generation, 0);
}

#[test]
fn a_minimised_window_keeps_the_last_good_size() {
    let scratch = tempfile::tempdir().unwrap();
    let mut app = started(&scratch);

    drop(app.update(Message::WindowResized(Size::new(0.0, 0.0))));

    assert_eq!(
        app.session_state().window,
        Some(WindowSize {
            width: 1000.0,
            height: 700.0
        })
    );
}

#[test]
fn without_a_state_path_nothing_is_scheduled() {
    let mut app = App::with_environment(Environment::isolated()).0;

    drop(app.update(Message::Panel(PanelMessage::SortBy {
        side: PanelSide::Left,
        column: SortColumn::Size,
    })));
    drop(app.update(Message::WindowResized(Size::new(900.0, 600.0))));

    assert_eq!(app.session_save_generation, 0);
    assert!(app.due_session_state(0).is_none());
}
