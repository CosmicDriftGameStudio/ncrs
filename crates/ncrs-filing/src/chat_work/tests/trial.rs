use super::*;

pub(super) fn target_reading() -> FolderReading {
    let mut files = Vec::new();
    for (seed, (year, month)) in [(2020, 1), (2020, 5), (2020, 9), (2021, 2), (2021, 6)]
        .into_iter()
        .enumerate()
    {
        let number = u8::try_from(seed).unwrap() + 1;
        files.push(plan_file(
            &format!("Alfa/{year}/{year}-{month:02}-10 Alfa Rechnung.pdf"),
            number,
            Some("Alfa AG"),
            date(year, month, 10),
        ));
    }
    for (seed, folder) in [
        (20, "Privat/Beta"),
        (21, "Privat/Beta"),
        (22, "Firma/Beta"),
        (23, "Firma/Beta"),
    ] {
        files.push(plan_file(
            &format!("{folder}/{seed}.pdf"),
            seed,
            Some("Beta GmbH"),
            date(2022, 3, 3),
        ));
    }
    reading("/archive", files)
}

pub(super) fn source_reading() -> FolderReading {
    let alfa = |path: &str, seed: u8| plan_file(path, seed, Some("Alfa AG"), date(2022, 7, 4));
    reading(
        "/inbox",
        vec![
            alfa("a1.pdf", 100),
            alfa("a2.pdf", 101),
            plan_file("same.pdf", 3, Some("Alfa AG"), date(2020, 9, 10)),
            plan_file("nobody.pdf", 102, Some("Unknown KG"), date(2022, 7, 4)),
            plan_file("beta.pdf", 103, Some("Beta GmbH"), date(2022, 7, 4)),
        ],
    )
}

pub(super) fn trial(setup: &Setup) -> TrialPlan {
    trial_of(setup, source_reading(), AnsweredGroups::default())
}

pub(super) fn trial_of(
    setup: &Setup,
    source: FolderReading,
    answered: AnsweredGroups,
) -> TrialPlan {
    match setup.access.run(ChatWork::Trial {
        source: Arc::new(source),
        target: Arc::new(target_reading()),
        answered,
        layout: None,
        scope: crate::chat_work::ModelScope::FirstPage,
    }) {
        ChatResult::Trial(plan) => *plan.unwrap(),
        other => panic!("{other:?}"),
    }
}

pub(super) fn save_group_rule(setup: &Setup, key: GroupKey, label: &str, area: &str) {
    match setup.access.run(ChatWork::SaveGroupRule {
        target: Arc::new(target_reading()),
        key,
        label: label.to_owned(),
        area: area.to_owned(),
    }) {
        ChatResult::RuleSaved(saved) => assert_eq!(saved.unwrap(), label),
        other => panic!("{other:?}"),
    }
}

pub(super) fn staying(name: &str, sender: Option<&str>, kind: Option<&str>) -> TrialEntry {
    TrialEntry {
        from: PathBuf::from("/inbox").join(name),
        name: name.to_owned(),
        hash: ContentHash([7; 32]),
        kind_word: kind.map(str::to_owned),
        fields: DocumentFields {
            sender: sender.map(str::to_owned),
            kind: kind.map(str::to_owned),
            ..DocumentFields::default()
        },
        outcome: TrialOutcome::Stays(StayReason::NoNeighbours),
        neighbour: None,
        wanted: None,
    }
}

pub(super) fn plan_of(entries: Vec<TrialEntry>) -> TrialPlan {
    TrialPlan {
        source_root: PathBuf::from("/inbox"),
        target_root: PathBuf::from("/archive"),
        stays: entries.len(),
        entries,
        filed: 0,
        unsure: 0,
        already_there: 0,
        taken: std::collections::BTreeSet::new(),
        groups: StayGroups::default(),
        model_left: std::collections::BTreeSet::new(),
        model_failure: None,
    }
}

pub(super) fn grouped(plan: &TrialPlan, answered: &AnsweredGroups) -> StayGroups {
    let setup = Setup::new();
    let kinds = setup.access.kinds().unwrap();
    stay_groups(
        plan,
        &kinds,
        &SenderAliases::default(),
        &target_reading(),
        answered,
    )
}

pub(super) fn outcome<'a>(plan: &'a TrialPlan, name: &str) -> &'a TrialOutcome {
    &plan
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
        .outcome
}

#[test]
fn the_trial_files_finds_duplicates_and_names_what_stays() {
    let plan = trial(&Setup::new());
    assert_eq!(
        plan.filed + plan.unsure + plan.already_there + plan.stays,
        plan.entries.len()
    );
    assert_eq!(plan.entries.len(), 5);
    assert_eq!(
        outcome(&plan, "same.pdf"),
        &TrialOutcome::AlreadyThere {
            at: PathBuf::from("Alfa/2020/2020-09-10 Alfa Rechnung.pdf")
        }
    );
    assert_eq!(
        outcome(&plan, "nobody.pdf"),
        &TrialOutcome::Stays(StayReason::NoNeighbours)
    );
    assert_eq!(
        outcome(&plan, "a1.pdf"),
        &TrialOutcome::Filed {
            to: PathBuf::from("Alfa/2022/2022-07-04 Alfa Rechnung.pdf")
        }
    );
    // The second file of the same day wants the name the first one claimed.
    assert_eq!(
        outcome(&plan, "a2.pdf"),
        &TrialOutcome::Stays(StayReason::NameTaken)
    );
    assert_eq!(
        outcome(&plan, "beta.pdf"),
        &TrialOutcome::Unsure {
            choices: vec![PathBuf::from("Firma/Beta"), PathBuf::from("Privat/Beta")],
            file_names: vec!["beta.pdf".to_owned(), "beta.pdf".to_owned()],
            kind_label: Some("Invoice".to_owned()),
        }
    );
}

#[test]
fn a_second_file_with_the_content_of_a_filed_one_is_a_duplicate_of_it() {
    let alfa = |path: &str, seed: u8| plan_file(path, seed, Some("Alfa AG"), date(2022, 7, 4));
    let source = reading(
        "/inbox",
        vec![alfa("scan 1.pdf", 100), alfa("scan 2.pdf", 100)],
    );

    let plan = trial_of(&Setup::new(), source, AnsweredGroups::default());

    let first = PathBuf::from("Alfa/2022/2022-07-04 Alfa Rechnung.pdf");
    assert_eq!(
        outcome(&plan, "scan 1.pdf"),
        &TrialOutcome::Filed { to: first.clone() }
    );
    assert_eq!(
        outcome(&plan, "scan 2.pdf"),
        &TrialOutcome::AlreadyThere { at: first }
    );
}

#[test]
fn the_sample_shows_one_file_per_area_then_unsure_then_the_rest() {
    let plan = trial(&Setup::new());
    let order: Vec<&str> = plan
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        order,
        ["a1.pdf", "beta.pdf", "same.pdf", "a2.pdf", "nobody.pdf"]
    );
    assert!(plan
        .taken
        .contains(Path::new("Alfa/2022/2022-07-04 Alfa Rechnung.pdf")));
}
