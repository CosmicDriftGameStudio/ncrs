use crate::app::*;
use crate::fs::FileEntry;
use crate::ui::format;
use std::ffi::OsString;
use std::sync::atomic::Ordering;

const SIDE: PanelSide = PanelSide::Left;

fn directory() -> PathBuf {
    PathBuf::from("/home/test/links")
}

fn row(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: directory().join(name),
        is_dir,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
    }
}

fn listing() -> Vec<FileEntry> {
    let mut entries = vec![
        FileEntry::parent(Path::new("/home/test")),
        row("alpha", true),
        row("beta", true),
        row("gamma", true),
        row("note.txt", false),
    ];
    fs::sort_entries(&mut entries, fs::SortKey::default());
    entries
}

fn size(bytes: u64) -> fs::FolderSize {
    fs::FolderSize {
        bytes,
        partial: false,
    }
}

fn load(app: &mut App, path: PathBuf, entries: Vec<FileEntry>, sorted_by: fs::SortKey) {
    let request_id = app.panel_mut(SIDE).begin_load();
    drop(app.update(Message::Panel(PanelMessage::DirectoryLoaded {
        side: SIDE,
        request_id,
        path,
        entries,
        error: None,
        sorted_by,
        select: None,
        archive: None,
    })));
}

fn app_in_links() -> App {
    let mut app = App::with_fixed_panels();
    load(&mut app, directory(), listing(), fs::SortKey::default());
    app
}

fn put_cursor_on(app: &mut App, name: &str) {
    let rows = app.visible_rows;
    let index = app
        .panel(SIDE)
        .entries
        .iter()
        .position(|entry| entry.name == name)
        .unwrap();
    app.panel_mut(SIDE).select(index, rows);
}

fn shown(app: &App, name: &str) -> String {
    let entry = app
        .panel(SIDE)
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap();
    format::entry_size(entry)
}

fn run_id(app: &App) -> u64 {
    app.panels.folder_size_run.as_ref().unwrap().id
}

fn measured(app: &mut App, run: u64, name: &str, bytes: u64) {
    drop(app.update(Message::FolderSizeMeasured {
        run,
        folder: directory().join(name),
        size: Some(size(bytes)),
    }));
}

fn names(app: &App) -> Vec<String> {
    app.panel(SIDE)
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect()
}

#[tokio::test]
async fn the_folder_under_the_cursor_is_measured_and_shows_its_value() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "beta");

    drop(app.update(Message::ComputeFolderSizes));

    let run = app.panels.folder_size_run.as_ref().unwrap();
    assert!(run.waiting.is_empty(), "one folder, already walking");
    assert_eq!(shown(&app, "beta"), "<DIR>", "until the value arrives");
    let id = run.id;
    measured(&mut app, id, "beta", 2048);
    assert_eq!(shown(&app, "beta"), "2.0 KB");
    assert_eq!(shown(&app, "alpha"), "<DIR>");
    assert!(app.panels.folder_size_run.is_none(), "the run is over");
}

#[tokio::test]
async fn tagged_folders_are_measured_one_after_another_and_the_cursor_is_ignored() {
    let mut app = app_in_links();
    for name in ["alpha", "gamma", "note.txt"] {
        app.panel_mut(SIDE)
            .selection
            .toggle(&directory().join(name));
    }
    put_cursor_on(&mut app, "beta");

    drop(app.update(Message::ComputeFolderSizes));

    let id = run_id(&app);
    assert_eq!(
        app.panels.folder_size_run.as_ref().unwrap().waiting,
        [directory().join("gamma")],
        "alpha is already walking, the file is not a target"
    );
    measured(&mut app, id, "alpha", 10);
    measured(&mut app, id, "gamma", 3_000_000);
    assert_eq!(shown(&app, "alpha"), "10 B");
    assert_eq!(shown(&app, "gamma"), "2.9 MB");
    assert_eq!(shown(&app, "beta"), "<DIR>");
}

#[tokio::test]
async fn on_the_parent_row_every_folder_of_the_listing_is_a_target() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "..");

    drop(app.update(Message::ComputeFolderSizes));

    let run = app.panels.folder_size_run.as_ref().unwrap();
    assert_eq!(
        run.waiting,
        [directory().join("beta"), directory().join("gamma")],
        "alpha is already walking; the file and `..` are not folders to measure"
    );
}

#[tokio::test]
async fn on_a_file_without_tags_nothing_starts() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "note.txt");

    drop(app.update(Message::ComputeFolderSizes));

    assert!(app.panels.folder_size_run.is_none());
    let enabled = |shown: &App| {
        shown
            .context_menu_facts()
            .action_enabled(ContextAction::ComputeFolderSizes, &shown.panels)
    };
    assert!(!enabled(&app));
    put_cursor_on(&mut app, "alpha");
    assert!(enabled(&app));
}

#[tokio::test]
async fn a_partial_value_is_marked() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "alpha");
    drop(app.update(Message::ComputeFolderSizes));

    let id = run_id(&app);
    drop(app.update(Message::FolderSizeMeasured {
        run: id,
        folder: directory().join("alpha"),
        size: Some(fs::FolderSize {
            bytes: 1024,
            partial: true,
        }),
    }));

    assert_eq!(shown(&app, "alpha"), "1.0 KB+");
}

#[tokio::test]
async fn a_stopped_walk_leaves_no_value_behind() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "alpha");
    drop(app.update(Message::ComputeFolderSizes));

    let id = run_id(&app);
    drop(app.update(Message::FolderSizeMeasured {
        run: id,
        folder: directory().join("alpha"),
        size: None,
    }));

    assert_eq!(shown(&app, "alpha"), "<DIR>");
    assert!(app.panels.folder_sizes.is_empty());
}

#[tokio::test]
async fn the_values_come_back_when_the_folder_is_visited_again() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "gamma");
    drop(app.update(Message::ComputeFolderSizes));
    let id = run_id(&app);
    measured(&mut app, id, "gamma", 4096);

    load(
        &mut app,
        PathBuf::from("/home/test"),
        vec![FileEntry::parent(Path::new("/home"))],
        fs::SortKey::default(),
    );
    load(&mut app, directory(), listing(), fs::SortKey::default());

    assert_eq!(shown(&app, "gamma"), "4.0 KB");
    assert_eq!(shown(&app, "alpha"), "<DIR>");
}

#[tokio::test]
async fn running_the_command_again_replaces_the_cached_value() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "gamma");
    drop(app.update(Message::ComputeFolderSizes));
    let first = run_id(&app);
    measured(&mut app, first, "gamma", 100);

    drop(app.update(Message::ComputeFolderSizes));
    let second = run_id(&app);
    assert_ne!(first, second);
    measured(&mut app, second, "gamma", 200);

    assert_eq!(shown(&app, "gamma"), "200 B");
    load(&mut app, directory(), listing(), fs::SortKey::default());
    assert_eq!(shown(&app, "gamma"), "200 B");
}

#[tokio::test]
async fn leaving_the_folder_stops_the_walk_and_a_late_result_changes_nothing() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "gamma");
    drop(app.update(Message::ComputeFolderSizes));
    let id = run_id(&app);
    let stop = Arc::clone(&app.panels.folder_size_run.as_ref().unwrap().stop);

    load(
        &mut app,
        PathBuf::from("/home/test"),
        vec![FileEntry::parent(Path::new("/home"))],
        fs::SortKey::default(),
    );

    assert!(stop.load(Ordering::Relaxed));
    assert!(app.panels.folder_size_run.is_none());
    measured(&mut app, id, "gamma", 777);
    assert!(app.panels.folder_sizes.is_empty());
    load(&mut app, directory(), listing(), fs::SortKey::default());
    assert_eq!(shown(&app, "gamma"), "<DIR>");
}

#[tokio::test]
async fn a_reload_of_the_same_folder_keeps_the_walk() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "gamma");
    drop(app.update(Message::ComputeFolderSizes));
    let id = run_id(&app);
    let stop = Arc::clone(&app.panels.folder_size_run.as_ref().unwrap().stop);

    load(&mut app, directory(), listing(), fs::SortKey::default());

    assert!(!stop.load(Ordering::Relaxed));
    assert_eq!(run_id(&app), id);
    measured(&mut app, id, "gamma", 5);
    assert_eq!(shown(&app, "gamma"), "5 B");
}

#[tokio::test]
async fn sorted_by_size_the_cursor_stays_and_valued_folders_come_first() {
    let mut app = app_in_links();
    app.panel_mut(SIDE).request_sort(fs::SortColumn::Size, 12);
    assert_eq!(names(&app), ["..", "alpha", "beta", "gamma", "note.txt"]);
    app.panel_mut(SIDE)
        .selection
        .toggle(&directory().join("gamma"));
    put_cursor_on(&mut app, "beta");
    drop(app.update(Message::ComputeFolderSizes));
    let id = run_id(&app);

    measured(&mut app, id, "gamma", 50);

    assert_eq!(names(&app), ["..", "gamma", "alpha", "beta", "note.txt"]);
    assert_eq!(
        app.panel(SIDE).selected_entry().unwrap().name,
        "beta",
        "the cursor follows its entry"
    );
}

#[tokio::test]
async fn cached_values_are_in_size_order_when_the_folder_loads_again() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "gamma");
    drop(app.update(Message::ComputeFolderSizes));
    let id = run_id(&app);
    measured(&mut app, id, "gamma", 50);
    app.panel_mut(SIDE).request_sort(fs::SortColumn::Size, 12);

    let by_name = fs::SortKey::default();
    let claimed = app.panel(SIDE).sort;
    assert_ne!(by_name, claimed);
    load(&mut app, directory(), listing(), claimed);

    assert_eq!(names(&app), ["..", "gamma", "alpha", "beta", "note.txt"]);
}

#[tokio::test]
async fn a_panel_inside_an_archive_does_not_start_a_run() {
    let mut app = app_in_links();
    put_cursor_on(&mut app, "alpha");
    app.panel_mut(SIDE).archive = Some(directory().join("fotos.zip"));

    drop(app.update(Message::ComputeFolderSizes));

    assert!(app.panels.folder_size_run.is_none());
}
