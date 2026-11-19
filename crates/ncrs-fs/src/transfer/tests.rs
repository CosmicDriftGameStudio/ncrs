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
fn a_file_with_a_very_long_name_is_copied() {
    let dir = scratch("long-name");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    let name = format!("{}.txt", "ä".repeat(125));
    assert_eq!(name.len(), 254);
    fs::write(src.join(&name), b"long").unwrap();

    transfer(
        &src.join(&name),
        &dst,
        &Copy,
        &mut Recorder::default(),
        OnConflict::Fail,
    )
    .unwrap();

    assert_eq!(fs::read(dst.join(&name)).unwrap(), b"long");
}

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

/// Overwriting a folder merges: files with the same name are replaced, what
/// only the target holds stays, and no scratch name is left.
#[test]
fn an_overwrite_of_a_folder_keeps_what_only_the_target_holds() {
    for kind in [ncrs_core::TransferKind::Copy, ncrs_core::TransferKind::Move] {
        let dir = scratch("merge");
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("same.txt"), b"new").unwrap();
        fs::write(src.join("sub/inner.txt"), b"new inner").unwrap();
        fs::create_dir_all(dst.join("src/sub")).unwrap();
        fs::write(dst.join("src/same.txt"), b"old").unwrap();
        fs::write(dst.join("src/only-target.txt"), b"keep me").unwrap();
        fs::write(dst.join("src/sub/only-target-inner.txt"), b"keep me too").unwrap();

        match kind {
            ncrs_core::TransferKind::Copy => {
                transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Overwrite).unwrap()
            }
            ncrs_core::TransferKind::Move => {
                transfer(&src, &dst, &Move, &mut Counting, OnConflict::Overwrite).unwrap()
            }
        };

        assert_eq!(fs::read(dst.join("src/same.txt")).unwrap(), b"new");
        assert_eq!(
            fs::read(dst.join("src/sub/inner.txt")).unwrap(),
            b"new inner"
        );
        assert_eq!(
            fs::read(dst.join("src/only-target.txt")).unwrap(),
            b"keep me"
        );
        assert_eq!(
            fs::read(dst.join("src/sub/only-target-inner.txt")).unwrap(),
            b"keep me too"
        );
        assert_eq!(
            fs::read_dir(&dst).unwrap().count(),
            1,
            "a scratch name is left"
        );
        assert_eq!(fs::read_dir(dst.join("src")).unwrap().count(), 3);
        // A move empties the source and removes it; a copy leaves it whole.
        assert_eq!(src.exists(), kind == ncrs_core::TransferKind::Copy);
    }
}

/// A file against a folder, in either direction, is refused and both survive.
#[test]
fn a_file_and_a_folder_do_not_replace_each_other() {
    for source_is_dir in [false, true] {
        let dir = scratch("in-the-way");
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        if source_is_dir {
            fs::create_dir_all(&src).unwrap();
            fs::write(src.join("new.txt"), b"new").unwrap();
            fs::write(dst.join("src"), b"old").unwrap();
        } else {
            fs::write(&src, b"new").unwrap();
            fs::create_dir_all(dst.join("src")).unwrap();
            fs::write(dst.join("src/old.txt"), b"old").unwrap();
        }

        let err = transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Overwrite).unwrap_err();

        let expected = if source_is_dir {
            io::ErrorKind::NotADirectory
        } else {
            io::ErrorKind::IsADirectory
        };
        assert_eq!(err.kind(), expected);
        assert!(src.exists());
        assert_eq!(fs::read_dir(&dst).unwrap().count(), 1);
    }
}

/// A file onto a file is replaced by a rename, whichever way it got there.
#[test]
fn a_file_overwrites_a_file_for_copy_and_move() {
    let dir = scratch("file-over-file");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    for (n, effect_moves) in [(0, false), (1, true)] {
        let src = dir.path().join(format!("f{n}.txt"));
        fs::write(&src, b"new").unwrap();
        fs::write(dst.join(format!("f{n}.txt")), b"old").unwrap();
        if effect_moves {
            transfer(&src, &dst, &Move, &mut Counting, OnConflict::Overwrite).unwrap();
        } else {
            transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Overwrite).unwrap();
        }
        assert_eq!(fs::read(dst.join(format!("f{n}.txt"))).unwrap(), b"new");
        assert_eq!(src.exists(), !effect_moves);
    }
    assert_eq!(
        fs::read_dir(&dst).unwrap().count(),
        2,
        "a scratch name is left"
    );
}

/// A cancel arriving after the source went to a hidden name, as it used to
/// during a move over an existing file, loses nothing.
#[test]
fn a_cancel_during_a_move_over_a_file_loses_no_content() {
    let dir = scratch("cancel-move");
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    fs::write(&src, b"new").unwrap();
    fs::write(dst.join("a.txt"), b"old").unwrap();

    let mut progress = Cancelled { after: 1, seen: 0 };
    let _ = transfer(&src, &dst, &Move, &mut progress, OnConflict::Overwrite);

    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .chain(fs::read_dir(&dst).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|name| !name.contains(".ncrs-")),
        "a hidden name is left: {names:?}"
    );
    let kept = fs::read(dst.join("a.txt")).unwrap();
    assert!(
        kept == b"old" || kept == b"new",
        "the target lost its content"
    );
    assert!(
        src.exists() || kept == b"new",
        "the moved content is nowhere"
    );
}

/// A second hard link to the file is not replaced with the file itself.
#[test]
fn a_target_that_is_the_same_file_is_refused_and_keeps_its_content() {
    let dir = scratch("same-file");
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    fs::write(&src, b"data").unwrap();
    fs::hard_link(&src, dst.join("a.txt")).unwrap();

    let err = transfer(&src, &dst, &Move, &mut Counting, OnConflict::Overwrite).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(fs::read(&src).unwrap(), b"data");
    assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"data");
}

/// A copy never shows half a file under its real name: the data is written
/// under a hidden name and renamed when complete.
#[test]
fn a_cancelled_copy_leaves_nothing_under_the_real_name() {
    let dir = scratch("cancel-copy");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    tree(&src);
    fs::create_dir_all(&dst).unwrap();

    let mut progress = Cancelled { after: 3, seen: 0 };
    transfer(&src, &dst, &Copy, &mut progress, OnConflict::Fail).unwrap_err();

    assert_eq!(
        fs::read_dir(&dst).unwrap().count(),
        0,
        "a half copy was left"
    );
}

/// The file is written in blocks and the channel is asked between them, so a
/// large file stops soon after a cancel.
#[test]
fn a_cancel_stops_inside_a_large_file() {
    struct StopAtOnce;
    impl Progress for StopAtOnce {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            Ok(())
        }
        fn keep_going(&mut self) -> io::Result<()> {
            Err(io::Error::other("cancelled"))
        }
    }
    let dir = scratch("cancel-big");
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    fs::write(&src, vec![7_u8; 3 * 1024 * 1024]).unwrap();

    let err = transfer(&src, &dst, &Copy, &mut StopAtOnce, OnConflict::Fail).unwrap_err();

    assert_eq!(err.to_string(), "cancelled");
    assert_eq!(fs::read_dir(&dst).unwrap().count(), 0);
}

/// The copy of a private file is never readable by others, and a folder
/// keeps its permissions.
#[cfg(unix)]
#[test]
fn a_copy_keeps_private_modes_of_files_and_folders() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("modes");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("secret"), b"s").unwrap();
    fs::set_permissions(src.join("secret"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir_all(&dst).unwrap();

    transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Fail).unwrap();

    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dst.join("src")), 0o700);
    assert_eq!(mode(&dst.join("src/secret")), 0o600);
}

/// A link that replaced the file since the listing is not followed.
#[cfg(unix)]
#[test]
fn a_source_that_became_a_link_is_not_followed() {
    let dir = scratch("nofollow");
    let secret = dir.path().join("secret.txt");
    fs::write(&secret, b"secret").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let file_meta = fs::metadata(&secret).unwrap();

    let err = create_entry(&link, &file_meta, &dir.path().join("out"), &mut Counting).unwrap_err();

    assert!(err.kind() != io::ErrorKind::NotFound, "{err}");
    assert!(!dir.path().join("out").exists());
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

    let err = create_entry(
        &src,
        &fs::symlink_metadata(&src).unwrap(),
        &planted,
        &mut Counting,
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&victim).unwrap(), b"untouched");
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

/// What a download marks a file with (the macOS quarantine, Finder tags) must
/// follow the copy, or the copy opens without the check the original has.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_copy_carries_the_extended_attributes() {
    let dir = scratch("xattr");
    let src = dir.path().join("a.txt");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&dst).unwrap();
    fs::write(&src, b"a").unwrap();
    let name = if cfg!(target_os = "macos") {
        "com.apple.quarantine"
    } else {
        "user.ncrs-test"
    };
    if rustix::fs::lsetxattr(
        &src,
        name,
        b"0081;5f3a;Safari",
        rustix::fs::XattrFlags::empty(),
    )
    .is_err()
    {
        // This file system holds no attributes.
        return;
    }

    transfer(&src, &dst, &Copy, &mut Counting, OnConflict::Fail).unwrap();

    let mut value = [0_u8; 64];
    let got = rustix::fs::lgetxattr(dst.join("a.txt"), name, &mut value[..]).unwrap();
    assert_eq!(value.get(..got), Some(&b"0081;5f3a;Safari"[..]));
}

/// The same file reached through a link to its folder is still the same file.
#[cfg(unix)]
#[test]
fn a_target_that_is_the_source_by_another_route_is_refused() {
    let dir = scratch("alias");
    let real = dir.path().join("real");
    fs::create_dir_all(&real).unwrap();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let src = real.join("a.txt");
    fs::write(&src, b"x").unwrap();

    let err = transfer_to(
        &src,
        &alias.join("a.txt"),
        &Copy,
        &mut Counting,
        OnConflict::Overwrite,
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(fs::read(&src).unwrap(), b"x");
}
