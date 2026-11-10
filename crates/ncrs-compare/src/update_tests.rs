// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_core::{CompareMark, CompareMarks, FileEntry, PanelSide};
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::KnownHostsFiles;
use ncrs_i18n::Language;

use crate::action::Action;
use crate::core::Compared;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::scan::ScanError;
use crate::state::Compare;
use crate::update::update;

fn pool() -> Arc<Pool> {
    Arc::new(Pool::over_tcp(KnownHostsFiles::default()))
}

fn run(compare: &mut Compare, message: Message, left: &Path, right: &Path) -> Vec<Action> {
    let pool = pool();
    let ctx = Ctx {
        lang: Language::English,
        pool: &pool,
        left,
        right,
        blocking_overlay_open: false,
    };
    update(compare, message, &ctx).1
}

const LEFT: &str = "/scans/left";
const RIGHT: &str = "/scans/right";

fn entry(side: &str, name: &str) -> FileEntry {
    let mut entry = FileEntry::parent(Path::new("/"));
    entry.name = OsString::from(name);
    entry.path = PathBuf::from(side).join(name);
    entry.is_parent = false;
    entry.is_dir = false;
    entry.size = 7;
    entry
}

fn compared() -> Compared {
    let name = |text: &str| OsString::from(text);
    Compared {
        left: CompareMarks::from([
            (name("new.pdf"), CompareMark::OnlyHere),
            (name("same.pdf"), CompareMark::Same),
        ]),
        right: CompareMarks::from([
            (name("old.pdf"), CompareMark::OnlyHere),
            (name("same.pdf"), CompareMark::Same),
        ]),
        left_rows: vec![entry(LEFT, "new.pdf"), entry(LEFT, "same.pdf")],
        right_rows: vec![entry(RIGHT, "old.pdf"), entry(RIGHT, "same.pdf")],
    }
}

/// A comparison of `LEFT` and `RIGHT` that has its result.
fn finished_comparison() -> Compare {
    let mut compare = Compare::default();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(&mut compare, Message::Open, left, right);
    run(&mut compare, Message::Confirm, left, right);
    let generation = compare.generation;
    run(
        &mut compare,
        Message::Finished {
            generation,
            result: Ok(compared()),
        },
        left,
        right,
    );
    compare
}

#[test]
fn the_start_dialog_opens_on_the_folders_of_the_panels_and_cancel_closes_it() {
    let mut compare = Compare::default();
    run(
        &mut compare,
        Message::Open,
        Path::new(LEFT),
        Path::new(RIGHT),
    );
    let folders = compare.start.clone().unwrap();
    assert_eq!(folders.left, Path::new(LEFT));
    assert_eq!(folders.right, Path::new(RIGHT));

    run(
        &mut compare,
        Message::Cancel,
        Path::new(LEFT),
        Path::new(RIGHT),
    );
    assert!(!compare.dialog_is_open());
    assert!(compare.run.is_none());
}

#[test]
fn the_choices_are_kept_for_the_next_time() {
    let mut compare = Compare::default();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(&mut compare, Message::Open, left, right);
    run(&mut compare, Message::ToggleContent, left, right);
    run(&mut compare, Message::ToggleDate, left, right);
    run(&mut compare, Message::Cancel, left, right);

    run(&mut compare, Message::Open, left, right);

    assert!(compare.choices.content);
    assert!(!compare.choices.date);
}

#[test]
fn the_marks_reach_the_panels_once_the_comparison_is_done() {
    let mut compare = Compare::default();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(&mut compare, Message::Open, left, right);
    run(&mut compare, Message::Confirm, left, right);
    assert!(compare.run.is_some());
    assert!(compare.marks(PanelSide::Left).is_none());

    let generation = compare.generation;
    let actions = run(
        &mut compare,
        Message::Finished {
            generation,
            result: Ok(compared()),
        },
        left,
        right,
    );

    assert!(actions.contains(&Action::MarksChanged));
    let marks = compare.marks(PanelSide::Left).unwrap();
    assert_eq!(
        marks.get(&OsString::from("new.pdf")),
        Some(&CompareMark::OnlyHere)
    );
}

#[test]
fn the_result_of_an_older_comparison_is_dropped() {
    let mut compare = Compare::default();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(&mut compare, Message::Open, left, right);
    run(&mut compare, Message::Confirm, left, right);
    let first = compare.generation;
    run(&mut compare, Message::Again, left, right);
    assert_ne!(first, compare.generation);

    let actions = run(
        &mut compare,
        Message::Finished {
            generation: first,
            result: Ok(compared()),
        },
        left,
        right,
    );

    assert!(compare.marks(PanelSide::Left).is_none());
    assert!(compare.run.is_some());
    assert!(actions.is_empty());
}

#[test]
fn a_comparison_that_could_not_read_a_folder_ends_with_the_error_and_no_marks() {
    let mut compare = Compare::default();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(&mut compare, Message::Open, left, right);
    run(&mut compare, Message::Confirm, left, right);
    let generation = compare.generation;

    let actions = run(
        &mut compare,
        Message::Finished {
            generation,
            result: Err(ScanError::Failed("permission denied".to_owned())),
        },
        left,
        right,
    );

    assert!(compare.run.is_none());
    assert!(actions.iter().any(
        |action| matches!(action, Action::ShowError(text) if text.contains("permission denied"))
    ));
}

#[test]
fn done_stops_the_scan_and_takes_the_bar_away() {
    let mut compare = Compare::default();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(&mut compare, Message::Open, left, right);
    run(&mut compare, Message::Confirm, left, right);
    let stop = Arc::clone(&compare.run.as_ref().unwrap().stop);

    let actions = run(&mut compare, Message::Done, left, right);

    assert!(compare.run.is_none());
    assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
    assert!(actions.contains(&Action::MarksChanged));
}

#[test]
fn a_panel_that_leaves_its_folder_ends_the_comparison() {
    let mut compare = finished_comparison();

    let still_there = run(
        &mut compare,
        Message::PanelsChanged,
        Path::new(LEFT),
        Path::new(RIGHT),
    );
    assert!(still_there.is_empty());
    assert!(compare.run.is_some());

    let moved = run(
        &mut compare,
        Message::PanelsChanged,
        Path::new("/scans/left/2025"),
        Path::new(RIGHT),
    );
    assert!(compare.run.is_none());
    assert!(moved.contains(&Action::MarksChanged));
}

#[test]
fn only_differences_hides_the_rows_that_are_the_same() {
    let mut compare = finished_comparison();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    assert!(compare.shows_row(PanelSide::Left, &entry(LEFT, "same.pdf")));

    run(&mut compare, Message::ToggleOnlyDifferences, left, right);

    assert!(!compare.shows_row(PanelSide::Left, &entry(LEFT, "same.pdf")));
    assert!(compare.shows_row(PanelSide::Left, &entry(LEFT, "new.pdf")));
    assert!(compare.shows_row(PanelSide::Right, &FileEntry::parent(Path::new("/"))));
}

#[test]
fn a_sync_copies_through_the_queue_and_deletes_nothing_unless_ticked() {
    let mut compare = finished_comparison();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(
        &mut compare,
        Message::OpenSync(crate::core::Direction::ToRight),
        left,
        right,
    );
    assert!(!compare.sync.unwrap().delete);

    let actions = run(&mut compare, Message::Confirm, left, right);
    assert_eq!(
        actions,
        [Action::Copy {
            sources: vec![PathBuf::from(LEFT).join("new.pdf")],
            target: PathBuf::from(RIGHT),
        }]
    );

    let after_copies = run(&mut compare, Message::OpsIdle, left, right);
    assert!(
        after_copies
            .iter()
            .all(|action| !matches!(action, Action::Delete { .. })),
        "{after_copies:?}"
    );
    assert!(
        compare.run.as_ref().unwrap().result.is_none(),
        "compares again"
    );
}

#[test]
fn a_ticked_delete_is_asked_for_after_the_copies_through_the_delete_flow() {
    let mut compare = finished_comparison();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    run(
        &mut compare,
        Message::OpenSync(crate::core::Direction::ToRight),
        left,
        right,
    );
    run(&mut compare, Message::ToggleDelete, left, right);
    let (_, plan) = compare.plan().unwrap();
    assert_eq!(plan.deletions.len(), 1);

    let copy = run(&mut compare, Message::Confirm, left, right);
    assert!(copy
        .iter()
        .all(|action| !matches!(action, Action::Delete { .. })));

    let after_copies = run(&mut compare, Message::OpsIdle, left, right);
    assert_eq!(
        after_copies,
        [Action::Delete {
            side: PanelSide::Right,
            sources: vec![PathBuf::from(RIGHT).join("old.pdf")],
        }]
    );

    let after_delete = run(&mut compare, Message::OpsIdle, left, right);
    assert!(after_delete.contains(&Action::MarksChanged));
    assert!(!compare.waits_for_ops());
}

#[test]
fn the_delete_box_starts_unticked_every_time() {
    let mut compare = finished_comparison();
    let (left, right) = (Path::new(LEFT), Path::new(RIGHT));
    let to_right = Message::OpenSync(crate::core::Direction::ToRight);
    run(&mut compare, to_right.clone(), left, right);
    run(&mut compare, Message::ToggleDelete, left, right);
    run(&mut compare, Message::Cancel, left, right);

    run(&mut compare, to_right, left, right);

    assert!(!compare.sync.unwrap().delete);
}
