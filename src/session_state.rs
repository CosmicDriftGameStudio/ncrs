//! What ncrs remembers between runs: the folders and sort of both panels, the
//! active panel and the window size.
//!
//! Kept apart from the config on purpose. The config is read-only for ncrs and
//! belongs to the user; this file is written by ncrs on every quit and shortly
//! after changes, and may be deleted at any time to start fresh. Where it lives
//! is decided once, by `config::default_state_path`.
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::fs::remote::RemotePath;
use crate::fs::{archive, SortColumn, SortKey};
use crate::messages::PanelSide;
use crate::ui::panel_history::PanelHistory;

/// Written on save so a later layout can tell old files from new ones.
pub const STATE_VERSION: u32 = 1;

/// Larger than any real state file; guards against reading something that is
/// not one.
const MAX_STATE_BYTES: u64 = 1024 * 1024;

/// Smaller than this is a minimised window (Windows reports 0×0), not a size
/// to come back to.
pub const MIN_RESTORED_SIZE: iced::Size = iced::Size::new(400.0, 300.0);

/// Orders the writes of one app. The debounced write runs on a background
/// thread and may still be waiting when ncrs quits; it must not replace the
/// newer state written at quit.
#[derive(Debug, Default)]
pub struct WriteOrder {
    newest_written: Mutex<u64>,
}

impl WriteOrder {
    /// The generation of the write at quit: later than any debounced one.
    pub const FINAL: u64 = u64::MAX;

    /// Writes `state` unless a write of a later generation came first.
    pub fn save(&self, generation: u64, path: &Path, state: &SessionState) -> io::Result<()> {
        let mut newest = self
            .newest_written
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if generation < *newest {
            return Ok(());
        }
        save(path, state)?;
        *newest = generation;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<PanelSide>,
    /// Plain values before the tables, so the file keeps its shape.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub favourites: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSize>,
    pub left: PanelRecord,
    pub right: PanelRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub sort: SortColumn,
    pub descending: bool,
    /// The folders Back goes through, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub back: Vec<String>,
    /// The folders Forward goes through; the next one is the last.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forward: Vec<String>,
    /// The name of the entry under the cursor; none for an empty folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// The folder as text. A non-UTF-8 folder is simply not remembered: TOML has
/// only UTF-8 strings.
pub fn record_path(path: &Path) -> Option<String> {
    // A server is not remembered: its address could not be opened again
    // without the password, and the file must never hold one.
    if RemotePath::parse(path).is_some() {
        return None;
    }
    path.to_str().map(str::to_owned)
}

/// The folders as text, dropping those that are not UTF-8.
pub fn record_paths<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Vec<String> {
    paths
        .into_iter()
        .filter_map(|path| record_path(path))
        .collect()
}

/// The folder a history entry stands for: a place inside an archive is the
/// folder holding the archive, because the state file must not make a restart
/// open an archive, which may be gone or replaced by then.
pub fn history_place(path: &Path) -> PathBuf {
    // A place on a server is no path on this computer: the disk is not asked.
    if RemotePath::parse(path).is_some() {
        return path.to_path_buf();
    }
    // Nearly every entry is a plain folder, so the disk is asked only when the
    // entry or a name above it looks like an archive.
    let may_be_inside = path
        .ancestors()
        .any(|ancestor| ancestor.file_name().is_some_and(archive::is_archive_name));
    if may_be_inside {
        if let Some(location) = archive::locate_in_archive(path) {
            if let Some(folder) = location.archive.parent() {
                return folder.to_path_buf();
            }
        }
    }
    path.to_path_buf()
}

/// A history stack as text, with its archive places mapped to real folders and
/// the repeats that leaves next to each other dropped.
pub fn record_history<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Vec<String> {
    let mut places: Vec<PathBuf> = Vec::new();
    for path in paths {
        let place = history_place(path);
        if places.last() != Some(&place) {
            places.push(place);
        }
    }
    record_paths(&places)
}

/// Reads the state. A missing, unreadable or broken file gives the defaults and
/// never stops ncrs from starting.
pub fn load(path: &Path) -> SessionState {
    let fall_back = |reason: String| {
        eprintln!(
            "ncrs: {}: {reason}; starting with the defaults",
            path.display()
        );
        SessionState::default()
    };
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return SessionState::default(),
        Err(err) => return fall_back(err.to_string()),
    };
    if size > MAX_STATE_BYTES {
        return fall_back(format!("{size} bytes is too large for a state file"));
    }
    match std::fs::read_to_string(path) {
        Ok(text) => match toml_edit::de::from_str(&text) {
            Ok(state) => state,
            Err(err) => fall_back(err.to_string()),
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => SessionState::default(),
        Err(err) => fall_back(err.to_string()),
    }
}

/// Writes the state atomically, creating its folder if needed.
pub fn save(path: &Path, state: &SessionState) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml_edit::ser::to_string_pretty(state).map_err(io::Error::other)?;
    crate::config::replace_file_atomically(path, ".state.toml.", &text).map_err(|(_, err)| err)
}

/// The first of `path` and its ancestors that is a folder. Relative paths are
/// refused: they would mean whatever the working directory is at that moment.
///
/// A place inside an archive file is returned as it is: the archive is what
/// exists, and the listing says if the folder in it does not.
pub fn nearest_existing_directory(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let nearest = path
        .ancestors()
        .find(|candidate| candidate.is_dir() || archive::is_archive_file(candidate))?;
    if nearest.is_dir() {
        Some(nearest.to_path_buf())
    } else {
        Some(path.to_path_buf())
    }
}

impl SessionState {
    /// The remembered window size, when it is one worth coming back to.
    pub fn window_size(&self) -> Option<iced::Size> {
        self.window?.restorable()
    }
}

impl WindowSize {
    pub fn restorable(self) -> Option<iced::Size> {
        let usable = self.width.is_finite()
            && self.height.is_finite()
            && self.width >= MIN_RESTORED_SIZE.width
            && self.height >= MIN_RESTORED_SIZE.height;
        usable.then(|| iced::Size::new(self.width, self.height))
    }
}

impl PanelRecord {
    /// The folder to open, or its nearest ancestor that still exists.
    pub fn start_directory(&self) -> Option<PathBuf> {
        nearest_existing_directory(Path::new(self.path.as_deref()?))
    }

    /// The entry to put the cursor on in the first listing. Only for the saved
    /// folder itself: a name from a folder that has gone says nothing about its
    /// parent.
    pub fn start_cursor(&self) -> Option<String> {
        let saved = Path::new(self.path.as_deref()?);
        (self.start_directory()? == saved)
            .then(|| self.cursor.clone())
            .flatten()
    }

    /// The saved Back and Forward stacks, capped again.
    pub fn history(&self) -> PanelHistory {
        // Mapped again on the way in: a file written before archive places
        // were mapped on the way out may still hold them.
        let paths = |saved: &[String]| {
            let places: Vec<PathBuf> = saved.iter().map(PathBuf::from).collect();
            record_history(&places)
                .into_iter()
                .map(PathBuf::from)
                .collect()
        };
        PanelHistory::restored(paths(&self.back), paths(&self.forward))
    }

    pub fn sort_key(&self) -> SortKey {
        SortKey {
            column: self.sort,
            descending: self.descending,
        }
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn sample(folder: &Path) -> SessionState {
        SessionState {
            version: STATE_VERSION,
            active: Some(PanelSide::Right),
            window: Some(WindowSize {
                width: 1024.0,
                height: 768.0,
            }),
            left: PanelRecord {
                path: record_path(folder),
                sort: SortColumn::Size,
                descending: true,
                back: vec!["/a".to_string(), "/b".to_string()],
                forward: vec!["/c".to_string()],
                cursor: Some("notes.txt".to_string()),
            },
            favourites: vec!["/fav/one".to_string(), "/fav/two".to_string()],
            right: PanelRecord::default(),
        }
    }

    #[test]
    fn a_saved_state_loads_back_equal() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("state.toml");
        let state = sample(scratch.path());

        save(&file, &state).unwrap();

        assert_eq!(load(&file), state);
    }

    #[test]
    fn a_state_saved_before_the_cursor_was_remembered_still_loads() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("state.toml");
        std::fs::write(&file, "version = 1\n[left]\npath = \"/x\"\n").unwrap();

        let state = load(&file);

        assert_eq!(state.left.path.as_deref(), Some("/x"));
        assert_eq!(state.left.cursor, None);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let scratch = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&scratch.path().join("state.toml")),
            SessionState::default()
        );
    }

    #[test]
    fn garbage_gives_the_defaults() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("state.toml");
        std::fs::write(&file, "this is [not toml").unwrap();
        assert_eq!(load(&file), SessionState::default());
    }

    #[test]
    fn a_wrongly_typed_field_gives_the_defaults() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("state.toml");
        std::fs::write(&file, "version = 1\nleft = 5\n").unwrap();
        assert_eq!(load(&file), SessionState::default());
    }

    #[test]
    fn an_oversized_file_gives_the_defaults() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("state.toml");
        let padding = "# x\n".repeat(300_000);
        std::fs::write(&file, format!("version = 1\n{padding}")).unwrap();
        assert!(std::fs::metadata(&file).unwrap().len() > MAX_STATE_BYTES);
        assert_eq!(load(&file), SessionState::default());
    }

    #[test]
    fn saving_creates_the_folder_replaces_the_file_and_leaves_no_temporaries() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("not").join("yet");
        let file = folder.join("state.toml");
        let mut state = sample(scratch.path());

        save(&file, &state).unwrap();
        state.active = Some(PanelSide::Left);
        save(&file, &state).unwrap();

        assert_eq!(load(&file), state);
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["state.toml"]);
    }

    #[test]
    fn a_debounced_write_after_the_final_one_is_dropped() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("state.toml");
        let order = WriteOrder::default();
        let earlier = sample(scratch.path());
        let at_quit = SessionState {
            active: Some(PanelSide::Left),
            ..earlier.clone()
        };

        order.save(1, &file, &earlier).unwrap();
        order.save(WriteOrder::FINAL, &file, &at_quit).unwrap();
        order.save(2, &file, &earlier).unwrap();

        assert_eq!(load(&file), at_quit);
    }

    #[test]
    fn a_deleted_folder_falls_back_to_its_nearest_existing_parent() {
        let scratch = tempfile::tempdir().unwrap();
        let gone = scratch.path().join("gone").join("deeper");
        assert_eq!(
            nearest_existing_directory(&gone),
            Some(scratch.path().to_path_buf())
        );
        assert_eq!(nearest_existing_directory(Path::new("relative/dir")), None);
    }

    #[test]
    fn a_place_in_an_archive_stays_where_it_is() {
        use crate::fs::archive::fixtures::{write_archive, Item};
        let scratch = tempfile::tempdir().unwrap();
        let archive = scratch.path().join("fotos.zip");
        write_archive(&archive, &[Item::File("2024/a.jpg", b"a")]);
        let inside = archive.join("2024").join("gone");

        assert_eq!(nearest_existing_directory(&inside), Some(inside.clone()));
        assert_eq!(nearest_existing_directory(&archive), Some(archive.clone()));
        let plain_file = scratch.path().join("plain.txt");
        std::fs::write(&plain_file, b"x").unwrap();
        assert_eq!(
            nearest_existing_directory(&plain_file.join("below")),
            Some(scratch.path().to_path_buf()),
            "only an archive makes a file a place"
        );
    }

    #[test]
    fn history_places_in_an_archive_become_the_folder_holding_it_without_repeats() {
        use crate::fs::archive::fixtures::{write_archive, Item};
        let scratch = tempfile::tempdir().unwrap();
        let archive = scratch.path().join("fotos.zip");
        write_archive(&archive, &[Item::File("2024/a.jpg", b"a")]);
        let other = scratch.path().join("other");
        std::fs::create_dir(&other).unwrap();

        let history = [
            scratch.path().to_path_buf(),
            archive.clone(),
            archive.join("2024"),
            other.clone(),
        ];
        assert_eq!(
            record_history(&history),
            record_paths(&[scratch.path().to_path_buf(), other]),
            "the three entries on and in the archive collapse into one"
        );
    }

    #[test]
    fn going_back_after_a_restart_never_lands_in_an_archive() {
        use crate::fs::archive::fixtures::{write_archive, Item};
        let scratch = tempfile::tempdir().unwrap();
        let archive = scratch.path().join("fotos.zip");
        write_archive(&archive, &[Item::File("2024/a.jpg", b"a")]);

        let mut history = PanelHistory::default();
        history.visit(scratch.path().to_path_buf());
        history.visit(archive.clone());
        history.visit(archive.join("2024"));
        let state_file = scratch.path().join("state.toml");
        let state = SessionState {
            left: PanelRecord {
                back: record_history(history.back()),
                forward: record_history(history.forward()),
                ..PanelRecord::default()
            },
            ..SessionState::default()
        };
        save(&state_file, &state).unwrap();

        let mut restored = load(&state_file).left.history();
        let target = restored
            .step_back(scratch.path().to_path_buf())
            .expect("a folder to go back to");
        assert!(target.is_dir(), "{} is not a folder", target.display());
        assert_eq!(target, scratch.path());
    }

    #[test]
    fn an_old_state_file_holding_archive_places_is_cleaned_on_load() {
        use crate::fs::archive::fixtures::{write_archive, Item};
        let scratch = tempfile::tempdir().unwrap();
        let archive = scratch.path().join("fotos.zip");
        write_archive(&archive, &[Item::File("2024/a.jpg", b"a")]);
        let record = PanelRecord {
            back: record_paths(&[archive.clone(), archive.join("2024")]),
            ..PanelRecord::default()
        };
        let mut history = record.history();
        assert_eq!(
            history.step_back(PathBuf::new()),
            Some(scratch.path().to_path_buf())
        );
        assert_eq!(history.step_back(PathBuf::new()), None);
    }

    #[test]
    fn a_minimised_or_broken_window_size_is_not_restored() {
        let with = |width, height| SessionState {
            window: Some(WindowSize { width, height }),
            ..SessionState::default()
        };
        assert_eq!(with(0.0, 0.0).window_size(), None);
        assert_eq!(with(f32::NAN, 800.0).window_size(), None);
        assert_eq!(with(f32::INFINITY, 800.0).window_size(), None);
        assert_eq!(
            with(900.0, 600.0).window_size(),
            Some(iced::Size::new(900.0, 600.0))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_not_utf8_is_not_remembered() {
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff"));
        assert_eq!(record_path(path), None);
    }

    #[test]
    fn a_server_is_never_written_down() {
        let server = Path::new("sftp://marc@example.org/home/marc");
        assert_eq!(record_path(server), None);
        let local = std::env::temp_dir();
        let history = vec![
            PathBuf::from("sftp://marc@example.org/a.zip"),
            local.clone(),
            PathBuf::from("sftp://marc@example.org/"),
        ];
        assert_eq!(record_history(&history), [record_path(&local).unwrap()]);
        assert_eq!(
            history_place(&history[0]),
            history[0],
            "the disk is not asked about an address"
        );
    }
}
