use super::*;

pub(in crate::chat_work::tests) fn run_plan(setup: &Setup, transfer: Transfer) -> RunPlan {
    let source = setup.dir.path().join("inbox");
    let item = |name: &str, content: &str| {
        let from = setup.write(&format!("inbox/{name}"), content);
        RunItem {
            hash: hash_path(&from).unwrap(),
            to: PathBuf::from("Alfa").join(name),
            from,
        }
    };
    RunPlan {
        name: "Tidy up inbox".to_owned(),
        transfer,
        source_root: source,
        target_root: setup.dir.path().join("archive"),
        inside: Vec::new(),
        items: vec![item("a.txt", "first"), item("b.txt", "second")],
    }
}

#[test]
fn a_run_outside_its_roots_is_refused_as_a_whole() {
    let setup = Setup::new();
    fs::create_dir_all(setup.dir.path().join("archive")).unwrap();
    let outside = setup.write("elsewhere/x.txt", "x");
    let refused_with = |change: &dyn Fn(&mut RunPlan)| {
        let mut run = run_plan(&setup, Transfer::Move);
        change(&mut run);
        match setup.access.run(ChatWork::File { run }) {
            ChatResult::Filed(result) => result.unwrap_err(),
            other => panic!("{other:?}"),
        }
    };

    refused_with(&|run| run.items[1].from = outside.clone());
    refused_with(&|run| run.items[0].to = PathBuf::from("../escape.txt"));
    refused_with(&|run| run.items[0].to = PathBuf::from("/etc/escape.txt"));
    refused_with(&|run| run.target_root = run.source_root.join("archive"));
    refused_with(&|run| run.source_root = run.target_root.join("inbox"));
    refused_with(&|run| run.target_root = run.source_root.clone());
    refused_with(&|run| run.transfer = Transfer::Trash);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.dir.path().join("archive/Alfa").exists());
}

#[test]
fn files_inside_the_target_are_renamed_in_the_same_run_and_undone_with_it() {
    let setup = Setup::new();
    let old = setup.write("archive/Gehalt/2017/old.txt", "old");
    setup.write("archive/Keep/2017/other.txt", "other");
    let mut run = run_plan(&setup, Transfer::Move);
    let archive = setup.dir.path().join("archive");
    run.inside = vec![RunItem {
        hash: hash_path(&old).unwrap(),
        from: old.clone(),
        to: PathBuf::from("Beta/new.txt"),
    }];

    let outside = |change: &dyn Fn(&mut RunPlan)| {
        let mut refused = run.clone();
        change(&mut refused);
        match setup.access.run(ChatWork::File { run: refused }) {
            ChatResult::Filed(result) => result.unwrap_err(),
            other => panic!("{other:?}"),
        }
    };
    outside(&|plan| plan.inside[0].from = setup.dir.path().join("inbox/a.txt"));
    outside(&|plan| plan.inside[0].to = PathBuf::from("../escape.txt"));
    assert!(old.exists());

    let ChatResult::Filed(filed) = setup.access.run(ChatWork::File { run }) else {
        panic!("not a filing result");
    };
    let report = filed.unwrap();
    assert_eq!((report.done, report.skipped, report.inside_done), (2, 0, 1));
    assert!(!old.exists());
    assert!(archive.join("Beta/new.txt").exists());
    assert!(!archive.join("Gehalt").exists(), "the emptied folders go");
    assert!(archive.join("Keep/2017/other.txt").exists());
    assert!(archive.exists());

    let summary = last_run(&setup).unwrap();
    assert_eq!(summary.items, 3);
    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: summary.batch,
    }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 3);
    assert!(old.exists());
    assert!(!archive.join("Beta").exists());
    assert!(setup.dir.path().join("inbox/a.txt").exists());
}

pub(in crate::chat_work::tests) fn last_run(setup: &Setup) -> Option<RunSummary> {
    match setup.access.run(ChatWork::LastRun) {
        ChatResult::LastRun(result) => result.unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_move_run_is_summarised_and_undone_from_the_journal() {
    let setup = Setup::new();
    fs::create_dir_all(setup.dir.path().join("archive")).unwrap();
    assert_eq!(last_run(&setup), None);
    let run = run_plan(&setup, Transfer::Move);
    let ChatResult::Filed(filed) = setup.access.run(ChatWork::File { run }) else {
        panic!("not a filing result");
    };
    let report = filed.unwrap();
    assert_eq!((report.done, report.skipped, report.inside_done), (2, 0, 0));
    assert!(!setup.dir.path().join("inbox/a.txt").exists());
    assert!(setup.dir.path().join("archive/Alfa/a.txt").exists());

    let summary = last_run(&setup).unwrap();
    assert_eq!(summary.name, "Tidy up inbox");
    assert_eq!(summary.transfer, Transfer::Move);
    assert_eq!(summary.items, 2);
    assert_eq!(
        summary.source_folder.as_deref(),
        Some(setup.dir.path().join("inbox").as_path())
    );

    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: summary.batch,
    }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 2);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.dir.path().join("archive/Alfa/a.txt").exists());
    assert_eq!(last_run(&setup), None);
}

#[test]
fn a_copy_run_leaves_the_originals_and_its_undo_removes_only_the_copies() {
    let setup = Setup::new();
    fs::create_dir_all(setup.dir.path().join("archive")).unwrap();
    let run = run_plan(&setup, Transfer::Copy);
    let ChatResult::Filed(report) = setup.access.run(ChatWork::File { run }) else {
        panic!("not a filing result");
    };
    assert_eq!(report.unwrap().done, 2);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(setup.dir.path().join("archive/Alfa/a.txt").exists());
    let summary = last_run(&setup).unwrap();
    assert_eq!(summary.transfer, Transfer::Copy);
    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: summary.batch,
    }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 2);
    assert!(setup.dir.path().join("inbox/a.txt").exists());
    assert!(!setup.dir.path().join("archive/Alfa/a.txt").exists());
}

#[test]
fn duplicates_go_to_the_trash_and_come_back_with_the_undo() {
    let setup = Setup::new();
    let keeper = setup.write("archive/x.txt", "same");
    let copy = setup.write("inbox/x.txt", "same");
    let item = TrashItem {
        path: copy.clone(),
        expected_hash: hash_path(&keeper).unwrap(),
        keeper,
    };
    let ChatResult::Trashed(report) = setup.access.run(ChatWork::TrashDuplicates {
        name: "Duplicates".to_owned(),
        root: setup.dir.path().join("inbox"),
        items: vec![item],
    }) else {
        panic!("not a trash result");
    };
    assert_eq!(
        report.unwrap(),
        TrashReport {
            trashed: 1,
            skipped: 0,
        }
    );
    assert!(!copy.exists());
    let summary = last_run(&setup).unwrap();
    assert_eq!(summary.transfer, Transfer::Trash);
    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: summary.batch,
    }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 1);
    assert_eq!(fs::read_to_string(copy).unwrap(), "same");
}

#[test]
fn files_filed_by_hermes_itself_are_the_last_run_and_can_be_put_back() {
    let setup = Setup::new();
    let archive = setup.dir.path().join("archive");
    fs::create_dir_all(&archive).unwrap();
    let from = setup.write("inbox/a.txt", "first");
    let item = crate::execute::ExecutionItem {
        from: from.clone(),
        to: archive.join("Alfa/a.txt"),
        expected_hash: ncrs_intake::hash_path(&from).unwrap(),
        document_id: None,
        duplicate: false,
    };
    setup
        .access
        .with_index(|index| {
            crate::execute::execute_with(
                index,
                ncrs_index::BatchKind::Filing,
                &archive,
                &[item],
                &mut ncrs_fs::transfer::Counting,
                &ncrs_fs::transfer::Move,
            )
            .map_err(|err| err.to_string())
        })
        .unwrap();

    let summary = last_run(&setup).unwrap();
    assert!(summary.automatic);
    assert_eq!(summary.items, 1);
    assert_eq!(summary.name, archive.to_string_lossy());
    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: summary.batch,
    }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 1);
    assert!(from.exists());
    assert_eq!(last_run(&setup), None);
}
