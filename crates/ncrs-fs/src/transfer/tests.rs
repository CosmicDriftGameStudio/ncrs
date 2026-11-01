use super::*;
use std::fs;

/// A scratch directory that removes itself, so a failing assertion does not
/// leave a tree that makes the next run fail.
fn scratch(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("ncrs-transfer-{label}-"))
        .tempdir()
        .expect("a scratch directory")
}

/// Collects the counts so a test can assert on progress.
#[derive(Default)]
struct Recorder {
    seen: Vec<(usize, usize)>,
}

impl Progress for Recorder {
    fn advanced(&mut self, done: usize, total: usize) -> io::Result<()> {
        self.seen.push((done, total));
        Ok(())
    }
}

#[derive(Default)]
struct Cancelled {
    after: usize,
    seen: usize,
}

impl Progress for Cancelled {
    fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
        self.seen += 1;
        if self.seen >= self.after {
            return Err(io::Error::other("cancelled"));
        }
        Ok(())
    }
}

fn tree(root: &Path) {
    fs::create_dir_all(root.join("sub/deeper")).unwrap();
    fs::write(root.join("a.txt"), b"a").unwrap();
    fs::write(root.join("sub/b.txt"), b"b").unwrap();
    fs::write(root.join("sub/deeper/c.txt"), b"c").unwrap();
}

#[test]
fn copies_a_tree_and_keeps_the_source() {
    let dir = scratch("copy");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let mut progress = Recorder::default();
    let done = transfer(&src, &dst, &Copy, &mut progress, OnConflict::Fail).unwrap();

    // src itself, a.txt, sub, sub/b.txt, sub/deeper, sub/deeper/c.txt
    assert_eq!(done, 6);
    assert!(src.exists(), "copy must not touch the source");
    assert_eq!(fs::read(src.join("sub/deeper/c.txt")).unwrap(), b"c");
    // The target keeps the source's name, so it is dst/src/...
    assert_eq!(fs::read(dst.join("src/sub/deeper/c.txt")).unwrap(), b"c");
}

/// A move is the same walk; only the source is removed at the end.
#[test]
fn moves_a_tree_and_removes_the_source() {
    let dir = scratch("move");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let mut progress = Counting;
    transfer(&src, &dst, &Move, &mut progress, OnConflict::Fail).unwrap();

    assert!(!src.exists(), "move must remove the source");
    // The target keeps the source's name, so it is dst/src/...
    assert_eq!(fs::read(dst.join("src/sub/deeper/c.txt")).unwrap(), b"c");
}

/// The default is to stop, not to overwrite. Overwriting by accident loses
/// data, and the "ask the user" case is the dialog's job.
#[test]
fn refuses_to_overwrite_by_default() {
    let dir = scratch("conflict");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"new").unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::create_dir_all(dst.join("src")).unwrap();
    fs::write(dst.join("src/a.txt"), b"old").unwrap();

    let mut progress = Counting;
    let err = transfer(&src, &dst, &Copy, &mut progress, OnConflict::Fail).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read(dst.join("src/a.txt")).unwrap(),
        b"old",
        "the existing file was touched"
    );
}

#[test]
fn overwrites_when_asked() {
    let dir = scratch("overwrite");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"new").unwrap();
    fs::create_dir_all(dst.join("src")).unwrap();
    fs::write(dst.join("src/a.txt"), b"old").unwrap();

    let mut progress = Counting;
    transfer(&src, &dst, &Copy, &mut progress, OnConflict::Overwrite).unwrap();

    assert_eq!(fs::read(dst.join("src/a.txt")).unwrap(), b"new");
}

/// A cancel has to stop the walk, not just be reported. Otherwise a large
/// tree keeps copying after the user pressed Escape.
#[test]
fn a_cancel_stops_the_walk() {
    let dir = scratch("cancel");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let mut progress = Cancelled { after: 2, seen: 0 };
    let err = transfer(&src, &dst, &Copy, &mut progress, OnConflict::Fail).unwrap_err();

    assert_eq!(err.to_string(), "cancelled");
    assert_eq!(progress.seen, 2, "the walk kept going after the cancel");
}

/// Progress reports the real total, so the bar is not a guess.
#[test]
fn progress_reports_the_total() {
    let dir = scratch("progress");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let mut progress = Recorder::default();
    transfer(&src, &dst, &Copy, &mut progress, OnConflict::Fail).unwrap();

    assert_eq!(progress.seen.len(), 6);
    for (done, total) in &progress.seen {
        assert_eq!(*total, 6, "the total changed mid-walk");
        assert!(*done <= 6);
    }
    assert_eq!(progress.seen.last().unwrap(), &(6, 6));
}

/// A symlink is copied as a link, so a marked tree does not silently pull
/// in whatever the link points at.
#[cfg(unix)]
#[test]
fn a_symlink_is_copied_as_a_link() {
    let dir = scratch("symlink");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("target.txt"), b"t").unwrap();
    std::os::unix::fs::symlink("target.txt", src.join("link")).unwrap();
    fs::create_dir_all(&dst).unwrap();

    let mut progress = Counting;
    transfer(&src, &dst, &Copy, &mut progress, OnConflict::Fail).unwrap();

    let copied = dst.join("src/link");
    assert!(
        fs::symlink_metadata(&copied)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link was followed instead of copied"
    );
}

/// Keep: nothing is touched, and a move does not delete the source it did
/// not copy.
#[test]
fn skip_leaves_both_sides_alone() {
    let dir = scratch("skip");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"new").unwrap();
    fs::create_dir_all(dst.join("src")).unwrap();
    fs::write(dst.join("src/old.txt"), b"old").unwrap();

    let done = transfer(&src, &dst, &Move, &mut Counting, OnConflict::Skip).unwrap();

    assert_eq!(done, 0);
    assert!(
        src.join("a.txt").exists(),
        "a skipped move removed the source"
    );
    assert_eq!(fs::read(dst.join("src/old.txt")).unwrap(), b"old");
    assert!(!dst.join("src/a.txt").exists(), "a skipped item was copied");
}

/// Both panels in one directory: overwriting would delete the source before
/// copying it.
#[test]
fn a_file_onto_itself_is_refused_and_survives() {
    let dir = scratch("onto-itself");
    fs::write(dir.path().join("a.txt"), b"precious").unwrap();

    for conflict in [OnConflict::Overwrite, OnConflict::Fail] {
        let err = transfer(
            &dir.path().join("a.txt"),
            dir.path(),
            &Move,
            &mut Counting,
            conflict,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(dir.path().join("a.txt")).unwrap(), b"precious");
    }
}

/// A directory into itself would recurse until the path got too long.
#[test]
fn a_directory_into_itself_or_below_is_refused() {
    let dir = scratch("into-itself");
    let src = dir.path().join("src");
    tree(&src);

    for target_dir in [src.clone(), src.join("sub/deeper")] {
        let err = transfer(&src, &target_dir, &Copy, &mut Counting, OnConflict::Fail).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
    assert!(
        !src.join("src").exists(),
        "something was copied before refusing"
    );
    assert!(!src.join("sub/deeper/src").exists());
}

/// The old target stays until the new one is complete. A copy that dies
/// halfway must not have taken the old one with it.
#[test]
fn a_failed_overwrite_keeps_the_old_target() {
    let dir = scratch("failed-overwrite");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(dst.join("src")).unwrap();
    fs::write(dst.join("src/old.txt"), b"old").unwrap();

    let mut progress = Cancelled { after: 3, seen: 0 };
    let err = transfer(&src, &dst, &Copy, &mut progress, OnConflict::Overwrite).unwrap_err();

    assert_eq!(err.to_string(), "cancelled");
    assert_eq!(fs::read(dst.join("src/old.txt")).unwrap(), b"old");
    let leftovers: Vec<_> = fs::read_dir(&dst).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "a half-built copy was left behind");
}

/// A finished overwrite replaces the old tree entirely, for every pairing of
/// file and directory, and leaves no scratch name behind.
#[test]
fn an_overwrite_replaces_whatever_was_there() {
    for (source_is_dir, target_is_dir) in
        [(false, false), (true, true), (false, true), (true, false)]
    {
        let dir = scratch("replace");
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        if source_is_dir {
            fs::create_dir_all(&src).unwrap();
            fs::write(src.join("new.txt"), b"new").unwrap();
        } else {
            fs::write(&src, b"new").unwrap();
        }
        if target_is_dir {
            fs::create_dir_all(dst.join("src")).unwrap();
            fs::write(dst.join("src/old.txt"), b"old").unwrap();
        } else {
            fs::write(dst.join("src"), b"old").unwrap();
        }

        transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Overwrite).unwrap();

        if source_is_dir {
            assert_eq!(fs::read(dst.join("src/new.txt")).unwrap(), b"new");
            assert!(!dst.join("src/old.txt").exists(), "old contents survived");
        } else {
            assert_eq!(fs::read(dst.join("src")).unwrap(), b"new");
        }
        assert_eq!(
            fs::read_dir(&dst).unwrap().count(),
            1,
            "a scratch name is left"
        );
    }
}

/// A dangling symlink is in the way: copying through it would create the
/// file it points at.
#[cfg(unix)]
#[test]
fn a_dangling_symlink_counts_as_taken() {
    let dir = scratch("dangling");
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst");
    fs::write(&src, b"new").unwrap();
    fs::create_dir_all(&dst).unwrap();
    let missing = dir.path().join("missing");
    std::os::unix::fs::symlink(&missing, dst.join("src.txt")).unwrap();

    let err = transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Fail).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(
        !missing.exists(),
        "the copy wrote through the dangling link"
    );
}

/// The staging entry is created, never opened: a planted symlink at the
/// name must not be written through.
#[cfg(unix)]
#[test]
fn a_new_entry_is_never_written_through_a_symlink() {
    let dir = scratch("exclusive");
    let src = dir.path().join("src.txt");
    fs::write(&src, b"new").unwrap();
    let victim = dir.path().join("victim.txt");
    fs::write(&victim, b"untouched").unwrap();
    let planted = dir.path().join("planted");
    std::os::unix::fs::symlink(&victim, &planted).unwrap();

    let err = create_entry(&src, &fs::symlink_metadata(&src).unwrap(), &planted).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&victim).unwrap(), b"untouched");
}

/// Once the new item is in place, an old target that cannot be removed
/// does not undo the move: the new one stays, the old one is left aside.
#[cfg(unix)]
#[test]
fn an_old_target_that_cannot_be_removed_does_not_undo_the_move() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("swap-leftover");
    let src = dir.path().join("item");
    let dst = dir.path().join("dst");
    fs::write(&src, b"new").unwrap();
    // The old target is a directory that cannot be emptied.
    let locked = dst.join("item/locked");
    fs::create_dir_all(&locked).unwrap();
    fs::write(locked.join("f"), b"old").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();

    let result = transfer(&src, &dst, &Move, &mut Counting, OnConflict::Overwrite);

    let leftover: Vec<_> = fs::read_dir(&dst)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name() != Some(std::ffi::OsStr::new("item")))
        .collect();
    for path in &leftover {
        let _ = fs::set_permissions(path.join("locked"), fs::Permissions::from_mode(0o755));
    }
    assert!(result.is_ok(), "the move itself succeeded: {result:?}");
    assert!(!src.exists(), "a move keeps no source");
    assert_eq!(fs::read(dst.join("item")).unwrap(), b"new");
    assert_eq!(leftover.len(), 1, "the old target is set aside once");
    assert_eq!(fs::read(leftover[0].join("locked/f")).unwrap(), b"old");
}

/// The target may be an ancestor of the source: replacing it would set the
/// source aside with it and delete both.
#[test]
fn a_target_that_contains_the_source_is_refused() {
    let dir = scratch("ancestor");
    let outer = dir.path().join("q");
    fs::create_dir_all(&outer).unwrap();
    fs::write(outer.join("q"), b"precious").unwrap();

    let err = transfer(
        &outer.join("q"),
        dir.path(),
        &Copy,
        &mut Counting,
        OnConflict::Overwrite,
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(fs::read(outer.join("q")).unwrap(), b"precious");
}

/// A FIFO opened for reading blocks forever, so it is refused up front.
#[cfg(unix)]
#[test]
fn a_fifo_is_refused_not_opened() {
    let dir = scratch("fifo");
    let src = dir.path().join("pipe");
    let dst = dir.path().join("dst");
    let made = std::process::Command::new("mkfifo")
        .arg(&src)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo failed");
    fs::create_dir_all(&dst).unwrap();

    let err = transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Fail).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

/// A copy is not a way to hand out a privilege bit.
#[cfg(unix)]
#[test]
fn a_copy_drops_setuid_and_keeps_the_ordinary_bits() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("setuid");
    let src = dir.path().join("tool");
    let dst = dir.path().join("dst");
    fs::write(&src, b"x").unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o4755)).unwrap();
    fs::create_dir_all(&dst).unwrap();

    transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Fail).unwrap();

    let mode = fs::metadata(dst.join("tool")).unwrap().permissions().mode();
    assert_eq!(mode & 0o7777, 0o755);
}

/// A move on one device is a rename: the file keeps its inode. A copy and
/// delete would give it a new one.
#[cfg(unix)]
#[test]
fn a_move_renames_when_it_can() {
    use std::os::unix::fs::MetadataExt as _;
    let dir = scratch("rename");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();
    let before = fs::metadata(src.join("a.txt")).unwrap().ino();

    transfer(&src, &dst, &Move, &mut Counting, OnConflict::Fail).unwrap();

    assert!(!src.exists());
    assert_eq!(fs::metadata(dst.join("src/a.txt")).unwrap().ino(), before);
}

/// Moving across devices cannot rename; the effect says so and the walk
/// copies and removes instead.
#[test]
fn a_move_that_cannot_rename_copies_and_removes() {
    struct AcrossDevices;
    impl Source for AcrossDevices {
        fn removes_source(&self) -> bool {
            true
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            Move.remove(path)
        }
    }
    let dir = scratch("exdev");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    transfer(&src, &dst, &AcrossDevices, &mut Counting, OnConflict::Fail).unwrap();

    assert!(!src.exists());
    assert_eq!(fs::read(dst.join("src/sub/deeper/c.txt")).unwrap(), b"c");
}

/// A move that always has to copy, as across volumes.
struct AcrossDevices;
impl Source for AcrossDevices {
    fn removes_source(&self) -> bool {
        true
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        Move.remove(path)
    }
    fn relocate(&self, _from: &Path, _to: &Path) -> io::Result<bool> {
        Ok(false)
    }
}

#[test]
fn a_file_is_renamed_inside_its_folder_to_a_new_name() {
    let dir = scratch("rename-here");
    let src = dir.path().join("scan 0042.pdf");
    fs::write(&src, b"Company A").unwrap();
    let target = dir.path().join("2024-04-05_invoice.pdf");

    transfer_to(&src, &target, &Move, &mut Counting, OnConflict::Fail).unwrap();

    assert!(!src.exists());
    assert_eq!(fs::read(&target).unwrap(), b"Company A");
}

#[test]
fn a_target_that_is_the_source_or_around_it_is_refused() {
    let dir = scratch("to-refused");
    let src = dir.path().join("a/b.txt");
    fs::create_dir_all(src.parent().unwrap()).unwrap();
    fs::write(&src, b"x").unwrap();

    for target in [src.clone(), dir.path().join("a")] {
        let err = transfer_to(&src, &target, &Move, &mut Counting, OnConflict::Fail).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{target:?}");
    }
    assert_eq!(fs::read(&src).unwrap(), b"x");
}

#[test]
fn a_move_across_devices_copies_verifies_and_only_then_removes_the_source() {
    let dir = scratch("exdev-to");
    let src = dir.path().join("Company A.pdf");
    fs::write(&src, b"letter").unwrap();
    let target = dir.path().join("new/Agency B.pdf");

    transfer_to(
        &src,
        &target,
        &AcrossDevices,
        &mut Counting,
        OnConflict::Fail,
    )
    .unwrap();

    assert!(!src.exists());
    assert_eq!(fs::read(&target).unwrap(), b"letter");
    let leftovers: Vec<_> = fs::read_dir(dir.path().join("new")).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "no staging copy is left");
}

#[test]
fn a_copy_that_differs_leaves_the_source_and_no_staging_copy() {
    struct Tamper<'a>(&'a Path);
    impl Progress for Tamper<'_> {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            for entry in fs::read_dir(self.0)? {
                let path = entry?.path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if name.starts_with(".ncrs-partial-") {
                    fs::write(&path, b"changed on the way")?;
                }
            }
            Ok(())
        }
    }
    let dir = scratch("exdev-differs");
    let src = dir.path().join("Company A.pdf");
    fs::write(&src, b"letter").unwrap();
    let target_dir = dir.path().join("new");
    fs::create_dir(&target_dir).unwrap();
    let target = target_dir.join("Agency B.pdf");

    let err = transfer_to(
        &src,
        &target,
        &AcrossDevices,
        &mut Tamper(&target_dir),
        OnConflict::Fail,
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(fs::read(&src).unwrap(), b"letter");
    assert_eq!(fs::read_dir(&target_dir).unwrap().count(), 0);
}

#[test]
fn a_target_that_appeared_during_the_copy_is_not_overwritten() {
    struct Occupy<'a>(&'a Path);
    impl Progress for Occupy<'_> {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            fs::write(self.0, b"someone else")
        }
    }
    let dir = scratch("exdev-appeared");
    let src = dir.path().join("Company A.pdf");
    fs::write(&src, b"letter").unwrap();
    let target = dir.path().join("Agency B.pdf");

    let err = transfer_to(
        &src,
        &target,
        &AcrossDevices,
        &mut Occupy(&target),
        OnConflict::Fail,
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&src).unwrap(), b"letter");
    assert_eq!(fs::read(&target).unwrap(), b"someone else");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn reports_a_conflicting_name_before_starting() {
    let dir = scratch("conflict-name");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(dst.join("src")).unwrap();

    assert_eq!(conflicts(&src, &dst), Some(dst.join("src")));
    assert_eq!(conflicts(&src, &dir.path().join("empty")), None);
}
