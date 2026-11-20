use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use ncrs_core::notes_sync::NotesSyncDirection;

use crate::sync::{run, SyncJob};

fn job(local: &Path, target: &Path, direction: NotesSyncDirection) -> SyncJob {
    SyncJob {
        local: local.to_owned(),
        target: target.to_owned(),
        direction,
        conflict_suffix: "Konflikt".to_owned(),
    }
}

fn write_at(path: &Path, text: &str, age_seconds: u64) {
    fs::write(path, text).unwrap();
    let time = SystemTime::now() - Duration::from_secs(age_seconds);
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

#[test]
fn one_way_copies_new_and_changed_notes_and_never_deletes() {
    let (local, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let job = job(local.path(), target.path(), NotesSyncDirection::OneWay);
    write_at(&local.path().join("a.md"), "first", 100);
    let report = run(&job, SystemTime::now()).unwrap();
    assert_eq!(report.copied, 1);
    assert_eq!(
        fs::read_to_string(target.path().join("a.md")).unwrap(),
        "first"
    );

    write_at(&local.path().join("a.md"), "second, longer", 50);
    run(&job, SystemTime::now()).unwrap();
    assert_eq!(
        fs::read_to_string(target.path().join("a.md")).unwrap(),
        "second, longer"
    );

    fs::remove_file(local.path().join("a.md")).unwrap();
    run(&job, SystemTime::now()).unwrap();
    assert!(
        target.path().join("a.md").exists(),
        "nothing is deleted there"
    );
}

#[test]
fn encrypted_notes_travel_as_they_are() {
    let (local, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let job = job(local.path(), target.path(), NotesSyncDirection::OneWay);
    fs::write(local.path().join("a.md.enc"), b"NCRSNOTE\x01 bytes").unwrap();
    run(&job, SystemTime::now()).unwrap();
    assert_eq!(
        fs::read(target.path().join("a.md.enc")).unwrap(),
        b"NCRSNOTE\x01 bytes"
    );
}

#[test]
fn both_ways_takes_the_newer_note_from_either_side() {
    let (local, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let job = job(local.path(), target.path(), NotesSyncDirection::BothWays);
    write_at(&local.path().join("a.md"), "one", 300);
    write_at(&target.path().join("b.md"), "from elsewhere", 300);
    run(&job, SystemTime::now()).unwrap();
    assert_eq!(
        fs::read_to_string(target.path().join("a.md")).unwrap(),
        "one"
    );
    assert_eq!(
        fs::read_to_string(local.path().join("b.md")).unwrap(),
        "from elsewhere"
    );

    write_at(&target.path().join("a.md"), "one, edited there", 100);
    run(&job, SystemTime::now()).unwrap();
    assert_eq!(
        fs::read_to_string(local.path().join("a.md")).unwrap(),
        "one, edited there"
    );
}

#[test]
fn both_ways_keeps_both_versions_when_both_sides_changed() {
    let (local, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let job = job(local.path(), target.path(), NotesSyncDirection::BothWays);
    write_at(&local.path().join("a.md"), "base", 300);
    run(&job, SystemTime::now()).unwrap();

    write_at(&local.path().join("a.md"), "changed here", 200);
    write_at(&target.path().join("a.md"), "changed there, newer", 100);
    let report = run(&job, SystemTime::now()).unwrap();
    assert_eq!(report.conflicts, 1);
    for side in [local.path(), target.path()] {
        assert_eq!(
            fs::read_to_string(side.join("a.md")).unwrap(),
            "changed there, newer"
        );
        assert_eq!(
            fs::read_to_string(side.join("a (Konflikt).md")).unwrap(),
            "changed here"
        );
    }
}

#[test]
fn a_sync_folder_inside_the_notes_folder_is_refused() {
    let local = tempfile::tempdir().unwrap();
    let inside = local.path().join("sync");
    fs::create_dir(&inside).unwrap();
    let job = job(local.path(), &inside, NotesSyncDirection::BothWays);
    run(&job, SystemTime::now()).unwrap_err();
}

#[test]
fn a_notes_folder_inside_the_sync_folder_is_refused() {
    let target = tempfile::tempdir().unwrap();
    let inside = target.path().join("notes");
    fs::create_dir(&inside).unwrap();
    let job = job(&inside, target.path(), NotesSyncDirection::OneWay);
    run(&job, SystemTime::now()).unwrap_err();
}
