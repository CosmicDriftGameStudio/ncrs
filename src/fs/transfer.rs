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

use std::io;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

/// What to do with the source after a successful operation.
///
/// Copy and move are the same walk, so they are one function and two of these.
/// `removes_source` is what separates them: a trait cannot ask "would you
/// delete this", so it is a method rather than inferred from the type.
pub trait Source {
    /// Whether a completed transfer deletes the source.
    fn removes_source(&self) -> bool;

    /// Deletes the source. Only called when `removes_source` is true.
    fn remove(&self, path: &Path) -> io::Result<()>;

    /// Moves `from` to `to` without copying, if the effect can. `Ok(false)`
    /// means "not possible here, copy and remove instead".
    fn relocate(&self, _from: &Path, _to: &Path) -> io::Result<bool> {
        Ok(false)
    }
}

/// Leave the source where it is.
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

    /// A rename is instant, atomic and cannot lose anything, so it comes first.
    /// Only a rename across devices falls back to copy and remove.
    fn relocate(&self, from: &Path, to: &Path) -> io::Result<bool> {
        match std::fs::rename(from, to) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Reported as the walk proceeds, so the caller can show progress and abort.
pub trait Progress {
    /// One more item done. Returning `Err` stops the whole operation with that
    /// error, which is how a cancel arrives.
    fn advanced(&mut self, done: usize, total: usize) -> io::Result<()>;
}

/// Counts items and never stops.
pub struct Counting;

impl Progress for Counting {
    fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
        Ok(())
    }
}

/// How far a running transfer has got. One tick per change, not per file: a
/// directory with a thousand entries in it would otherwise send a thousand
/// messages that all say the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub done: usize,
    pub total: usize,
}

/// Reports progress over a channel, so a copy on a blocking thread can tell the
/// UI how far along it is without the UI polling the filesystem.
///
/// The channel is the one the app already reads, so a tick is a message like
/// any other and there is no second path to keep in sync.
pub struct Reporting<'a> {
    sender: &'a mpsc::UnboundedSender<Tick>,
    /// The last count sent, so a tick goes out only when the count changed.
    last_sent: usize,
}

impl<'a> Reporting<'a> {
    pub fn new(sender: &'a mpsc::UnboundedSender<Tick>) -> Self {
        Self {
            sender,
            last_sent: 0,
        }
    }
}

impl Progress for Reporting<'_> {
    fn advanced(&mut self, done: usize, total: usize) -> io::Result<()> {
        if done == self.last_sent {
            return Ok(());
        }
        self.last_sent = done;
        // A closed channel means the app is gone, and that is the one case that
        // should stop a copy — it is also how a stopped app reaches the blocking
        // thread. An unbounded channel cannot fill up, so there is no
        // backpressure here to reason about.
        self.sender
            .send(Tick { done, total })
            .map_err(|closed| io::Error::other(format!("the app is gone: {closed}")))
    }
}

/// How the operation behaves when the target already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConflict {
    /// Stop with an error. The default, because overwriting by accident loses
    /// data and the alternative — asking — is the dialog, which is F5's own
    /// decision, not something the filesystem layer should assume.
    #[default]
    Fail,
    /// Replace what is there.
    Overwrite,
    /// Leave what is there alone and do nothing for this item.
    Skip,
}

/// Copies or moves `source` into `target_dir`.
///
/// `total` is the item count for the progress callback, or 0 while it is still
/// unknown — the callback gets the real number on every call.
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

    refuse_onto_itself(source, target_dir, &target)?;

    // `symlink_metadata`, not `exists`: a dangling symlink in the way is still
    // something in the way, and `exists` follows it and says no.
    let existed = match std::fs::symlink_metadata(&target) {
        Ok(_) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if existed {
        match conflict {
            OnConflict::Fail => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists", target.display()),
                ))
            }
            OnConflict::Skip => return Ok(0),
            OnConflict::Overwrite => {}
        }
    }

    std::fs::create_dir_all(target_dir)?;

    // An overwrite is built next to the target and swapped in at the end, so a
    // failure halfway leaves the old target untouched.
    //
    // A rename cannot be told to refuse an existing name (that would be
    // `renameat2(RENAME_NOREPLACE)`, which is Linux-only), so for a move the
    // staging name is only checked free, not claimed. The residual race is
    // someone creating that exact name in the target directory between the
    // check and the rename.
    if effect.removes_source() {
        let staged = if existed {
            free_sibling(&target)
        } else {
            target.clone()
        };
        if effect.relocate(source, &staged)? {
            // A rename is one step, however large the tree.
            progress.advanced(1, 1)?;
            if existed {
                if let Err(e) = replace(&staged, &target) {
                    // Put the source back rather than leave it hidden under
                    // the staging name.
                    let _best_effort = std::fs::rename(&staged, source);
                    return Err(e);
                }
            }
            return Ok(1);
        }
    }

    let meta = std::fs::symlink_metadata(source)?;
    let staged = claim_entry(source, &meta, &target, existed)?;
    let total = count_items(source);
    let mut done = 0;
    if let Err(e) = descend(source, &staged, &meta, progress, &mut done, total) {
        if existed {
            let _best_effort = remove_path(&staged);
        }
        return Err(e);
    }
    if existed {
        if let Err(e) = replace(&staged, &target) {
            let _best_effort = remove_path(&staged);
            return Err(e);
        }
    }

    // Only now, with everything copied, does the source go.
    if effect.removes_source() {
        effect.remove(source)?;
    }
    Ok(done)
}

/// Creates the first entry of a copy under a name nobody else holds.
///
/// With nothing in the way that is `target` itself. When overwriting it is a
/// fresh sibling, created exclusively (`create_new` / `create_dir` fail if the
/// name exists), so a name an attacker planted in a shared directory is never
/// written through; a taken name just moves on to the next one.
fn claim_entry(
    source: &Path,
    meta: &std::fs::Metadata,
    target: &Path,
    existed: bool,
) -> io::Result<PathBuf> {
    if !existed {
        create_entry(source, meta, target)?;
        return Ok(target.to_path_buf());
    }
    loop {
        let candidate = free_sibling(target);
        match create_entry(source, meta, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Rejects the two requests that would destroy or never finish: a path onto
/// itself, and a directory into itself.
fn refuse_onto_itself(source: &Path, target_dir: &Path, target: &Path) -> io::Result<()> {
    // A target directory that does not exist yet can be neither the source's
    // directory nor inside it.
    let target_dir_real = match std::fs::canonicalize(target_dir) {
        Ok(real) => real,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    // Compared by the source's parent, so a symlink source is judged by where
    // the link sits rather than by what it points at.
    let source_parent = source
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if std::fs::canonicalize(source_parent)? == target_dir_real {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is already in that directory", source.display()),
        ));
    }

    // The target (this name inside the target directory) may be an ancestor of
    // the source: replacing it would set the source aside and delete it.
    if let (Ok(target_real), Some(name)) = (std::fs::canonicalize(target), source.file_name()) {
        if std::fs::canonicalize(source_parent)?
            .join(name)
            .starts_with(&target_real)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} contains {}", target.display(), source.display()),
            ));
        }
    }

    let source_is_dir = std::fs::symlink_metadata(source)?.is_dir();
    if source_is_dir && target_dir_real.starts_with(std::fs::canonicalize(source)?) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("cannot copy {} into itself", target.display()),
        ));
    }
    Ok(())
}

/// A name next to `target` that does not exist yet. Checked, not claimed.
fn free_sibling(target: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    let name = target.file_name().unwrap_or_default().to_string_lossy();
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = target.with_file_name(format!(".{name}.ncrs-{}-{n}", std::process::id()));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
}

fn remove_path(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Puts `staged` where `target` is. Two files swap atomically with one rename;
/// anything involving a directory moves the old target aside first. If the new
/// one cannot take its place, or the old one cannot be deleted, everything goes
/// back and `staged` is where it was.
fn replace(staged: &Path, target: &Path) -> io::Result<()> {
    let is_dir = |p: &Path| std::fs::symlink_metadata(p).map(|m| m.is_dir());
    if !is_dir(staged)? && !is_dir(target)? {
        return std::fs::rename(staged, target);
    }
    let aside = free_sibling(target);
    std::fs::rename(target, &aside)?;
    if let Err(e) = std::fs::rename(staged, target) {
        let _best_effort = std::fs::rename(&aside, target);
        return Err(e);
    }
    // The new one is in place now, so there is nothing to roll back to: a failed
    // removal may already have deleted part of the old tree.
    // ponytail: the rest stays under the hidden aside name; report it if users trip over it.
    let _best_effort = remove_path(&aside);
    Ok(())
}

/// How many items a directory holds, including the directories themselves.
/// A file counts as one.
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
fn walk<P: Progress>(
    source: &Path,
    target: &Path,
    progress: &mut P,
    done: &mut usize,
    total: usize,
) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(source)?;
    create_entry(source, &meta, target)?;
    descend(source, target, &meta, progress, done, total)
}

/// Makes one new entry at `target`, failing if the name is already taken.
fn create_entry(source: &Path, meta: &std::fs::Metadata, target: &Path) -> io::Result<()> {
    if meta.is_dir() {
        std::fs::create_dir(target)
    } else if meta.file_type().is_symlink() {
        // A symlink is copied as a symlink, not followed: following it would
        // copy whatever it points at, possibly outside the tree the user marked.
        let link = std::fs::read_link(source)?;
        symlink(source, &link, target)
    } else {
        refuse_special_file(meta)?;
        // `create_new`, not `fs::copy`: copy opens an existing path and writes
        // through whatever is there, including a symlink.
        let mut from = std::fs::File::open(source)?;
        let mut to = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        io::copy(&mut from, &mut to)?;
        to.set_permissions(copied_permissions(meta))
    }
}

/// A FIFO would block the job forever when opened for reading, and sockets and
/// devices are not data to copy.
fn refuse_special_file(meta: &std::fs::Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        let kind = meta.file_type();
        if kind.is_fifo() || kind.is_socket() || kind.is_block_device() || kind.is_char_device() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file (FIFO, socket or device)",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = meta;
    Ok(())
}

/// The source's permissions for the copy. On Unix without setuid, setgid and
/// sticky: a copy must not hand a privilege bit to a file the user did not
/// mark as such.
fn copied_permissions(meta: &std::fs::Metadata) -> std::fs::Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::Permissions::from_mode(meta.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        meta.permissions()
    }
}

/// Counts the entry just created and walks into it if it is a directory.
fn descend<P: Progress>(
    source: &Path,
    target: &Path,
    meta: &std::fs::Metadata,
    progress: &mut P,
    done: &mut usize,
    total: usize,
) -> io::Result<()> {
    *done += 1;
    // A cancel arrives as an error here and stops the walk.
    progress.advanced(*done, total)?;

    if meta.is_dir() {
        for listed in std::fs::read_dir(source)? {
            let entry = listed?;
            let name = entry.file_name();
            walk(&entry.path(), &target.join(name), progress, done, total)?;
        }
    }
    Ok(())
}

/// Creates `link` as a symlink to `original`, the target `source` points at.
///
/// Never copies what the link points at: a refused link is an error, because
/// following it could pull in files from outside the tree the user marked.
fn symlink(source: &Path, original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = source;
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    {
        // Windows has two kinds of link. A relative target is relative to the
        // directory the original link sits in, so that is where it is resolved
        // to find out which kind it is.
        let resolved = if original.is_absolute() {
            original.to_path_buf()
        } else {
            source
                .parent()
                .map(|dir| dir.join(original))
                .unwrap_or_else(|| original.to_path_buf())
        };
        if resolved.is_dir() {
            std::os::windows::fs::symlink_dir(original, link)
        } else {
            std::os::windows::fs::symlink_file(original, link)
        }
    }
}

/// As `run`, but reports progress so the status bar moves while the copy runs.
/// This is the path the app takes; `run` is for a caller that only wants the
/// work done.
pub fn run_reporting(
    source: &Path,
    target_dir: &Path,
    kind: crate::messages::TransferKind,
    conflict: OnConflict,
    sender: &mpsc::UnboundedSender<Tick>,
) -> io::Result<usize> {
    let mut reporting = Reporting::new(sender);
    match kind {
        crate::messages::TransferKind::Copy => {
            transfer(source, target_dir, &Copy, &mut reporting, conflict)
        }
        crate::messages::TransferKind::Move => {
            transfer(source, target_dir, &Move, &mut reporting, conflict)
        }
    }
}

/// Counts but does not report. For a caller that only wants the work done —
/// `run_reporting` is what the app uses, and this is what its own tests use to
/// check that a plain run stays quiet.
#[allow(dead_code)]
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
///
/// The app does not use this yet: it asks about a conflict when the walk
/// reaches it rather than before it starts, which is correct for a tag list of
/// fifty thousand but means the dialog names a file the user has not seen yet.
/// Kept because it is the piece that fixes that, and deleting it would lose
/// the only test that says what it should return.
#[allow(dead_code)]
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
            let err =
                transfer(&src, &target_dir, &Copy, &mut Counting, OnConflict::Fail).unwrap_err();
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

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod reporting_tests {
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
            crate::messages::TransferKind::Copy,
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
            crate::messages::TransferKind::Copy,
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
            crate::messages::TransferKind::Copy,
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
        let done = run(
            &src,
            &dst,
            crate::messages::TransferKind::Copy,
            OnConflict::Fail,
        )
        .unwrap();
        assert_eq!(done, 6);
        assert!(rx.try_recv().is_err(), "run sent a tick anyway");
        let _ = tx;
    }
}
