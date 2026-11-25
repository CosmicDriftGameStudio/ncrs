use super::*;

fn file_one(setup: &Setup, name: &str, folder: &str) -> BatchId {
    let to = format!("{folder}/{name}");
    setup.file(
        &format!("File {name}"),
        Transfer::Move,
        &[(name, to.as_str())],
    )
}

fn batches(runs: &[RecentRun]) -> Vec<BatchId> {
    runs.iter().map(|run| run.summary.batch).collect()
}

#[test]
fn the_recent_runs_come_newest_first_up_to_the_limit() {
    let setup = Setup::new();
    let first = file_one(&setup, "a.txt", "Bank/2024");
    let second = file_one(&setup, "b.txt", "Steuer");
    let third = file_one(&setup, "c.txt", "Bank/2024");

    let runs = setup.recent_runs(50);
    assert_eq!(batches(&runs), [third, second, first]);
    assert_eq!(setup.recent_runs(2).len(), 2);
    assert_eq!(
        runs[0].target_folder.as_deref(),
        Some(setup.archive().as_path())
    );
    assert_eq!(runs[0].examples, [PathBuf::from("Bank").join("2024")]);
    assert_eq!(runs[0].summary.items, 1);
    assert!(runs.iter().all(|run| run.undoable));
}

#[test]
fn a_named_batch_is_undone_and_the_others_stay() {
    let setup = Setup::new();
    let older = file_one(&setup, "a.txt", "Bank");
    let newer = file_one(&setup, "b.txt", "Bank");

    assert_eq!(setup.undo(older), 1);

    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.archive().join("Bank/a.txt").exists());
    assert!(setup.archive().join("Bank/b.txt").exists());
    assert_eq!(batches(&setup.recent_runs(50)), [newer]);
}

#[test]
fn a_trash_run_is_listed_without_the_right_to_undo_it() {
    let setup = Setup::new();
    setup.trash_copy();
    file_one(&setup, "b.txt", "Bank");

    let flags: Vec<(Transfer, bool)> = setup
        .recent_runs(50)
        .iter()
        .map(|run| (run.summary.transfer, run.undoable))
        .collect();
    assert_eq!(flags, [(Transfer::Move, true), (Transfer::Trash, false)]);
}

#[test]
fn a_run_into_a_folder_below_its_source_is_listed_without_the_right_to_undo_it() {
    let setup = Setup::new();
    let item = setup.item("a.txt", "first", "Bank/a.txt");
    setup.execute("Whole folder", Transfer::Move, "", &[item], false);

    assert!(!setup.recent_runs(1)[0].undoable);
}
