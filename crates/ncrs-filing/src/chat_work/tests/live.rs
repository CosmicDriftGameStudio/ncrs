use super::trial::*;
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
        no_folder_in_every_language: vec!["no folder".to_owned(), "kein Ordner".to_owned()],
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
    assert_eq!(state_of(&plan, "c.pdf"), filed("Alfa/2022", "c.pdf", false));
    assert_eq!(state_of(&plan, "d.pdf"), filed("First", "d.pdf", true));
    assert_eq!(state_of(&plan, "e.pdf"), filed("Neighbour", "e.pdf", true));
    assert_eq!(state_of(&plan, "f.pdf"), filed("Alfa", "f (3).pdf", true));
    assert_eq!(state_of(&plan, "g.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&plan, "h.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&plan, "i.pdf"), LiveState::Looking);
    assert_eq!(state_of(&plan, "j.pdf"), LiveState::NoFolder);
    assert_eq!(state_of(&plan, "k.pdf"), LiveState::Unreadable);
}

#[test]
fn a_place_from_the_layout_alone_is_unsure_one_from_a_rule_or_neighbours_is_not() {
    let source = reading(
        "/inbox",
        vec![
            readable("a.pdf", 1),
            readable("b.pdf", 2),
            readable("c.pdf", 3),
        ],
    );
    let placed = |name: &str, seed: u8, by: PlacedBy| TrialEntry {
        placed_by: Some(by),
        ..entry_for(
            name,
            seed,
            TrialOutcome::Filed {
                to: PathBuf::from(format!("Bank{seed}")).join(name),
            },
        )
    };
    let plan = classify(
        &plan_of(vec![
            placed("a.pdf", 1, PlacedBy::Rule),
            placed("b.pdf", 2, PlacedBy::Neighbours),
            placed("c.pdf", 3, PlacedBy::Layout),
        ]),
        &source,
    );

    assert_eq!(state_of(&plan, "a.pdf"), filed("Bank1", "a.pdf", false));
    assert_eq!(state_of(&plan, "b.pdf"), filed("Bank2", "b.pdf", false));
    assert_eq!(state_of(&plan, "c.pdf"), filed("Bank3", "c.pdf", true));
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
    index_known(setup, "archive/Alfa/2021/old.txt", "Alfa AG invoice");
    index_known(setup, "archive/Beta/old.txt", "Beta GmbH invoice");
}

/// A file of the target as the index holds it after the intake has seen it.
pub fn index_known(setup: &Setup, relative: &str, content: &str) {
    let path = setup.write(relative, content);
    let mut guard = setup.access.slot.lock().unwrap();
    guard
        .as_mut()
        .unwrap()
        .insert_document(&NewDocument {
            hash: hash_path(&path).unwrap(),
            path: path.clone(),
            source: DocumentSource::Archive,
            received_at: Utc::now(),
            stamp: Some(super::reading::stamp_of_file(&path)),
        })
        .unwrap();
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
fn a_live_plan_shows_what_needs_no_model_before_the_first_answer_and_ends_done() {
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
    assert!(first.files.iter().all(|file| file.hash.is_some()));
    let before_answer = tally(&first);
    assert_eq!(before_answer.waiting, 0);
    assert_eq!(before_answer.looking, 27);
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
fn a_file_already_in_the_target_shows_before_the_model_answers_the_others() {
    let setup = Setup::new();
    index_known(&setup, "archive/Beta/old.txt", "Beta GmbH invoice");
    setup.write("inbox/copy.txt", "Beta GmbH invoice");
    setup.write("inbox/new.txt", "Contract number 7");
    let run = Arc::new(LiveRun::default());
    let model = RecordingModel {
        run: Some(Arc::clone(&run)),
        ..RecordingModel::default()
    };

    run_live(&setup, &model, &run).unwrap();

    let first = model.seen.lock().unwrap()[0].clone().unwrap();
    assert!(matches!(
        state_of(&first, "copy.txt"),
        LiveState::AlreadyThere { .. }
    ));
    assert_eq!(state_of(&first, "new.txt"), LiveState::Looking);
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
    assert!(
        last.files
            .iter()
            .all(|file| file.state != LiveState::Looking),
        "a finished plan has no file that is still being read"
    );
    assert!(last
        .files
        .iter()
        .any(|file| file.state == LiveState::NeedsModel));
}

fn stored_document(seed: u8) -> NewDocument {
    NewDocument {
        hash: ContentHash([seed; 32]),
        path: PathBuf::from(format!("/elsewhere/{seed}.pdf")),
        source: DocumentSource::Inbox,
        received_at: Utc.with_ymd_and_hms(2026, 3, 14, 9, 30, 0).unwrap(),
        stamp: None,
    }
}

#[test]
fn a_none_fits_answer_of_an_older_version_loses_its_tag_in_every_language() {
    let setup = Setup::new();
    write_source(&setup);
    setup
        .access
        .with_index(|index| {
            index.insert_document(&stored_document(1)).unwrap();
            index.insert_document(&stored_document(2)).unwrap();
            let german_hash = ContentHash([1; 32]);
            let other_hash = ContentHash([2; 32]);
            index.add_document_tag(&german_hash, "kein Ordner").unwrap();
            index.add_document_tag(&other_hash, "no folder").unwrap();
            index
                .store_folder_picks(
                    &setup.dir.path().join("archive"),
                    &[(german_hash, None), (other_hash, Some("Alfa".to_owned()))],
                    1,
                )
                .unwrap();
            Ok(())
        })
        .unwrap();
    let run = Arc::new(LiveRun::default());
    let model = RecordingModel {
        unsupported: true,
        ..RecordingModel::default()
    };

    run_live(&setup, &model, &run).unwrap();

    assert_eq!(tag_count(&setup, "kein Ordner"), 0);
    assert_eq!(tag_count(&setup, "no folder"), 1);
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
        placed_by: None,
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
fn two_files_typed_to_the_same_name_in_one_folder_do_not_collide() {
    let overrides = BTreeMap::from([
        (
            PathBuf::from("a.pdf"),
            Override::Name("same.pdf".to_owned()),
        ),
        (
            PathBuf::from("c.pdf"),
            Override::Name("same.pdf".to_owned()),
        ),
        (
            PathBuf::from("d.pdf"),
            Override::Name("Same.PDF".to_owned()),
        ),
    ]);

    let changed = apply_overrides(&plan_to_override(), &overrides);

    assert_eq!(
        state_of(&changed, "a.pdf"),
        filed("Alfa", "same.pdf", false)
    );
    assert_eq!(
        state_of(&changed, "c.pdf"),
        filed("Alfa", "same (2).pdf", false)
    );
    assert_eq!(
        state_of(&changed, "d.pdf"),
        filed("Alfa", "Same (3).PDF", false)
    );
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

const DKB_SELL: &str = "Verkauf_-_WKN_HAFX4V_-_Wertpapierabrechnung.pdf";
const BALANCE_NOTICE: &str = "Saldenmitteilung_706014687835EUR_2023-12-31_SAPCML_1.pdf";

fn with_outcome(
    name: &str,
    fields: DocumentFields,
    outcome: TrialOutcome,
    placed_by: Option<PlacedBy>,
) -> TrialEntry {
    TrialEntry {
        fields,
        outcome,
        placed_by,
        ..staying(name, None, None)
    }
}

fn dkb_fields() -> DocumentFields {
    DocumentFields {
        sender: Some("10919 Berlin".to_owned()),
        kind: Some("Beleg".to_owned()),
        date: chrono::NaiveDate::from_ymd_opt(2019, 12, 13),
        ..DocumentFields::default()
    }
}

fn unsure_in(folder: &str, offered_name: &str) -> TrialOutcome {
    TrialOutcome::Unsure {
        choices: vec![PathBuf::from(folder)],
        file_names: vec![offered_name.to_owned()],
        kind_label: None,
    }
}

fn classify_named(entries: Vec<TrialEntry>, taken: &[&str]) -> LivePlan {
    let source = reading(
        "/inbox",
        entries
            .iter()
            .zip(1..)
            .map(|(entry, seed)| readable(&entry.name, seed))
            .collect(),
    );
    let mut plan = plan_of(entries);
    plan.taken = taken.iter().map(PathBuf::from).collect();
    classify(&plan, &source)
}

#[test]
fn a_file_without_a_name_pattern_keeps_its_name_and_a_pattern_name_stays() {
    let mut no_pattern = with_outcome(
        "c.pdf",
        dkb_fields(),
        TrialOutcome::Stays(StayReason::NoPattern),
        None,
    );
    no_pattern.neighbour = Some(PathBuf::from("Bank/dkb"));
    let live = classify_named(
        vec![
            with_outcome(
                DKB_SELL,
                dkb_fields(),
                TrialOutcome::Filed {
                    to: PathBuf::from("Bank/dkb").join(DKB_SELL),
                },
                Some(PlacedBy::ModelAndNeighbours),
            ),
            with_outcome("b.pdf", dkb_fields(), unsure_in("Bank/dkb", "b.pdf"), None),
            no_pattern,
            with_outcome(
                "d.pdf",
                dkb_fields(),
                unsure_in("Bank/dkb", "2019-12-13_Abrechnung.pdf"),
                None,
            ),
            with_outcome(
                "scan.pdf",
                dkb_fields(),
                unsure_in("Bank/dkb", "x.pdf"),
                None,
            ),
        ],
        &["Bank/dkb/scan.pdf"],
    );

    assert_eq!(
        state_of(&live, DKB_SELL),
        filed("Bank/dkb", DKB_SELL, false)
    );
    assert_eq!(state_of(&live, "b.pdf"), filed("Bank/dkb", "b.pdf", true));
    assert_eq!(state_of(&live, "c.pdf"), filed("Bank/dkb", "c.pdf", true));
    assert_eq!(state_of(&live, "d.pdf"), filed("Bank/dkb", "d.pdf", true));
    assert_eq!(
        state_of(&live, "scan.pdf"),
        filed("Bank/dkb", "scan (2).pdf", true)
    );
}

#[test]
fn a_dkb_sale_keeps_its_name_whether_it_is_sure_or_unsure() {
    let mut no_pattern = with_outcome(
        "Verkauf_2.pdf",
        dkb_fields(),
        TrialOutcome::Stays(StayReason::NoPattern),
        None,
    );
    no_pattern.neighbour = Some(PathBuf::from("Bank/dkb"));
    let live = classify_named(
        vec![
            with_outcome(
                DKB_SELL,
                dkb_fields(),
                TrialOutcome::Filed {
                    to: PathBuf::from("Bank/dkb").join(DKB_SELL),
                },
                Some(PlacedBy::Neighbours),
            ),
            no_pattern,
        ],
        &[],
    );

    assert_eq!(
        state_of(&live, DKB_SELL),
        filed("Bank/dkb", DKB_SELL, false)
    );
    assert_eq!(
        state_of(&live, "Verkauf_2.pdf"),
        filed("Bank/dkb", "Verkauf_2.pdf", true)
    );
}

#[test]
fn a_file_with_only_a_date_is_never_named_after_the_date() {
    let only_date = DocumentFields {
        date: chrono::NaiveDate::from_ymd_opt(2024, 4, 30),
        ..DocumentFields::default()
    };
    let mut no_pattern = with_outcome(
        "scan_1.pdf",
        only_date.clone(),
        TrialOutcome::Stays(StayReason::NoPattern),
        None,
    );
    no_pattern.neighbour = Some(PathBuf::from("Post"));
    let live = classify_named(
        vec![
            no_pattern,
            with_outcome(
                "scan_2.pdf",
                only_date,
                unsure_in("Post", "scan_2.pdf"),
                None,
            ),
        ],
        &[],
    );

    assert_eq!(
        state_of(&live, "scan_1.pdf"),
        filed("Post", "scan_1.pdf", true)
    );
    assert_eq!(
        state_of(&live, "scan_2.pdf"),
        filed("Post", "scan_2.pdf", true)
    );
}

#[test]
fn an_unsure_balance_notice_is_not_renamed_after_its_kind() {
    let fields = DocumentFields {
        sender: Some("Deutsche Bank AG".to_owned()),
        kind: Some("bank_statement".to_owned()),
        date: chrono::NaiveDate::from_ymd_opt(2024, 1, 25),
        ..DocumentFields::default()
    };
    let live = classify_named(
        vec![with_outcome(
            BALANCE_NOTICE,
            fields,
            unsure_in("Bank/Deutsche Bank", "2024-01-25_Kontoauszug.pdf"),
            None,
        )],
        &[],
    );

    assert_eq!(
        state_of(&live, BALANCE_NOTICE),
        filed("Bank/Deutsche Bank", BALANCE_NOTICE, true)
    );
}

#[derive(Debug)]
struct CountingText(Arc<Mutex<Vec<PathBuf>>>);

impl ncrs_intake::TextSource for CountingText {
    fn extract(&self, path: &Path) -> Result<ncrs_intake::Extracted, TextError> {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(path.to_path_buf());
        }
        PlainText.extract(path)
    }
}

#[test]
fn the_target_folder_is_not_read_for_text_while_the_plan_waits_on_the_source() {
    let reads = Arc::new(Mutex::new(Vec::new()));
    let setup = Setup::with_chain(TextChain::new().with_source(CountingText(Arc::clone(&reads))));
    setup.write("inbox/new.txt", "Contract");
    setup.write("archive/Alfa/never_indexed.txt", "Alfa AG invoice");
    index_known(&setup, "archive/Beta/old.txt", "Beta GmbH invoice");
    let run = Arc::new(LiveRun::default());
    let model = RecordingModel {
        run: Some(Arc::clone(&run)),
        ..RecordingModel::default()
    };

    run_live(&setup, &model, &run).unwrap();

    let archive = setup.dir.path().join("archive");
    assert!(reads
        .lock()
        .unwrap()
        .iter()
        .all(|path| !path.starts_with(&archive)));
    assert!(!reads.lock().unwrap().is_empty());
    let first = model.seen.lock().unwrap()[0].clone().unwrap();
    assert!(first
        .files
        .iter()
        .all(|file| file.state == LiveState::Looking));
    let counted = tally(&published(&run));
    assert_eq!(counted.waiting + counted.looking, 0);
}
