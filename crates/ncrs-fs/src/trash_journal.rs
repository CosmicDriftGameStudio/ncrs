//! The record of what ncrs moved to the trash, for systems that give an app no
//! list of their trash folder (macOS without Full Disk Access).
//!
//! The file is only a hint where things were. Every restore checks it against
//! the disk first, so a manipulated file cannot make ncrs move anything but an
//! item that really sits in the trash folder, and only to the place it names.

use std::cmp::Reverse;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::delete::{RestoreError, Trash, TrashReach, TrashedItem};
use crate::replace_file::replace_file_atomically;

const JOURNAL_VERSION: u32 = 1;

/// The oldest entries fall off first.
const MAX_ENTRIES: usize = 1000;

/// What the platform does with the trash itself.
pub trait TrashBackend: Send + Sync {
    /// Moves `path` to the trash and returns where it landed.
    fn move_to_trash(&self, path: &Path) -> Result<PathBuf, String>;

    /// The trash folder that entries from `original`'s volume go to.
    fn trash_folder_for(&self, original: &Path) -> Option<PathBuf>;

    /// Moves `from` to `to` without ever replacing `to`: a taken `to` is
    /// `Taken`.
    fn move_back(&self, from: &Path, to: &Path) -> Result<(), RestoreError>;
}

/// A trash that remembers what went into it in `journal`.
#[derive(Debug)]
pub struct JournaledTrash<B> {
    journal: PathBuf,
    backend: B,
    /// One read-change-write of the file at a time.
    writing: Mutex<()>,
}

/// A tagged enum so a later general undo can add kinds to the same file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JournalEntry {
    Trash {
        at: i64,
        original: PathBuf,
        trashed: PathBuf,
    },
}

impl JournalEntry {
    fn original(&self) -> &Path {
        match self {
            Self::Trash { original, .. } => original,
        }
    }

    fn trashed(&self) -> &Path {
        match self {
            Self::Trash { trashed, .. } => trashed,
        }
    }

    fn at(&self) -> i64 {
        match self {
            Self::Trash { at, .. } => *at,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct JournalFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    entries: Vec<JournalEntry>,
}

/// Where an entry's item is, as far as the disk can tell.
enum Presence {
    Here,
    /// The trash folder is there and the item is not: emptied, or put back
    /// somewhere else.
    Gone,
    /// The trash folder itself is not there, e.g. an unplugged volume.
    Unknown,
}

fn presence(trashed: &Path) -> Presence {
    match std::fs::symlink_metadata(trashed) {
        Ok(_) => Presence::Here,
        Err(err)
            if err.kind() == io::ErrorKind::NotFound
                && trashed.parent().is_some_and(Path::is_dir) =>
        {
            Presence::Gone
        }
        Err(_) => Presence::Unknown,
    }
}

/// An absolute path with a file name and nothing like `..` in it.
fn is_plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.file_name().is_some()
        && path.components().all(|part| {
            matches!(
                part,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
}

fn failed(reason: impl Into<String>) -> RestoreError {
    RestoreError::Failed(reason.into())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_secs()).ok())
        .unwrap_or(0)
}

impl<B: TrashBackend> JournaledTrash<B> {
    pub fn new(journal: PathBuf, backend: B) -> Self {
        Self {
            journal,
            backend,
            writing: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.writing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A missing file is an empty journal. One that cannot be read, or that a
    /// newer ncrs wrote, is an error, so it is neither used nor overwritten.
    fn load(&self) -> Result<Vec<JournalEntry>, String> {
        let shown = self.journal.display();
        let text = match std::fs::read_to_string(&self.journal) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(format!("{shown}: {err}")),
        };
        let file: JournalFile =
            toml_edit::de::from_str(&text).map_err(|err| format!("{shown}: {err}"))?;
        if file.version > JOURNAL_VERSION {
            return Err(format!(
                "{shown} is version {}, written by a newer ncrs",
                file.version
            ));
        }
        Ok(file.entries)
    }

    fn save(&self, mut entries: Vec<JournalEntry>) -> io::Result<()> {
        let excess = entries.len().saturating_sub(MAX_ENTRIES);
        entries.drain(..excess);
        if let Some(folder) = self.journal.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = toml_edit::ser::to_string_pretty(&JournalFile {
            version: JOURNAL_VERSION,
            entries,
        })
        .map_err(io::Error::other)?;
        replace_file_atomically(&self.journal, ".journal.toml.", &text).map_err(|(_, err)| err)
    }

    /// Losing the record is the lesser harm next to failing a move that
    /// already happened, so a failed save is not reported.
    fn save_quietly(&self, entries: Vec<JournalEntry>) {
        drop(self.save(entries));
    }

    fn check_in_trash_folder(&self, original: &Path, trashed: &Path) -> Result<(), RestoreError> {
        let folder = self
            .backend
            .trash_folder_for(original)
            .ok_or_else(|| failed("there is no trash folder for this place"))?;
        let holder = trashed
            .parent()
            .ok_or_else(|| failed("the item is not in a folder"))?;
        let canonical = |path: &Path| {
            std::fs::canonicalize(path).map_err(|err| failed(format!("{}: {err}", path.display())))
        };
        if canonical(holder)? == canonical(&folder)? {
            Ok(())
        } else {
            Err(failed("the item is not in the trash folder"))
        }
    }
}

impl<B: TrashBackend> Trash for JournaledTrash<B> {
    fn trash(&self, path: &Path) -> Result<(), String> {
        let landed = self.backend.move_to_trash(path)?;
        let _writing = self.lock();
        if let Ok(mut entries) = self.load() {
            entries.push(JournalEntry::Trash {
                at: now(),
                original: path.to_path_buf(),
                trashed: landed,
            });
            self.save_quietly(entries);
        }
        Ok(())
    }

    fn reach(&self) -> TrashReach {
        TrashReach::OwnDeletions
    }

    fn list(&self) -> Result<Vec<TrashedItem>, String> {
        let _writing = self.lock();
        let entries = self.load()?;
        let before = entries.len();
        let mut kept = Vec::with_capacity(before);
        let mut items = Vec::new();
        for entry in entries {
            match presence(entry.trashed()) {
                Presence::Here => {
                    items.push(TrashedItem {
                        original: entry.original().to_path_buf(),
                        deleted_at: entry.at(),
                        key: entry.trashed().as_os_str().to_owned(),
                    });
                    kept.push(entry);
                }
                Presence::Unknown => kept.push(entry),
                Presence::Gone => {}
            }
        }
        if kept.len() != before {
            self.save_quietly(kept);
        }
        items.reverse();
        items.sort_by_key(|item| Reverse(item.deleted_at));
        Ok(items)
    }

    fn restore(&self, item: &TrashedItem) -> Result<(), RestoreError> {
        let _writing = self.lock();
        let mut entries = self.load().map_err(RestoreError::Failed)?;
        let position = entries
            .iter()
            .position(|entry| {
                entry.trashed().as_os_str() == item.key && entry.original() == item.original
            })
            .ok_or_else(|| failed("this item is not in the record of ncrs"))?;
        let original = item.original.as_path();
        let trashed = Path::new(&item.key);
        if !is_plain_absolute(original) || !is_plain_absolute(trashed) {
            return Err(failed("the record holds a path that can not be trusted"));
        }
        // `symlink_metadata`: the item may be a link, and a link must be moved
        // as itself, never followed.
        if let Err(err) = std::fs::symlink_metadata(trashed) {
            if err.kind() == io::ErrorKind::NotFound {
                entries.remove(position);
                self.save_quietly(entries);
            }
            return Err(failed("the item is no longer in the trash"));
        }
        self.check_in_trash_folder(original, trashed)?;
        if std::fs::symlink_metadata(original).is_ok() {
            return Err(RestoreError::Taken);
        }
        if let Some(parent) = original.parent() {
            std::fs::create_dir_all(parent).map_err(|err| failed(err.to_string()))?;
        }
        self.backend.move_back(trashed, original)?;
        entries.remove(position);
        self.save_quietly(entries);
        Ok(())
    }
}

/// A trash that is a folder, for tests: an entry "in the trash" is one that was
/// renamed into it.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct FolderBackend(pub PathBuf);

#[cfg(any(test, feature = "test-support"))]
impl TrashBackend for FolderBackend {
    fn move_to_trash(&self, path: &Path) -> Result<PathBuf, String> {
        let name = path.file_name().ok_or("no file name")?.to_string_lossy();
        let mut target = self.0.join(name.as_ref());
        let mut copy = 1;
        while std::fs::symlink_metadata(&target).is_ok() {
            copy += 1;
            target = self.0.join(format!("{name} {copy}"));
        }
        std::fs::rename(path, &target).map_err(|err| err.to_string())?;
        Ok(target)
    }

    fn trash_folder_for(&self, _original: &Path) -> Option<PathBuf> {
        Some(self.0.clone())
    }

    fn move_back(&self, from: &Path, to: &Path) -> Result<(), RestoreError> {
        if std::fs::symlink_metadata(to).is_ok() {
            return Err(RestoreError::Taken);
        }
        std::fs::rename(from, to).map_err(|err| failed(err.to_string()))
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    struct Scene {
        _root: tempfile::TempDir,
        work: PathBuf,
        bin: PathBuf,
        journal: PathBuf,
        trash: JournaledTrash<FolderBackend>,
    }

    fn scene() -> Scene {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let journal = root.path().join("data/journal.toml");
        let trash = JournaledTrash::new(journal.clone(), FolderBackend(bin.clone()));
        Scene {
            _root: root,
            work,
            bin,
            journal,
            trash,
        }
    }

    fn write_journal(scene: &Scene, text: &str) {
        std::fs::create_dir_all(scene.journal.parent().unwrap()).unwrap();
        std::fs::write(&scene.journal, text).unwrap();
    }

    fn entry_text(at: i64, original: &Path, trashed: &Path) -> String {
        format!(
            "[[entries]]\nkind = \"trash\"\nat = {at}\noriginal = {:?}\ntrashed = {:?}\n\n",
            original.to_str().unwrap(),
            trashed.to_str().unwrap()
        )
    }

    fn only_item(scene: &Scene) -> TrashedItem {
        let mut items = scene.trash.list().unwrap();
        assert_eq!(items.len(), 1);
        items.remove(0)
    }

    #[test]
    fn a_trashed_file_is_listed_with_its_original_and_recorded() {
        let scene = scene();
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "hello").unwrap();

        scene.trash.trash(&file).unwrap();

        assert!(!file.exists());
        assert_eq!(
            std::fs::read_to_string(scene.bin.join("a.txt")).unwrap(),
            "hello"
        );
        let item = only_item(&scene);
        assert_eq!(item.original, file);
        assert_eq!(scene.trash.reach(), TrashReach::OwnDeletions);
        let text = std::fs::read_to_string(&scene.journal).unwrap();
        assert!(text.contains("version = 1"), "{text}");
        assert!(text.contains("kind = \"trash\""), "{text}");
        assert!(text.contains("a.txt"), "{text}");
    }

    #[test]
    fn a_restored_file_is_back_and_forgotten() {
        let scene = scene();
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "hello").unwrap();
        scene.trash.trash(&file).unwrap();

        scene.trash.restore(&only_item(&scene)).unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
        assert!(scene.trash.list().unwrap().is_empty());
        let text = std::fs::read_to_string(&scene.journal).unwrap();
        assert!(!text.contains("a.txt"), "{text}");
    }

    #[test]
    fn a_folder_with_a_file_inside_comes_back_whole() {
        let scene = scene();
        let folder = scene.work.join("tree");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("inner/b.txt"), "deep").unwrap();
        scene.trash.trash(&folder).unwrap();
        assert!(!folder.exists());

        scene.trash.restore(&only_item(&scene)).unwrap();

        assert_eq!(
            std::fs::read_to_string(folder.join("inner/b.txt")).unwrap(),
            "deep"
        );
    }

    #[test]
    fn a_taken_name_is_reported_and_nothing_moves() {
        let scene = scene();
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "old").unwrap();
        scene.trash.trash(&file).unwrap();
        std::fs::write(&file, "new").unwrap();
        let item = only_item(&scene);

        assert_eq!(scene.trash.restore(&item), Err(RestoreError::Taken));

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(scene.bin.join("a.txt")).unwrap(),
            "old"
        );
        assert_eq!(scene.trash.list().unwrap().len(), 1);
    }

    #[test]
    fn a_missing_original_folder_is_created() {
        let scene = scene();
        let folder = scene.work.join("gone");
        std::fs::create_dir(&folder).unwrap();
        let file = folder.join("a.txt");
        std::fs::write(&file, "x").unwrap();
        scene.trash.trash(&file).unwrap();
        std::fs::remove_dir(&folder).unwrap();

        scene.trash.restore(&only_item(&scene)).unwrap();

        assert!(file.is_file());
    }

    #[test]
    fn an_item_taken_out_of_the_trash_is_purged_from_the_record() {
        let scene = scene();
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "x").unwrap();
        scene.trash.trash(&file).unwrap();
        std::fs::remove_file(scene.bin.join("a.txt")).unwrap();

        assert!(scene.trash.list().unwrap().is_empty());

        let text = std::fs::read_to_string(&scene.journal).unwrap();
        assert!(!text.contains("a.txt"), "{text}");
    }

    #[test]
    fn a_missing_trash_folder_hides_the_entry_but_keeps_it() {
        let scene = scene();
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "x").unwrap();
        scene.trash.trash(&file).unwrap();
        let moved = scene.bin.with_file_name("bin-away");
        std::fs::rename(&scene.bin, &moved).unwrap();

        assert!(scene.trash.list().unwrap().is_empty());
        assert!(std::fs::read_to_string(&scene.journal)
            .unwrap()
            .contains("a.txt"));

        std::fs::rename(&moved, &scene.bin).unwrap();
        assert_eq!(scene.trash.list().unwrap().len(), 1);
    }

    #[test]
    fn a_recorded_item_outside_the_trash_folder_is_never_moved() {
        let scene = scene();
        let victim = scene.work.join("victim.txt");
        std::fs::write(&victim, "keep").unwrap();
        let target = scene.work.join("elsewhere.txt");
        write_journal(
            &scene,
            &format!("version = 1\n\n{}", entry_text(1, &target, &victim)),
        );

        let result = scene.trash.restore(&only_item(&scene));

        assert!(matches!(result, Err(RestoreError::Failed(_))), "{result:?}");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert!(!target.exists());
    }

    #[test]
    fn an_original_with_dots_or_without_a_root_is_refused() {
        let scene = scene();
        let inside = scene.bin.join("a.txt");
        std::fs::write(&inside, "x").unwrap();
        let sneaky = scene.work.join("sub/../escaped.txt");
        for original in [sneaky.as_path(), Path::new("relative.txt")] {
            write_journal(
                &scene,
                &format!("version = 1\n\n{}", entry_text(1, original, &inside)),
            );
            let item = only_item(&scene);

            let result = scene.trash.restore(&item);

            assert!(matches!(result, Err(RestoreError::Failed(_))), "{result:?}");
            assert!(inside.is_file());
        }
        assert!(!scene.work.join("escaped.txt").exists());
    }

    #[test]
    fn an_item_that_is_not_in_the_record_is_refused() {
        let scene = scene();
        let inside = scene.bin.join("a.txt");
        std::fs::write(&inside, "x").unwrap();
        let stranger = TrashedItem {
            original: scene.work.join("a.txt"),
            deleted_at: 1,
            key: inside.clone().into_os_string(),
        };

        let result = scene.trash.restore(&stranger);

        assert!(matches!(result, Err(RestoreError::Failed(_))), "{result:?}");
        assert!(inside.is_file());
        assert!(!stranger.original.exists());
    }

    #[test]
    fn a_journal_of_a_newer_ncrs_is_left_alone() {
        let scene = scene();
        let text = "version = 2\nsomething = \"new\"\n";
        write_journal(&scene, text);
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "x").unwrap();

        let listed = scene.trash.list().unwrap_err();
        scene.trash.trash(&file).unwrap();

        assert!(listed.contains("newer ncrs"), "{listed}");
        assert!(scene.bin.join("a.txt").is_file());
        assert_eq!(std::fs::read_to_string(&scene.journal).unwrap(), text);
    }

    #[test]
    fn a_broken_journal_is_not_overwritten() {
        let scene = scene();
        write_journal(&scene, "this is [not toml");
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "x").unwrap();

        scene.trash.list().unwrap_err();
        scene.trash.trash(&file).unwrap();

        assert!(scene.bin.join("a.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(&scene.journal).unwrap(),
            "this is [not toml"
        );
    }

    #[test]
    fn only_the_newest_thousand_entries_are_kept() {
        let scene = scene();
        let mut text = String::from("version = 1\n\n");
        for age in 0..MAX_ENTRIES {
            let at = i64::try_from(age).unwrap();
            text.push_str(&entry_text(at, Path::new("/o/x"), Path::new("/b/x")));
        }
        write_journal(&scene, &text);
        let file = scene.work.join("a.txt");
        std::fs::write(&file, "x").unwrap();

        scene.trash.trash(&file).unwrap();

        let saved: JournalFile =
            toml_edit::de::from_str(&std::fs::read_to_string(&scene.journal).unwrap()).unwrap();
        assert_eq!(saved.entries.len(), MAX_ENTRIES);
        assert_eq!(saved.entries.first().map(JournalEntry::at), Some(1));
        assert_eq!(
            saved.entries.last().map(JournalEntry::original),
            Some(file.as_path())
        );
    }
}
