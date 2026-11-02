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
use ncrs_intake::{hash_path, location_structure, PlainText, PlanFile};
use tempfile::TempDir;

use super::*;

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
    match setup.access.run(ChatWork::Trial {
        source: Arc::new(source_reading()),
        target: Arc::new(target_reading()),
        changes,
    }) {
        ChatResult::Trial(plan) => plan.unwrap(),
        other => panic!("{other:?}"),
    }
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
        year_depth: 1,
        unit: TimeUnit::Month,
        only,
    };
    let to = || PathBuf::from("Alfa/2022/2022-07-04 Alfa Rechnung.pdf");
    let when = Some(date(2022, 7, 4));
    assert_eq!(
        change::apply_changes(to(), when, &[change(None)]),
        PathBuf::from("Alfa/2022/07/2022-07-04 Alfa Rechnung.pdf")
    );
    assert_eq!(
        change::apply_changes(to(), when, &[change(Some((0, "Alfa".to_owned())))]),
        PathBuf::from("Alfa/2022/07/2022-07-04 Alfa Rechnung.pdf")
    );
    assert_eq!(
        change::apply_changes(to(), when, &[change(Some((0, "Beta".to_owned())))]),
        to()
    );
    assert_eq!(change::apply_changes(to(), None, &[change(None)]), to());
    let other_area = PathBuf::from("Beta/2022/x.pdf");
    assert_eq!(
        change::apply_changes(other_area.clone(), when, &[change(None)]),
        other_area
    );
    let no_year = PathBuf::from("Alfa/Misc/x.pdf");
    assert_eq!(
        change::apply_changes(no_year.clone(), when, &[change(None)]),
        no_year
    );
}

#[test]
fn the_trial_applies_the_order_change_to_the_filed_names() {
    let change = OrderChange {
        area: PathBuf::from("Alfa"),
        year_depth: 1,
        unit: TimeUnit::Month,
        only: None,
    };
    let plan = trial(&Setup::new(), vec![change]);
    assert_eq!(
        outcome(&plan, "a1.pdf"),
        &TrialOutcome::Filed {
            to: PathBuf::from("Alfa/2022/07/2022-07-04 Alfa Rechnung.pdf")
        }
    );
}

fn bank_reading() -> FolderReading {
    let mut files = Vec::new();
    let mut seed = 0;
    for (bank, sender) in [("Sparkasse", "Sparkasse Chemnitz"), ("DKB", "DKB")] {
        for year in [2020, 2021] {
            for month in [1, 2, 3] {
                seed += 1;
                files.push(plan_file(
                    &format!("Bank/{bank}/{year}/{year}-{month:02}-01 {bank} Auszug {seed}.pdf"),
                    seed,
                    Some(sender),
                    date(year, month, 1),
                ));
            }
        }
    }
    reading("/archive", files)
}

fn preview(setup: &Setup, word: &str) -> Option<ChangePreview> {
    match setup.access.run(ChatWork::PreviewChange {
        reading: Arc::new(bank_reading()),
        kind_word: word.to_owned(),
        unit: TimeUnit::Month,
    }) {
        ChatResult::Preview(preview) => preview,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_preview_shows_the_newest_document_with_its_month_and_the_scope() {
    let preview = preview(&Setup::new(), "Rechnungen").unwrap();
    assert_eq!(preview.kind_label, "Invoice");
    assert_eq!(preview.change.area, PathBuf::from("Bank"));
    assert_eq!(preview.change.year_depth, 2);
    assert_eq!(preview.highlighted, 3);
    assert_eq!(
        preview.example,
        ["Bank", "DKB", "2021", "03", "2021-03-01 DKB Auszug 12.pdf"]
    );
    let scope = preview.scope.unwrap();
    assert_eq!(scope.values, ["DKB", "Sparkasse"]);
}

#[test]
fn no_preview_for_a_word_that_names_no_kind_in_the_folder() {
    assert_eq!(preview(&Setup::new(), "Verträge"), None);
    assert_eq!(preview(&Setup::new(), "Wetter"), None);
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
