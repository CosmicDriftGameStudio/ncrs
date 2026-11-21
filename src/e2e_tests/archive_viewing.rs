use super::*;
use crate::fs::archive::fixtures::{write_archive, Item};
use crate::i18n::{Language, Msg};
use iced::keyboard::key::Named;
use std::path::PathBuf;
use std::sync::Arc;

const TEXT: &[u8] = b"plain words\n";
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
const FORMATS: [&str; 3] = ["view.zip", "view.tar", "view.7z"];

/// A panel inside `file` with `a.txt` and `b.jpg` in it, and the cache the
/// copies go to.
fn inside(
    file: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<RecordingLauncher>,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let dir = scratch("archive-view");
    let cache = scratch("archive-view-cache");
    write_archive(
        &dir.path().join(file),
        &[Item::File("a.txt", TEXT), Item::File("b.jpg", JPEG)],
    );
    let launcher = Arc::new(RecordingLauncher::default());
    let recorder = Arc::clone(&launcher);
    let shared: Arc<dyn crate::fs::Launcher> = recorder;
    let mut session = session_with_view_copies(dir.path(), &cache.path().join("view"), shared);
    cursor_to(&mut session, file);
    session.press(Key::Named(Named::Enter));
    (dir, cache, launcher, session)
}

/// What is in the one folder this run made in `cache`.
fn run_folder(cache: &Path) -> Vec<String> {
    let runs: Vec<_> = std::fs::read_dir(cache)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(runs[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("run-"));
    let mut names: Vec<String> = std::fs::read_dir(&runs[0])
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn started_path(launcher: &RecordingLauncher, index: usize) -> PathBuf {
    PathBuf::from(launcher.calls()[index].args.last().unwrap())
}

#[test]
fn f3_views_a_copy_with_the_content_of_the_entry_in_zip_tar_and_7z() {
    for file in FORMATS {
        let (_dir, cache, launcher, mut session) = inside(file);
        for (index, (name, content)) in [("a.txt", TEXT), ("b.jpg", JPEG)].iter().enumerate() {
            cursor_to(&mut session, name);

            session.press(Key::Named(Named::F3));

            assert_eq!(launcher.calls().len(), index + 1, "{file} {name}");
            let copy = started_path(&launcher, index);
            assert!(copy.starts_with(cache.path()), "{file} {name}: {copy:?}");
            assert_eq!(copy.file_name().unwrap(), *name, "{file}");
            assert_eq!(std::fs::read(&copy).unwrap(), *content, "{file} {name}");
        }
        assert!(session.app.job_is_idle(), "{file}");
        assert!(session.app.job_error_for_test().is_none(), "{file}");
    }
}

#[test]
fn the_copies_are_removed_when_the_panel_leaves_the_archive() {
    let (_dir, cache, launcher, mut session) = inside("view.zip");
    cursor_to(&mut session, "a.txt");
    session.press(Key::Named(Named::F3));
    let copy = started_path(&launcher, 0);
    assert!(copy.exists());
    assert_eq!(
        run_folder(&cache.path().join("view")).len(),
        2,
        "the lock and one slot"
    );

    session.send(Message::Panel(PanelMessage::GoUp));

    assert!(session.app.panel(PanelSide::Left).archive.is_none());
    assert!(!copy.exists(), "the copy outlived the archive panel");
    assert_eq!(run_folder(&cache.path().join("view")), [".lock"]);
}

#[test]
fn the_copies_stay_while_the_archive_is_open() {
    let (_dir, cache, launcher, mut session) = inside("view.tar");
    for name in ["a.txt", "b.jpg"] {
        cursor_to(&mut session, name);
        session.press(Key::Named(Named::F3));
    }

    assert!(started_path(&launcher, 0).exists());
    assert!(started_path(&launcher, 1).exists());
    assert_eq!(run_folder(&cache.path().join("view")).len(), 3);
}

#[test]
fn f4_in_an_archive_says_it_is_read_only_and_unpacks_nothing() {
    let (_dir, cache, launcher, mut session) = inside("view.zip");
    cursor_to(&mut session, "a.txt");

    session.press(Key::Named(Named::F4));

    assert_eq!(
        session.app.job_error_for_test(),
        Some(Language::default().text(Msg::StatusArchiveReadOnly))
    );
    assert!(launcher.calls().is_empty());
    assert_eq!(run_folder(&cache.path().join("view")), [".lock"]);
}

#[test]
fn escape_stops_an_unpacking_and_no_viewer_starts() {
    let (_dir, _cache, launcher, mut session) = inside("view.zip");
    cursor_to(&mut session, "a.txt");
    let held = session.send_held(Message::OpenExternal(crate::fs::OpenKind::View));
    assert!(session.app.is_extracting_for_test());
    assert!(!session.app.job_is_idle());

    session.press(Key::Named(Named::Escape));

    assert!(!session.app.is_extracting_for_test());
    assert!(session.app.job_is_idle());
    session.drive(held);
    session.settle();
    assert!(
        launcher.calls().is_empty(),
        "the aborted job started a viewer"
    );
    assert!(session.app.job_is_idle());
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    // The unpacking renames its staging folder while this looks; a folder that
    // is gone by the time it is read has nothing left to find.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        }
        found.push(path);
    }
    found
}

/// The thread has put the copy in its slot, the staging folder is gone;
/// waits for that, so the panel leaves with the copy on disk and its
/// result still on the way.
fn wait_for_a_copy(cache: &Path) {
    let in_a_slot = |path: &PathBuf| {
        path.file_name().is_some_and(|name| name == "a.txt")
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|parent| parent.to_string_lossy().starts_with("slot-"))
    };
    let staging = |path: &PathBuf| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".ncrs-partial-"))
    };
    for _ in 0..1000 {
        let found = walk(cache);
        if found.iter().any(in_a_slot) && !found.iter().any(staging) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the copy never appeared");
}

#[test]
fn leaving_the_archive_while_it_unpacks_leaves_no_copy_and_starts_no_viewer() {
    let (_dir, cache, launcher, mut session) = inside("view.zip");
    cursor_to(&mut session, "a.txt");
    let held = session.send_held(Message::OpenExternal(crate::fs::OpenKind::View));
    wait_for_a_copy(&cache.path().join("view"));

    session.send(Message::Panel(PanelMessage::GoUp));

    assert!(!session.app.is_extracting_for_test());
    assert!(session.app.job_is_idle());
    assert_eq!(run_folder(&cache.path().join("view")), [".lock"]);
    session.drive(held);
    session.settle();
    assert!(launcher.calls().is_empty());
    assert_eq!(run_folder(&cache.path().join("view")), [".lock"]);
}
