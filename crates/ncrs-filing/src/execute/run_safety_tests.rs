//! What a run and its undo do when the folders under them are swapped for
//! links, and which leftovers a settle may remove.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use ncrs_fs::transfer::Counting;
use ncrs_fs::{FolderBackend, JournaledTrash, Trash as _};
use tempfile::TempDir;

use super::*;

struct Place {
    dir: TempDir,
    source: PathBuf,
    target: PathBuf,
    index: Index,
}

impl Place {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("inbox");
        let target = dir.path().join("archive");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        Self {
            dir,
            source,
            target,
            index,
        }
    }

    #[cfg(unix)]
    fn outside(&self) -> PathBuf {
        let outside = self.dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        outside
    }

    fn item(&self, relative: &str, content: &str) -> ExecutionItem {
        let from = self.source.join(relative);
        fs::create_dir_all(from.parent().unwrap()).unwrap();
        fs::write(&from, content).unwrap();
        ExecutionItem {
            to: self.target.join("filed").join(relative),
            expected_hash: hash_path(&from).unwrap(),
            from,
            document_id: None,
            duplicate: false,
        }
    }

    fn run(&self, transfer: Transfer, items: &[ExecutionItem]) -> ExecutionReport {
        execute_run(
            &self.index,
            BatchKind::Hermes,
            "Tidy up",
            transfer,
            RunPlaces {
                source: &self.source,
                target: &self.target,
            },
            items,
            &mut Counting,
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn states(&self, batch: BatchId) -> Vec<JournalState> {
        self.index
            .journal_items(batch)
            .unwrap()
            .iter()
            .map(|item| item.state)
            .collect()
    }
}

#[cfg(unix)]
fn entries_in(folder: &Path) -> usize {
    fs::read_dir(folder).unwrap().count()
}

#[cfg(unix)]
#[test]
fn an_undo_does_not_put_a_file_back_through_a_link_in_the_source() {
    let place = Place::new();
    let item = place.item("sub/a.pdf", "alpha");
    let report = place.run(Transfer::Move, std::slice::from_ref(&item));
    let outside = place.outside();
    fs::remove_dir(item.from.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, item.from.parent().unwrap()).unwrap();

    let undone = undo(
        &place.index,
        report.batch,
        std::slice::from_ref(&place.target),
    )
    .unwrap();

    assert_eq!(undone, UndoReport { undone: 0, left: 1 });
    assert_eq!(place.states(report.batch), [JournalState::UndoSkipped]);
    assert!(item.to.exists());
    assert_eq!(entries_in(&outside), 0);
}

#[cfg(unix)]
#[test]
fn an_undo_is_refused_when_the_source_folder_itself_became_a_link() {
    let place = Place::new();
    let item = place.item("a.pdf", "alpha");
    let report = place.run(Transfer::Move, std::slice::from_ref(&item));
    let outside = place.outside();
    fs::remove_dir(&place.source).unwrap();
    std::os::unix::fs::symlink(&outside, &place.source).unwrap();

    let refused = undo(
        &place.index,
        report.batch,
        std::slice::from_ref(&place.target),
    );

    assert!(matches!(refused, Err(ExecuteError::Io { .. })));
    assert!(item.to.exists());
    assert_eq!(entries_in(&outside), 0);
    assert_eq!(place.states(report.batch), [JournalState::Moved]);
}

#[cfg(unix)]
#[test]
fn a_source_subfolder_swapped_for_a_link_before_the_run_is_not_transferred() {
    for transfer in [Transfer::Move, Transfer::Copy] {
        let place = Place::new();
        let item = place.item("sub/a.pdf", "alpha");
        let outside = place.outside();
        fs::write(outside.join("a.pdf"), "alpha").unwrap();
        fs::remove_dir_all(place.source.join("sub")).unwrap();
        std::os::unix::fs::symlink(&outside, place.source.join("sub")).unwrap();

        let report = place.run(transfer, std::slice::from_ref(&item));

        assert_eq!(
            report.items[0].outcome,
            ItemOutcome::Skipped(SkipReason::LeavesLocation)
        );
        assert!(outside.join("a.pdf").exists());
        assert!(!item.to.exists());
    }
}

#[test]
fn a_file_that_is_not_below_the_source_is_not_transferred() {
    let place = Place::new();
    let mut item = place.item("a.pdf", "alpha");
    let elsewhere = place.dir.path().join("elsewhere.pdf");
    fs::write(&elsewhere, "alpha").unwrap();
    item.from = elsewhere.clone();

    let report = place.run(Transfer::Move, std::slice::from_ref(&item));

    assert_eq!(
        report.items[0].outcome,
        ItemOutcome::Skipped(SkipReason::OutsideLocation)
    );
    assert!(elsewhere.exists());
}

#[test]
fn a_settle_removes_only_the_staging_file_of_the_transfer() {
    let place = Place::new();
    let item = place.item("a.pdf", "alpha");
    let batch = place
        .index
        .begin_named_batch(BatchKind::Hermes, "Tidy up", Transfer::Copy)
        .unwrap();
    place
        .index
        .add_journal_item(batch, &journal_entry(&item, JournalState::Pending, None))
        .unwrap();
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::copy(&item.from, &item.to).unwrap();
    let prefix = partial_prefix(&item.to);
    let beside = |name: String| item.to.with_file_name(name);
    let staging = beside(format!("{prefix}4242-3"));
    let foreign_file = beside(format!("{prefix}notes"));
    let foreign_folder = beside(format!("{prefix}4242-4"));
    let other_name = beside(".ncrs-partial-b.pdf-1-1".to_owned());
    fs::write(&staging, "half").unwrap();
    fs::write(&foreign_file, "mine").unwrap();
    fs::create_dir(&foreign_folder).unwrap();
    fs::write(foreign_folder.join("keep.txt"), "mine").unwrap();
    fs::write(&other_name, "mine").unwrap();

    reconcile(&place.index).unwrap();

    assert!(!staging.exists());
    assert!(foreign_file.exists());
    assert!(foreign_folder.join("keep.txt").exists());
    assert!(other_name.exists());
}

mod trash_entries {
    use super::*;

    fn bin(place: &Place) -> JournaledTrash<FolderBackend> {
        let folder = place.dir.path().join("bin");
        fs::create_dir_all(&folder).unwrap();
        JournaledTrash::new(place.dir.path().join("trash.toml"), FolderBackend(folder))
    }

    fn trash_item(path: &Path, keeper: &Path) -> TrashItem {
        TrashItem {
            path: path.to_path_buf(),
            expected_hash: hash_path(keeper).unwrap(),
            keeper: keeper.to_path_buf(),
        }
    }

    #[test]
    fn the_undo_puts_back_the_entry_of_its_own_run_among_older_ones() {
        let place = Place::new();
        let bin = bin(&place);
        let path = place.source.join("a.pdf");
        fs::write(&path, "older, someone else's").unwrap();
        bin.trash(&path).unwrap();
        fs::write(&path, "alpha").unwrap();
        let keeper = place.target.join("keep.pdf");
        fs::write(&keeper, "alpha").unwrap();
        trash_run(
            &place.index,
            "Cleared",
            &place.source,
            &[trash_item(&path, &keeper)],
            &bin,
        )
        .unwrap();
        let batch = place
            .index
            .last_undoable_batch(BatchKind::Hermes)
            .unwrap()
            .unwrap();

        let undone = undo_with_trash(&place.index, batch, &[], Some(&bin)).unwrap();

        assert_eq!(undone, UndoReport { undone: 1, left: 0 });
        assert_eq!(fs::read_to_string(&path).unwrap(), "alpha");
    }

    #[test]
    fn an_entry_with_other_content_is_not_put_back() {
        let place = Place::new();
        let bin = bin(&place);
        let path = place.source.join("a.pdf");
        fs::write(&path, "alpha").unwrap();
        let keeper = place.target.join("keep.pdf");
        fs::write(&keeper, "alpha").unwrap();
        trash_run(
            &place.index,
            "Cleared",
            &place.source,
            &[trash_item(&path, &keeper)],
            &bin,
        )
        .unwrap();
        let batch = place
            .index
            .last_undoable_batch(BatchKind::Hermes)
            .unwrap()
            .unwrap();
        let entry = bin.list().unwrap().remove(0);
        fs::write(&entry.key, "swapped in the trash").unwrap();

        let undone = undo_with_trash(&place.index, batch, &[], Some(&bin)).unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert!(!path.exists());
    }
}
