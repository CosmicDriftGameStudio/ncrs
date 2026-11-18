use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use chrono::{TimeZone as _, Utc};
use ncrs_ai::FolderChoiceLane;
use ncrs_index::{DocumentSource, NewDocument};

use super::picks::RecordingModel;
use super::*;
use crate::chat_work::{
    apply_overrides, classify, mark_files, plan_live, tally, LiveFile, LivePlan, LiveRun,
    LiveState, LiveTags, Override, PlanFailure,
};

fn tags() -> LiveTags {
    LiveTags {
        unreadable: "unreadable".to_owned(),
        no_folder: "no folder".to_owned(),
    }
}

fn readable(path: &str, seed: u8) -> PlanFile {
    PlanFile {
        text: Some("some text".to_owned()),
        ..plan_file(path, seed, Some("Alfa AG"), date(2022, 7, 4))
    }
}

fn entry_for(name: &str, seed: u8, outcome: TrialOutcome) -> TrialEntry {
    TrialEntry {
        hash: ContentHash([seed; 32]),
        outcome,
        ..staying(name, Some("Alfa AG"), None)
    }
}

fn state_of(plan: &LivePlan, name: &str) -> LiveState {
    plan.files
        .iter()
        .find(|file| file.from == Path::new(name))
        .unwrap()
        .state
        .clone()
}

fn filed(folder: &str, name: &str, unsure: bool) -> LiveState {
    LiveState::Filed {
        folder: PathBuf::from(folder),
        name: name.to_owned(),
        unsure,
    }
}

fn classified() -> LivePlan {
    let unreadable = PlanFile {
        text: None,
        ..plan_file("b.pdf", 2, Some("Alfa AG"), date(2022, 7, 4))
    };
    let unreadable_there = PlanFile {
        text: None,
        ..plan_file("b2.pdf", 12, Some("Alfa AG"), date(2022, 7, 4))
    };
    let source = reading(
        "/inbox",
        vec![
            readable("a.pdf", 1),
            unreadable,
            unreadable_there,
            readable("c.pdf", 3),
            readable("d.pdf", 4),
            readable("e.pdf", 5),
            readable("f.pdf", 6),
            readable("g.pdf", 7),
            readable("h.pdf", 8),
            readable("i.pdf", 9),
            readable("j.pdf", 10),
            PlanFile {
                text: Some(" \n".to_owned()),
                ..readable("k.pdf", 11)
            },
        ],
    );
    let mut no_pattern = entry_for("e.pdf", 5, TrialOutcome::Stays(StayReason::NoPattern));
    no_pattern.neighbour = Some(PathBuf::from("Neighbour"));
    let mut name_taken = entry_for("f.pdf", 6, TrialOutcome::Stays(StayReason::NameTaken));
    name_taken.wanted = Some(PathBuf::from("Alfa/f.pdf"));
    let entries = vec![
        entry_for(
            "a.pdf",
            1,
            TrialOutcome::AlreadyThere {
                at: PathBuf::from("Alfa/a.pdf"),
            },
        ),
        entry_for(
            "b.pdf",
            2,
            TrialOutcome::Filed {
                to: PathBuf::from("Alfa/b.pdf"),
            },
        ),
        entry_for(
            "b2.pdf",
            12,
            TrialOutcome::AlreadyThere {
                at: PathBuf::from("Alfa/b2.pdf"),
            },
        ),
        entry_for(
            "c.pdf",
            3,
            TrialOutcome::Filed {
                to: PathBuf::from("Alfa/2022/c.pdf"),
            },
        ),
        entry_for(
            "d.pdf",
            4,
            TrialOutcome::Unsure {
                choices: vec![PathBuf::from("First"), PathBuf::from("Second")],
                file_names: vec!["d first.pdf".to_owned(), "d second.pdf".to_owned()],
                kind_label: None,
            },
        ),
        no_pattern,
        name_taken,
        entry_for("g.pdf", 7, TrialOutcome::Stays(StayReason::NoNeighbours)),
        entry_for(
            "i.pdf",
            9,
            TrialOutcome::Stays(StayReason::MissingField(ncrs_index::Field::Date)),
        ),
        entry_for("j.pdf", 10, TrialOutcome::Stays(StayReason::NoPattern)),
    ];
    let mut plan = plan_of(entries);
    plan.taken = BTreeSet::from([PathBuf::from("Alfa/f.pdf"), PathBuf::from("Alfa/f (2).pdf")]);
    plan.model_left = BTreeSet::from([PathBuf::from("/inbox/i.pdf")]);
    classify(&plan, &source)
}

#[test]
fn classify_maps_every_row_of_the_table() {
    let plan = classified();

    assert_eq!(plan.files.len(), 12);
    assert_eq!(
        state_of(&plan, "a.pdf"),
        LiveState::AlreadyThere {
            at: PathBuf::from("Alfa/a.pdf")
        }
    );
    assert_eq!(state_of(&plan, "b.pdf"), LiveState::Unreadable);
    assert_eq!(
        state_of(&plan, "b2.pdf"),
        LiveState::AlreadyThere {
            at: PathBuf::from("Alfa/b2.pdf")
        }
    );
    assert_eq!(
        state_of(&plan, "c.pdf"),
        filed("Alfa/2022", "Alfa AG.pdf", false)
    );
    assert_eq!(
        state_of(&plan, "d.pdf"),
        filed("First", "d first.pdf", true)
    );
    assert_eq!(
        state_of(&plan, "e.pdf"),
        filed("Neighbour", "Alfa AG.pdf", true)
    );
    assert_eq!(state_of(&plan, "f.pdf"), filed("Alfa", "f (3).pdf", true));
    assert_eq!(state_of(&plan, "g.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&plan, "h.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&plan, "i.pdf"), LiveState::Looking);
    assert_eq!(state_of(&plan, "j.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&plan, "k.pdf"), LiveState::Unreadable);
}

#[test]
fn the_tally_buckets_add_up_to_the_files() {
    let counted = tally(&classified());

    assert_eq!(counted.filed, 4);
    assert_eq!(counted.folders, 4);
    assert_eq!(counted.already_there, 2);
    assert_eq!(counted.unreadable, 2);
    assert_eq!(counted.looking, 1);
    assert_eq!(counted.no_folder, 3);
    assert_eq!(
        counted.waiting
            + counted.looking
            + counted.filed
            + counted.already_there
            + counted.no_folder
            + counted.unreadable,
        12
    );
}

/// 25 readable files (the empty one and desktop.ini join them at the model), a `desktop.ini` and a hidden one.
fn write_source(setup: &Setup) {
    for number in 0..25 {
        setup.write(
            &format!("inbox/doc{number:02}.txt"),
            &format!("Contract number {number}"),
        );
    }
    setup.write("inbox/desktop.ini", "[.ShellClassInfo]");
    setup.write("inbox/blank.dat", "");
    setup.write("inbox/.hidden.txt", "never read");
    setup.write("archive/Alfa/2021/old.txt", "Alfa AG invoice");
    setup.write("archive/Beta/old.txt", "Beta GmbH invoice");
}

fn run_live(setup: &Setup, model: &RecordingModel, run: &LiveRun) -> Result<(), PlanFailure> {
    plan_live(
        &setup.access,
        &setup.dir.path().join("inbox"),
        &setup.dir.path().join("archive"),
        model,
        &tags(),
        run,
    )
}

fn published(run: &LiveRun) -> LivePlan {
    run.plan.lock().unwrap().clone().unwrap()
}

#[test]
fn a_live_plan_starts_all_waiting_and_ends_done_with_every_bucket_counted() {
    let setup = Setup::new();
    write_source(&setup);
    let run = Arc::new(LiveRun::default());
    let model = RecordingModel {
        run: Some(Arc::clone(&run)),
        ..RecordingModel::default()
    };

    run_live(&setup, &model, &run).unwrap();

    let first = model.seen.lock().unwrap()[0].clone().unwrap();
    assert_eq!(first.files.len(), 27);
    assert!(!first.done);
    assert!(first
        .files
        .iter()
        .all(|file| file.state == LiveState::Waiting && file.hash.is_none()));
    let calls: Vec<usize> = model.calls().iter().map(|(count, _)| *count).collect();
    assert_eq!(calls, vec![10, 10, 7]);
    assert!(model
        .calls()
        .iter()
        .all(|(_, lane)| *lane == FolderChoiceLane::Waiting));
    let last = published(&run);
    assert!(last.done);
    assert_eq!(last.files.len(), 27);
    let counted = tally(&last);
    assert_eq!(counted.waiting + counted.looking, 0);
    assert_eq!(
        counted.filed + counted.already_there + counted.no_folder + counted.unreadable,
        last.files.len()
    );
    assert_eq!(state_of(&last, "blank.dat"), LiveState::Unreadable);
}

#[test]
fn a_model_that_cannot_choose_folders_leaves_the_files_untagged_and_stores_nothing() {
    let setup = Setup::new();
    write_source(&setup);
    let run = Arc::new(LiveRun::default());
    let model = RecordingModel {
        unsupported: true,
        ..RecordingModel::default()
    };

    let result = run_live(&setup, &model, &run);

    assert_eq!(result, Ok(()));
    assert_eq!(tag_count(&setup, "no folder"), 0);
    let stored: i64 = setup
        .access
        .with_index(|index| {
            Ok(index
                .connection()
                .query_row("SELECT count(*) FROM folder_pick", (), |row| row.get(0))
                .unwrap())
        })
        .unwrap();
    assert_eq!(stored, 0);
    let last = published(&run);
    assert!(last.done);
    assert!(last.folder_choice_unsupported);
    assert_eq!(tally(&last).no_folder, 0);
}

#[test]
fn a_stop_ends_the_plan_after_the_round_that_is_running() {
    let setup = Setup::new();
    write_source(&setup);
    let run = Arc::new(LiveRun::default());
    let model = RecordingModel {
        run: Some(Arc::clone(&run)),
        stops_the_run: true,
        ..RecordingModel::default()
    };

    run_live(&setup, &model, &run).unwrap();

    assert_eq!(model.calls().len(), 1);
    assert!(run.stop.load(Ordering::Relaxed));
    let plan = published(&run);
    assert!(!plan.done);
    assert_eq!(tally(&plan).looking, 17);
}

fn tag_count(setup: &Setup, tag: &str) -> i64 {
    setup
        .access
        .with_index(|index| {
            Ok(index
                .connection()
                .query_row(
                    &format!(
                        "SELECT count(*) FROM document_tag dt JOIN tag t ON t.id = dt.tag_id
                         WHERE t.name = '{tag}' AND dt.automatic = 0"
                    ),
                    (),
                    |row| row.get(0),
                )
                .unwrap())
        })
        .unwrap()
}

#[test]
fn unreadable_and_folderless_files_get_their_tag_when_the_plan_is_done() {
    let setup = Setup::new();
    setup.write("inbox/blank.dat", "");
    setup.write("inbox/note.txt", "Something nobody knows");
    setup.write("archive/Alfa/old.txt", "Alfa AG invoice");
    for name in ["blank.dat", "note.txt"] {
        let path = setup.dir.path().join("inbox").join(name);
        let text = (name == "note.txt").then_some("Something nobody knows");
        setup
            .access
            .with_index_mut(|index| {
                let id = index
                    .insert_document(&NewDocument {
                        hash: hash_path(&path).unwrap(),
                        path,
                        source: DocumentSource::Inbox,
                        received_at: Utc.with_ymd_and_hms(2026, 3, 14, 9, 30, 0).unwrap(),
                        stamp: None,
                    })
                    .map_err(|err| err.to_string())?;
                if let Some(body) = text {
                    index
                        .set_document_text(id, body)
                        .map_err(|err| err.to_string())?;
                }
                Ok(())
            })
            .unwrap();
    }
    let run = LiveRun::default();
    let model = RecordingModel {
        declines: true,
        ..RecordingModel::default()
    };

    run_live(&setup, &model, &run).unwrap();

    let plan = published(&run);
    assert_eq!(state_of(&plan, "blank.dat"), LiveState::Unreadable);
    assert_eq!(state_of(&plan, "note.txt"), LiveState::NoFolder);
    assert_eq!(tag_count(&setup, "unreadable"), 1);
    assert_eq!(tag_count(&setup, "no folder"), 1);

    let overrides = BTreeMap::from([(
        PathBuf::from("note.txt"),
        Override::Folder(PathBuf::from("Alfa")),
    )]);
    mark_files(&setup.access, &apply_overrides(&plan, &overrides), &tags()).unwrap();
    assert_eq!(tag_count(&setup, "unreadable"), 1);
    assert_eq!(tag_count(&setup, "no folder"), 0);
}

fn plan_to_override() -> LivePlan {
    let file = |from: &str, seed: u8, state: LiveState| LiveFile {
        from: PathBuf::from(from),
        hash: Some(ContentHash([seed; 32])),
        state,
        by_user: false,
    };
    LivePlan {
        source_root: PathBuf::from("/inbox"),
        target_root: PathBuf::from("/archive"),
        files: vec![
            file("a.pdf", 1, filed("Alfa", "a renamed.pdf", true)),
            file("b.pdf", 2, LiveState::NoFolder),
            file("c.pdf", 3, filed("Alfa", "c.pdf", false)),
            file("d.pdf", 4, filed("Alfa", "d.pdf", false)),
            file("e.pdf", 5, LiveState::Unreadable),
        ],
        done: true,
        folder_choice_unsupported: false,
    }
}

#[test]
fn overrides_move_rename_or_keep_a_file_and_mark_it_as_the_users() {
    let overrides = BTreeMap::from([
        (
            PathBuf::from("a.pdf"),
            Override::Folder(PathBuf::from("Beta")),
        ),
        (
            PathBuf::from("b.pdf"),
            Override::Folder(PathBuf::from("Beta")),
        ),
        (PathBuf::from("c.pdf"), Override::Name("new.pdf".to_owned())),
        (PathBuf::from("d.pdf"), Override::Stay),
        (
            PathBuf::from("e.pdf"),
            Override::Name("ignored.pdf".to_owned()),
        ),
    ]);

    let changed = apply_overrides(&plan_to_override(), &overrides);

    assert_eq!(
        state_of(&changed, "a.pdf"),
        filed("Beta", "a renamed.pdf", false)
    );
    assert_eq!(state_of(&changed, "b.pdf"), filed("Beta", "b.pdf", false));
    assert_eq!(state_of(&changed, "c.pdf"), filed("Alfa", "new.pdf", false));
    assert_eq!(state_of(&changed, "d.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&changed, "e.pdf"), LiveState::Unreadable);
    let by_user: Vec<bool> = changed.files.iter().map(|file| file.by_user).collect();
    assert_eq!(by_user, vec![true, true, true, true, false]);
}

#[test]
fn the_run_holds_only_the_filed_files_with_their_overridden_names_and_folders() {
    let overrides = BTreeMap::from([
        (
            PathBuf::from("a.pdf"),
            Override::Folder(PathBuf::from("Beta")),
        ),
        (PathBuf::from("c.pdf"), Override::Name("new.pdf".to_owned())),
        (PathBuf::from("d.pdf"), Override::Stay),
    ]);
    let changed = apply_overrides(&plan_to_override(), &overrides);

    let run = crate::chat_work::run_plan(&changed, Transfer::Copy, "Tidy up".to_owned());

    let items: Vec<(PathBuf, PathBuf)> = run
        .items
        .iter()
        .map(|item| (item.from.clone(), item.to.clone()))
        .collect();
    assert_eq!(
        items,
        vec![
            (
                PathBuf::from("/inbox/a.pdf"),
                PathBuf::from("Beta/a renamed.pdf")
            ),
            (PathBuf::from("/inbox/c.pdf"), PathBuf::from("Alfa/new.pdf")),
        ]
    );
    assert_eq!(run.transfer, Transfer::Copy);
    assert_eq!(run.source_root, PathBuf::from("/inbox"));
    assert_eq!(run.target_root, PathBuf::from("/archive"));
    assert!(run.inside.is_empty());
}

fn scan_named(
    name: &str,
    sender: Option<&str>,
    kind: Option<&str>,
    day: Option<u32>,
) -> TrialEntry {
    TrialEntry {
        fields: DocumentFields {
            sender: sender.map(str::to_owned),
            kind: kind.map(str::to_owned),
            date: day.and_then(|of_april| chrono::NaiveDate::from_ymd_opt(2020, 4, of_april)),
            ..DocumentFields::default()
        },
        outcome: TrialOutcome::Filed {
            to: PathBuf::from("Bank/2020").join(name),
        },
        ..staying(name, sender, kind)
    }
}

#[test]
fn a_file_that_would_keep_its_scan_name_gets_one_from_its_fields() {
    let plan = plan_of(vec![
        scan_named("2020092_001.pdf", Some("Alfa"), Some("Rechnung"), Some(4)),
        scan_named("2020092_002.pdf", Some("Alfa"), Some("Rechnung"), Some(4)),
        scan_named("2020092_003.pdf", Some("Alfa"), None, None),
        scan_named("2020092_004.pdf", None, Some("Rechnung"), None),
        TrialEntry {
            outcome: TrialOutcome::Unsure {
                choices: vec![PathBuf::from("Bank/2020")],
                file_names: vec!["2020092_005.pdf".to_owned()],
                kind_label: None,
            },
            ..scan_named("2020092_005.pdf", Some("Alfa"), Some("Rechnung"), Some(4))
        },
    ]);
    let source = reading(
        "/inbox",
        [
            "2020092_001.pdf",
            "2020092_002.pdf",
            "2020092_003.pdf",
            "2020092_004.pdf",
            "2020092_005.pdf",
        ]
        .iter()
        .zip(1..)
        .map(|(name, seed)| readable(name, seed))
        .collect(),
    );
    let live = classify(&plan, &source);

    assert_eq!(
        state_of(&live, "2020092_001.pdf"),
        filed("Bank/2020", "2020-04-04 Alfa Rechnung.pdf", false)
    );
    assert_eq!(
        state_of(&live, "2020092_002.pdf"),
        filed("Bank/2020", "2020-04-04 Alfa Rechnung (2).pdf", false)
    );
    assert_eq!(
        state_of(&live, "2020092_003.pdf"),
        filed("Bank/2020", "Alfa.pdf", false)
    );
    assert_eq!(
        state_of(&live, "2020092_004.pdf"),
        filed("Bank/2020", "2020092_004.pdf", false)
    );
    assert_eq!(
        state_of(&live, "2020092_005.pdf"),
        filed("Bank/2020", "2020-04-04 Alfa Rechnung (3).pdf", true)
    );
}
