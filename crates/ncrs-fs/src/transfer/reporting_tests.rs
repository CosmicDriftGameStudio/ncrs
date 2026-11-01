use super::*;
use std::fs;

fn tree(root: &Path) {
    fs::create_dir_all(root.join("sub/deeper")).unwrap();
    fs::write(root.join("a.txt"), b"a").unwrap();
    fs::write(root.join("sub/b.txt"), b"b").unwrap();
    fs::write(root.join("sub/deeper/c.txt"), b"c").unwrap();
}

fn scratch(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("ncrs-report-{label}-"))
        .tempdir()
        .expect("a scratch directory")
}

/// The count the bar shows has to be the real one. A line reading "3 of 1"
/// is worse than no line at all.
#[tokio::test]
async fn a_copy_reports_its_progress() {
    let dir = scratch("progress");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    run_reporting(
        &src,
        &dst,
        ncrs_core::TransferKind::Copy,
        OnConflict::Fail,
        &tx,
    )
    .unwrap();

    let mut newest: Option<Tick> = None;
    while let Ok(tick) = rx.try_recv() {
        newest = Some(tick);
    }
    let last = newest.expect("the copy reported nothing at all");
    // src, a.txt, sub, sub/b.txt, sub/deeper, sub/deeper/c.txt
    assert_eq!(last.done, 6, "the final count is wrong");
    assert_eq!(last.total, 6, "the total does not match what was copied");
}

/// The last tick has to be the finished one, so the bar reaches full rather
/// than stopping one short.
#[tokio::test]
async fn the_last_tick_is_the_finished_one() {
    let dir = scratch("last");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    run_reporting(
        &src,
        &dst,
        ncrs_core::TransferKind::Copy,
        OnConflict::Fail,
        &tx,
    )
    .unwrap();

    let ticks: Vec<Tick> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(
        ticks.len() >= 2,
        "expected more than one tick, got {}",
        ticks.len()
    );
    assert_eq!(ticks.last().copied(), Some(Tick { done: 6, total: 6 }));
}

/// An app that is gone stops the copy rather than running to the end into
/// nothing. This is also how a stopped app reaches the blocking thread.
#[tokio::test]
async fn a_closed_channel_stops_the_copy() {
    let dir = scratch("closed");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    let err = run_reporting(
        &src,
        &dst,
        ncrs_core::TransferKind::Copy,
        OnConflict::Fail,
        &tx,
    )
    .unwrap_err();
    assert!(err.to_string().contains("app is gone"), "got {err}");
}

/// A plain `run` reports nothing, so a caller that does not want the bar
/// moving does not get messages it has to ignore.
#[tokio::test]
async fn run_reports_nothing() {
    let dir = scratch("quiet");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel::<Tick>();
    let done = run(&src, &dst, ncrs_core::TransferKind::Copy, OnConflict::Fail).unwrap();
    assert_eq!(done, 6);
    assert!(rx.try_recv().is_err(), "run sent a tick anyway");
    let _ = tx;
}

fn zip_with_a_file(dir: &Path) -> PathBuf {
    use crate::archive::fixtures::{write_archive, Item};
    let archive = dir.join("box.zip");
    write_archive(&archive, &[Item::File("in/doc.txt", b"content")]);
    archive
}

#[tokio::test]
async fn a_source_inside_an_archive_is_copied_out_with_progress() {
    let dir = scratch("from-archive");
    let archive = zip_with_a_file(dir.path());
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    let source = archive.join("in").join("doc.txt");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let done = run_reporting(
        &source,
        &dst,
        ncrs_core::TransferKind::Copy,
        OnConflict::Fail,
        &tx,
    )
    .unwrap();

    assert_eq!(done, 1);
    assert_eq!(fs::read(dst.join("doc.txt")).unwrap(), b"content");
    assert_eq!(rx.try_recv().ok(), Some(Tick { done: 1, total: 1 }));
    assert!(archive.is_file(), "the archive stays");
}

#[test]
fn a_move_out_of_an_archive_is_refused_and_changes_nothing() {
    let dir = scratch("move-from-archive");
    let archive = zip_with_a_file(dir.path());
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();

    let err = run(
        &archive.join("in"),
        &dst,
        ncrs_core::TransferKind::Move,
        OnConflict::Fail,
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    assert_eq!(fs::read_dir(&dst).unwrap().count(), 0);
}

#[tokio::test]
async fn a_closed_channel_stops_a_copy_out_of_an_archive() {
    let dir = scratch("closed-archive");
    let archive = zip_with_a_file(dir.path());
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    let err = run_reporting(
        &archive.join("in"),
        &dst,
        ncrs_core::TransferKind::Copy,
        OnConflict::Fail,
        &tx,
    )
    .unwrap_err();

    assert!(err.to_string().contains("app is gone"), "got {err}");
    assert_eq!(fs::read_dir(&dst).unwrap().count(), 0);
}
