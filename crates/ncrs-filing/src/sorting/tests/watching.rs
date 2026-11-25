use ncrs_config::config::{PauseReason, SourceSetting};
use ncrs_index::{BatchKind, DocumentStatus, JournalState, NewJournalItem, ReviewReason, Transfer};

use super::*;
use crate::execute::{open_run_batch, RunPlaces};
use crate::reading_support::ReadError;

/// A source whose first look is done.
fn watched(setup: &Setup, targets: &[&str]) -> SortSource {
    let source = setup.source("Q1", targets);
    assert_eq!(
        setup.watch(&source),
        WatchedReport::BacklogRecorded { files: 0 }
    );
    source
}

#[test]
fn what_lay_in_the_source_stays_and_new_files_go() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let old = setup.write("Q1/old.txt", "acme Ref A1");
    let replaced = setup.write("Q1/replaced.txt", "acme Ref A2");
    // The filter plays no part in what counts as old.
    setup.write("Q1/old.md", "acme Ref A3");
    let source = SortSource {
        include: "*.txt".to_owned(),
        ..setup.source("Q1", &["t1"])
    };
    assert_eq!(
        setup.watch_after_reading(&source),
        WatchedReport::BacklogRecorded { files: 3 }
    );
    assert_eq!(filed(&setup.watch(&source)), 0);
    assert_eq!(setup.runs(), 0, "an empty run is no entry");

    setup.write("Q1/new.txt", "acme Ref A4");
    setup.write_at("Q1/replaced.txt", "acme Ref A5 under an old name", 300);
    assert_eq!(filed(&setup.watch_after_reading(&source)), 2);
    assert!(setup.path("T1/Acme/A4.txt").exists());
    assert!(setup.path("T1/Acme/A5.txt").exists());
    assert!(old.exists() && !replaced.exists());
    assert_eq!(setup.runs(), 1);

    let unfiltered = SortSource {
        include: String::new(),
        ..source
    };
    assert_eq!(filed(&setup.watch_after_reading(&unfiltered)), 0);
}

#[test]
fn what_came_while_the_source_was_not_watched_stays() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let meanwhile = setup.write("Q1/meanwhile.txt", "acme Ref A1");

    forget_backlog(&setup.access, std::slice::from_ref(&source.path)).unwrap();

    assert_eq!(
        setup.watch_after_reading(&source),
        WatchedReport::BacklogRecorded { files: 1 }
    );
    assert_eq!(filed(&setup.watch(&source)), 0);
    assert!(meanwhile.exists());
    setup.write("Q1/new.txt", "acme Ref A2");
    assert_eq!(filed(&setup.watch_after_reading(&source)), 1);
}

#[test]
fn an_empty_source_records_that_it_was_empty() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    setup.write("Q1/a.txt", "acme Ref A1");
    assert_eq!(filed(&setup.watch_after_reading(&source)), 1);
}

#[test]
fn only_changed_files_are_looked_at_when_they_are_named() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let named = setup.write("Q1/named.txt", "acme Ref A1");
    let other = setup.write("Q1/other.txt", "acme Ref A2");
    setup.plan(&source);

    let report = run_watched(
        &setup.access,
        &source,
        &WatchedFiles::Changed(vec![named.clone(), setup.path("Q1/gone.txt")]),
        None,
        "watched",
        &mut Counting,
    )
    .unwrap();
    assert_eq!(filed(&report), 1);
    assert!(!named.exists() && other.exists());
}

#[test]
fn a_file_the_pipeline_has_not_read_waits() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    assert_eq!(filed(&setup.watch(&source)), 0);
    assert!(path.exists());
}

#[test]
fn sources_that_are_not_to_be_filed_from_are_skipped() {
    let mut setup = Setup::new();
    let source = setup.source("Q1", &["t1"]);
    let skipped = |why| WatchedReport::Skipped(why);
    let off = SortSource {
        watch: false,
        ..source.clone()
    };
    assert_eq!(setup.watch(&off), skipped(WatchedSkip::NotWatched));
    assert_eq!(
        setup.watch(&setup.source("T1/inside", &["t1"])),
        skipped(WatchedSkip::InsideTarget)
    );
    let busy = run_watched(
        &setup.access,
        &source,
        &WatchedFiles::All,
        Some(&source.path),
        "watched",
        &mut Counting,
    );
    assert_eq!(busy, Ok(skipped(WatchedSkip::Busy)));
    setup.access.sources.push(SourceSetting {
        source: source.clone(),
        paused: Some(PauseReason::UnusablePath("x".to_owned())),
    });
    assert_eq!(setup.watch(&source), skipped(WatchedSkip::Paused));
    assert!(!setup.index(|index| index.has_source_backlog(&source.path).unwrap()));
}

#[test]
fn a_source_that_is_gone_waits_and_records_nothing() {
    let setup = Setup::new();
    let source = setup.source("Q1", &["t1"]);
    fs::remove_dir(&source.path).unwrap();
    assert_eq!(
        setup.watch(&source),
        WatchedReport::SourceWaiting(SourceWait::Folder(ReadError::Gone))
    );
    let on_drive = SortSource {
        own_volume: Some(true),
        ..source.clone()
    };
    assert_eq!(setup.watch(&on_drive), drive_is_missing());
    assert!(!setup.index(|index| index.has_source_backlog(&source.path).unwrap()));
}

fn drive_is_missing() -> WatchedReport {
    WatchedReport::SourceWaiting(SourceWait::NotMounted)
}

#[test]
fn an_empty_mount_point_is_not_what_lay_in_the_source() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = setup.source("Q1", &["t1"]);
    let has_backlog =
        || setup.index(|index| index.has_source_backlog(&source.path).unwrap());

    assert_eq!(setup.watch_drive(&source, || false), drive_is_missing());
    assert!(!has_backlog());

    let path = setup.write("Q1/a.txt", "acme Ref A1");
    setup.plan(&source);
    let looks = std::cell::Cell::new(0);
    let unplugged_on_the_way = || looks.replace(looks.get() + 1) == 0;
    assert_eq!(
        setup.watch_drive(&source, unplugged_on_the_way),
        drive_is_missing()
    );
    assert!(!has_backlog());

    assert_eq!(
        setup.watch_drive(&source, || true),
        WatchedReport::BacklogRecorded { files: 1 }
    );
    assert_eq!(filed(&setup.watch_drive(&source, || true)), 0);
    assert!(path.exists());

    let new = setup.write("Q1/b.txt", "acme Ref A2");
    setup.plan(&source);
    assert_eq!(setup.watch_drive(&source, || false), drive_is_missing());
    assert!(new.exists());
    assert_eq!(filed(&setup.watch_drive(&source, || true)), 1);
    assert!(path.exists() && !new.exists());
}

#[test]
fn a_backlog_taken_on_an_empty_mount_point_is_taken_anew_once_the_drive_is_there() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = setup.source("Q1", &["t1"]);
    let watch_on = |own_volume: Option<bool>| {
        setup.watch_volume(
            &source,
            Volume {
                is_mounted: |_: &Path| true,
                is_own: move |_: &Path| own_volume,
            },
        )
    };
    assert_eq!(
        watch_on(Some(false)),
        WatchedReport::BacklogRecorded { files: 0 }
    );

    let old = setup.write("Q1/old.txt", "acme Ref A1");
    setup.plan(&source);
    let looks = std::cell::Cell::new(0);
    let plugged_in_on_the_way = setup.watch_volume(
        &source,
        Volume {
            is_mounted: |_: &Path| true,
            is_own: |_: &Path| Some(looks.replace(looks.get() + 1) > 0),
        },
    );
    assert_eq!(plugged_in_on_the_way, drive_is_missing());
    assert_eq!(
        watch_on(Some(true)),
        WatchedReport::BacklogRecorded { files: 1 }
    );
    assert_eq!(filed(&watch_on(Some(true))), 0);
    assert!(old.exists());

    let new = setup.write("Q1/new.txt", "acme Ref A22");
    setup.plan(&source);
    assert_eq!(filed(&watch_on(Some(true))), 1);
    assert!(old.exists() && !new.exists());
}

#[test]
fn a_source_saved_on_a_drive_waits_on_the_system_disk() {
    let setup = Setup::new();
    let source = SortSource {
        own_volume: Some(true),
        ..setup.source("Q1", &["t1"])
    };
    // The folder for temporary files may be on a volume of its own.
    let expected = if is_on_own_volume(&setup.root) == Some(true) {
        WatchedReport::BacklogRecorded { files: 0 }
    } else {
        drive_is_missing()
    };
    assert_eq!(setup.watch(&source), expected);
}

#[test]
fn a_target_that_is_not_there_makes_the_source_wait() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    fs::remove_dir(setup.path("T1")).unwrap();
    assert_eq!(
        setup.watch_after_reading(&source),
        WatchedReport::Waiting(vec![(id("t1"), WaitReason::Missing)])
    );
    assert!(path.exists() && !setup.path("T1").exists());
}

#[test]
fn several_targets_ask_once_and_the_answer_survives_a_restart() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "acme", "Both/{number}");
    let source = watched(&setup, &["t1", "t2"]);
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    assert_eq!(asked(&setup.watch_after_reading(&source)), 1);
    assert_eq!(asked(&setup.watch(&source)), 0);
    assert!(path.exists());

    // The question lies in the index, not in the job.
    let db = setup.root.join("index.db");
    let closed = setup.access.slot.lock().unwrap().take();
    drop(closed);
    *setup.access.slot.lock().unwrap() =
        Some(Index::open(&db, &ncrs_index::InMemoryKeyStore::empty()).unwrap());

    assert_eq!(question_count(&setup.access, &source), Ok(1));
    let questions = open_questions(&setup.access, &source).unwrap();
    assert_eq!(questions.len(), 1);
    let QuestionKind::SeveralTargets(places) = &questions[0].kind else {
        panic!("{questions:?}");
    };
    assert_eq!(places.len(), 2);

    let chosen = vec![(places[0].target.clone(), places[0].relative.clone())];
    let report = answer(
        &setup.access,
        &source,
        questions[0].document,
        chosen,
        "answered",
        &mut Counting,
    )
    .unwrap();
    assert_eq!(report.done(), 1);
    assert!(setup.path("T1/Acme/A1.txt").exists() && !path.exists());
    assert_eq!(question_count(&setup.access, &source), Ok(0));
}

#[test]
fn an_answer_that_does_not_work_leaves_the_question_open() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "acme", "Both/{number}");
    let source = watched(&setup, &["t1", "t2"]);
    setup.write("Q1/a.txt", "acme Ref A1");
    setup.watch_after_reading(&source);
    let question = open_questions(&setup.access, &source).unwrap().remove(0);
    let place = setup.write("T1/Acme/A1.txt", "somebody else's");
    let relative = place.strip_prefix(setup.path("T1")).unwrap().to_path_buf();

    let report = answer(
        &setup.access,
        &source,
        question.document,
        vec![(id("t1"), relative)],
        "answered",
        &mut Counting,
    )
    .unwrap();
    assert_eq!(report.done(), 0);
    assert_eq!(question_count(&setup.access, &source), Ok(1));
}

#[test]
fn several_targets_are_copied_into_when_the_source_says_so() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "acme", "Both/{number}");
    let source = SortSource {
        several: OnSeveralTargets::CopyToAll,
        ..watched(&setup, &["t1", "t2"])
    };
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    assert_eq!(filed(&setup.watch_after_reading(&source)), 1);
    assert!(path.exists(), "the original stays although the source moves");
    assert!(setup.path("T1/Acme/A1.txt").exists());
    assert!(setup.path("T2/Both/A1.txt").exists());
    assert_eq!(setup.runs(), 1);
    assert_eq!(filed(&setup.watch_after_reading(&source)), 0);
    assert_eq!(setup.runs(), 1);
}

#[test]
fn an_open_question_about_several_targets_solves_itself_once_one_place_is_left() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let second = setup.rule("t2", "acme", "Both/{number}");
    let source = watched(&setup, &["t1", "t2"]);
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    assert_eq!(asked(&setup.watch_after_reading(&source)), 1);

    setup.index_mut(|index| {
        let mut spec = rule_spec(ConditionKind::Substring, "zorg", "Both/{number}");
        spec.confirmed = true;
        index.update_rule(second, &spec).unwrap();
    });
    assert_eq!(filed(&setup.watch(&source)), 1);
    assert!(!path.exists() && setup.path("T1/Acme/A1.txt").exists());
    assert!(setup.index(|index| index.open_reviews().unwrap().is_empty()));
}

#[test]
fn a_file_without_text_is_asked_about_or_left_as_the_source_says() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let skipping = SortSource {
        no_text: OnNoText::Skip,
        ..source.clone()
    };
    let path = setup.write("Q1/blank.txt", " \n");
    assert_eq!(asked(&setup.watch_after_reading(&skipping)), 0);
    assert_eq!(question_count(&setup.access, &source), Ok(0));

    assert_eq!(asked(&setup.watch(&source)), 1);
    assert_eq!(asked(&setup.watch(&source)), 0);
    let questions = open_questions(&setup.access, &source).unwrap();
    assert_eq!(questions[0].kind, QuestionKind::NoText);

    // Text that comes later decides it.
    let document = setup.document(&path).id;
    setup.index_mut(|index| index.set_document_text(document, "acme Ref A1").unwrap());
    assert_eq!(filed(&setup.watch(&source)), 1);
    assert!(setup.path("T1/Acme/A1.txt").exists());
}

#[test]
fn a_file_the_user_leaves_is_never_asked_about_or_filed_again() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let second = setup.rule("t2", "acme", "Both/{number}");
    let source = watched(&setup, &["t1", "t2"]);
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    setup.watch_after_reading(&source);
    let question = open_questions(&setup.access, &source).unwrap().remove(0);

    leave(&setup.access, question.document).unwrap();
    assert_eq!(setup.document(&path).status, DocumentStatus::InReview);
    setup.index(|index| index.delete_rule(second).unwrap());
    let report = setup.watch(&source);
    assert_eq!((filed(&report), asked(&report)), (0, 0));
    assert!(path.exists());

    assert_filed_when_asked(&setup, &source, &path);
}

#[test]
fn a_question_is_gone_with_its_file_or_its_place_in_the_source() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let path = setup.write("Q1/blank.txt", " ");
    assert_eq!(asked(&setup.watch_after_reading(&source)), 1);
    let filtered = SortSource {
        exclude: "blank*".to_owned(),
        ..source.clone()
    };
    assert_eq!(question_count(&setup.access, &filtered), Ok(0));
    assert_eq!(
        question_count(&setup.access, &setup.source("Q2", &["t1"])),
        Ok(0)
    );
    fs::remove_file(path).unwrap();
    assert_eq!(question_count(&setup.access, &source), Ok(0));
}

#[test]
fn a_run_taken_back_is_never_done_again_by_itself() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    let WatchedReport::Done { run, .. } = setup.watch_after_reading(&source) else {
        panic!("nothing was filed");
    };
    setup.access.undo(run.run.unwrap()).unwrap();
    let document = setup.document(&path).id;
    let reason = setup.index(|index| index.open_review_of(document).unwrap().unwrap().reason);
    assert_eq!(reason, ReviewReason::Undone);

    assert_eq!(filed(&setup.watch_after_reading(&source)), 0);
    assert!(path.exists());
    assert_eq!(question_count(&setup.access, &source), Ok(0));

    assert_filed_when_asked(&setup, &source, &path);
}

/// A plan the user asks for files it, whatever was set aside before.
fn assert_filed_when_asked(setup: &Setup, source: &SortSource, path: &Path) {
    let document = setup.document(path).id;
    assert_eq!(setup.run(source).unwrap().done(), 1);
    let to = setup.path("T1/Acme/A1.txt");
    assert!(to.exists() && !path.exists());
    let known = setup.document(&to);
    assert_eq!((known.id, known.status), (document, DocumentStatus::Filed));
    setup.index(|index| assert!(index.open_review_of(document).unwrap().is_none()));
}

#[test]
fn a_move_the_app_died_in_is_settled_before_the_next_run() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let source = watched(&setup, &["t1"]);
    let from = setup.write("Q1/a.txt", "acme Ref A1");
    let next = setup.write("Q1/b.txt", "acme Ref A2");
    setup.plan(&source);
    let document = setup.document(&from);
    let to = setup.path("T1/Acme/A1.txt");
    setup.index(|index| {
        let target = setup.path("T1");
        let places = RunPlaces {
            source: &source.path,
            target: &target,
            inside_target: false,
        };
        let batch =
            open_run_batch(index, BatchKind::Filing, "died", Transfer::Move, &places).unwrap();
        index.join_run(batch, batch).unwrap();
        index
            .add_journal_item(
                batch,
                &NewJournalItem {
                    from: from.clone(),
                    to: to.clone(),
                    hash: document.hash,
                    document_id: Some(document.id),
                    duplicate: false,
                    state: JournalState::Pending,
                    reason: None,
                },
            )
            .unwrap();
    });
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::rename(&from, &to).unwrap();

    assert_eq!(filed(&setup.watch(&source)), 1);
    let settled = setup.document(&to);
    assert_eq!(
        (settled.id, settled.status),
        (document.id, DocumentStatus::Filed)
    );
    assert!(!next.exists() && setup.path("T1/Acme/A2.txt").exists());
    assert!(setup.index(|index| index.pending_journal_items().unwrap().is_empty()));
}
