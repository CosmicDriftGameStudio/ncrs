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

#[test]
fn a_freed_row_records_the_last_use_and_that_the_check_answered() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("cache");
    write(&folder.join("a"), 100);
    let mut entry = entry_for(&folder, CatalogId::Bun);
    let used = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    entry.facts.last_used = Some(used);

    let log = delete_safe(&[entry], &Fake::quiet(), UnknownUse::Skip, &stop(), |_| {});

    assert_eq!(log.rows[0].last_used, Some(used));
    assert!(log.rows[0].use_checked);
}

#[test]
fn a_row_let_through_after_the_confirm_says_the_check_did_not_answer() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("cache");
    write(&folder.join("a"), 100);
    let entry = entry_for(&folder, CatalogId::Bun);
    let blind = Fake {
        processes: None,
        ..Fake::quiet()
    };

    let log = delete_safe(
        &[entry],
        &blind,
        UnknownUse::AllowAfterConfirm,
        &stop(),
        |_| {},
    );

    assert!(log.rows[0].outcome.is_ok());
    assert!(!log.rows[0].use_checked);
}

#[test]
fn a_written_log_is_read_back_with_its_runs_rows_and_reasons() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let freed_folder = home.path().join("a");
    write(&freed_folder.join("x"), 100);
    let kept_folder = home.path().join("b");
    write(&kept_folder.join("x"), 50);
    let mut freed = entry_for(&freed_folder, CatalogId::Bun);
    freed.facts.last_used =
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
    let kept = entry_for(&kept_folder, CatalogId::Bun);
    let log = delete_safe(
        &[freed, kept],
        &ChromeThenFree(std::sync::atomic::AtomicBool::new(false)),
        UnknownUse::Skip,
        &stop(),
        |_| {},
    );
    append_log(dir.path(), &log).unwrap();

    let file = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let runs = ncrs_space::parse_log(&std::fs::read_to_string(file).unwrap());

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].freed, log.freed_bytes());
    assert_eq!(runs[0].rows.len(), 2);
    let outcomes: Vec<_> = runs[0].rows.iter().map(|row| row.outcome.clone()).collect();
    assert!(outcomes.iter().any(Result::is_ok));
    assert!(outcomes.iter().any(
        |outcome| matches!(outcome, Err(Skip::ToolRunning(holder)) if holder.name == "bun" && holder.pid == 42)
    ));
    let done = runs[0].rows.iter().find(|row| row.outcome.is_ok()).unwrap();
    assert!(done.use_checked);
    assert_eq!(
        done.last_used,
        Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000))
    );
}

/// The first folder is free, the second has a running "bun".
struct ChromeThenFree(std::sync::atomic::AtomicBool);

impl ncrs_space::ProcessSource for ChromeThenFree {
    fn processes(&self) -> Option<Vec<ncrs_space::Holder>> {
        let second = self.0.swap(true, std::sync::atomic::Ordering::Relaxed);
        Some(if second {
            vec![ncrs_space::Holder {
                pid: 42,
                name: "bun".to_owned(),
            }]
        } else {
            Vec::new()
        })
    }

    fn folder_use(
        &self,
        _folder: &std::path::Path,
        _limit: std::time::Duration,
    ) -> ncrs_space::FolderUse {
        ncrs_space::FolderUse::Free
    }
}

#[test]
fn the_words_of_a_skipped_row_are_read_back_into_a_skip() {
    for skip in [
        Skip::UseUnknown,
        Skip::Symlink,
        Skip::OtherDrive,
        Skip::Changed,
        Skip::Gone,
        Skip::NotGranted,
        Skip::Failed("disk full".to_owned()),
        Skip::ToolRunning(ncrs_space::Holder {
            pid: 9,
            name: "Google Chrome".to_owned(),
        }),
        Skip::FolderOpen(ncrs_space::Holder {
            pid: 3,
            name: "editor".to_owned(),
        }),
    ] {
        let dir = TempDir::new().unwrap();
        let now = std::time::SystemTime::now();
        let mut log = ncrs_space::RunLog {
            started: now,
            finished: now,
            rows: Vec::new(),
        };
        log.rows.push(ncrs_space::LogRow {
            path: "/x".into(),
            size: 1,
            outcome: Err(skip.clone()),
            last_used: None,
            use_checked: false,
        });
        append_log(dir.path(), &log).unwrap();
        let file = std::fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let runs = ncrs_space::parse_log(&std::fs::read_to_string(file).unwrap());
        assert_eq!(runs[0].rows[0].outcome, Err(skip));
    }
}
