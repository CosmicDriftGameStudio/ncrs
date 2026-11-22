//! Flow 1: the folder files by itself while new files arrive.

use std::path::PathBuf;
use std::time::Duration;

use ncrs_config::config::LocationTransfer;

use super::archive::{self, Arrivals};
use super::drive;
use super::probe::{files_in, wait_quiet_and, World};
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;

pub fn run<P: Live>(world: &World, session: &mut Session<P>) -> Arrivals {
    let task = session.runtime.enter(|| session
        .app
        .switch_continuous_for_test(&world.inbox_dir, LocationTransfer::Move));
    session.drive(task);
    session.settle();
    let saved = std::fs::read_to_string(world.root.join("config.toml")).unwrap();
    println!("config has continuous: {}", saved.contains("continuous = true"));

    let arrivals = archive::write_arrivals(&world.inbox_dir, &world.archive_dir);
    // Scans of letters the archive holds already, with the very same bytes.
    let scans = write_identical_scans(world);
    // The scripted model files nothing it is unsure of: the rule-sure letters
    // go, the others wait in the inbox as candidates, without a review.
    let settled = wait_quiet_and(session, Duration::from_secs(120), |_| {
        arrivals.by_rule.iter().all(|(from, _)| !from.exists())
    });
    assert!(settled, "the continuous filing did not settle");
    check_identical_scans(world, &scans);
    println!(
        "continuous: inbox {} files; neighbours moved {}/{}, by rule moved {}/{}, strangers left {}/{}",
        files_in(&world.inbox_dir),
        arrivals.neighbours.iter().filter(|(from, _)| !from.exists()).count(),
        arrivals.neighbours.len(),
        arrivals.by_rule.iter().filter(|(from, _)| !from.exists()).count(),
        arrivals.by_rule.len(),
        arrivals.strangers.iter().filter(|path| path.exists()).count(),
        arrivals.strangers.len(),
    );
    for (_info, items) in world.index().recent_batches(5).unwrap() {
        println!("batch with {} items", items.len());
    }
    arrivals
}

pub fn check(world: &World, arrivals: &Arrivals) {
    let saved = std::fs::read_to_string(world.root.join("config.toml")).unwrap();
    assert!(saved.contains("continuous = true"), "config: {saved}");
    for (from, target) in &arrivals.by_rule {
        assert!(!from.exists() && target.is_file(), "not filed by rule: {from:?} -> {target:?}");
    }
    assert!(
        arrivals.strangers.iter().all(|path| path.exists()),
        "an unknown sender was moved"
    );
    // The watcher may split the arrivals over several runs; each file has its item.
    for (from, target) in &arrivals.by_rule {
        assert_eq!(
            drive::journal_moved(world, from).as_deref(),
            Some(target.as_path()),
            "no journal item for {from:?}"
        );
    }
    // ponytail: known gap, the test model declines every folder choice, so the
    // neighbour-sure letters stay in place; the real model is out of scope here.
    println!(
        "neighbour-sure moved: {}/{}",
        arrivals.neighbours.iter().filter(|(from, _)| !from.exists()).count(),
        arrivals.neighbours.len()
    );
}

/// Copies of three archive files, new in the inbox while the folder files by
/// itself: the scan of a letter that was filed already.
fn write_identical_scans(world: &World) -> Vec<(PathBuf, PathBuf)> {
    world
        .inbox
        .duplicates
        .iter()
        .take(3)
        .enumerate()
        .map(|(number, (_, original))| {
            let scan = world.inbox_dir.join(format!("Neu {} Scan nochmal.txt", 8800 + number));
            std::fs::write(&scan, drive::text_of(original)).unwrap();
            (scan, original.clone())
        })
        .collect()
}

/// What a person gets from such a scan: nothing. It stays in the inbox, is no
/// document, has no review and no entry in the activity, and the index only
/// notes it as a repeat of the archive document.
fn check_identical_scans(world: &World, scans: &[(PathBuf, PathBuf)]) {
    let index = world.index();
    for (scan, original) in scans {
        assert!(scan.is_file(), "{scan:?} left the inbox");
        assert!(original.is_file());
        assert!(index.document_by_path(scan).unwrap().is_none(), "{scan:?} became a document");
        let original_document = index.document_by_path(original).unwrap().expect("the archive file is known");
        assert_eq!(super::probe::copy_of(&index, scan), Some(original_document.id), "{scan:?} is not noted as a copy");
        assert!(!drive::has_open_review(world, scan), "{scan:?} got a review");
        assert!(drive::journal_moved(world, scan).is_none(), "{scan:?} is in the activity");
    }
}
