use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
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
    _watchers: Vec<RecommendedWatcher>,
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
                let event = match received {
                    Ok(event) => event,
                    Err(err) => {
                        let failed_root = err
                            .paths
                            .iter()
                            .find_map(|path| all_roots.iter().find(|root| path.starts_with(root)))
                            .unwrap_or(&watched.path)
                            .clone();
                        forward_events(WatchEvent::Error {
                            location: failed_root,
                            message: err.to_string(),
                        });
                        return;
                    }
                };
                if !is_file_change(event.kind) {
                    return;
                }
                for path in event_targets(&event) {
                    forward_files(&watched, path, &forward_events);
                }
            };
            // Not following links: a linked directory could lead out of the
            // location, and its files would be reported as if they were inside.
            let config = Config::default().with_follow_symlinks(false);
            let started = RecommendedWatcher::new(handler, config).and_then(|mut watcher| {
                watcher
                    .watch(&location.path, RecursiveMode::Recursive)
                    .map(|()| watcher)
            });
            match started {
                Ok(watcher) => watchers.push(watcher),
                Err(err) => failed.push((location.path.clone(), IntakeError::Watch(err))),
            }
        }
        (
            Self {
                _watchers: watchers,
            },
            failed,
        )
    }
}

/// A folder that is created or moved in brings files the backend may never
/// report one by one: a moved folder is a single event, and on Linux the watch
/// on a new folder starts after files may already have been written into it.
/// notify adds that watch only after this handler returns, so a file written
/// between the walk and the watch is left to the next scan.
fn forward_files(location: &Location, path: &Path, forward: &impl Fn(WatchEvent)) {
    let is_folder = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir());
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

fn is_file_change(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(
                ModifyKind::Any
                    | ModifyKind::Data(_)
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
