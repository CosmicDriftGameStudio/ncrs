use super::*;

const TWO_FILES: [(&str, &str); 2] = [("a.txt", "Alfa/a.txt"), ("b.txt", "Alfa/b.txt")];

#[test]
fn files_moved_inside_the_target_count_to_the_run_and_come_back_with_it() {
    let setup = Setup::new();
    let items = [
        setup.item("inbox/a.txt", "first", "Alfa/a.txt"),
        setup.item("archive/Gehalt/2017/old.txt", "old", "Beta/new.txt"),
    ];
    setup.execute("Tidy up inbox", Transfer::Move, "inbox", &items, true);
    assert!(setup.archive().join("Beta/new.txt").exists());

    let summary = setup.last_run().unwrap();
    assert_eq!(summary.items, 2);
    assert!(setup.recent_runs(1)[0].undoable);

    assert_eq!(setup.undo(summary.batch), 2);
    assert!(items[0].from.exists());
    assert!(items[1].from.exists());
    assert!(!setup.archive().join("Beta").exists());
}

#[test]
fn a_move_run_is_summarised_and_undone_from_the_journal() {
    let setup = Setup::new();
    assert_eq!(setup.last_run(), None);
    setup.file("Tidy up inbox", Transfer::Move, &TWO_FILES);
    assert!(!setup.dir.path().join("inbox/a.txt").exists());

    let summary = setup.last_run().unwrap();
    assert_eq!(summary.name, "Tidy up inbox");
    assert_eq!(summary.transfer, Transfer::Move);
    assert_eq!(summary.items, 2);
    assert_eq!(
        summary.source_folder.as_deref(),
        Some(setup.dir.path().join("inbox").as_path())
    );

    assert_eq!(setup.undo(summary.batch), 2);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.archive().join("Alfa/a.txt").exists());
    assert_eq!(setup.last_run(), None);
}

#[test]
fn a_copy_run_leaves_the_originals_and_its_undo_removes_only_the_copies() {
    let setup = Setup::new();
    setup.file("Tidy up inbox", Transfer::Copy, &TWO_FILES);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(setup.archive().join("Alfa/a.txt").exists());

    let summary = setup.last_run().unwrap();
    assert_eq!(summary.transfer, Transfer::Copy);

    assert_eq!(setup.undo(summary.batch), 2);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.archive().join("Alfa/a.txt").exists());
}

#[test]
fn duplicates_in_the_trash_come_back_with_the_undo() {
    let setup = Setup::new();
    let copy = setup.trash_copy();

    let summary = setup.last_run().unwrap();
    assert_eq!(summary.transfer, Transfer::Trash);

    assert_eq!(setup.undo(summary.batch), 1);
    assert_eq!(fs::read_to_string(copy).unwrap(), "same");
}

#[test]
fn files_filed_by_hermes_itself_are_the_last_run_and_can_be_put_back() {
    let setup = Setup::new();
    let archive = setup.archive();
    fs::create_dir_all(&archive).unwrap();
    let item = setup.item("inbox/a.txt", "first", "Alfa/a.txt");
    let from = item.from.clone();
    setup
        .access
        .with_index(|index| {
            execute_run(
                index,
                BatchKind::Filing,
                "Downloads",
                Transfer::Move,
                RunPlaces {
                    source: &setup.dir.path().join("inbox"),
                    target: &archive,
                    inside_target: false,
                },
                &[item],
            )
            .map_err(|err| err.to_string())
        })
        .unwrap();

    let summary = setup.last_run().unwrap();
    assert!(summary.automatic);
    assert_eq!(summary.items, 1);
    assert_eq!(summary.name, "Downloads");
    assert_eq!(setup.undo(summary.batch), 1);
    assert!(from.exists());
    assert_eq!(setup.last_run(), None);
}
