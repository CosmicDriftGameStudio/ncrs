//! The scan: what it lists, where it files it, and that it stops.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use std::sync::atomic::AtomicBool;

use common::{write, Fake};
use ncrs_space::{scan, CatalogId, EntryKind, Group, Os, ScanRequest, SpaceSettings};
use tempfile::TempDir;

fn request(home: &TempDir, settings: SpaceSettings) -> ScanRequest {
    ScanRequest {
        os: Os::current(),
        home: home.path().to_path_buf(),
        repos: Vec::new(),
        settings: SpaceSettings { show_worktrees: false, ..settings },
    }
}

fn bun_cache(home: &TempDir) {
    write(&home.path().join(".bun/install/cache/a"), 300);
    write(&home.path().join(".bun/install/cache/deep/b"), 200);
}

fn bun_entry(found: &ncrs_space::Scan) -> &ncrs_space::Entry {
    found
        .entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Catalog(CatalogId::Bun))
        .unwrap()
}

#[test]
fn a_catalog_folder_is_measured_and_listed_as_safe() {
    let home = TempDir::new().unwrap();
    bun_cache(&home);
    let mut running = Vec::new();

    let found = scan(
        &request(&home, SpaceSettings::default()),
        &Fake::quiet(),
        &AtomicBool::new(false),
        |bytes| running.push(bytes),
    )
    .unwrap();

    let entry = bun_entry(&found);
    assert_eq!(entry.group, Group::Safe);
    assert_eq!(entry.size.bytes, 500);
    assert!(entry.modified.is_some());
    assert!(entry.held_by.is_empty());
}

#[test]
fn a_running_tool_or_an_unknown_answer_moves_it_to_in_use() {
    let home = TempDir::new().unwrap();
    bun_cache(&home);
    let settings = SpaceSettings::default;

    let running = scan(&request(&home, settings()), &Fake::running("bun"), &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(bun_entry(&running).group, Group::InUse);
    assert_eq!(bun_entry(&running).held_by.len(), 1);

    let no_look = Fake {
        folder: ncrs_space::FolderUse::Unknown(ncrs_space::UnknownWhy::TimedOut),
        ..Fake::quiet()
    };
    let unknown = scan(&request(&home, settings()), &no_look, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(bun_entry(&unknown).group, Group::InUse);
    assert!(bun_entry(&unknown).held_by.is_empty());
}

#[test]
fn without_a_process_list_the_catalog_stays_safe_and_the_scan_says_so() {
    let home = TempDir::new().unwrap();
    bun_cache(&home);
    let blind = Fake { processes: None, ..Fake::quiet() };

    let found = scan(&request(&home, SpaceSettings::default()), &blind, &AtomicBool::new(false), |_| {}).unwrap();
    assert!(!found.processes_known);
    assert_eq!(bun_entry(&found).group, Group::Safe);

    let known = scan(&request(&home, SpaceSettings::default()), &Fake::quiet(), &AtomicBool::new(false), |_| {}).unwrap();
    assert!(known.processes_known);
}

#[test]
fn a_never_touch_folder_is_not_listed() {
    let home = TempDir::new().unwrap();
    bun_cache(&home);
    let settings = SpaceSettings { never_touch: vec![home.path().join(".bun")], ..SpaceSettings::default() };

    let found = scan(&request(&home, settings), &Fake::quiet(), &AtomicBool::new(false), |_| {}).unwrap();

    assert!(!found.entries.iter().any(|entry| entry.kind == EntryKind::Catalog(CatalogId::Bun)));
}

#[test]
fn a_raised_stop_flag_gives_no_result() {
    let home = TempDir::new().unwrap();
    bun_cache(&home);
    let found = scan(&request(&home, SpaceSettings::default()), &Fake::quiet(), &AtomicBool::new(true), |_| {});
    assert!(found.is_none());
}
