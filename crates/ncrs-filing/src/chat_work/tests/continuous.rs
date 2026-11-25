use std::collections::BTreeSet;

use ncrs_index::{Condition, ConditionKind, DocumentStatus, RuleConditions, RuleSpec};

use super::live::index_known;
use super::picks::RecordingModel;
use super::*;
use crate::chat_work::{file_new, open_in, plan_listed, ContinuousReport};

fn rule_for(text: &str, folder: &str) -> RuleSpec {
    RuleSpec {
        name: Some(text.to_owned()),
        conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, text)]),
        extract_number: Some(r"number (\w+)".to_owned()),
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: format!("{folder}/{{number}}"),
        confirmed: true,
        enabled: true,
    }
}

/// Two files a stored rule files, and one nobody knows a folder for.
fn setup_with_rule() -> Setup {
    let setup = Setup::new();
    setup.write("inbox/one.txt", "Zeta contract number one");
    setup.write("inbox/two.txt", "Zeta contract number two");
    setup.write("inbox/other.txt", "Something nobody knows");
    index_known(&setup, "archive/Alfa/old.txt", "Alfa AG invoice");
    let ChatResult::RuleStored(stored) = setup.access.run(ChatWork::SaveRule {
        spec: rule_for("Zeta contract", "Zeta"),
    }) else {
        panic!("not a rule result");
    };
    stored.unwrap();
    setup
}

fn all_files() -> Vec<PathBuf> {
    ["one.txt", "two.txt", "other.txt"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

fn file_all(setup: &Setup, files: &[PathBuf], transfer: Transfer) -> ContinuousReport {
    let model = RecordingModel {
        declines: true,
        ..RecordingModel::default()
    };
    file_new(
        &setup.access,
        &setup.dir.path().join("inbox"),
        &setup.dir.path().join("archive"),
        files,
        transfer,
        &model,
        "inbox filed continuously".to_owned(),
    )
    .unwrap()
}

fn files_in(folder: &Path) -> usize {
    fs::read_dir(folder).map_or(0, |entries| entries.count())
}

#[test]
fn sure_files_are_filed_as_a_run_in_the_history_and_the_rest_stays() {
    let setup = setup_with_rule();
    let inbox = setup.dir.path().join("inbox");
    let zeta = setup.dir.path().join("archive/Zeta");

    let report = file_all(&setup, &all_files(), Transfer::Move);

    assert_eq!(report, ContinuousReport { filed: 2, left: 1 });
    assert_eq!(files_in(&zeta), 2);
    assert!(inbox.join("other.txt").exists());
    assert!(!inbox.join("one.txt").exists());
    let runs = setup.access.recent_runs(5).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].summary.name, "inbox filed continuously");
    assert_eq!(runs[0].summary.items, 2);
    assert!(runs[0].undoable);

    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: runs[0].summary.batch,
    }) else {
        panic!("not an undo result");
    };
    assert_eq!(undone.unwrap().undone, 2);
    assert!(inbox.join("one.txt").exists() && inbox.join("two.txt").exists());
    assert_eq!(files_in(&zeta), 0);
}

#[test]
fn a_copy_leaves_the_source_files_in_place() {
    let setup = setup_with_rule();
    let inbox = setup.dir.path().join("inbox");

    let report = file_all(&setup, &all_files(), Transfer::Copy);

    assert_eq!(report.filed, 2);
    assert_eq!(files_in(&setup.dir.path().join("archive/Zeta")), 2);
    assert!(inbox.join("one.txt").exists() && inbox.join("two.txt").exists());
}

#[test]
fn nothing_is_filed_without_a_sure_place_and_no_run_is_made() {
    let setup = Setup::new();
    setup.write("inbox/other.txt", "Something nobody knows");
    index_known(&setup, "archive/Alfa/old.txt", "Alfa AG invoice");

    let report = file_all(&setup, &[PathBuf::from("other.txt")], Transfer::Move);

    assert_eq!(report, ContinuousReport { filed: 0, left: 1 });
    assert!(setup.access.recent_runs(5).unwrap().is_empty());
}

#[test]
fn a_file_only_the_model_picked_a_folder_for_waits_for_the_tab() {
    let setup = Setup::new();
    setup.write("inbox/other.txt", "Gamma KG invoice");
    index_known(&setup, "archive/Alfa/old.txt", "Alfa AG invoice");
    index_known(&setup, "archive/Beta/old.txt", "Beta GmbH invoice");
    let inbox = setup.dir.path().join("inbox");
    let beta = setup.dir.path().join("archive/Beta");
    let model = RecordingModel {
        picks_folder: Some("Beta"),
        ..RecordingModel::default()
    };

    let report = file_new(
        &setup.access,
        &inbox,
        &setup.dir.path().join("archive"),
        &[PathBuf::from("other.txt")],
        Transfer::Move,
        &model,
        "inbox filed continuously".to_owned(),
    )
    .unwrap();

    assert_eq!(report, ContinuousReport { filed: 0, left: 1 });
    assert!(inbox.join("other.txt").exists());
    assert_eq!(files_in(&beta), 1);
    assert!(setup.access.recent_runs(5).unwrap().is_empty());
}

#[test]
fn only_the_listed_files_are_planned_and_filed() {
    let setup = setup_with_rule();
    let listed = [PathBuf::from("one.txt")];
    let model = RecordingModel {
        declines: true,
        ..RecordingModel::default()
    };

    let plan = plan_listed(
        &setup.access,
        &setup.dir.path().join("inbox"),
        &setup.dir.path().join("archive"),
        &listed,
        &model,
        &LiveRun::default(),
    )
    .unwrap()
    .unwrap();

    let planned: Vec<&Path> = plan.files.iter().map(|file| file.from.as_path()).collect();
    assert_eq!(planned, [Path::new("one.txt")]);

    let report = file_all(&setup, &listed, Transfer::Move);
    assert_eq!(report, ContinuousReport { filed: 1, left: 0 });
    assert!(setup.dir.path().join("inbox/two.txt").exists());
    assert!(setup.dir.path().join("inbox/other.txt").exists());
}

#[test]
fn a_file_that_is_not_listed_is_not_even_hashed() {
    let setup = setup_with_rule();
    let run = LiveRun::default();
    let model = RecordingModel {
        declines: true,
        ..RecordingModel::default()
    };

    plan_listed(
        &setup.access,
        &setup.dir.path().join("inbox"),
        &setup.dir.path().join("archive"),
        &[PathBuf::from("one.txt")],
        &model,
        &run,
    )
    .unwrap();

    assert_eq!(run.progress.total(), 1);
}

#[test]
fn new_documents_without_review_under_a_root_are_found_for_a_restart() {
    let setup = Setup::new();
    let root = setup.dir.path().join("inbox");
    let at = |name: &str| root.join(name);
    let outside = setup.dir.path().join("elsewhere/c.pdf");
    setup
        .access
        .with_index_mut(|index| {
            let mut ids = Vec::new();
            let seeded: [(u8, PathBuf); 3] =
                [(1, at("a.pdf")), (2, at("b.pdf")), (3, outside.clone())];
            for (seed, path) in seeded {
                ids.push(
                    index
                        .insert_document(&ncrs_index::NewDocument {
                            hash: ncrs_index::ContentHash([seed; 32]),
                            path,
                            source: ncrs_index::DocumentSource::Inbox,
                            received_at: chrono::Utc::now(),
                            stamp: None,
                        })
                        .map_err(|err| err.to_string())?,
                );
            }
            let reviewed = ids.get(1).copied().ok_or("no id")?;
            index
                .add_review(reviewed, ncrs_index::ReviewReason::NoMatchingRule)
                .map_err(|err| err.to_string())?;
            Ok(())
        })
        .unwrap();

    let found = crate::chat_work::open_in(&setup.access, std::slice::from_ref(&root)).unwrap();

    assert_eq!(found, [(root.clone(), at("a.pdf"))]);
}

fn index_inbox_file(setup: &Setup, relative: &str) -> ncrs_index::DocumentId {
    let path = setup.dir.path().join(relative);
    let mut guard = setup.access.slot.lock().unwrap();
    let index = guard.as_mut().unwrap();
    let id = index
        .insert_document(&ncrs_index::NewDocument {
            hash: hash_path(&path).unwrap(),
            path: path.clone(),
            source: ncrs_index::DocumentSource::Inbox,
            received_at: chrono::Utc::now(),
            stamp: Some(super::reading::stamp_of_file(&path)),
        })
        .unwrap();
    index
        .set_document_text(id, &fs::read_to_string(&path).unwrap())
        .unwrap();
    id
}

#[test]
fn a_moved_document_and_its_undo_keep_file_and_index_together() {
    let setup = setup_with_rule();
    let inbox = setup.dir.path().join("inbox");
    let archive = setup.dir.path().join("archive");
    let id = index_inbox_file(&setup, "inbox/one.txt");

    let report = file_all(&setup, &all_files(), Transfer::Move);

    assert_eq!(report.filed, 2);
    let filed = setup
        .access
        .with_index(|index| Ok(index.document(id).unwrap().unwrap()))
        .unwrap();
    assert_eq!(filed.path, archive.join("Zeta/one.txt"));
    assert_eq!(filed.status, DocumentStatus::Filed);
    assert!(archive.join("Zeta/one.txt").exists() && !inbox.join("one.txt").exists());

    let runs = setup.access.recent_runs(5).unwrap();
    let ChatResult::Undone(undone) = setup.access.run(ChatWork::Undo {
        batch: runs[0].summary.batch,
    }) else {
        panic!("not an undo result");
    };

    assert_eq!(undone.unwrap().undone, 2);
    assert_eq!(
        fs::read_to_string(inbox.join("one.txt")).unwrap(),
        "Zeta contract number one"
    );
    assert!(!archive.join("Zeta").exists());
    let back = setup
        .access
        .with_index(|index| {
            Ok((
                index.document(id).unwrap().unwrap(),
                index.open_review_of(id).unwrap().is_some(),
            ))
        })
        .unwrap();
    assert_eq!(back.0.path, inbox.join("one.txt"));
    assert_eq!(
        back.0.stamp,
        Some(super::reading::stamp_of_file(&inbox.join("one.txt")))
    );
    assert_eq!(back.0.status, DocumentStatus::InReview);
    assert!(back.1);
    assert!(setup.access.recent_runs(5).unwrap().is_empty());
    assert!(open_in(&setup.access, &[inbox]).unwrap().is_empty());
}

#[test]
fn a_folder_that_is_copied_refuses_a_run_that_moves() {
    let setup = setup_with_rule();
    let model = RecordingModel {
        declines: true,
        ..RecordingModel::default()
    };
    let access = setup
        .access
        .clone()
        .with_copy_roots(vec![setup.dir.path().join("inbox")]);

    let result = file_new(
        &access,
        &setup.dir.path().join("inbox"),
        &setup.dir.path().join("archive"),
        &all_files(),
        Transfer::Move,
        &model,
        "inbox filed continuously".to_owned(),
    );

    assert!(result.unwrap_err().contains("copied, not moved"));
}
