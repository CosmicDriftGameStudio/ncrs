use super::trial::*;
use super::*;

pub(in crate::chat_work::tests) fn source_with_staying_files() -> FolderReading {
    let sender =
        |path: &str, seed: u8, name: &str| plan_file(path, seed, Some(name), date(2022, 7, 4));
    let mut no_kind = plan_file("neither.pdf", 120, None, date(2022, 7, 4));
    no_kind.fields.kind = None;
    reading(
        "/inbox",
        vec![
            sender("g1.pdf", 110, "Gamma AG"),
            sender("g2.pdf", 111, "Gamma AG"),
            sender("g3.pdf", 112, "gamma ag"),
            sender("d1.pdf", 113, "Delta AG"),
            sender("d2.pdf", 114, "Delta AG"),
            plan_file("k1.pdf", 115, None, date(2022, 7, 4)),
            plan_file("k2.pdf", 116, None, date(2022, 7, 4)),
            plan_file("k3.pdf", 117, None, date(2022, 7, 4)),
            plan_file("k4.pdf", 118, None, date(2022, 7, 4)),
            sender("solo.pdf", 119, "Solo KG"),
            no_kind,
        ],
    )
}

#[test]
fn staying_files_are_grouped_largest_first_sender_before_kind() {
    let plan = trial_of(
        &Setup::new(),
        source_with_staying_files(),
        AnsweredGroups::default(),
    );
    let groups = &plan.groups;
    let asked: Vec<(&GroupKey, usize)> = groups
        .asked
        .iter()
        .map(|group| (&group.key, group.count()))
        .collect();
    assert_eq!(
        asked,
        [
            (&GroupKey::Kind("invoice".to_owned()), 4),
            (&GroupKey::Sender("gamma ag".to_owned()), 3),
            (&GroupKey::Sender("delta ag".to_owned()), 2),
        ]
    );
    assert_eq!(groups.asked[0].label, "Invoice");
    assert_eq!(groups.asked[1].label, "Gamma AG");
    assert_eq!(groups.asked[1].examples.len(), 3);
    assert_eq!(groups.groups, 3);
    // One sender seen once and one file with neither sender nor kind.
    assert_eq!(groups.single_pieces, 2);
    assert_eq!(groups.rest.len(), 2);
    // No neighbours point anywhere, so the largest areas are offered.
    assert_eq!(groups.asked[1].areas, ["Alfa", "Firma", "Privat"]);
}

#[test]
fn a_kind_the_catalog_does_not_know_forms_no_group() {
    let entries = vec![
        staying("a1.pdf", None, Some("Abschäuss")),
        staying("a2.pdf", None, Some("abschäuss")),
        staying("i1.pdf", None, Some("Invoice")),
        staying("i2.pdf", None, Some("invoice")),
    ];
    let groups = grouped(&plan_of(entries), &AnsweredGroups::default());
    let asked: Vec<&GroupKey> = groups.asked.iter().map(|group| &group.key).collect();
    assert_eq!(asked, [&GroupKey::Kind("invoice".to_owned())]);
    assert_eq!(groups.single_pieces, 2);
}

#[test]
fn the_neighbour_folder_of_a_file_without_a_pattern_comes_first_in_the_areas() {
    let mut entries = vec![
        staying("p1.pdf", Some("Pi AG"), None),
        staying("p2.pdf", Some("Pi AG"), None),
    ];
    for entry in &mut entries {
        entry.outcome = TrialOutcome::Stays(StayReason::NoPattern);
        entry.neighbour = Some(PathBuf::from("Privat/Beta"));
    }
    let groups = grouped(&plan_of(entries), &AnsweredGroups::default());
    assert_eq!(groups.asked[0].areas, ["Privat", "Alfa", "Firma"]);
}

#[test]
fn at_most_six_groups_are_asked_and_answered_ones_are_not_asked_again() {
    let entries: Vec<TrialEntry> = (0..8)
        .flat_map(|group| {
            (0..2).map(move |piece| {
                staying(
                    &format!("{group}-{piece}.pdf"),
                    Some(&format!("Sender {group}")),
                    None,
                )
            })
        })
        .collect();
    let plan = plan_of(entries);
    assert_eq!(MAX_GROUP_QUESTIONS, 6);
    let groups = grouped(&plan, &AnsweredGroups::default());
    assert_eq!(
        (groups.groups, groups.asked.len(), groups.rest.len()),
        (8, 6, 4)
    );
    assert_eq!(groups.single_pieces, 0);

    let answered = AnsweredGroups {
        questions: 2,
        left: vec![GroupKey::Sender("sender 0".to_owned())],
    };
    let later = grouped(&plan, &answered);
    assert_eq!(later.asked.len(), 4);
    assert!(later
        .asked
        .iter()
        .all(|group| group.key != GroupKey::Sender("sender 0".to_owned())));
    // The left group is not asked, and its files are not "rest" either.
    assert_eq!(later.rest.len(), 6);
}

#[test]
fn a_taken_name_gets_the_next_free_number_that_the_plan_does_not_use() {
    let plan = trial(&Setup::new());
    let taken = &plan.groups.name_taken;
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].name, "a2.pdf");
    assert_eq!(
        taken[0].suggested,
        PathBuf::from("Alfa/2022/2022-07-04 Alfa Rechnung (2).pdf")
    );
    assert!(!plan.taken.contains(&taken[0].suggested));

    let mut used = std::collections::BTreeSet::from([
        PathBuf::from("Alfa/x.pdf"),
        PathBuf::from("Alfa/x (2).pdf"),
        PathBuf::from("Alfa/X (3).pdf"),
    ]);
    assert_eq!(
        numbered_name(Path::new("Alfa/x.pdf"), &mut used),
        PathBuf::from("Alfa/x (4).pdf")
    );
    assert_eq!(
        numbered_name(Path::new("Alfa/x.pdf"), &mut used),
        PathBuf::from("Alfa/x (5).pdf")
    );
}

#[test]
fn the_trial_applies_a_stored_rule_before_the_neighbours() {
    let setup = Setup::new();
    let before = trial(&setup);
    assert_eq!(
        outcome(&before, "nobody.pdf"),
        &TrialOutcome::Stays(StayReason::NoNeighbours)
    );
    save_group_rule(
        &setup,
        GroupKey::Sender("unknown kg".to_owned()),
        "Unknown KG",
        "Zeta",
    );
    let after = trial(&setup);
    assert_eq!(
        outcome(&after, "nobody.pdf"),
        &TrialOutcome::Filed {
            to: PathBuf::from("Zeta/2022-07-04_Unknown-KG.pdf")
        }
    );
    assert_eq!(after.filed, before.filed + 1);
    assert!(after
        .taken
        .contains(Path::new("Zeta/2022-07-04_Unknown-KG.pdf")));
    // The other files are decided as before.
    assert_eq!(outcome(&after, "a1.pdf"), outcome(&before, "a1.pdf"));
}

#[test]
fn a_kind_condition_matches_only_its_kind() {
    let setup = Setup::new();
    let mut contract = plan_file("contract.pdf", 131, None, date(2022, 7, 4));
    contract.fields.kind = Some("contract".to_owned());
    let invoice = plan_file("invoice.pdf", 130, None, date(2022, 7, 4));
    let source = || reading("/inbox", vec![invoice.clone(), contract.clone()]);
    save_group_rule(
        &setup,
        GroupKey::Kind("invoice".to_owned()),
        "Invoice",
        "Kay",
    );

    let plan = trial_of(&setup, source(), AnsweredGroups::default());
    assert_eq!(
        outcome(&plan, "invoice.pdf"),
        &TrialOutcome::Filed {
            to: PathBuf::from("Kay/2022-07-04.pdf")
        }
    );
    assert_eq!(
        outcome(&plan, "contract.pdf"),
        &TrialOutcome::Stays(StayReason::NoNeighbours)
    );
}

#[test]
fn a_rule_the_form_would_refuse_is_refused_and_nothing_is_stored() {
    let setup = Setup::new();
    let stored = |kept: &Setup| {
        let guard = kept.access.slot.lock().unwrap();
        guard.as_ref().unwrap().rules().unwrap().len()
    };
    let result = setup.access.run(ChatWork::SaveGroupRule {
        target: Arc::new(target_reading()),
        key: GroupKey::Sender(String::new()),
        label: "  ".to_owned(),
        area: "Zeta".to_owned(),
    });
    let ChatResult::RuleSaved(saved) = result else {
        panic!("{result:?}");
    };
    assert_eq!(
        saved,
        Err(RuleSaveFailure::Refused(RuleProblem::Condition(
            ConditionProblem::Empty
        )))
    );
    assert_eq!(stored(&setup), 0);
}

/// Alfa documents filed by year in two areas, three each, so a new one is
/// unsure which area it belongs to.
pub(in crate::chat_work::tests) fn alfa_in_two_areas() -> FolderReading {
    let mut files = Vec::new();
    for (area, base) in [("Haus", 10), ("Firma", 20)] {
        for (offset, year) in [2019, 2020, 2021].into_iter().enumerate() {
            files.push(plan_file(
                &format!("{area}/Alfa/{year}/{year}-03-10 Alfa Rechnung.pdf"),
                base + u8::try_from(offset).unwrap(),
                Some("Alfa AG"),
                date(year, 3, 10),
            ));
        }
    }
    reading("/archive", files)
}

pub(in crate::chat_work::tests) fn unsure_in_two_areas(
    file: PlanFile,
) -> (Vec<PathBuf>, Vec<String>) {
    let setup = Setup::new();
    let plan = match setup.access.run(ChatWork::Trial {
        source: Arc::new(reading("/inbox", vec![file])),
        target: Arc::new(alfa_in_two_areas()),
        answered: AnsweredGroups::default(),
        layout: None,
        scope: crate::chat_work::ModelScope::FirstPage,
    }) {
        ChatResult::Trial(plan) => *plan.unwrap(),
        other => panic!("{other:?}"),
    };
    match &plan.entries[0].outcome {
        TrialOutcome::Unsure {
            choices,
            file_names,
            ..
        } => (choices.clone(), file_names.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unsure_file_is_offered_the_folders_with_the_year_of_its_own_date() {
    let (choices, file_names) =
        unsure_in_two_areas(plan_file("new.pdf", 100, Some("Alfa AG"), date(2022, 7, 4)));

    assert_eq!(
        choices,
        [
            PathBuf::from("Firma/Alfa/2022"),
            PathBuf::from("Haus/Alfa/2022")
        ]
    );
    assert_eq!(file_names.len(), 2);
    assert!(
        file_names.iter().all(|name| name.contains("2022-07-04")),
        "{file_names:?}"
    );
}

#[test]
fn an_unsure_file_without_a_date_is_offered_folders_without_a_year_level() {
    let mut file = plan_file("new.pdf", 100, Some("Alfa AG"), date(2022, 7, 4));
    file.fields.date = None;

    let (choices, file_names) = unsure_in_two_areas(file);

    assert_eq!(
        choices,
        [PathBuf::from("Firma/Alfa"), PathBuf::from("Haus/Alfa")]
    );
    assert_eq!(file_names, ["new.pdf", "new.pdf"]);
}

#[test]
fn an_unsure_file_names_the_kind_only_when_the_catalog_knows_it() {
    let setup = Setup::new();
    let label = |kind: &str| {
        let mut file = plan_file("new.pdf", 100, Some("Alfa AG"), date(2022, 7, 4));
        file.fields.kind = Some(kind.to_owned());
        let plan = match setup.access.run(ChatWork::Trial {
            source: Arc::new(reading("/inbox", vec![file])),
            target: Arc::new(alfa_in_two_areas()),
            answered: AnsweredGroups::default(),
            layout: None,
            scope: crate::chat_work::ModelScope::FirstPage,
        }) {
            ChatResult::Trial(plan) => *plan.unwrap(),
            other => panic!("{other:?}"),
        };
        match &plan.entries[0].outcome {
            TrialOutcome::Unsure { kind_label, .. } => kind_label.clone(),
            other => panic!("{other:?}"),
        }
    };

    assert_eq!(label("invoice").as_deref(), Some("Invoice"));
    assert_eq!(
        label("Steuerbescheid-Entwurf-xyz").as_deref(),
        Some("Alfa AG")
    );
}

#[test]
fn a_mail_text_beside_its_eml_is_not_a_document_of_its_own() {
    let setup = Setup::new();
    setup.write("in/mail/email.eml", "From: a@b.c");
    setup.write("in/mail/Body.TXT", "the mail text");
    setup.write("in/mail/a.pdf", "attachment");
    setup.write("in/loose/body.txt", "a text without a mail");

    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    let paths: Vec<&Path> = read
        .files
        .iter()
        .map(|file| file.relative_path.as_path())
        .collect();

    assert_eq!(
        paths,
        [
            Path::new("loose/body.txt"),
            Path::new("mail/a.pdf"),
            Path::new("mail/email.eml")
        ]
    );
}

#[test]
fn the_ui_gets_no_answer_instead_of_waiting_while_a_run_holds_the_index() {
    let setup = Setup::new();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let slot = Arc::clone(&setup.access.slot);
    let holder = std::thread::spawn(move || {
        let _guard = slot.lock().unwrap();
        locked_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    locked_rx.recv().unwrap();

    let kinds = setup.access.try_kinds();
    let rules = setup.access.try_rules();

    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(kinds.err(), Some(super::INDEX_BUSY.to_owned()));
    assert_eq!(rules.err(), Some(super::INDEX_BUSY.to_owned()));
    assert_eq!(setup.access.try_kinds().err(), None);
    assert_eq!(setup.access.try_rules().unwrap().len(), 0);
}

#[test]
fn the_ui_gets_the_last_known_kinds_while_a_run_holds_the_index() {
    let setup = Setup::new();
    let known = setup.access.try_kinds().unwrap();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let slot = Arc::clone(&setup.access.slot);
    let holder = std::thread::spawn(move || {
        let _guard = slot.lock().unwrap();
        locked_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    locked_rx.recv().unwrap();

    let busy = setup.access.try_kinds();

    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(busy, Ok(known));
}
