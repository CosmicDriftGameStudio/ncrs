//! What a run and its undo do when the folders under them are swapped for
//! links, and which leftovers a settle may remove.

use ncrs_fs::{FolderBackend, JournaledTrash, Trash as _};
use tempfile::TempDir;

use super::test_runs::{execute_run, ExecutionReport};
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
                inside_target: false,
            },
            items,
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
            report.outcomes[0],
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
        report.outcomes[0],
        ItemOutcome::Skipped(SkipReason::OutsideLocation)
    );
    assert!(elsewhere.exists());
}

#[test]
fn a_settle_removes_only_old_staging_names_never_a_live_or_foreign_one() {
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
    let prefix = ncrs_fs::transfer::partial_prefix(&item.to);
    let beside = |name: String| item.to.with_file_name(name);
    let crashed = beside(format!("{prefix}4242-3"));
    let live = beside(format!("{prefix}4243-3"));
    let foreign_file = beside(format!("{prefix}notes"));
    let backup_scheme = beside(".a.pdf.ncrs-4242-1".to_owned());
    for path in [&crashed, &live, &foreign_file, &backup_scheme] {
        fs::write(path, "x").unwrap();
    }
    let three_days = std::time::Duration::from_secs(3 * 86_400);
    // The change time of a file cannot be set back: the sweep's clock goes
    // forward, and the live copy is dated ahead of it.
    ncrs_fs::transfer::let_time_pass_for_sweep(three_days);
    // Windows sets the time only through a handle opened for writing.
    fs::OpenOptions::new()
        .write(true)
        .open(&live)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + three_days)
        .unwrap();

    reconcile(&place.index).unwrap();

    assert!(
        !crashed.exists(),
        "the leftover of a crashed run is removed"
    );
    assert!(live.exists(), "a fresh one may still be written to");
    assert!(foreign_file.exists());
    assert!(backup_scheme.exists());
}

#[test]
fn the_recorded_target_folder_is_checked_by_the_undo() {
    let place = Place::new();
    let item = place.item("a.pdf", "alpha");
    let report = place.run(Transfer::Move, std::slice::from_ref(&item));
    let info = place.index.batch_info(report.batch).unwrap().unwrap();
    let recorded = info.target_root.unwrap();
    assert_eq!(recorded.given, place.target);
    assert_eq!(
        recorded.resolved,
        ncrs_intake::canonical_path(&place.target).unwrap()
    );

    #[cfg(unix)]
    {
        let outside = place.outside();
        fs::rename(&place.target, place.dir.path().join("moved-away")).unwrap();
        std::os::unix::fs::symlink(&outside, &place.target).unwrap();

        let refused = undo(&place.index, report.batch, &[]);

        assert!(matches!(refused, Err(ExecuteError::Io { .. })));
        assert_eq!(place.states(report.batch), [JournalState::Moved]);
    }
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

    fn journaled_trash_item(place: &Place, from: &Path, to: &Path, hash: ContentHash) -> BatchId {
        let batch = place
            .index
            .begin_named_batch(BatchKind::Hermes, "Cleared", Transfer::Trash)
            .unwrap();
        let item = ExecutionItem {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            expected_hash: hash,
            document_id: None,
            duplicate: false,
        };
        place
            .index
            .add_journal_item(batch, &journal_entry(&item, JournalState::Moved, None))
            .unwrap();
        batch
    }

    #[test]
    fn an_entry_that_is_gone_is_not_put_back() {
        let place = Place::new();
        let bin = bin(&place);
        let path = place.source.join("a.pdf");
        fs::write(&path, "alpha").unwrap();
        let hash = hash_path(&path).unwrap();
        bin.trash(&path).unwrap();
        let entry = bin.list().unwrap().remove(0);
        fs::remove_file(&entry.key).unwrap();
        let batch = journaled_trash_item(&place, &path, Path::new(&entry.key), hash);

        let undone = undo_with_trash(&place.index, batch, &[], Some(&bin)).unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert!(!path.exists());
    }

    #[test]
    fn a_later_deletion_from_the_same_place_is_not_put_back() {
        let place = Place::new();
        let bin = bin(&place);
        let path = place.source.join("a.pdf");
        fs::write(&path, "alpha").unwrap();
        let hash = hash_path(&path).unwrap();
        bin.trash(&path).unwrap();
        fs::write(&path, "something else, deleted later").unwrap();
        bin.trash(&path).unwrap();
        let batch = journaled_trash_item(&place, &path, &path, hash);

        let undone = undo_with_trash(&place.index, batch, &[], Some(&bin)).unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert!(!path.exists());
    }

    #[test]
    fn an_entry_of_another_original_place_is_not_put_back() {
        let place = Place::new();
        let bin = bin(&place);
        let other = place.source.join("other.pdf");
        fs::write(&other, "alpha").unwrap();
        let hash = hash_path(&other).unwrap();
        bin.trash(&other).unwrap();
        let entry = bin.list().unwrap().remove(0);
        let planned = place.source.join("a.pdf");
        let batch = journaled_trash_item(&place, &planned, Path::new(&entry.key), hash);

        let undone = undo_with_trash(&place.index, batch, &[], Some(&bin)).unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert!(!other.exists() && !planned.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_trash_undo_does_not_put_a_file_back_through_a_link() {
        let place = Place::new();
        let bin = bin(&place);
        let outside = place.outside();
        std::os::unix::fs::symlink(&outside, place.source.join("sub")).unwrap();
        let planned = place.source.join("sub").join("a.pdf");
        let batch = journaled_trash_item(&place, &planned, &planned, ContentHash([0; 32]));

        let undone = undo_with_trash(
            &place.index,
            batch,
            std::slice::from_ref(&place.source),
            Some(&bin),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert_eq!(entries_in(&outside), 0);
    }
}
