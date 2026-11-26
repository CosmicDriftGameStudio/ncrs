use std::io;

use ncrs_fs::transfer::Progress;
use ncrs_index::{DocumentStatus, ReviewReason};
use ncrs_intake::{scan_location, Location, LocationMode, StabilityTracker};

use super::*;
use crate::execute::SkipReason;

#[test]
fn a_source_is_filed_into_its_target_and_taken_back() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let from = setup.write("Q1/deep/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1"]);

    let report = setup.run(&source).unwrap();

    let to = setup.path("T1/Acme/A1.txt");
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(to.exists() && !from.exists());
    assert!(!setup.path("Q1/deep").exists(), "the emptied folder goes");
    assert!(setup.path("Q1").exists(), "the source itself stays");
    assert_eq!(setup.document(&to).status, DocumentStatus::Filed);
    assert_eq!(setup.runs(), 1);

    let undone = setup.access.undo(report.run.unwrap()).unwrap();
    assert_eq!((undone.undone, undone.left), (1, 0));
    assert!(from.exists() && !to.exists());
    let document = setup.document(&from);
    let question = setup.index(|index| index.open_review_of(document.id).unwrap().unwrap());
    assert_eq!(question.reason, ReviewReason::Undone);

    assert_eq!(setup.run(&source).unwrap().done(), 1);
    assert!(to.exists() && !from.exists());
    let filed_again = setup.document(&to);
    assert_eq!(filed_again.status, DocumentStatus::Filed);
    setup.index(|index| assert!(index.open_review_of(filed_again.id).unwrap().is_none()));
}

#[test]
fn two_sources_share_one_target_and_the_second_one_sees_the_names_of_the_first() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.write("Q1/a.txt", "acme Ref A1");
    setup.write("Q2/b.txt", "acme Ref A2");
    let clash = setup.write("Q2/c.txt", "acme once more Ref A1");
    let first = setup.source("Q1", &["t1"]);
    let second = setup.source("Q2", &["t1"]);

    assert_eq!(setup.run(&first).unwrap().done(), 1);
    assert_eq!(setup.run(&second).unwrap().done(), 1);

    assert!(setup.path("T1/Acme/A1.txt").exists());
    assert!(setup.path("T1/Acme/A2.txt").exists());
    assert!(clash.exists());
    assert!(matches!(
        state_of(&setup.plan(&second), "c.txt"),
        RowState::NameTaken {
            by: Taken::OnDisk,
            ..
        }
    ));
}

#[test]
fn several_targets_are_copied_into_and_taken_back_as_one_run() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "acme", "Both/{number}");
    let from = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1", "t2"]);

    let report = setup.run(&source).unwrap();

    let first = setup.path("T1/Acme/A1.txt");
    let second = setup.path("T2/Both/A1.txt");
    assert!(from.exists() && first.exists() && second.exists());
    let document = setup.document(&from);
    assert_eq!(document.status, DocumentStatus::Filed);
    let known = setup.index(|index| index.duplicate_paths(document.id).unwrap());
    assert_eq!(known, [first.clone(), second.clone()]);
    assert_eq!(setup.runs(), 1);

    let undone = setup.access.undo(report.run.unwrap()).unwrap();
    assert_eq!((undone.undone, undone.left), (2, 0));
    assert!(from.exists() && !first.exists() && !second.exists());
    let known_after_undo = setup.index(|index| index.duplicate_paths(document.id).unwrap());
    assert!(known_after_undo.is_empty());

    assert_eq!(setup.run(&source).unwrap().done(), 1);
    assert!(from.exists() && first.exists() && second.exists());
    let filed_again = setup.document(&from);
    assert_eq!(
        (filed_again.id, filed_again.status),
        (document.id, DocumentStatus::Filed)
    );
    setup.index(|index| assert!(index.open_review_of(document.id).unwrap().is_none()));
}

#[test]
fn two_sources_and_two_targets_each_file_goes_where_its_rule_says() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "zorg", "Zorg/{number}");
    setup.write("Q1/a.txt", "acme Ref A1");
    setup.write("Q1/z.txt", "zorg Ref Z1");
    setup.write("Q2/a.txt", "acme Ref A2");
    setup.write("Q2/z.txt", "zorg Ref Z2");

    for folder in ["Q1", "Q2"] {
        let report = setup.run(&setup.source(folder, &["t1", "t2"])).unwrap();
        assert_eq!(report.done(), 2);
    }

    for place in ["T1/Acme/A1", "T1/Acme/A2", "T2/Zorg/Z1", "T2/Zorg/Z2"] {
        assert!(setup.path(&format!("{place}.txt")).exists(), "{place}");
    }
    assert_eq!(setup.runs(), 2, "one entry per run, whatever its targets");
}

#[test]
fn one_copy_that_fails_leaves_the_others_and_is_told() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "acme", "Both/{number}");
    let from = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1", "t2"]);
    let request = request_of(&source, &setup.plan(&source));
    setup.write("T2/Both/A1.txt", "came in between");

    let report = run(&setup.access, &request, &mut Counting).unwrap();

    let outcomes: Vec<_> = report.entries[0]
        .places
        .iter()
        .map(|place| &place.outcome)
        .collect();
    assert_eq!(
        outcomes,
        [
            &PlaceOutcome::Done,
            &PlaceOutcome::Skipped(SkipReason::TargetExists)
        ]
    );
    assert!(from.exists() && setup.path("T1/Acme/A1.txt").exists());
}

#[test]
fn a_copied_file_is_not_decided_again_after_the_folders_are_scanned() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let from = setup.write("Q1/a.txt", "acme Ref A1");
    let source = SortSource {
        transfer: LocationTransfer::Copy,
        ..setup.source("Q1", &["t1"])
    };
    setup.watch(&SortSource {
        path: setup.path("T2"),
        ..source.clone()
    });
    setup.run(&source).unwrap();
    let id = setup.document(&from).id;

    for (folder, mode) in [("Q1", LocationMode::Inbox), ("T1", LocationMode::Archive)] {
        let location = Location::new(&setup.path(folder), mode, true).unwrap();
        let mut tracker = StabilityTracker::new();
        for ahead in [0, 60] {
            let now = SystemTime::now() + Duration::from_secs(ahead);
            setup.index_mut(|index| {
                scan_location(index, &location, &mut tracker, now).unwrap();
            });
        }
    }
    setup.index(|index| crate::execute::reconcile(index).unwrap());

    let document = setup.document(&from);
    assert_eq!((document.id, document.status), (id, DocumentStatus::Filed));
    assert!(matches!(
        state_of(&setup.plan(&source), "a.txt"),
        RowState::AlreadyThere { path, .. } if *path == setup.path("T1/Acme/A1.txt")
    ));
}

#[test]
fn a_copy_tidied_away_by_hand_is_only_made_again_when_the_user_asks() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = SortSource {
        transfer: LocationTransfer::Copy,
        ..setup.source("Q1", &["t1"])
    };
    setup.watch(&source);
    let from = setup.write("Q1/a.txt", "acme Ref A1");
    let copy = setup.path("T1/Acme/A1.txt");
    assert_eq!(filed(&setup.watch_after_reading(&source)), 1);
    assert!(from.exists() && copy.exists());
    assert!(matches!(
        state_of(&setup.plan(&source), "a.txt"),
        RowState::AlreadyThere { path, .. } if *path == copy
    ));

    fs::remove_file(&copy).unwrap();
    assert_eq!(filed(&setup.watch(&source)), 0);
    assert!(!copy.exists());
    assert_eq!(place_of(&setup.plan(&source), "a.txt").target, id("t1"));
    assert_eq!(setup.run(&source).unwrap().done(), 1);
    assert!(from.exists() && copy.exists());
    assert!(matches!(
        state_of(&setup.plan(&source), "a.txt"),
        RowState::AlreadyThere { path, .. } if *path == copy
    ));
}

#[test]
fn a_missing_target_stops_the_whole_source_and_is_never_made() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "zorg", "Zorg/{number}");
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1", "t2"]);
    fs::remove_dir(setup.path("T2")).unwrap();

    let plan = setup.plan(&source);
    assert_eq!(plan.waiting, [(id("t2"), WaitReason::Missing)]);
    place_of(&plan, "a.txt");

    let refused = run(&setup.access, &request_of(&source, &plan), &mut Counting);
    assert_eq!(
        refused,
        Err(RunError::Waiting(vec![(id("t2"), WaitReason::Missing)]))
    );
    assert!(path.exists() && !setup.path("T2").exists());
    assert_eq!(setup.runs(), 0);
}

#[test]
fn a_paused_target_waits() {
    let mut setup = Setup::new();
    setup.access.targets[0].paused = Some(ncrs_config::config::PauseReason::UnusablePath(
        "x".to_owned(),
    ));
    let plan = setup.plan(&setup.source("Q1", &["t1"]));
    assert_eq!(plan.waiting, [(id("t1"), WaitReason::Paused)]);
}

#[test]
fn a_target_taken_over_waits_until_one_of_its_documents_is_seen() {
    let mut setup = Setup::new();
    setup.add_target("old", "Old", None);
    setup.rule("old", "acme", "Acme/{number}");
    setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["old"]);
    assert_eq!(
        setup.plan(&source).waiting,
        [(id("old"), WaitReason::Unconfirmed)]
    );

    setup.write("Old/known.txt", "a letter filed long ago");
    setup.plan(&setup.source("Old", &["old"]));
    assert_eq!(setup.run(&source).unwrap().done(), 1);
}

#[test]
fn documents_that_are_all_gone_from_a_target_are_counted_those_marked_missing_too() {
    let mut setup = Setup::new();
    setup.add_target("old", "Old", None);
    let gone = |folder: &str| setup.access.all_gone_below(&setup.path(folder));
    assert_eq!(gone("Old"), Ok(0));

    let first = setup.write("Old/a.txt", "a letter filed long ago");
    let second = setup.write("Old/2024/b.txt", "another letter");
    setup.plan(&setup.source("Old", &["t1"]));
    assert_eq!(gone("Old"), Ok(0));

    fs::remove_file(&second).unwrap();
    assert_eq!(gone("Old"), Ok(0), "one of them is there");
    fs::remove_file(&first).unwrap();
    let document = setup.document(&first).id;
    setup.index(|index| index.mark_document_missing(document).unwrap());
    assert_eq!(gone("Old"), Ok(2));
    assert_eq!(gone("T1"), Ok(0));
}

#[test]
fn a_file_or_place_that_changed_since_the_plan_is_left() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let changed = setup.write("Q1/changed.txt", "acme Ref A1");
    let gone = setup.write("Q1/gone.txt", "acme Ref A2");
    let taken = setup.write("Q1/taken.txt", "acme Ref A3");
    let moved = setup.write("Q1/works.txt", "acme Ref A4");
    let source = setup.source("Q1", &["t1"]);
    let request = request_of(&source, &setup.plan(&source));
    setup.write("Q1/changed.txt", "acme Ref A1 and more");
    fs::remove_file(&gone).unwrap();
    setup.write("T1/Acme/A3.txt", "came in between");

    let report = run(&setup.access, &request, &mut Counting).unwrap();

    let outcomes: Vec<_> = report
        .entries
        .iter()
        .map(|entry| entry.places[0].outcome.clone())
        .collect();
    assert_eq!(
        outcomes,
        [
            PlaceOutcome::Skipped(SkipReason::SourceChanged),
            PlaceOutcome::Skipped(SkipReason::SourceMissing),
            PlaceOutcome::Skipped(SkipReason::TargetExists),
            PlaceOutcome::Done,
        ]
    );
    assert!(changed.exists() && taken.exists() && !moved.exists());
}

#[test]
fn a_place_typed_by_hand_that_leaves_the_target_is_refused() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1"]);
    let plan = setup.plan(&source);
    for (target, place) in [("t1", "../a.txt"), ("t1", "CON.txt"), ("t2", "Fine/a.txt")] {
        let mut request = request_of(&source, &plan);
        request.entries[0].places = vec![(id(target), PathBuf::from(place))];
        let report = run(&setup.access, &request, &mut Counting).unwrap();
        assert_eq!(
            report.entries[0].places[0].outcome,
            PlaceOutcome::BadPlace,
            "{target} {place}"
        );
        assert_eq!(report.run, None);
    }
    assert!(path.exists());

    let mut request = request_of(&source, &plan);
    request.entries[0].places = vec![(id("t1"), Path::new("Mine").join("b.txt"))];
    run(&setup.access, &request, &mut Counting).unwrap();
    assert!(setup.path("T1/Mine/b.txt").exists());
}

#[test]
fn the_index_is_free_between_two_files_and_the_run_can_be_stopped_there() {
    struct Stopping<'a> {
        access: &'a SortingAccess,
        free: usize,
    }
    impl Progress for Stopping<'_> {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            if self.access.slot.try_lock().is_ok() {
                self.free += 1;
            }
            Err(io::Error::other("stop"))
        }
    }
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.write("Q1/a.txt", "acme Ref A1");
    let second = setup.write("Q1/b.txt", "acme Ref A2");
    let source = setup.source("Q1", &["t1"]);
    let request = request_of(&source, &setup.plan(&source));
    let mut progress = Stopping {
        access: &setup.access,
        free: 0,
    };

    let report = run(&setup.access, &request, &mut progress).unwrap();

    assert_eq!((report.done(), progress.free), (1, 1));
    assert_eq!(
        report.entries[1].places[0].outcome,
        PlaceOutcome::Skipped(SkipReason::Cancelled)
    );
    assert!(second.exists());
    let batches = setup.index(|index| index.recent_batches(10).unwrap());
    assert!(batches.iter().all(|(batch, _)| batch.finished_at.is_some()));
}
