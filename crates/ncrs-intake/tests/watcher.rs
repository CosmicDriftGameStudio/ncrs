use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use ncrs_intake::{Location, LocationMode, LocationWatcher, WatchEvent};
use tempfile::TempDir;

const INVOICE: &[u8] = b"Invoice 4711 from Company A";
const REMINDER: &[u8] = b"Reminder 12 from Agency B";

fn location(dir: &TempDir, mode: LocationMode, images: bool) -> Location {
    Location::new(dir.path(), mode, images).unwrap()
}

/// `relative` is written with `/`; each part is joined so the separators
/// are the ones the walk reports on Windows too.
fn below(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn put(location: &Location, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = below(&location.path, relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, bytes).unwrap();
    path
}

/// Real file writes seen by the polling backend; FSEvents delivery is not
/// reliable under load.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn receive_until(receiver: &Receiver<WatchEvent>, wanted: &Path) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    loop {
        let event = receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("no event for {} after {seen:?}", wanted.display()));
        let WatchEvent::File(path) = event else {
            panic!("unexpected {event:?}");
        };
        let reached = path == wanted;
        seen.push(path);
        if reached {
            return seen;
        }
    }
}

#[test]
fn the_watcher_forwards_new_files_and_not_hidden_ones() {
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox, false);
    let (_watcher, receiver, failed) =
        LocationWatcher::start_polling(std::slice::from_ref(&inbox), POLL_INTERVAL);
    assert!(failed.is_empty());

    let first = put(&inbox, "first.pdf", INVOICE);
    let seen = receive_until(&receiver, &first);
    assert!(seen.iter().all(|path| *path == first), "{seen:?}");

    let hidden = put(&inbox, ".hidden.pdf", REMINDER);
    let barrier = put(&inbox, "barrier.pdf", b"barrier");
    let after_hidden = receive_until(&receiver, &barrier);
    assert!(!after_hidden.contains(&hidden), "{after_hidden:?}");
}

#[test]
fn the_watcher_forwards_the_files_of_a_folder_moved_in() {
    let outside = TempDir::new().unwrap();
    let folder = outside.path().join("batch");
    fs::create_dir_all(folder.join("nested")).unwrap();
    fs::write(folder.join("a.pdf"), INVOICE).unwrap();
    fs::write(folder.join("nested/b.pdf"), REMINDER).unwrap();
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox, false);
    let (_watcher, receiver, failed) =
        LocationWatcher::start_polling(std::slice::from_ref(&inbox), POLL_INTERVAL);
    assert!(failed.is_empty());

    fs::rename(&folder, inbox.path.join("batch")).unwrap();

    let first = inbox.path.join("batch/a.pdf");
    let nested = inbox.path.join("batch/nested/b.pdf");
    let mut seen = receive_until(&receiver, &nested);
    if !seen.contains(&first) {
        seen.extend(receive_until(&receiver, &first));
    }
    assert!(seen.contains(&first) && seen.contains(&nested), "{seen:?}");
}

#[test]
fn a_location_that_cannot_be_watched_is_reported_and_the_others_carry_on() {
    let gone_dir = TempDir::new().unwrap();
    let gone = location(&gone_dir, LocationMode::Archive, false);
    let gone_path = gone.path.clone();
    drop(gone_dir);
    let dir = TempDir::new().unwrap();
    let inbox = location(&dir, LocationMode::Inbox, false);

    let (_watcher, receiver, failed) =
        LocationWatcher::start_polling(&[gone, inbox.clone()], POLL_INTERVAL);

    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, gone_path);
    let file = put(&inbox, "a.pdf", INVOICE);
    receive_until(&receiver, &file);
}
