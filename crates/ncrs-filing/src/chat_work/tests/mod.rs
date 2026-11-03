#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::NaiveDate;
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_index::{ContentHash, DocumentFields, Index, KindCatalog, Transfer};
use ncrs_intake::{
    hash_path, location_structure, ConditionProblem, PlainText, PlanFile, RuleProblem,
};
use tempfile::TempDir;

use super::*;

mod orders;

struct Setup {
    dir: TempDir,
    access: ChatAccess,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let trash = JournaledTrash::new(dir.path().join("trash.toml"), FolderBackend(bin));
        let access = ChatAccess::new(
            Arc::new(Mutex::new(Some(index))),
            Arc::new(TextChain::new().with_source(PlainText)),
            Arc::new(trash),
        );
        Self { dir, access }
    }

    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.dir.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn read(&self, folder: &Path) -> Result<Arc<FolderReading>, ReadError> {
        match self.access.run(ChatWork::Read {
            folder: folder.to_path_buf(),
        }) {
            ChatResult::Read(result) => result,
            other => Err(ReadError::Io(format!("{other:?}"))),
        }
    }
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn fields(when: Option<NaiveDate>, kind: Option<&str>, sender: Option<&str>) -> DocumentFields {
    DocumentFields {
        sender: sender.map(str::to_owned),
        kind: kind.map(str::to_owned),
        date: when,
        ..DocumentFields::default()
    }
}

fn kinds() -> ncrs_index::Kinds {
    ncrs_index::Kinds::new(
        Arc::new(KindCatalog::built_in()),
        ncrs_index::KindLanguage::English,
    )
}

fn plan_file(path: &str, seed: u8, sender: Option<&str>, when: NaiveDate) -> PlanFile {
    PlanFile {
        relative_path: PathBuf::from(path),
        hash: ContentHash([seed; 32]),
        fields: DocumentFields {
            sender: sender.map(str::to_owned),
            kind: Some("invoice".to_owned()),
            date: Some(when),
            ..DocumentFields::default()
        },
        text: None,
        recorded_at: i64::from(seed),
    }
}

fn reading(root: &str, mut files: Vec<PlanFile>) -> FolderReading {
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let structure = location_structure(&files, &KindCatalog::built_in());
    let mut hashes = HashMap::new();
    for file in &files {
        hashes
            .entry(file.hash)
            .or_insert_with(|| file.relative_path.clone());
    }
    FolderReading {
        patterns: Vec::new(),
        root: PathBuf::from(root),
        files,
        photos: 0,
        without_text: 0,
        structure,
        duplicates: Vec::new(),
        hashes,
    }
}

#[test]
fn reading_counts_files_photos_and_skips_hidden_files_and_links() {
    let setup = Setup::new();
    setup.write("in/a.txt", "Rechnung der Alfa AG");
    setup.write("in/sub/b.txt", "Mahnung der Beta GmbH");
    setup.write("in/.hidden.txt", "never read");
    setup.write("in/.cache/c.txt", "never read");
    setup.write("in/photo.jpg", "not a picture");
    setup.write("in/blank.dat", "");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        setup.dir.path().join("in/a.txt"),
        setup.dir.path().join("in/link.txt"),
    )
    .unwrap();

    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    let paths: Vec<&Path> = read
        .files
        .iter()
        .map(|file| file.relative_path.as_path())
        .collect();
    assert_eq!(
        paths,
        [
            Path::new("a.txt"),
            Path::new("blank.dat"),
            Path::new("photo.jpg"),
            Path::new("sub/b.txt")
        ]
    );
    assert_eq!(read.photos, 1);
    assert_eq!(read.without_text, 1);
    assert_eq!(read.hashes.len(), 4);
    assert_eq!(
        read.files[0].hash,
        hash_path(&setup.dir.path().join("in/a.txt")).unwrap()
    );
}

#[test]
fn reading_a_file_or_a_missing_folder_or_too_many_files_is_refused() {
    let setup = Setup::new();
    let file = setup.write("one.txt", "x");
    assert_eq!(setup.read(&file), Err(ReadError::NotADirectory));
    assert_eq!(
        setup.read(&setup.dir.path().join("missing")),
        Err(ReadError::NotADirectory)
    );
    let big = setup.dir.path().join("big");
    fs::create_dir_all(&big).unwrap();
    for number in 0..=FILE_LIMIT {
        fs::write(big.join(format!("{number}.txt")), "").unwrap();
    }
    assert_eq!(
        setup.read(&big),
        Err(ReadError::TooMany { limit: FILE_LIMIT })
    );
}

#[test]
fn a_known_document_brings_its_fields_and_text_from_the_index() {
    let setup = Setup::new();
    let path = setup.write("in/known.txt", "no letter date in here");
    let hash = hash_path(&path).unwrap();
    let mut guard = setup.access.slot.lock().unwrap();
    let index = guard.as_mut().unwrap();
    let id = index
        .insert_document(&ncrs_index::NewDocument {
            hash,
            path: path.clone(),
            source: ncrs_index::DocumentSource::Archive,
            received_at: chrono::Utc::now(),
            stamp: None,
        })
        .unwrap();
    index
        .set_document_fields(
            id,
            &DocumentFields {
                sender: Some("Alfa AG".to_owned()),
                ..DocumentFields::default()
            },
            |_| ncrs_index::FieldOrigin::Unknown,
        )
        .unwrap();
    index.set_document_text(id, "stored text").unwrap();
    drop(guard);
    let read = setup.read(&setup.dir.path().join("in")).unwrap();
    assert_eq!(read.files[0].fields.sender.as_deref(), Some("Alfa AG"));
    assert_eq!(read.files[0].text.as_deref(), Some("stored text"));
}

fn target_reading() -> FolderReading {
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

fn source_reading() -> FolderReading {
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

fn trial(setup: &Setup, changes: Vec<OrderChange>) -> TrialPlan {
    trial_of(setup, source_reading(), changes, AnsweredGroups::default())
}

fn trial_of(
    setup: &Setup,
    source: FolderReading,
    changes: Vec<OrderChange>,
    answered: AnsweredGroups,
) -> TrialPlan {
    match setup.access.run(ChatWork::Trial {
        source: Arc::new(source),
        target: Arc::new(target_reading()),
        changes,
        answered,
        layout: None,
    }) {
        ChatResult::Trial(plan) => plan.unwrap(),
        other => panic!("{other:?}"),
    }
}

fn save_group_rule(setup: &Setup, key: GroupKey, label: &str, area: &str) {
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

fn staying(name: &str, sender: Option<&str>, kind: Option<&str>) -> TrialEntry {
    TrialEntry {
        from: PathBuf::from("/inbox").join(name),
        name: name.to_owned(),
        hash: ContentHash([7; 32]),
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

fn plan_of(entries: Vec<TrialEntry>) -> TrialPlan {
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
    }
}

fn grouped(plan: &TrialPlan, answered: &AnsweredGroups) -> StayGroups {
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

fn outcome<'a>(plan: &'a TrialPlan, name: &str) -> &'a TrialOutcome {
    &plan
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
        .outcome
}

#[test]
fn the_trial_files_finds_duplicates_and_names_what_stays() {
    let plan = trial(&Setup::new(), Vec::new());
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
            kind_label: Some("Invoice".to_owned()),
        }
    );
}

#[test]
fn the_sample_shows_one_file_per_area_then_unsure_then_the_rest() {
    let plan = trial(&Setup::new(), Vec::new());
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

#[test]
fn a_month_folder_goes_in_after_the_year_with_or_without_only() {
    let change = |only| OrderChange {
        area: PathBuf::from("Alfa"),
        depth: 1,
        order: ChangeOrder::ByMonth,
        only,
        kind_names: Vec::new(),
        pattern: None,
    };
    let to = || PathBuf::from("Alfa/2022/2022-07-04 Alfa Rechnung.pdf");
    let when = Some(date(2022, 7, 4));
    assert_eq!(
        change::apply_changes(to(), &fields(when, None, None), &kinds(), &[change(None)]),
        PathBuf::from("Alfa/2022/07/2022-07-04 Alfa Rechnung.pdf")
    );
    assert_eq!(
        change::apply_changes(
            to(),
            &fields(when, None, None),
            &kinds(),
            &[change(Some((0, "Alfa".to_owned())))]
        ),
        PathBuf::from("Alfa/2022/07/2022-07-04 Alfa Rechnung.pdf")
    );
    assert_eq!(
        change::apply_changes(
            to(),
            &fields(when, None, None),
            &kinds(),
            &[change(Some((0, "Beta".to_owned())))]
        ),
        to()
    );
    assert_eq!(
        change::apply_changes(to(), &fields(None, None, None), &kinds(), &[change(None)]),
        to()
    );
    let other_area = PathBuf::from("Beta/2022/x.pdf");
    assert_eq!(
        change::apply_changes(
            other_area.clone(),
            &fields(when, None, None),
            &kinds(),
            &[change(None)]
        ),
        other_area
    );
    let no_year = PathBuf::from("Alfa/Misc/x.pdf");
    assert_eq!(
        change::apply_changes(
            no_year.clone(),
            &fields(when, None, None),
            &kinds(),
            &[change(None)]
        ),
        no_year
    );
}

#[test]
fn a_change_only_touches_documents_of_its_kind_or_in_a_folder_that_names_it() {
    let change = OrderChange {
        area: PathBuf::from("Bank"),
        depth: 3,
        order: ChangeOrder::ByMonth,
        only: None,
        kind_names: vec!["bank_statement".to_owned(), "Kontoauszüge".to_owned()],
        pattern: None,
    };
    let when = Some(date(2022, 7, 4));
    let in_folder = PathBuf::from("Bank/Sparkasse/Kontoauszüge/2022/a.pdf");
    let card = PathBuf::from("Bank/DKB/Kreditkarte/2022/b.pdf");
    let apply = |to: &PathBuf, kind| {
        change::apply_changes(
            to.clone(),
            &fields(when, kind, None),
            &kinds(),
            std::slice::from_ref(&change),
        )
    };
    assert_eq!(
        apply(&in_folder, None),
        PathBuf::from("Bank/Sparkasse/Kontoauszüge/2022/07/a.pdf")
    );
    assert_eq!(apply(&card, None), card);
    assert_eq!(apply(&card, Some("invoice")), card);
    assert_eq!(
        apply(&card, Some("Kontoauszug")),
        PathBuf::from("Bank/DKB/Kreditkarte/2022/07/b.pdf")
    );
}

#[test]
fn the_trial_applies_the_order_change_to_the_filed_names() {
    let change = OrderChange {
        area: PathBuf::from("Alfa"),
        depth: 1,
        order: ChangeOrder::ByMonth,
        only: None,
        kind_names: Vec::new(),
        pattern: None,
    };
    let plan = trial(&Setup::new(), vec![change]);
    assert_eq!(
        outcome(&plan, "a1.pdf"),
        &TrialOutcome::Filed {
            to: PathBuf::from("Alfa/2022/07/2022-07-04 Alfa Rechnung.pdf")
        }
    );
}

fn run_plan(setup: &Setup, transfer: Transfer) -> RunPlan {
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

fn last_run(setup: &Setup) -> Option<RunSummary> {
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
    let ChatResult::Filed(report) = setup.access.run(ChatWork::File { run }) else {
        panic!("not a filing result");
    };
    assert_eq!(
        report.unwrap(),
        RunReport {
            done: 2,
            skipped: 0
        }
    );
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
            skipped: 0
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

fn source_with_staying_files() -> FolderReading {
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
        Vec::new(),
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
    let plan = trial(&Setup::new(), Vec::new());
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
    let before = trial(&setup, Vec::new());
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
    let after = trial(&setup, Vec::new());
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

    let plan = trial_of(&setup, source(), Vec::new(), AnsweredGroups::default());
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
