//! What the check remembers between runs: when it last asked GitHub and which
//! version the user skipped. A file of its own in the data folder next to the
//! session state, not the config the user edits.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const THROTTLE_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_version: Option<String>,
    /// The newest release the last check found, so the notice survives a
    /// restart within the 24 hours. Read back through the same check as the
    /// answer of GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found_notes_url: Option<String>,
}

impl Stored {
    /// At most one request per 24 hours. A stamp in the future (the clock was
    /// set back) does not block the check for good.
    pub fn is_due(&self, now_unix: i64) -> bool {
        self.last_checked_unix
            .is_none_or(|last| now_unix < last || now_unix - last >= THROTTLE_SECONDS)
    }
}

/// The file of the check, next to the session state file the app already knows.
pub fn file_beside(session_state_path: &Path) -> PathBuf {
    session_state_path.with_file_name("updates.toml")
}

/// A missing or damaged file is a first run.
pub fn load(path: &Path) -> Stored {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Stored::default();
    };
    toml_edit::de::from_str(&text).unwrap_or_else(|err| {
        eprintln!(
            "ncrs: update check state {} is unreadable: {err}",
            path.display()
        );
        Stored::default()
    })
}

pub fn save(path: &Path, stored: &Stored) {
    let written = toml_edit::ser::to_string(stored)
        .map_err(|err| err.to_string())
        .and_then(|text| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
            }
            write_atomically(path, |file| file.write_all(text.as_bytes()))
                .map_err(|err| err.to_string())
        });
    if let Err(err) = written {
        eprintln!(
            "ncrs: update check state {} not saved: {err}",
            path.display()
        );
    }
}

/// Writes through a new file in the same folder and renames it over `path`, so
/// a crash or a full disk never leaves a truncated state file behind: the old
/// one stays until the new one is complete on disk.
fn write_atomically(
    path: &Path,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = path.with_file_name(format!(
        ".{name}.{}-{}.part",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let finished = fill(&mut file)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&temporary, path)
        });
    if finished.is_err() {
        let _best_effort = std::fs::remove_file(&temporary);
    }
    finished
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_after_24_hours_and_not_before() {
        let stored = Stored {
            last_checked_unix: Some(1_000),
            skipped_version: None,
            ..Stored::default()
        };
        assert!(!stored.is_due(1_000));
        assert!(!stored.is_due(1_000 + THROTTLE_SECONDS - 1));
        assert!(stored.is_due(1_000 + THROTTLE_SECONDS));
    }

    #[test]
    fn a_first_run_and_a_clock_set_back_are_due() {
        assert!(Stored::default().is_due(5));
        let stored = Stored {
            last_checked_unix: Some(10_000),
            skipped_version: None,
            ..Stored::default()
        };
        assert!(stored.is_due(5));
    }

    #[test]
    fn the_state_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_beside(&dir.path().join("sub").join("state.toml"));
        assert_eq!(load(&path), Stored::default());
        let stored = Stored {
            last_checked_unix: Some(42),
            skipped_version: Some("1.3.0".to_owned()),
            found_version: Some("1.4.0".to_owned()),
            found_notes_url: Some("https://example.test/n".to_owned()),
        };
        save(&path, &stored);
        assert_eq!(load(&path), stored);
    }

    #[test]
    fn a_write_that_fails_halfway_leaves_the_old_state_and_no_leftover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updates.toml");
        std::fs::write(&path, "last_checked_unix = 7\n").unwrap();

        let result = write_atomically(&path, |file| {
            file.write_all(b"last_checked_unix = ")?;
            Err(std::io::Error::other("disk full"))
        });

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "last_checked_unix = 7\n"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_damaged_file_is_a_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updates.toml");
        std::fs::write(&path, "last_checked_unix = [").unwrap();
        assert_eq!(load(&path), Stored::default());
    }
}
