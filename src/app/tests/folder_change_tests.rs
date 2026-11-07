use crate::app::*;
use crate::fs::FileEntry;
use ncrs_files::PanelMessage;
use std::ffi::OsString;

fn entries_in(folder: &Path, names: &[&str]) -> Vec<FileEntry> {
    names
        .iter()
        .map(|name| FileEntry {
            name: OsString::from(*name),
            path: folder.join(name),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
            folder_size: None,
            details: Default::default(),
        })
        .collect()
}

fn loaded(app: &mut App, folder: &Path, names: &[&str]) {
    let request_id = app.panels.left.request_id;
    let _ = app.update(Message::Panel(PanelMessage::DirectoryLoaded {
        side: PanelSide::Left,
        request_id,
        path: folder.to_path_buf(),
        entries: entries_in(folder, names),
        error: None,
        sorted_by: app.panels.left.sort,
        select: None,
        archive: None,
    }));
}

fn app_showing(folder: &Path, names: &[&str]) -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = folder.to_path_buf();
    app.panels.left.begin_load();
    loaded(&mut app, folder, names);
    app
}

/// A change on disk reloads through the ordinary load: the cursor stays on its
/// file, a tag on a file that is still there stays, one on a vanished file goes.
#[test]
fn a_change_on_disk_keeps_cursor_and_tags_and_drops_vanished_ones() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let folder = scratch.path().to_path_buf();
    let mut app = app_showing(&folder, &["a", "b", "c", "d"]);
    app.panels.left.selected = 2;
    app.panels.left.selection.toggle(&folder.join("b"));
    app.panels.left.selection.toggle(&folder.join("d"));
    let before = app.panels.left.request_id;

    let _ = app.update(Message::Panel(PanelMessage::FolderChangedOnDisk(
        PanelSide::Left,
    )));
    assert!(app.panels.left.loading, "no reload was started");
    assert_ne!(app.panels.left.request_id, before);

    // "b" was deleted by another program, "new" was added.
    loaded(&mut app, &folder, &["a", "c", "d", "new"]);

    let panel = &app.panels.left;
    assert_eq!(panel.entries[panel.selected].name, "c");
    assert!(!panel.selection.is_tagged(&folder.join("b")));
    assert!(panel.selection.is_tagged(&folder.join("d")));
}

/// A change that arrives while a read is in flight is not lost: the folder is
/// read once more when that read is in.
#[test]
fn a_change_during_a_read_reloads_after_it() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let folder = scratch.path().to_path_buf();
    let mut app = app_showing(&folder, &["a", "b"]);
    app.panels.left.begin_load();
    let in_flight = app.panels.left.request_id;

    let _ = app.update(Message::Panel(PanelMessage::FolderChangedOnDisk(
        PanelSide::Left,
    )));
    assert_eq!(
        app.panels.left.request_id, in_flight,
        "a second read was started on top of the one in flight"
    );

    loaded(&mut app, &folder, &["a", "b"]);
    assert_ne!(
        app.panels.left.request_id, in_flight,
        "the change was forgotten"
    );
    assert!(app.panels.left.loading);
}

/// In a folder longer than the window, the rows in view stay where they were.
#[test]
fn a_change_on_disk_keeps_the_scroll_position_of_a_long_folder() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let folder = scratch.path().to_path_buf();
    let names: Vec<String> = (0..200).map(|n| format!("f{n:03}")).collect();
    let all: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut app = app_showing(&folder, &all);
    app.panels.left.selected = 150;
    app.panels.left.scroll_offset = 140;

    let _ = app.update(Message::Panel(PanelMessage::FolderChangedOnDisk(
        PanelSide::Left,
    )));
    let without_f010: Vec<&str> = all.iter().copied().filter(|name| *name != "f010").collect();
    loaded(&mut app, &folder, &without_f010);

    let panel = &app.panels.left;
    assert_eq!(panel.entries[panel.selected].name, "f150");
    assert_eq!(panel.scroll_offset, 140);
}
