use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use notify::event::{MetadataKind, ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use walkdir::WalkDir;

use crate::classify::classify;
use crate::error::IntakeError;
use crate::location::Location;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    File(PathBuf),
    /// The watcher backend failed for this location (event overflow, FSEvents
    /// errors), or a folder that appeared in it could not be read. The runner
    /// must rescan the location, because files may have been missed.
    Error {
        location: PathBuf,
        message: String,
    },
}

/// Watches the locations for files that appear or change. Dropping it stops
/// the watching.
#[derive(Debug)]
pub struct LocationWatcher {
    _backend: Backend,
}

#[derive(Debug)]
enum Backend {
    Native { _watchers: Vec<RecommendedWatcher> },
    Polling { _watchers: Vec<PollWatcher> },
}

impl LocationWatcher {
    /// The receiver gets every created, written or renamed-to file that passes
    /// `classify`, and the backend errors. Nothing is hashed or indexed on the watcher
    /// thread; the caller does that with `take_in`.
    ///
    /// A location that cannot be watched is reported and the others carry on:
    /// on Linux a large archive can exceed `max_user_watches`. Such a location
    /// is then only seen by the scan at start.
    pub fn start(
        locations: &[Location],
    ) -> (Self, Receiver<WatchEvent>, Vec<(PathBuf, IntakeError)>) {
        let (sender, receiver) = mpsc::channel();
        let (watcher, failed) = Self::start_forwarding(locations, move |event| {
            // The receiver is gone when the app shuts down.
            let _ = sender.send(event);
        });
        (watcher, receiver, failed)
    }

    /// Like `start`, but every event is handed to `forward`, which is called on
    /// the backend's threads and must not block for long.
    pub fn start_forwarding(
        locations: &[Location],
        forward: impl Fn(WatchEvent) + Clone + Send + 'static,
    ) -> (Self, Vec<(PathBuf, IntakeError)>) {
        let (watchers, failed) = watch_locations::<RecommendedWatcher>(
            locations,
            Config::default().with_follow_symlinks(false),
            &forward,
        );
        (
            Self {
                _backend: Backend::Native {
                    _watchers: watchers,
                },
            },
            failed,
        )
    }

    /// Like `start`, but compares the folders every `interval` instead of
    /// relying on change notifications of the file system. For file systems
    /// that send none, such as network shares. A rewrite is seen by its
    /// modification time, which has a resolution of one second.
    pub fn start_polling(
        locations: &[Location],
        interval: Duration,
    ) -> (Self, Receiver<WatchEvent>, Vec<(PathBuf, IntakeError)>) {
        let (sender, receiver) = mpsc::channel();
        let (watcher, failed) = Self::start_polling_forwarding(locations, interval, move |event| {
            // The receiver is gone when the app shuts down.
            let _ = sender.send(event);
        });
        (watcher, receiver, failed)
    }

    /// Like `start_polling`, but every event is handed to `forward`, which is
    /// called on the polling threads and must not block for long.
    pub fn start_polling_forwarding(
        locations: &[Location],
        interval: Duration,
        forward: impl Fn(WatchEvent) + Clone + Send + 'static,
    ) -> (Self, Vec<(PathBuf, IntakeError)>) {
        let (watchers, failed) = watch_locations::<PollWatcher>(
            locations,
            Config::default()
                .with_follow_symlinks(false)
                .with_poll_interval(interval),
            &forward,
        );
        (
            Self {
                _backend: Backend::Polling {
                    _watchers: watchers,
                },
            },
            failed,
        )
    }
}

/// `PollWatcher::watch` accepts a path that does not exist and reports it as
/// an event instead, so the path is checked here for every backend.
fn watch_locations<W: Watcher>(
    locations: &[Location],
    config: Config,
    forward: &(impl Fn(WatchEvent) + Clone + Send + 'static),
) -> (Vec<W>, Vec<(PathBuf, IntakeError)>) {
    let roots: Vec<PathBuf> = locations
        .iter()
        .map(|location| location.path.clone())
        .collect();
    let mut watchers = Vec::new();
    let mut failed = Vec::new();
    for location in locations {
        let forward_events = forward.clone();
        let watched = location.clone();
        let all_roots = roots.clone();
        let handler = move |received: notify::Result<Event>| {
            forward_received(received, &watched, &all_roots, &forward_events);
        };
        let started = fs::metadata(&location.path)
            .map_err(|err| match err.kind() {
                io::ErrorKind::NotFound => notify::Error::path_not_found(),
                _ => notify::Error::io(err),
            })
            .and_then(|_| W::new(handler, config))
            .and_then(|mut watcher| {
                watcher
                    .watch(&location.path, RecursiveMode::Recursive)
                    .map(|()| watcher)
            });
        match started {
            Ok(watcher) => watchers.push(watcher),
            Err(err) => failed.push((
                location.path.clone(),
                IntakeError::Watch(err.add_path(location.path.clone())),
            )),
        }
    }
    (watchers, failed)
}

fn forward_received(
    received: notify::Result<Event>,
    watched: &Location,
    all_roots: &[PathBuf],
    forward: &impl Fn(WatchEvent),
) {
    let event = match received {
        Ok(event) => event,
        Err(err) => {
            let failed_root = err
                .paths
                .iter()
                .find_map(|path| all_roots.iter().find(|root| path.starts_with(root)))
                .unwrap_or(&watched.path)
                .clone();
            forward(WatchEvent::Error {
                location: failed_root,
                message: err.to_string(),
            });
            return;
        }
    };
    // The backend dropped events (FSEvents MustScanSubDirs/UserDropped/
    // KernelDropped, inotify overflow): any file may have been missed.
    if event.need_rescan() {
        let message = match event.info() {
            Some(info) => format!("the system dropped file events ({info})"),
            None => "the system dropped file events".to_owned(),
        };
        forward(WatchEvent::Error {
            location: watched.path.clone(),
            message,
        });
        return;
    }
    if !is_file_change(event.kind) {
        return;
    }
    let expands_folders = !matches!(event.kind, EventKind::Modify(ModifyKind::Metadata(_)));
    for path in event_targets(&event) {
        forward_files(watched, path, expands_folders, forward);
    }
}

/// A folder that is created or moved in brings files the backend may never
/// report one by one: a moved folder is a single event, and on Linux the watch
/// on a new folder starts after files may already have been written into it.
/// notify adds that watch only after this handler returns, so a file written
/// between the walk and the watch is left to the next scan.
/// A changed modification time of a folder only says that something in it was
/// added or removed, which has its own event; it does not expand.
fn forward_files(
    location: &Location,
    path: &Path,
    expands_folders: bool,
    forward: &impl Fn(WatchEvent),
) {
    let is_folder = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir());
    if is_folder && !expands_folders {
        return;
    }
    let files: Vec<PathBuf> = if is_folder {
        let mut found = Vec::new();
        for step in WalkDir::new(path).follow_links(false) {
            match step {
                Ok(entry) if !entry.file_type().is_dir() => found.push(entry.into_path()),
                Ok(_folder) => {}
                Err(err) => {
                    forward(WatchEvent::Error {
                        location: location.path.clone(),
                        message: err.to_string(),
                    });
                }
            }
        }
        found
    } else {
        vec![path.to_path_buf()]
    };
    for file in files {
        if classify(location, &file).is_ok() {
            forward(WatchEvent::File(file));
        }
    }
}

/// A new modification time is the only change a polling backend reports for a
/// rewritten file; other metadata changes (permissions, attributes) are not a
/// new version of the file.
fn is_file_change(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(
                ModifyKind::Any
                    | ModifyKind::Data(_)
                    | ModifyKind::Metadata(MetadataKind::WriteTime)
                    | ModifyKind::Name(RenameMode::To | RenameMode::Both | RenameMode::Any)
            )
    )
}

/// A rename reported with both names lists the old one first; only the new one
/// is a file to look at.
fn event_targets(event: &Event) -> &[PathBuf] {
    match event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            event.paths.get(1..).unwrap_or_default()
        }
        _ => &event.paths,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::cell::RefCell;

    use notify::event::Flag;
    use tempfile::TempDir;

    use super::*;
    use crate::location::LocationMode;

    fn forwarded(dropped: Event) -> (PathBuf, Vec<WatchEvent>) {
        let dir = TempDir::new().unwrap();
        let watched = Location::new(dir.path(), LocationMode::Inbox, false).unwrap();
        let seen = RefCell::new(Vec::new());
        forward_received(
            Ok(dropped),
            &watched,
            std::slice::from_ref(&watched.path),
            &|event| {
                seen.borrow_mut().push(event);
            },
        );
        (watched.path, seen.into_inner())
    }

    #[test]
    fn dropped_events_are_reported_as_an_error_for_the_watched_location() {
        let event = Event::new(EventKind::Other).set_flag(Flag::Rescan);

        let (watched, seen) = forwarded(event);

        assert_eq!(
            seen,
            vec![WatchEvent::Error {
                location: watched,
                message: "the system dropped file events".to_owned(),
            }]
        );
    }

    fn forwarded_for_file(kind: EventKind, name: &str) -> Vec<WatchEvent> {
        let dir = TempDir::new().unwrap();
        let watched = Location::new(dir.path(), LocationMode::Inbox, false).unwrap();
        let file = watched.path.join(name);
        fs::write(&file, b"content").unwrap();
        let seen = RefCell::new(Vec::new());
        forward_received(
            Ok(Event::new(kind).add_path(file)),
            &watched,
            std::slice::from_ref(&watched.path),
            &|event| {
                seen.borrow_mut().push(event);
            },
        );
        seen.into_inner()
            .into_iter()
            .map(|event| match event {
                WatchEvent::File(path) => {
                    WatchEvent::File(path.strip_prefix(&watched.path).unwrap().to_path_buf())
                }
                other => other,
            })
            .collect()
    }

    #[test]
    fn the_events_of_a_polling_backend_are_file_changes_like_native_ones() {
        use notify::event::{CreateKind, DataChange, RemoveKind};

        let expected = vec![WatchEvent::File(PathBuf::from("a.pdf"))];
        for kind in [
            EventKind::Create(CreateKind::Any),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        ] {
            assert_eq!(forwarded_for_file(kind, "a.pdf"), expected, "{kind:?}");
        }
        for kind in [
            EventKind::Remove(RemoveKind::Any),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions)),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
        ] {
            assert!(forwarded_for_file(kind, "a.pdf").is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn a_new_modification_time_of_a_folder_does_not_report_its_files() {
        use notify::event::CreateKind;

        let dir = TempDir::new().unwrap();
        let watched = Location::new(dir.path(), LocationMode::Inbox, false).unwrap();
        fs::write(watched.path.join("a.pdf"), b"content").unwrap();
        let forward_for = |kind: EventKind| {
            let seen = RefCell::new(Vec::new());
            forward_received(
                Ok(Event::new(kind).add_path(watched.path.clone())),
                &watched,
                std::slice::from_ref(&watched.path),
                &|event| {
                    seen.borrow_mut().push(event);
                },
            );
            seen.into_inner()
        };

        assert!(forward_for(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::WriteTime
        )))
        .is_empty());
        assert_eq!(
            forward_for(EventKind::Create(CreateKind::Folder)),
            vec![WatchEvent::File(watched.path.join("a.pdf"))]
        );
    }

    #[test]
    fn an_other_event_without_the_rescan_flag_reports_nothing() {
        let (_, seen) = forwarded(Event::new(EventKind::Other));

        assert!(seen.is_empty());
    }
}
