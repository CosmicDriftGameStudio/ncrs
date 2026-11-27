//! The delete of the safe group: checked again per folder, for good, logged.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use std::sync::atomic::AtomicBool;

use common::{entry_for, write, Fake};
use ncrs_space::{append_log, delete_safe, CatalogId, Group, Skip, UnknownUse};
use tempfile::TempDir;

fn stop() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn a_safe_folder_is_deleted_for_good_and_its_bytes_are_logged() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join(".bun/install/cache");
    write(&folder.join("a"), 300);
    let entry = entry_for(&folder, CatalogId::Bun);
    let mut seen = 0;

    let log = delete_safe(&[entry], &Fake::quiet(), UnknownUse::Skip, &stop(), |_| {
        seen += 1
    });

    assert!(!folder.exists());
    assert_eq!(seen, 1);
    assert_eq!(log.rows.len(), 1);
    assert_eq!(log.rows[0].outcome, Ok(300));
    assert_eq!(log.freed_bytes(), 300);

    let logs = home.path().join("logs");
    append_log(&logs, &log).unwrap();
    let written = std::fs::read_dir(&logs)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(written).unwrap();
    assert!(text.contains("freed=300"));
    assert!(text.contains(&format!("{}\t300\tfreed\t300", folder.display())));
}

#[test]
fn a_folder_that_changed_since_the_scan_stays() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("cache");
    write(&folder.join("a"), 100);
    let entry = entry_for(&folder, CatalogId::Bun);
    write(&folder.join("a"), 500);

    let log = delete_safe(&[entry], &Fake::quiet(), UnknownUse::Skip, &stop(), |_| {});

    assert!(folder.join("a").exists());
    assert_eq!(log.rows[0].outcome, Err(Skip::Changed));
    assert_eq!(log.freed_bytes(), 0);
}

#[cfg(unix)]
#[test]
fn a_symlink_entry_is_never_followed() {
    let home = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    write(&outside.path().join("precious"), 100);
    let link = home.path().join("cache");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    let mut entry = entry_for(outside.path(), CatalogId::Bun);
    entry.path = link.clone();

    let log = delete_safe(&[entry], &Fake::quiet(), UnknownUse::Skip, &stop(), |_| {});

    assert_eq!(log.rows[0].outcome, Err(Skip::Symlink));
    assert!(outside.path().join("precious").exists());
    assert!(link.exists());
}

#[test]
fn a_running_tool_keeps_its_folder() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("target");
    write(&folder.join("a"), 100);
    let entry = entry_for(&folder, CatalogId::CargoTarget);

    let log = delete_safe(
        &[entry],
        &Fake::running("rustc"),
        UnknownUse::Skip,
        &stop(),
        |_| {},
    );

    assert!(matches!(log.rows[0].outcome, Err(Skip::ToolRunning(_))));
    assert!(folder.exists());
}

#[test]
fn only_entries_of_the_safe_group_are_touched() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("mine");
    write(&folder.join("a"), 100);
    let mut entry = entry_for(&folder, CatalogId::OwnRegenerable);
    entry.group = Group::Decide;

    let log = delete_safe(&[entry], &Fake::quiet(), UnknownUse::Skip, &stop(), |_| {});

    assert!(matches!(log.rows[0].outcome, Err(Skip::Failed(_))));
    assert!(folder.exists());
}

#[test]
fn a_raised_stop_flag_ends_the_run_between_entries() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("cache");
    write(&folder.join("a"), 100);
    let entry = entry_for(&folder, CatalogId::Bun);

    let log = delete_safe(
        &[entry],
        &Fake::quiet(),
        UnknownUse::Skip,
        &AtomicBool::new(true),
        |_| {},
    );

    assert!(log.rows.is_empty());
    assert!(folder.exists());
}
