use super::*;

fn file_one(setup: &Setup, name: &str, folder: &str) -> BatchId {
    let from = setup.write(&format!("inbox/{name}"), name);
    let run = RunPlan {
        name: format!("File {name}"),
        transfer: Transfer::Move,
        source_root: setup.dir.path().join("inbox"),
        target_root: setup.dir.path().join("archive"),
        inside: Vec::new(),
        items: vec![RunItem {
            hash: hash_path(&from).unwrap(),
            to: PathBuf::from(folder).join(name),
            from,
        }],
    };
    let ChatResult::Filed(report) = setup.access.run(ChatWork::File { run }) else {
        panic!("not a filing result");
    };
    assert_eq!(report.unwrap().done, 1);
    setup
        .access
        .recent_runs(1)
        .unwrap()
        .first()
        .unwrap()
        .summary
        .batch
}

fn undo(setup: &Setup, batch: BatchId) {
    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo { batch }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 1);
}

#[test]
fn the_recent_runs_come_newest_first_up_to_the_limit() {
    let setup = Setup::new();
    fs::create_dir_all(setup.dir.path().join("archive")).unwrap();
    let first = file_one(&setup, "a.txt", "Bank/2024");
    let second = file_one(&setup, "b.txt", "Steuer");
    let third = file_one(&setup, "c.txt", "Bank/2024");

    let runs = setup.access.recent_runs(50).unwrap();
    let batches: Vec<BatchId> = runs.iter().map(|run| run.summary.batch).collect();
    assert_eq!(batches, [third, second, first]);
    assert_eq!(setup.access.recent_runs(2).unwrap().len(), 2);
    assert_eq!(
        runs[0].target_folder.as_deref(),
        Some(setup.dir.path().join("archive").as_path())
    );
    assert_eq!(runs[0].examples, [PathBuf::from("Bank").join("2024")]);
    assert_eq!(runs[0].summary.items, 1);
    assert!(runs.iter().all(|run| run.undoable));
}

#[test]
fn a_named_batch_is_undone_and_the_others_stay() {
    let setup = Setup::new();
    fs::create_dir_all(setup.dir.path().join("archive")).unwrap();
    let older = file_one(&setup, "a.txt", "Bank");
    let newer = file_one(&setup, "b.txt", "Bank");

    undo(&setup, older);

    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.dir.path().join("archive/Bank/a.txt").exists());
    assert!(setup.dir.path().join("archive/Bank/b.txt").exists());
    let left: Vec<BatchId> = setup
        .access
        .recent_runs(50)
        .unwrap()
        .iter()
        .map(|run| run.summary.batch)
        .collect();
    assert_eq!(left, [newer]);
}

#[test]
fn a_trash_run_is_listed_without_the_right_to_undo_it() {
    let setup = Setup::new();
    fs::create_dir_all(setup.dir.path().join("archive")).unwrap();
    let keeper = setup.write("archive/x.txt", "same");
    let copy = setup.write("inbox/x.txt", "same");
    let ChatResult::Trashed(report) = setup.access.run(ChatWork::TrashDuplicates {
        name: "Duplicates".to_owned(),
        root: setup.dir.path().join("inbox"),
        items: vec![TrashItem {
            path: copy,
            expected_hash: hash_path(&keeper).unwrap(),
            keeper,
        }],
    }) else {
        panic!("not a trash result");
    };
    assert_eq!(report.unwrap().trashed, 1);
    file_one(&setup, "b.txt", "Bank");

    let runs = setup.access.recent_runs(50).unwrap();
    let flags: Vec<(Transfer, bool)> = runs
        .iter()
        .map(|run| (run.summary.transfer, run.undoable))
        .collect();
    assert_eq!(flags, [(Transfer::Move, true), (Transfer::Trash, false)]);
}

#[test]
fn a_missing_folder_has_no_known_structure_and_an_empty_one_has_no_areas() {
    let setup = Setup::new();
    let archive = setup.dir.path().join("archive");
    setup.access.known_structure(&archive).unwrap_err();

    fs::create_dir_all(&archive).unwrap();
    let structure = setup.access.known_structure(&archive).unwrap();
    assert!(structure.areas.is_empty());
}
