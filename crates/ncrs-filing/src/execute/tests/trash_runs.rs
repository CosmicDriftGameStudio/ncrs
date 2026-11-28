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
fn a_run_reads_the_trash_list_once_however_many_files_it_trashes() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ListCounting<'a> {
        inner: &'a JournaledTrash<FolderBackend>,
        lists: AtomicUsize,
    }
    impl ncrs_fs::Trash for ListCounting<'_> {
        fn trash(&self, path: &Path) -> Result<(), String> {
            self.inner.trash(path)
        }
        fn reach(&self) -> ncrs_fs::TrashReach {
            self.inner.reach()
        }
        fn list(&self) -> Result<Vec<ncrs_fs::TrashedItem>, String> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            self.inner.list()
        }
        fn restore(&self, item: &ncrs_fs::TrashedItem) -> Result<(), ncrs_fs::RestoreError> {
            self.inner.restore(item)
        }
    }

    let setup = Setup::new();
    let bin = bin(&setup);
    let counting = ListCounting {
        inner: &bin.trash,
        lists: AtomicUsize::new(0),
    };
    let items: Vec<TrashItem> = ["a", "b", "c"]
        .iter()
        .map(|name| duplicate(&setup, &format!("{name}.pdf"), name).0)
        .collect();

    trash_run(&setup.index, "Cleared", &setup.root, &items, &counting).unwrap();

    assert_eq!(counting.lists.load(Ordering::SeqCst), 1);
    let journaled = setup.index.journal_items(last_batch(&setup)).unwrap();
    assert!(journaled
        .iter()
        .all(|item| item.to.starts_with(&bin.folder)));
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

fn own_item(path: &Path) -> crate::execute::trash::OwnTrashItem {
    let meta = fs::metadata(path).unwrap();
    crate::execute::trash::OwnTrashItem {
        path: path.to_path_buf(),
        size: meta.len(),
        modified: meta.modified().ok(),
    }
}

#[test]
fn an_own_file_goes_to_the_trash_and_the_undo_brings_it_back() {
    let setup = Setup::new();
    let bin = bin(&setup);
    let file = setup.write("downloads/old.dmg", "installer");
    let item = own_item(&file);

    let report = crate::execute::trash::trash_run_own(
        &setup.index,
        "Cleared",
        &setup.root,
        &[item],
        &bin.trash,
    )
    .unwrap();

    assert_eq!(
        report,
        TrashReport {
            trashed: 1,
            skipped: 0
        }
    );
    assert!(!file.exists());
    let undone = undo_with_trash(
        &setup.index,
        last_batch(&setup),
        std::slice::from_ref(&setup.root),
        Some(&bin.trash),
    )
    .unwrap();
    assert_eq!(undone, UndoReport { undone: 1, left: 0 });
    assert_eq!(fs::read_to_string(&file).unwrap(), "installer");
}

#[test]
fn an_own_file_that_changed_since_it_was_shown_stays() {
    let setup = Setup::new();
    let bin = bin(&setup);
    let file = setup.write("downloads/notes.txt", "short");
    let item = own_item(&file);
    fs::write(&file, "much longer than before").unwrap();

    let report = crate::execute::trash::trash_run_own(
        &setup.index,
        "Cleared",
        &setup.root,
        &[item],
        &bin.trash,
    )
    .unwrap();

    assert_eq!(
        report,
        TrashReport {
            trashed: 0,
            skipped: 1
        }
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "much longer than before"
    );
}

#[test]
fn an_own_item_outside_the_root_stays() {
    let setup = Setup::new();
    let bin = bin(&setup);
    let outside = setup._dir.path().join("elsewhere.txt");
    fs::write(&outside, "x").unwrap();
    let item = own_item(&outside);

    let report = crate::execute::trash::trash_run_own(
        &setup.index,
        "Cleared",
        &setup.root,
        &[item],
        &bin.trash,
    )
    .unwrap();

    assert_eq!(
        report,
        TrashReport {
            trashed: 0,
            skipped: 1
        }
    );
    assert!(outside.exists());
}

#[test]
fn empty_after_deletes_only_the_entry_this_run_made() {
    let setup = Setup::new();
    let bin = bin(&setup);
    let old_file = setup.write("downloads/report.pdf", "old one");
    bin.trash.trash(&old_file).unwrap();
    let file = setup.write("downloads/report.pdf", "new one");
    let item = own_item(&file);

    let run =
        crate::execute::trash_own_items(None, "Cleared", &setup.root, &[item], &bin.trash, true);

    assert_eq!(run.gone, vec![file.clone()]);
    let left = bin.trash.list().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].original, file);
    assert_eq!(
        fs::read_to_string(Path::new(&left[0].key)).unwrap(),
        "old one"
    );
}

#[test]
fn a_folder_with_a_changed_file_inside_stays_with_and_without_the_journal() {
    let setup = Setup::new();
    let bin = bin(&setup);
    let inner = setup.write("projects/app/main.rs", "fn main() {}");
    let folder = inner.parent().unwrap().to_path_buf();
    let item = own_item(&folder);
    fs::write(&inner, "fn main() { more() }").unwrap();
    let changed = vec![crate::execute::SkipReason::SourceChanged];

    let plain = crate::execute::trash_own_items(
        None,
        "Cleared",
        &setup.root,
        std::slice::from_ref(&item),
        &bin.trash,
        true,
    );
    let journaled = crate::execute::trash_own_items(
        Some(&setup.index),
        "Cleared",
        &setup.root,
        &[item],
        &bin.trash,
        true,
    );

    assert!(plain.gone.is_empty() && journaled.gone.is_empty());
    assert_eq!(plain.refused, changed);
    assert_eq!(journaled.refused, changed);
    assert!(inner.exists());
}
