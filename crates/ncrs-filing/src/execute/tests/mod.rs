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

#[test]
fn a_partial_undo_keeps_the_batch_so_that_a_second_undo_takes_the_rest_back() {
    let setup = Setup::new();
    let first = setup.document("a/1.pdf", "one");
    let second = setup.document("b/2.pdf", "two");
    let report = setup.run(&[first.clone(), second.clone()]);
    let away = setup.root.join("away.pdf");
    fs::rename(&second.to, &away).unwrap();
    let roots = [setup.root.clone()];

    let partial = undo(&setup.index, report.batch, &roots).unwrap();

    assert_eq!(partial, UndoReport { undone: 1, left: 1 });
    assert_eq!(
        setup
            .index
            .undoable_batches(BatchKind::Reorganisation, 5)
            .unwrap(),
        [report.batch]
    );

    // The first undo removed the folders that were empty by then.
    fs::create_dir_all(second.to.parent().unwrap()).unwrap();
    fs::rename(&away, &second.to).unwrap();
    let rest = undo(&setup.index, report.batch, &roots).unwrap();

    assert_eq!(rest, UndoReport { undone: 1, left: 0 });
    assert_eq!(fs::read_to_string(&second.from).unwrap(), "two");
    assert_eq!(setup.states(report.batch), [JournalState::Undone; 2]);
    assert!(setup
        .index
        .undoable_batches(BatchKind::Reorganisation, 5)
        .unwrap()
        .is_empty());
}

#[test]
fn undoing_a_move_puts_a_filed_document_back_into_review() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let id = item.document_id.unwrap();
    let report = setup.run(std::slice::from_ref(&item));
    setup
        .index
        .set_document_status(id, DocumentStatus::Filed)
        .unwrap();

    undo(
        &setup.index,
        report.batch,
        std::slice::from_ref(&setup.root),
    )
    .unwrap();

    let document = setup.index.document(id).unwrap().unwrap();
    assert_eq!(document.path, item.from);
    assert_eq!(document.status, DocumentStatus::InReview);
    assert_eq!(
        setup.index.open_review_of(id).unwrap().unwrap().reason,
        ncrs_index::ReviewReason::Undone
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
fn reconcile_brings_a_file_left_under_the_hidden_respell_name_to_its_new_name() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    let hidden = item.to.with_file_name(format!(
        ".{}.ncrs-respell",
        item.to.file_name().unwrap().to_string_lossy()
    ));
    fs::rename(&item.from, &hidden).unwrap();

    reconcile(&setup.index).unwrap();

    assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
    assert!(item.to.exists());
    assert!(!hidden.exists());
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
        ncrs_fs::transfer::partial_prefix(&item.to),
        std::process::id()
    ));
    fs::write(&staging, "half").unwrap();
    ncrs_fs::transfer::let_time_pass_for_sweep(std::time::Duration::from_secs(3 * 86_400));

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

mod trash_runs;

fn pending_copy(setup: &Setup, item: &ExecutionItem) -> BatchId {
    let batch = setup
        .index
        .begin_named_batch(BatchKind::Filing, "copy", Transfer::Copy)
        .unwrap();
    setup
        .index
        .add_journal_item(batch, &journal_entry(item, JournalState::Pending, None))
        .unwrap();
    batch
}

#[test]
fn reconcile_keeps_a_file_at_the_target_that_is_the_start_of_the_source() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha and the rest");
    let batch = pending_copy(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::write(&item.to, "alpha").unwrap();

    reconcile(&setup.index).unwrap();

    assert_eq!(
        state_of(&setup, batch).reason.as_deref(),
        Some("interrupted")
    );
    assert_eq!(fs::read_to_string(&item.to).unwrap(), "alpha");
    assert!(item.from.exists());
}

#[test]
fn reconcile_keeps_the_only_file_of_a_pending_move_whose_names_are_one_entry() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::hard_link(&item.from, &item.to).unwrap();

    reconcile(&setup.index).unwrap();

    assert!(
        item.from.exists() && item.to.exists(),
        "the file is still there"
    );
    assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
}

#[test]
fn reconcile_keeps_a_file_at_the_target_that_is_not_the_start_of_the_source() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha and the rest");
    pending_copy(&setup, &item);
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::write(&item.to, "other").unwrap();

    reconcile(&setup.index).unwrap();

    assert_eq!(fs::read_to_string(&item.to).unwrap(), "other");
}

#[test]
fn reconcile_does_not_remove_a_source_outside_the_folders_its_run_recorded() {
    let setup = Setup::new();
    let item = setup.document("a.pdf", "alpha");
    let (batch, _) = pending(&setup, &item);
    let elsewhere = setup.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    setup
        .index
        .set_batch_target_root(
            batch,
            &SourceRoot {
                given: elsewhere.clone(),
                resolved: elsewhere,
            },
        )
        .unwrap();
    fs::create_dir_all(item.to.parent().unwrap()).unwrap();
    fs::copy(&item.from, &item.to).unwrap();

    reconcile(&setup.index).unwrap();

    assert!(item.from.exists(), "the source stays");
    assert_eq!(state_of(&setup, batch).state, JournalState::Skipped);
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_new_name_that_differs_only_in_case_renames_the_file_where_the_file_system_does_not_tell_them_apart(
) {
    let setup = Setup::new();
    let from = setup.write("rechnung.pdf", "alpha");
    assert!(
        setup.root.join("RECHNUNG.PDF").exists(),
        "the scratch folder is on a case-sensitive file system"
    );
    let item = ExecutionItem {
        expected_hash: hash_path(&from).unwrap(),
        to: setup.root.join("Rechnung.pdf"),
        from,
        document_id: None,
        duplicate: false,
    };

    let report = setup.run(std::slice::from_ref(&item));

    assert_eq!(report.moved(), 1);
    let names: Vec<_> = fs::read_dir(&setup.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["Rechnung.pdf"]);
}
