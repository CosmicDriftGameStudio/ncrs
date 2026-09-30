//! Copying and moving trees, as one algorithm with two effects.
//!
//! Copy and move differ in one thing: what happens to the source. Everything
//! else — walking, creating directories, reporting progress, noticing when the
//! target is in the way — is the same code, and writing it twice is how the two
//! drift apart. So this is one function and a small effect type:
//!
//! - [`Copy`] leaves the source alone
//! - [`Move`] removes it afterwards
//!
//! That is the review's point that a recursive operation is a generic algorithm
//! and not an operation: `LocalFs` and a future `ArchiveFs` would both drive the
//! same walk.
//!
//! Nothing here is async. The caller runs it on a blocking thread; the queue in
//! `jobs.rs` decides when, and the progress callback is what keeps the window
//! alive.
//!
//! **Not wired up yet.** No key calls this — that is F5/F6, the next task. The
//! parts with no caller are marked with what needs them rather than silenced
//! wholesale, so an `#[allow]` on the module does not also hide a field that
//! does get used later.

use std::io;
use std::path::{Path, PathBuf};

/// What to do with the source after a successful operation.
///
/// Copy and move are the same walk, so they are one function and two of these.
/// `removes_source` is what separates them: a trait cannot ask "would you
/// delete this", so it is a method rather than inferred from the type.
#[allow(dead_code)] // TODO(F5/F6)
pub trait Source {
    /// Whether a completed transfer deletes the source.
    fn removes_source(&self) -> bool;

    /// Deletes the source. Only called when `removes_source` is true.
    fn remove(&self, path: &Path) -> io::Result<()>;
}

/// Leave the source where it is.
#[allow(dead_code)] // TODO(F5)
pub struct Copy;

impl Source for Copy {
    fn removes_source(&self) -> bool {
        false
    }

    fn remove(&self, _path: &Path) -> io::Result<()> {
        // `transfer` only calls this when `removes_source` is true, so this is
        // unreachable by construction. An error rather than a panic: if a
        // future caller gets it wrong, the walk reports "cannot remove" and the
        // source stays, which is what copy means.
        Err(io::Error::other("copy never removes its source"))
    }
}

/// Remove the source once the target exists.
#[allow(dead_code)] // TODO(F6)
pub struct Move;

impl Source for Move {
    fn removes_source(&self) -> bool {
        true
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }
}

/// Reported as the walk proceeds, so the caller can show progress and abort.
#[allow(dead_code)] // TODO(F5/F6)
pub trait Progress {
    /// One more item done. Returning `Err` stops the whole operation with that
    /// error, which is how a cancel arrives.
    fn advanced(&mut self, done: usize, total: usize) -> io::Result<()>;
}

/// Counts items and never stops.
#[allow(dead_code)] // TODO(F5/F6)
pub struct Counting;

impl Progress for Counting {
    fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
        Ok(())
    }
}

/// How the operation behaves when the target already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)] // TODO(F5/F6, with the overwrite dialog)
pub enum OnConflict {
    /// Stop with an error. The default, because overwriting by accident loses
    /// data and the alternative — asking — is the dialog, which is F5's own
    /// decision, not something the filesystem layer should assume.
    #[default]
    Fail,
    /// Replace what is there.
    Overwrite,
}

/// Copies or moves `source` into `target_dir`.
///
/// `total` is the item count for the progress callback, or 0 while it is still
/// unknown — the callback gets the real number on every call.
#[allow(dead_code)] // TODO(F5/F6)
pub fn transfer<S: Source, P: Progress>(
    source: &Path,
    target_dir: &Path,
    effect: &S,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source has no name"))?;
    let target = target_dir.join(name);

    if target.exists() {
        match conflict {
            OnConflict::Fail => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists", target.display()),
                ))
            }
            OnConflict::Overwrite => {
                let meta = std::fs::symlink_metadata(&target)?;
                if meta.is_dir() {
                    std::fs::remove_dir_all(&target)?;
                } else {
                    std::fs::remove_file(&target)?;
                }
            }
        }
    }

    let total = count_items(source);
    let mut done = 0;
    walk(source, &target, progress, &mut done, total)?;

    // Only now, with everything copied, does the source go.
    if effect.removes_source() {
        effect.remove(source)?;
    }
    Ok(done)
}

/// How many items a directory holds, including the directories themselves.
/// A file counts as one.
#[allow(dead_code)] // TODO(F5/F6)
fn count_items(path: &Path) -> usize {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return 1;
    }
    // This directory counts too, the same way the walk counts it — otherwise
    // the total is short by one per level and the bar never fills.
    let Ok(entries) = std::fs::read_dir(path) else {
        return 1;
    };
    1 + entries
        .filter_map(Result::ok)
        .map(|e| count_items(&e.path()))
        .sum::<usize>()
}

/// The recursion, with the effect and the progress applied at each step.
#[allow(dead_code)] // TODO(F5/F6)
fn walk<P: Progress>(
    source: &Path,
    target: &Path,
    progress: &mut P,
    done: &mut usize,
    total: usize,
) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(source)?;

    if meta.is_dir() {
        std::fs::create_dir_all(target)?;
    } else {
        // A symlink is copied as a symlink, not followed: following it would
        // copy whatever it points at, possibly outside the tree the user marked.
        if meta.file_type().is_symlink() {
            let link = std::fs::read_link(source)?;
            symlink(&link, target)?;
        } else {
            std::fs::copy(source, target)?;
        }
    }

    *done += 1;
    // A cancel arrives as an error here and stops the walk.
    progress.advanced(*done, total)?;

    if meta.is_dir() {
        for entry in std::fs::read_dir(source)?.filter_map(Result::ok) {
            let name = entry.file_name();
            walk(&entry.path(), &target.join(name), progress, done, total)?;
        }
    }
    Ok(())
}

/// Creates a symlink, or copies its target on platforms that need privilege.
/// Windows without developer mode cannot create links at all, and a plain copy
/// is the closest honest fallback.
#[allow(dead_code)] // TODO(F5/F6)
fn symlink(original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    {
        // `original` is the link target as stored; for a relative link the
        // target has to be resolved against the link's own directory.
        let resolved = if original.is_absolute() {
            original.to_path_buf()
        } else {
            link.parent()
                .map(|dir| dir.join(original))
                .unwrap_or_else(|| original.to_path_buf())
        };
        match std::fs::copy(&resolved, link) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
                std::os::windows::fs::symlink_file(&resolved, link)
            }
            Err(e) => Err(e),
        }
    }
}

/// Copies or moves one path into `target_dir`, applying the conflict rule.
///
/// The thin wrapper the app calls: it picks the effect from the kind and the
/// progress sink that does nothing, because the app reports through the job
/// queue rather than from inside the walk.
pub fn run(
    source: &Path,
    target_dir: &Path,
    kind: crate::messages::TransferKind,
    conflict: OnConflict,
) -> io::Result<usize> {
    match kind {
        crate::messages::TransferKind::Copy => {
            transfer(source, target_dir, &Copy, &mut Counting, conflict)
        }
        crate::messages::TransferKind::Move => {
            transfer(source, target_dir, &Move, &mut Counting, conflict)
        }
    }
}

/// The names a transfer would create in `target_dir`, so the caller can show
/// them before the dialog and detect a name that is already taken.
#[allow(dead_code)] // TODO(F5/F6, to show a name that is taken)
pub fn conflicts(source: &Path, target_dir: &Path) -> Option<PathBuf> {
    let name = source.file_name()?;
    let target = target_dir.join(name);
    target.exists().then_some(target)
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
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
}
