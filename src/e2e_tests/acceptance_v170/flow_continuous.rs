//! Flow 1: the watched source files by itself while new files arrive.

use std::path::PathBuf;
use std::time::Duration;

use ncrs_hermes_ui::SortingMessage;
use ncrs_index::{BatchId, BatchKind, DocumentStatus};

use super::archive::{self, Arrivals};
use super::drive;
use super::probe::{files_in, wait_quiet_and, World};
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;
use crate::messages::Message;

pub fn run<P: Live>(world: &World, session: &mut Session<P>) -> Arrivals {
    drive::config_intact(world);
    let arrivals = archive::write_arrivals(&world.inbox_dir, &world.archive_dir);
    // Scans of letters the archive holds already, with the very same bytes.
    let scans = write_identical_scans(world);
    let settled = wait_quiet_and(session, Duration::from_secs(120), |_| {
        arrivals.by_rule.iter().all(|(from, _)| !from.exists())
    });
    assert!(settled, "the background filing did not settle");
    check_identical_scans(world, &scans);
    println!(
        "watched: inbox {} files; by rule moved {}/{}",
        files_in(&world.inbox_dir),
        arrivals.by_rule.iter().filter(|(from, _)| !from.exists()).count(),
        arrivals.by_rule.len(),
    );
    for (_info, items) in world.index().recent_batches(5).unwrap() {
        println!("batch with {} items", items.len());
    }
    arrivals
}

pub fn check(world: &World, arrivals: &Arrivals) {
    drive::config_intact(world);
    for (from, target) in &arrivals.by_rule {
        assert!(
            !from.exists() && target.is_file(),
            "not filed by rule: {from:?} -> {target:?}"
        );
    }
    for path in arrivals.known_sender.iter().chain(&arrivals.strangers) {
        assert!(path.exists(), "no rule takes {path:?}, but it was moved");
        assert!(
            !drive::has_open_review(world, path),
            "{path:?} became a question"
        );
    }
    for (path, _) in &world.inbox.sure {
        assert!(path.exists(), "{path:?} lay there before and was moved");
    }
    // The watcher may split the arrivals over several runs; each file has its item.
    for (from, target) in &arrivals.by_rule {
        assert_eq!(
            drive::journal_moved(world, from).as_deref(),
            Some(target.as_path()),
            "no journal item for {from:?}"
        );
    }
}

/// Copies of three archive files, new in the inbox while it is watched: the
/// scan of a letter that was filed already.
fn write_identical_scans(world: &World) -> Vec<(PathBuf, PathBuf)> {
    world
        .inbox
        .duplicates
        .iter()
        .take(3)
        .enumerate()
        .map(|(number, (_, original))| {
            let scan = world
                .inbox_dir
                .join(format!("Neu {} Scan nochmal.txt", 8800 + number));
            std::fs::write(&scan, drive::text_of(original)).unwrap();
            (scan, original.clone())
        })
        .collect()
}

/// What such a scan becomes: it stays in the inbox, is no document and has no
/// question and no entry in the activity; the index notes it as a repeat of
/// the archive document.
fn check_identical_scans(world: &World, scans: &[(PathBuf, PathBuf)]) {
    let index = world.index();
    for (scan, original) in scans {
        assert!(scan.is_file(), "{scan:?} left the inbox");
        assert!(original.is_file());
        assert!(
            index.document_by_path(scan).unwrap().is_none(),
            "{scan:?} became a document"
        );
        let original_document = index
            .document_by_path(original)
            .unwrap()
            .expect("the archive file is known");
        assert_eq!(
            super::probe::copy_of(&index, scan),
            Some(original_document.id),
            "{scan:?} is not noted as a copy"
        );
        assert!(
            !drive::has_open_review(world, scan),
            "{scan:?} became a question"
        );
        assert!(
            drive::journal_moved(world, scan).is_none(),
            "{scan:?} is in the activity"
        );
    }
}

/// The runs the watched source made are put back from the settings page: the files
/// lie in the inbox again, the index does not call them filed, and nothing
/// files them a second time.
pub fn undo_and_check<P: Live>(world: &World, session: &mut Session<P>, arrivals: &Arrivals) {
    let batches: Vec<BatchId> = {
        let index = world.index();
        [BatchKind::Hermes, BatchKind::Filing]
            .into_iter()
            .flat_map(|kind| index.undoable_batches(kind, 50).unwrap())
            .filter(|batch| {
                index
                    .journal_items(*batch)
                    .unwrap()
                    .iter()
                    .any(|item| arrivals.by_rule.iter().any(|(from, _)| *from == item.from))
            })
            .collect()
    };
    assert!(
        !batches.is_empty(),
        "no run of the background filing to undo"
    );
    for batch in batches {
        session.send(Message::Sorting(SortingMessage::Undo(batch.number())));
        assert!(
            wait_quiet_and(session, Duration::from_secs(60), |_| true),
            "the pipeline did not settle after the undo"
        );
    }
    let index = world.index();
    for (from, target) in &arrivals.by_rule {
        assert!(
            from.is_file() && !target.exists(),
            "not put back: {from:?} <- {target:?}"
        );
        let document = index
            .document_by_path(from)
            .unwrap()
            .unwrap_or_else(|| panic!("{from:?} is no document after the undo"));
        assert_ne!(
            document.status,
            DocumentStatus::Filed,
            "{from:?} still counts as filed"
        );
        assert!(
            drive::journal_moved(world, from).is_none(),
            "{from:?} is still a moved item"
        );
    }
    drop(index);
    // The put-back files are not filed a second time by the watcher.
    let quiet = wait_quiet_and(session, Duration::from_secs(10), |_| true);
    assert!(quiet && arrivals.by_rule.iter().all(|(from, _)| from.is_file()));
    let index_after_quiet = world.index();
    let again = [BatchKind::Hermes, BatchKind::Filing]
        .into_iter()
        .flat_map(|kind| index_after_quiet.undoable_batches(kind, 50).unwrap())
        .any(|batch| {
            index_after_quiet
                .journal_items(batch)
                .unwrap()
                .iter()
                .any(|item| arrivals.by_rule.iter().any(|(from, _)| *from == item.from))
        });
    assert!(!again, "the put-back files were filed again");
}
