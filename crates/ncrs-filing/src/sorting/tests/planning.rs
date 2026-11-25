use ncrs_index::Field;
use ncrs_intake::TextError;

use super::*;

#[test]
fn every_row_is_in_one_pot() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "both", "Both/{number}");
    setup.write("Q1/placed.txt", "acme Ref A1");
    setup.write("Q1/second.txt", "acme again Ref A1");
    setup.write("Q1/several.txt", "acme both Ref A2");
    setup.write("Q1/missing.txt", "acme without a number");
    setup.write("Q1/none.txt", "somebody else");
    setup.write("Q1/empty.txt", " ");
    setup.write("Q1/there.txt", "acme Ref A3");
    setup.write("T1/Acme/A3.txt", "acme Ref A3");
    setup.write("Q1/taken.txt", "acme Ref A4");
    setup.write("T1/Acme/A4.txt", "something different");
    setup.write("Q1/tool.exe", "acme Ref A5");
    setup.write_at("Q1/fresh.txt", "acme Ref A6", 0);
    setup.write("Q1/.hidden.txt", "acme Ref A7");
    let source = setup.source("Q1", &["t1", "t2"]);

    let plan = setup.plan(&source);

    let counts = plan.counts();
    assert_eq!(
        counts,
        SortCounts {
            read: 8,
            by_rule: 1,
            already_there: 1,
            no_rule: 1,
            no_text: 1,
            several: 1,
            missing_value: 1,
            bad_name: 0,
            name_taken: 2,
            not_read: 2,
            aside: 0,
        },
        "{plan:?}"
    );
    assert_eq!(plan.rows.len(), counts.read + counts.not_read);
    assert!(matches!(
        state_of(&plan, "second.txt"),
        RowState::NameTaken {
            by: Taken::InPlan,
            ..
        }
    ));
    assert!(matches!(
        state_of(&plan, "taken.txt"),
        RowState::NameTaken {
            by: Taken::OnDisk,
            ..
        }
    ));
    assert_eq!(
        *state_of(&plan, "fresh.txt"),
        RowState::NotRead(ReadProblem::StillWriting)
    );
    assert_eq!(
        *state_of(&plan, "tool.exe"),
        RowState::NotRead(ReadProblem::Unsupported)
    );
    assert!(setup.path("Q1/placed.txt").exists());
}

#[test]
fn a_plan_that_may_not_read_leaves_unknown_files_unread_and_remembers_read_ones() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1"]);

    let unread = setup.plan_with(&source, ReadUnknown::No);
    assert_eq!(
        *state_of(&unread, "a.txt"),
        RowState::NotRead(ReadProblem::Unknown)
    );
    assert!(setup.index(|index| index.document_by_path(&path).unwrap().is_none()));

    setup.plan(&source);
    place_of(&setup.plan_with(&source, ReadUnknown::No), "a.txt");
}

#[test]
fn a_file_whose_reading_fails_is_not_one_without_text() {
    #[derive(Debug)]
    struct Broken;
    impl ncrs_intake::TextSource for Broken {
        fn extract(&self, path: &Path) -> Result<ncrs_intake::Extracted, TextError> {
            Err(TextError::LibraryMissing {
                path: path.to_path_buf(),
            })
        }
    }
    let mut setup = Setup::new();
    setup.access.text_chain = Arc::new(TextChain::new().with_source(Broken));
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1"]);

    for _ in 0..2 {
        assert_eq!(
            *state_of(&setup.plan(&source), "a.txt"),
            RowState::NotRead(ReadProblem::LibraryMissing)
        );
    }
    let document = setup.document(&path).id;
    assert!(!setup.index(|index| index.has_document_text_row(document).unwrap()));
}

#[test]
fn already_there_counts_for_each_target() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.rule("t2", "acme", "Copies/{number}");
    let copying = SortSource {
        transfer: LocationTransfer::Copy,
        ..setup.source("Q1", &["t1"])
    };
    setup.write("Q1/a.txt", "acme Ref A1");
    setup.run(&copying).unwrap();

    let again = setup.write("Q2/again.txt", "acme Ref A1");
    let to_first = setup.plan(&setup.source("Q2", &["t1"]));
    assert!(matches!(
        state_of(&to_first, "again.txt"),
        RowState::AlreadyThere { target, path }
            if *target == id("t1") && *path == setup.path("T1/Acme/A1.txt")
    ));
    let to_both = setup.plan(&setup.source("Q2", &["t1", "t2"]));
    assert_eq!(place_of(&to_both, "again.txt").target, id("t2"));
    assert!(again.exists());
}

#[test]
fn a_target_below_the_source_is_left_out_and_still_filed_into() {
    let mut setup = Setup::new();
    setup.add_target("inner", "Q1/Sorted", Some(false));
    setup.rule("inner", "acme", "Acme/{number}");
    setup.write("Q1/Sorted/old.txt", "acme Ref A9");
    setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["inner"]);

    let plan = setup.plan(&source);
    assert_eq!(plan.rows.len(), 1, "{plan:?}");

    setup.run(&source).unwrap();
    assert!(setup.path("Q1/Sorted/Acme/A1.txt").exists());
    assert!(setup.path("Q1/Sorted/old.txt").exists());

    let stray = RunRequest {
        entries: vec![RunEntry {
            path: setup.path("Q1/Sorted/old.txt"),
            hash: ncrs_intake::hash_path(&setup.path("Q1/Sorted/old.txt")).unwrap(),
            document: None,
            places: vec![(id("inner"), PathBuf::from("Acme/A9.txt"))],
        }],
        ..request_of(&source, &plan)
    };
    let report = run(&setup.access, &stray, &mut Counting).unwrap();
    assert_eq!((report.run, report.done()), (None, 0));
    assert!(setup.path("Q1/Sorted/old.txt").exists());
}

#[test]
fn a_target_as_source_renames_and_does_not_hit_itself() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.write("T1/Acme/A1.txt", "acme Ref A1");
    setup.write("T1/Acme/A3.txt", "acme Ref A2");
    // Wants the name of the file above, which makes room first.
    setup.write("T1/Acme/zzz.txt", "acme Ref A3");
    let source = setup.source("T1", &["t1"]);

    let plan = setup.plan(&source);
    assert!(matches!(
        state_of(&plan, "A1.txt"),
        RowState::AlreadyThere { .. }
    ));
    place_of(&plan, "A3.txt");
    place_of(&plan, "zzz.txt");

    let report = setup.run(&source).unwrap();
    assert_eq!(report.done(), 2, "{report:?}");
    assert_eq!(
        fs::read_to_string(setup.path("T1/Acme/A3.txt")).unwrap(),
        "acme Ref A3"
    );
    assert!(setup.path("T1/Acme/A2.txt").exists());
    assert!(!setup.path("T1/Acme/zzz.txt").exists());
    assert_eq!(setup.plan(&source).counts().already_there, 3);
}

#[test]
fn a_swap_of_two_names_in_a_target_has_no_first_step() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.write("T1/Acme/A1.txt", "acme Ref A2");
    setup.write("T1/Acme/A2.txt", "acme Ref A1");
    let source = setup.source("T1", &["t1"]);

    assert_eq!(setup.plan(&source).counts().name_taken, 2);
    assert_eq!(setup.run(&source).unwrap(), SortReport::default());
}

#[test]
fn the_filter_of_the_source_decides_which_files_are_rows() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    setup.write("Q1/a.txt", "acme Ref A1");
    setup.write("Q1/b.md", "acme Ref A2");
    setup.write("Q1/skip/c.txt", "acme Ref A3");
    let source = SortSource {
        include: "*.txt".to_owned(),
        exclude: "skip".to_owned(),
        ..setup.source("Q1", &["t1"])
    };

    let plan = setup.plan(&source);
    assert_eq!(plan.rows.len(), 1, "{plan:?}");
    place_of(&plan, "a.txt");
}

#[test]
fn a_value_from_a_model_makes_a_missing_value_a_place() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let path = setup.write("Q1/a.txt", "acme without a number");
    let source = setup.source("Q1", &["t1"]);
    assert!(matches!(
        state_of(&setup.plan(&source), "a.txt"),
        RowState::MissingValue {
            field: Field::Number,
            ..
        }
    ));

    let fields = DocumentFields {
        number: Some("M7".to_owned()),
        ..setup.document(&path).fields
    };
    setup.set_fields(
        &path,
        &fields,
        FieldOrigin::Ai {
            confidence_percent: 95,
        },
    );
    assert_eq!(
        place_of(&setup.plan(&source), "a.txt").relative,
        Path::new("Acme").join("M7.txt")
    );
}

#[test]
fn a_field_set_by_hand_wins_over_the_rule() {
    let setup = Setup::new();
    setup.rule("t1", "acme", "Acme/{number}");
    let path = setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1"]);
    setup.plan(&source);
    let fields = DocumentFields {
        number: Some("HAND".to_owned()),
        ..setup.document(&path).fields
    };
    setup.set_fields(&path, &fields, FieldOrigin::Manual);

    assert_eq!(
        place_of(&setup.plan(&source), "a.txt").relative,
        Path::new("Acme").join("HAND.txt")
    );
}

#[test]
fn a_confirmed_alias_counts_as_the_sender() {
    let setup = Setup::new();
    setup.rule_on("t1", ConditionKind::Sender, "Acme", "Acme/{number}");
    let path = setup.write("Q1/a.txt", "a letter Ref A1");
    let source = setup.source("Q1", &["t1"]);
    setup.plan(&source);
    let fields = DocumentFields {
        sender: Some("Roadrunner Traps".to_owned()),
        ..DocumentFields::default()
    };
    setup.set_fields(&path, &fields, FieldOrigin::Manual);
    assert_eq!(*state_of(&setup.plan(&source), "a.txt"), RowState::NoRule);

    setup.index(|index| {
        index
            .connection()
            .execute_batch(
                "INSERT OR IGNORE INTO sender (name, name_key) VALUES ('Acme', 'acme');
                 INSERT INTO sender_alias (sender_id, kind, value, state)
                 SELECT id, 'name', 'roadrunner traps', 'confirmed'
                 FROM sender WHERE name_key = 'acme';",
            )
            .unwrap();
    });
    place_of(&setup.plan(&source), "a.txt");
}

#[test]
fn a_rule_is_tried_without_being_saved() {
    let setup = Setup::new();
    let saved = setup.rule("t1", "acme", "Acme/{number}");
    setup.write("Q1/a.txt", "acme Ref A1");
    let source = setup.source("Q1", &["t1"]);
    let request = PlanRequest {
        source: source.clone(),
        files: PlanFiles::All,
        read: ReadUnknown::Yes,
    };
    let mut rules = setup.index(|index| index.rules_of(&id("t1")).unwrap());
    rules[0].spec.filing_pattern = "Tried/{number}".to_owned();

    let tried = plan_with_rules(
        &setup.access,
        &request,
        &id("t1"),
        rules,
        &ReadProgress::default(),
        None,
    )
    .unwrap();
    let place = place_of(&tried, "a.txt");
    assert_eq!(place.rule, saved);
    assert_eq!(place.relative, Path::new("Tried").join("A1.txt"));
    assert_eq!(
        place_of(&setup.plan(&source), "a.txt").relative,
        Path::new("Acme").join("A1.txt")
    );
}

#[test]
fn a_stopped_plan_says_so() {
    let setup = Setup::new();
    setup.write("Q1/a.txt", "acme Ref A1");
    let request = PlanRequest {
        source: setup.source("Q1", &["t1"]),
        files: PlanFiles::All,
        read: ReadUnknown::Yes,
    };
    let stop = std::sync::atomic::AtomicBool::new(true);
    let stopped = plan(
        &setup.access,
        &request,
        &ReadProgress::default(),
        Some(&stop),
    );
    assert_eq!(stopped, Err(PlanError::Stopped));
}
