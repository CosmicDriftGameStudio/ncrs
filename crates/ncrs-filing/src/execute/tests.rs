#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use chrono::{TimeZone as _, Utc};
use ncrs_index::{DocumentSource, NewDocument};
use tempfile::TempDir;

use super::*;
use ncrs_fs::transfer::Counting;

struct Setup {
    _dir: TempDir,
    root: PathBuf,
    index: Index,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        fs::create_dir_all(&root).unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        Self {
            _dir: dir,
            root,
            index,
        }
    }

    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    /// A file the index knows as a document.
    fn document(&self, relative: &str, content: &str) -> ExecutionItem {
        let from = self.write(relative, content);
        let hash = hash_path(&from).unwrap();
        let id = self
            .index
            .insert_document(&NewDocument {
                hash,
                path: from.clone(),
                source: DocumentSource::Archive,
                received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: Some(stamp_of(&from).unwrap()),
            })
            .unwrap();
        ExecutionItem {
            to: self.root.join("moved").join(relative),
            from,
            expected_hash: hash,
            document_id: Some(id),
            duplicate: false,
        }
    }

    fn run(&self, items: &[ExecutionItem]) -> ExecutionReport {
        execute(
            &self.index,
            BatchKind::Reorganisation,
            &self.root,
            items,
            &mut Counting,
        )
        .unwrap()
    }

    fn states(&self, batch: BatchId) -> Vec<JournalState> {
        self.index
            .journal_items(batch)
            .unwrap()
            .into_iter()
            .map(|item| item.state)
            .collect()
    }
}

struct StopAfter(usize);

impl Progress for StopAfter {
    fn advanced(&mut self, done: usize, _total: usize) -> io::Result<()> {
        if done >= self.0 {
            Err(io::Error::other("stopped"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn a_moved_document_is_followed_by_the_index_with_the_stamp_of_the_target() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");

    let report = setup.run(std::slice::from_ref(&item));

    assert_eq!(report.moved(), 1);
    assert!(!item.from.exists());
    assert_eq!(fs::read_to_string(&item.to).unwrap(), "alpha");
    let document = setup.index.document_by_path(&item.to).unwrap().unwrap();
    assert_eq!(Some(document.id), item.document_id);
    assert_eq!(document.stamp, Some(stamp_of(&item.to).unwrap()));
    assert_eq!(setup.states(report.batch), [JournalState::Moved]);
    assert_eq!(
        setup.index.created_dirs(report.batch).unwrap(),
        [setup.root.join("moved")]
    );
}

#[test]
fn a_duplicate_copy_changes_its_duplicate_path_and_the_keeper_stays() {
    let setup = Setup::new();
    let keeper = setup.document("keeper.pdf", "same");
    let copy_path = setup.write("copy.pdf", "same");
    let id = keeper.document_id.unwrap();
    setup
        .index
        .record_duplicate_path(id, &copy_path, stamp_of(&copy_path).unwrap())
        .unwrap();
    let item = ExecutionItem {
        from: copy_path.clone(),
        to: setup.root.join("dupes/copy.pdf"),
        expected_hash: keeper.expected_hash,
        document_id: Some(id),
        duplicate: true,
    };

    let report = setup.run(std::slice::from_ref(&item));

    assert_eq!(report.moved(), 1);
    assert!(keeper.from.exists());
    assert_eq!(
        setup.index.duplicate_paths(id).unwrap(),
        std::slice::from_ref(&item.to)
    );
    assert_eq!(
        setup
            .index
            .document_by_path(&keeper.from)
            .unwrap()
            .map(|d| d.id),
        Some(id)
    );
}

#[test]
fn nothing_is_overwritten_and_nothing_foreign_is_touched() {
    let setup = Setup::new();
    let taken = setup.document("taken.pdf", "one");
    setup.write("moved/taken.pdf", "someone else");
    let changed = setup.document("changed.pdf", "two");
    fs::write(&changed.from, "edited after the plan").unwrap();
    let missing = setup.document("missing.pdf", "three");
    fs::remove_file(&missing.from).unwrap();
    let outside = ExecutionItem {
        to: setup._dir.path().join("elsewhere.pdf"),
        ..setup.document("outside.pdf", "four")
    };

    let report = setup.run(&[taken.clone(), changed.clone(), missing, outside.clone()]);

    let outcomes: Vec<_> = report
        .items
        .iter()
        .map(|item| item.outcome.clone())
        .collect();
    assert_eq!(
        outcomes,
        [
            ItemOutcome::Skipped(SkipReason::TargetExists),
            ItemOutcome::Skipped(SkipReason::SourceChanged),
            ItemOutcome::Skipped(SkipReason::SourceMissing),
            ItemOutcome::Skipped(SkipReason::OutsideLocation),
        ]
    );
    assert_eq!(
        fs::read_to_string(setup.root.join("moved/taken.pdf")).unwrap(),
        "someone else"
    );
    assert_eq!(fs::read_to_string(&taken.from).unwrap(), "one");
    assert_eq!(
        fs::read_to_string(&changed.from).unwrap(),
        "edited after the plan"
    );
    assert!(outside.from.exists());
    assert_eq!(setup.states(report.batch), [JournalState::Skipped; 4]);
}

#[test]
fn a_cancel_between_two_items_journals_the_rest_as_cancelled() {
    let setup = Setup::new();
    let first = setup.document("1.pdf", "one");
    let second = setup.document("2.pdf", "two");
    let third = setup.document("3.pdf", "three");

    let report = execute(
        &setup.index,
        BatchKind::Reorganisation,
        &setup.root,
        &[first.clone(), second.clone(), third.clone()],
        &mut StopAfter(1),
    )
    .unwrap();

    assert_eq!(report.moved(), 1);
    assert!(first.to.exists());
    assert!(second.from.exists() && third.from.exists());
    let items = setup.index.journal_items(report.batch).unwrap();
    assert_eq!(items[1].state, JournalState::Skipped);
    assert_eq!(items[1].reason.as_deref(), Some("cancelled"));
    assert_eq!(items[2].reason.as_deref(), Some("cancelled"));
}

#[test]
fn undo_moves_everything_back_and_removes_the_folders_the_run_created() {
    let setup = Setup::new();
    let first = setup.document("a/1.pdf", "one");
    let second = setup.document("b/2.pdf", "two");
    let report = setup.run(&[first.clone(), second.clone()]);

    let undone = undo(
        &setup.index,
        report.batch,
        std::slice::from_ref(&setup.root),
    )
    .unwrap();

    assert_eq!(undone, UndoReport { undone: 2, left: 0 });
    assert_eq!(fs::read_to_string(&first.from).unwrap(), "one");
    assert_eq!(fs::read_to_string(&second.from).unwrap(), "two");
    assert!(!setup.root.join("moved").exists());
    assert_eq!(
        setup
            .index
            .document_by_path(&first.from)
            .unwrap()
            .map(|d| d.id),
        first.document_id
    );
    assert_eq!(setup.states(report.batch), [JournalState::Undone; 2]);
    assert_eq!(
        setup
            .index
            .last_undoable_batch(BatchKind::Reorganisation)
            .unwrap(),
        None
    );
}

#[test]
fn undo_leaves_a_file_that_changed_or_whose_old_place_is_taken() {
    let setup = Setup::new();
    let edited = setup.document("edited.pdf", "one");
    let blocked = setup.document("blocked.pdf", "two");
    let fine = setup.document("fine.pdf", "three");
    let report = setup.run(&[edited.clone(), blocked.clone(), fine.clone()]);
    fs::write(&edited.to, "edited later").unwrap();
    fs::write(&blocked.from, "new file at the old place").unwrap();
    setup.write("moved/keep.txt", "the user's own file");

    let undone = undo(
        &setup.index,
        report.batch,
        std::slice::from_ref(&setup.root),
    )
    .unwrap();

    assert_eq!(undone, UndoReport { undone: 1, left: 2 });
    assert_eq!(fs::read_to_string(&edited.to).unwrap(), "edited later");
    assert_eq!(fs::read_to_string(&blocked.to).unwrap(), "two");
    assert_eq!(
        fs::read_to_string(&blocked.from).unwrap(),
        "new file at the old place"
    );
    assert!(fine.from.exists());
    assert!(
        setup.root.join("moved").exists(),
        "a folder with content stays"
    );
    assert_eq!(
        setup.states(report.batch),
        [
            JournalState::UndoSkipped,
            JournalState::UndoSkipped,
            JournalState::Undone
        ]
    );
}

/// A pending item as an interrupted run leaves it.
fn pending(setup: &Setup, item: &ExecutionItem) -> (BatchId, u32) {
    let batch = setup.index.begin_batch(BatchKind::Reorganisation).unwrap();
    let seq = setup
        .index
        .add_journal_item(batch, &journal_entry(item, JournalState::Pending, None))
        .unwrap();
    (batch, seq)
}

fn state_of(setup: &Setup, batch: BatchId) -> JournalItem {
    setup.index.journal_items(batch).unwrap().remove(0)
}

#[test]
fn reconcile_takes_a_finished_move_into_the_index() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::rename(&item.from, &item.to).unwrap();

    assert_eq!(reconcile(&setup.index).unwrap(), 1);

    assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
    assert_eq!(
        setup
            .index
            .document_by_path(&item.to)
            .unwrap()
            .map(|d| d.id),
        item.document_id
    );
}

#[test]
fn reconcile_marks_a_move_that_never_began_as_interrupted() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);

    reconcile(&setup.index).unwrap();

    let settled = state_of(&setup, batch);
    assert_eq!(settled.state, JournalState::Skipped);
    assert_eq!(settled.reason.as_deref(), Some("interrupted"));
    assert!(item.from.exists());
}

#[test]
fn reconcile_finishes_a_copy_that_was_made_but_whose_source_was_not_removed() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::copy(&item.from, &item.to).unwrap();
    let staging = item.to.with_file_name(format!(
        "{}{}-1",
        partial_prefix(&item.to),
        std::process::id()
    ));
    fs::write(&staging, "half").unwrap();

    reconcile(&setup.index).unwrap();

    assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
    assert!(!item.from.exists());
    assert!(item.to.exists());
    assert!(!staging.exists(), "the staging copy is removed");
}

#[test]
fn reconcile_does_not_guess_when_both_files_differ() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::write(&item.to, "something else").unwrap();

    reconcile(&setup.index).unwrap();

    assert_eq!(state_of(&setup, batch).state, JournalState::Skipped);
    assert!(item.from.exists());
    assert_eq!(fs::read_to_string(&item.to).unwrap(), "something else");
}

#[cfg(unix)]
#[test]
fn a_link_folder_that_leads_out_of_the_location_is_not_followed() {
    let setup = Setup::new();
    let outside = setup._dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, setup.root.join("link")).unwrap();
    let item = ExecutionItem {
        to: setup.root.join("link/deeper/a.pdf"),
        ..setup.document("a.pdf", "alpha")
    };

    let report = setup.run(std::slice::from_ref(&item));

    assert_eq!(
        report.items[0].outcome,
        ItemOutcome::Skipped(SkipReason::LeavesLocation)
    );
    assert!(item.from.exists());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn undo_does_not_move_a_file_back_through_a_link_folder() {
    let setup = Setup::new();
    let item = setup.document("a/a.pdf", "alpha");
    let report = setup.run(std::slice::from_ref(&item));
    let outside = setup._dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::remove_dir(item.from.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, item.from.parent().unwrap()).unwrap();

    let undone = undo(
        &setup.index,
        report.batch,
        std::slice::from_ref(&setup.root),
    )
    .unwrap();

    assert_eq!(undone, UndoReport { undone: 0, left: 1 });
    assert!(item.to.exists());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn reconcile_goes_on_after_an_item_it_cannot_settle() {
    use std::os::unix::fs::PermissionsExt as _;

    let setup = Setup::new();
    let stuck = setup.document("locked/a.pdf", "alpha");
    let (stuck_batch, _) = pending(&setup, &stuck);
    fs::create_dir_all(stuck.to.parent().unwrap()).unwrap();
    fs::copy(&stuck.from, &stuck.to).unwrap();
    let locked = stuck.from.parent().unwrap().to_path_buf();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let fine = setup.document("b.pdf", "beta");
    let (fine_batch, _) = pending(&setup, &fine);
    fs::create_dir_all(fine.to.parent().unwrap()).unwrap();
    fs::rename(&fine.from, &fine.to).unwrap();

    let settled = reconcile(&setup.index);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(settled.unwrap(), 1);
    assert_eq!(state_of(&setup, fine_batch).state, JournalState::Moved);
    assert_eq!(state_of(&setup, stuck_batch).state, JournalState::Pending);
}

fn copy_run(setup: &Setup, items: &[ExecutionItem]) -> ExecutionReport {
    execute_run(
        &setup.index,
        BatchKind::Hermes,
        "Tidy up",
        Transfer::Copy,
        RunPlaces {
            source: &setup.root,
            target: &setup.root,
            inside_target: false,
        },
        items,
        &mut Counting,
    )
    .unwrap()
}

#[test]
fn a_copy_run_leaves_the_sources_and_the_index_paths_untouched() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");

    let report = copy_run(&setup, std::slice::from_ref(&item));

    assert_eq!(report.moved(), 1);
    assert_eq!(fs::read_to_string(&item.from).unwrap(), "alpha");
    assert_eq!(fs::read_to_string(&item.to).unwrap(), "alpha");
    let document = setup.index.document_by_path(&item.from).unwrap().unwrap();
    assert_eq!(Some(document.id), item.document_id);
    assert_eq!(document.status, DocumentStatus::New);
    assert!(setup.index.document_by_path(&item.to).unwrap().is_none());
    let info = setup.index.batch_info(report.batch).unwrap().unwrap();
    assert_eq!(
        (info.kind, info.name.as_deref(), info.transfer),
        (BatchKind::Hermes, Some("Tidy up"), Transfer::Copy)
    );
    assert_eq!(setup.states(report.batch), [JournalState::Moved]);
}

#[test]
fn a_move_run_of_the_chat_files_the_document() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");

    let report = execute_run(
        &setup.index,
        BatchKind::Hermes,
        "Tidy up",
        Transfer::Move,
        RunPlaces {
            source: &setup.root,
            target: &setup.root,
            inside_target: false,
        },
        std::slice::from_ref(&item),
        &mut Counting,
    )
    .unwrap();

    assert_eq!(report.moved(), 1);
    assert!(!item.from.exists());
    let document = setup.index.document_by_path(&item.to).unwrap().unwrap();
    assert_eq!(document.status, DocumentStatus::Filed);
}

#[test]
fn reconcile_never_removes_the_source_of_an_interrupted_copy() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let batch = setup
        .index
        .begin_named_batch(BatchKind::Hermes, "Tidy up", Transfer::Copy)
        .unwrap();
    setup
        .index
        .add_journal_item(batch, &journal_entry(&item, JournalState::Pending, None))
        .unwrap();
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::copy(&item.from, &item.to).unwrap();

    assert_eq!(reconcile(&setup.index).unwrap(), 1);

    assert_eq!(hash_path(&item.from).unwrap(), item.expected_hash);
    assert_eq!(hash_path(&item.to).unwrap(), item.expected_hash);
    assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
}

#[test]
fn reconcile_marks_a_copy_that_never_began_as_interrupted() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let batch = setup
        .index
        .begin_named_batch(BatchKind::Hermes, "Tidy up", Transfer::Copy)
        .unwrap();
    setup
        .index
        .add_journal_item(batch, &journal_entry(&item, JournalState::Pending, None))
        .unwrap();

    reconcile(&setup.index).unwrap();

    let settled = state_of(&setup, batch);
    assert_eq!(settled.state, JournalState::Skipped);
    assert_eq!(settled.reason.as_deref(), Some("interrupted"));
    assert!(item.from.exists());
}

#[test]
fn undoing_a_copy_run_removes_only_the_intact_copies_and_their_empty_folders() {
    let setup = Setup::new();
    let fine = setup.document("fine/1.pdf", "one");
    let edited = setup.document("edited/2.pdf", "two");
    let orphaned = setup.document("orphaned/3.pdf", "three");
    let report = copy_run(&setup, &[fine.clone(), edited.clone(), orphaned.clone()]);
    fs::write(&edited.to, "edited later").unwrap();
    fs::remove_file(&orphaned.from).unwrap();

    let undone = undo(
        &setup.index,
        report.batch,
        std::slice::from_ref(&setup.root),
    )
    .unwrap();

    assert_eq!(undone, UndoReport { undone: 1, left: 2 });
    assert!(!fine.to.exists());
    assert!(fine.from.exists());
    assert_eq!(fs::read_to_string(&edited.to).unwrap(), "edited later");
    assert_eq!(
        fs::read_to_string(&orphaned.to).unwrap(),
        "three",
        "a copy whose original is gone is the only one left"
    );
    assert_eq!(
        setup.states(report.batch),
        [
            JournalState::Undone,
            JournalState::UndoSkipped,
            JournalState::UndoSkipped
        ]
    );
}

mod trash_runs {
    use ncrs_fs::{FolderBackend, JournaledTrash, Trash as _};

    use super::*;

    struct Bin {
        trash: JournaledTrash<FolderBackend>,
        folder: PathBuf,
    }

    fn bin(setup: &Setup) -> Bin {
        let folder = setup._dir.path().join("bin");
        fs::create_dir_all(&folder).unwrap();
        Bin {
            trash: JournaledTrash::new(
                setup._dir.path().join("trash.toml"),
                FolderBackend(folder.clone()),
            ),
            folder,
        }
    }

    fn duplicate(setup: &Setup, name: &str, content: &str) -> (TrashItem, PathBuf) {
        let keeper = setup.write(&format!("keep/{name}"), content);
        let path = setup.write(&format!("inbox/{name}"), content);
        let item = TrashItem {
            path,
            expected_hash: hash_path(&keeper).unwrap(),
            keeper: keeper.clone(),
        };
        (item, keeper)
    }

    fn last_batch(setup: &Setup) -> BatchId {
        setup
            .index
            .last_undoable_batch(BatchKind::Hermes)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn trashing_and_undoing_puts_the_bytes_back_at_the_old_path() {
        let setup = Setup::new();
        let bin = bin(&setup);
        let (first, keeper) = duplicate(&setup, "a.pdf", "alpha");
        let (second, _) = duplicate(&setup, "b.pdf", "beta");

        let report = trash_run(
            &setup.index,
            "Cleared",
            &setup.root,
            &[first.clone(), second.clone()],
            &bin.trash,
        )
        .unwrap();

        assert_eq!(
            report,
            TrashReport {
                trashed: 2,
                skipped: 0
            }
        );
        assert!(!first.path.exists() && !second.path.exists());
        assert!(keeper.exists());
        assert_eq!(fs::read_dir(&bin.folder).unwrap().count(), 2);
        let batch = last_batch(&setup);
        let info = setup.index.batch_info(batch).unwrap().unwrap();
        assert_eq!(
            (info.transfer, info.name.as_deref()),
            (Transfer::Trash, Some("Cleared"))
        );
        let items = setup.index.journal_items(batch).unwrap();
        assert!(
            items.iter().all(|item| item.to.starts_with(&bin.folder)),
            "the key is recorded"
        );

        let undone = undo_with_trash(
            &setup.index,
            batch,
            std::slice::from_ref(&setup.root),
            Some(&bin.trash),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 2, left: 0 });
        assert_eq!(fs::read_to_string(&first.path).unwrap(), "alpha");
        assert_eq!(fs::read_to_string(&second.path).unwrap(), "beta");
        assert_eq!(
            setup.index.last_undoable_batch(BatchKind::Hermes).unwrap(),
            None
        );
    }

    #[test]
    fn a_changed_file_a_missing_keeper_or_a_path_outside_the_root_is_skipped() {
        let setup = Setup::new();
        let bin = bin(&setup);
        let (changed, _) = duplicate(&setup, "changed.pdf", "one");
        fs::write(&changed.path, "edited after the plan").unwrap();
        let (no_keeper, keeper) = duplicate(&setup, "gone.pdf", "two");
        fs::remove_file(keeper).unwrap();
        let (fine, _) = duplicate(&setup, "fine.pdf", "three");
        let elsewhere = setup._dir.path().join("elsewhere.pdf");
        fs::write(&elsewhere, "four").unwrap();
        let outside = TrashItem {
            path: elsewhere.clone(),
            expected_hash: hash_path(&elsewhere).unwrap(),
            keeper: fine.keeper.clone(),
        };
        let itself = TrashItem {
            keeper: fine.path.clone(),
            ..fine.clone()
        };

        let report = trash_run(
            &setup.index,
            "Cleared",
            &setup.root,
            &[
                changed.clone(),
                no_keeper.clone(),
                outside,
                itself,
                fine.clone(),
            ],
            &bin.trash,
        )
        .unwrap();

        assert_eq!(
            report,
            TrashReport {
                trashed: 1,
                skipped: 4
            }
        );
        assert!(changed.path.exists() && no_keeper.path.exists() && elsewhere.exists());
        assert!(!fine.path.exists());
    }

    #[test]
    fn a_name_that_is_taken_again_is_left_and_counted() {
        let setup = Setup::new();
        let bin = bin(&setup);
        let (first, _) = duplicate(&setup, "a.pdf", "alpha");
        let (second, _) = duplicate(&setup, "b.pdf", "beta");
        trash_run(
            &setup.index,
            "Cleared",
            &setup.root,
            &[first.clone(), second.clone()],
            &bin.trash,
        )
        .unwrap();
        fs::write(&first.path, "someone else").unwrap();

        let undone = undo_with_trash(
            &setup.index,
            last_batch(&setup),
            std::slice::from_ref(&setup.root),
            Some(&bin.trash),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 1, left: 1 });
        assert_eq!(fs::read_to_string(&first.path).unwrap(), "someone else");
        assert_eq!(fs::read_to_string(&second.path).unwrap(), "beta");
        assert_eq!(
            bin.trash.list().unwrap().len(),
            1,
            "the taken one stays in the trash"
        );
    }

    /// One item in the trash whose entry the app cannot read as a file (as in
    /// the Windows trash), and the journal item that recorded it.
    fn trashed_with_unreadable_entry(setup: &Setup, bin: &Bin) -> (TrashItem, JournalItem) {
        let (item, _) = duplicate(setup, "a.pdf", "alpha");
        trash_run(
            &setup.index,
            "Cleared",
            &setup.root,
            std::slice::from_ref(&item),
            &bin.trash,
        )
        .unwrap();
        let held = bin.trash.list().unwrap().remove(0).key;
        fs::remove_file(&held).unwrap();
        fs::create_dir(&held).unwrap();
        let journaled = setup
            .index
            .journal_items(last_batch(setup))
            .unwrap()
            .remove(0);
        assert_eq!(journaled.to, PathBuf::from(&held), "the key is recorded");
        (item, journaled)
    }

    #[test]
    fn an_entry_that_cannot_be_read_is_put_back_under_the_recorded_key() {
        let setup = Setup::new();
        let bin = bin(&setup);
        let (item, _) = trashed_with_unreadable_entry(&setup, &bin);

        let undone = undo_with_trash(
            &setup.index,
            last_batch(&setup),
            std::slice::from_ref(&setup.root),
            Some(&bin.trash),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 1, left: 0 });
        assert!(item.path.exists());
        assert!(bin.trash.list().unwrap().is_empty());
    }

    #[test]
    fn an_entry_that_cannot_be_read_is_not_the_newest_one_from_that_place() {
        let setup = Setup::new();
        let bin = bin(&setup);
        let (item, journaled) = trashed_with_unreadable_entry(&setup, &bin);
        let batch = last_batch(&setup);
        setup
            .index
            .set_journal_target(batch, journaled.seq, &item.path)
            .unwrap();

        let undone = undo_with_trash(
            &setup.index,
            batch,
            std::slice::from_ref(&setup.root),
            Some(&bin.trash),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert!(!item.path.exists());
        assert_eq!(bin.trash.list().unwrap().len(), 1);
    }

    #[test]
    fn reconcile_takes_a_trashed_item_as_done_and_an_untouched_one_as_interrupted() {
        let setup = Setup::new();
        let (gone, _) = duplicate(&setup, "gone.pdf", "alpha");
        let (kept, _) = duplicate(&setup, "kept.pdf", "beta");
        let batch = setup
            .index
            .begin_named_batch(BatchKind::Hermes, "Cleared", Transfer::Trash)
            .unwrap();
        for item in [&gone, &kept] {
            setup
                .index
                .add_journal_item(
                    batch,
                    &super::journal_entry(
                        &ExecutionItem {
                            from: item.path.clone(),
                            to: item.path.clone(),
                            expected_hash: item.expected_hash,
                            document_id: None,
                            duplicate: false,
                        },
                        JournalState::Pending,
                        None,
                    ),
                )
                .unwrap();
        }
        fs::remove_file(&gone.path).unwrap();

        assert_eq!(reconcile(&setup.index).unwrap(), 2);

        let items = setup.index.journal_items(batch).unwrap();
        assert_eq!(items[0].state, JournalState::Moved);
        assert_eq!(items[1].state, JournalState::Skipped);
        assert!(kept.path.exists());
    }
}
