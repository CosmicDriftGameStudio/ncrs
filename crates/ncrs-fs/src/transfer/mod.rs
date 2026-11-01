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

use super::archive;

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
#[derive(Debug)]
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
#[derive(Debug)]
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

    /// Asked between the blocks of one large file, where no item completes for
    /// a long time. `Err` stops the operation like a failed `advanced`.
    fn keep_going(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Counts items and never stops.
#[derive(Debug)]
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
#[derive(Debug)]
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

    fn keep_going(&mut self) -> io::Result<()> {
        if self.sender.is_closed() {
            return Err(io::Error::other("the app is gone"));
        }
        Ok(())
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

/// Copies or moves `source` into `target_dir`, keeping its name.
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
    place(source, &target, effect, progress, conflict)
}

/// Copies or moves `source` to exactly `target`, which may have another name
/// and may be in the same folder (a rename). Refuses a target that is the
/// source, an ancestor of it, or inside it.
pub fn transfer_to<S: Source, P: Progress>(
    source: &Path,
    target: &Path,
    effect: &S,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    refuse_target_in_the_way(source, target)?;
    place(source, target, effect, progress, conflict)
}

fn refuse_target_in_the_way(source: &Path, target: &Path) -> io::Result<()> {
    let refused = |text: String| Err(io::Error::new(io::ErrorKind::InvalidInput, text));
    if target == source {
        return refused(format!("{} is already there", source.display()));
    }
    if source.starts_with(target) {
        return refused(format!(
            "{} contains {}",
            target.display(),
            source.display()
        ));
    }
    if std::fs::symlink_metadata(source)?.is_dir() && target.starts_with(source) {
        return refused(format!("cannot copy {} into itself", target.display()));
    }
    Ok(())
}

fn place<S: Source, P: Progress>(
    source: &Path,
    target: &Path,
    effect: &S,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    // `symlink_metadata`, not `exists`: a dangling symlink in the way is still
    // something in the way, and `exists` follows it and says no.
    let existed = match std::fs::symlink_metadata(target) {
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

    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

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
            free_sibling(target)
        } else {
            target.to_path_buf()
        };
        if effect.relocate(source, &staged)? {
            // A rename is one step, however large the tree.
            progress.advanced(1, 1)?;
            if existed {
                if let Err(e) = replace(&staged, target) {
                    // Put the source back rather than leave it hidden under
                    // the staging name.
                    let _best_effort = std::fs::rename(&staged, source);
                    return Err(e);
                }
            }
            return Ok(1);
        }
        return copy_then_remove(source, target, existed, effect, progress);
    }

    let meta = std::fs::symlink_metadata(source)?;
    let staged = claim_entry(source, &meta, target, existed)?;
    let total = count_items(source);
    let mut done = 0;
    if let Err(e) = descend(source, &staged, &meta, progress, &mut done, total) {
        if existed {
            let _best_effort = remove_path(&staged);
        }
        return Err(e);
    }
    if existed {
        if let Err(e) = replace(&staged, target) {
            let _best_effort = remove_path(&staged);
            return Err(e);
        }
    }
    Ok(done)
}

/// `false` when the item is to be skipped.
pub fn resolve_conflict(target: &Path, conflict: OnConflict) -> io::Result<bool> {
    match std::fs::symlink_metadata(target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
        Ok(_) => match conflict {
            OnConflict::Fail => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", target.display()),
            )),
            OnConflict::Skip => Ok(false),
            OnConflict::Overwrite => Ok(true),
        },
    }
}

/// The prefix of the hidden name a move across devices copies to first. A
/// leftover of an interrupted move is found by it.
pub fn partial_prefix(target: &Path) -> String {
    format!(
        ".ncrs-partial-{}-",
        target.file_name().unwrap_or_default().to_string_lossy()
    )
}

/// A move that cannot rename. The copy is made under a hidden name next to the
/// target and compared with the source; only a copy that matches is renamed to
/// the target, and only then does the source go. A failure at any step removes
/// the hidden copy and leaves the source alone.
fn copy_then_remove<S: Source, P: Progress>(
    source: &Path,
    target: &Path,
    existed: bool,
    effect: &S,
    progress: &mut P,
) -> io::Result<usize> {
    let meta = std::fs::symlink_metadata(source)?;
    let staged = claim_partial(source, &meta, target)?;
    let total = count_items(source);
    let mut done = 0;
    let finished = descend(source, &staged, &meta, progress, &mut done, total)
        .and_then(|()| verify_copy(source, &staged))
        .and_then(|()| {
            if existed {
                replace(&staged, target)
            } else {
                claim_free_target(&staged, target)
            }
        });
    if let Err(e) = finished {
        let _best_effort = remove_path(&staged);
        return Err(e);
    }
    effect.remove(source)?;
    Ok(done)
}

/// Renames `staged` to `target` after checking once more that nothing is there.
fn claim_free_target(staged: &Path, target: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(target) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => std::fs::rename(staged, target),
        Err(e) => Err(e),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        )),
    }
}

/// Creates the first entry of a copy under a hidden name nobody else holds.
fn claim_partial(source: &Path, meta: &std::fs::Metadata, target: &Path) -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = target.with_file_name(format!(
            "{}{}-{n}",
            partial_prefix(target),
            std::process::id()
        ));
        match create_entry(source, meta, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Whether `copy` holds exactly what `source` holds: the same names, the same
/// links and the same bytes in every regular file.
fn verify_copy(source: &Path, copy: &Path) -> io::Result<()> {
    let differs = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the copy of {} differs from the original", source.display()),
        )
    };
    let from = std::fs::symlink_metadata(source)?;
    let to = std::fs::symlink_metadata(copy)?;
    if from.is_dir() {
        if !to.is_dir() {
            return Err(differs());
        }
        let names = |dir: &Path| -> io::Result<Vec<std::ffi::OsString>> {
            let mut names = std::fs::read_dir(dir)?
                .map(|entry| entry.map(|found| found.file_name()))
                .collect::<io::Result<Vec<_>>>()?;
            names.sort();
            Ok(names)
        };
        let listed = names(source)?;
        if listed != names(copy)? {
            return Err(differs());
        }
        for name in listed {
            verify_copy(&source.join(&name), &copy.join(&name))?;
        }
        Ok(())
    } else if from.file_type().is_symlink() {
        if to.file_type().is_symlink() && std::fs::read_link(source)? == std::fs::read_link(copy)? {
            Ok(())
        } else {
            Err(differs())
        }
    } else if to.is_file() && from.len() == to.len() && same_bytes(source, copy)? {
        Ok(())
    } else {
        Err(differs())
    }
}

fn same_bytes(left: &Path, right: &Path) -> io::Result<bool> {
    use io::Read as _;
    let mut left_reader = io::BufReader::new(std::fs::File::open(left)?);
    let mut right_reader = io::BufReader::new(std::fs::File::open(right)?);
    let mut left_buffer: [u8; 64 * 1024] = [0; 64 * 1024];
    let mut right_buffer: [u8; 64 * 1024] = [0; 64 * 1024];
    loop {
        let read = left_reader.read(&mut left_buffer)?;
        if read == 0 {
            return Ok(right_reader.read(&mut right_buffer)? == 0);
        }
        let Some(expected) = left_buffer.get(..read) else {
            return Ok(false);
        };
        let mut got = 0;
        while got < read {
            let Some(room) = right_buffer.get_mut(got..read) else {
                return Ok(false);
            };
            let n = right_reader.read(room)?;
            if n == 0 {
                return Ok(false);
            }
            got += n;
        }
        if right_buffer.get(..read) != Some(expected) {
            return Ok(false);
        }
    }
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
    kind: ncrs_core::TransferKind,
    conflict: OnConflict,
    sender: &mpsc::UnboundedSender<Tick>,
) -> io::Result<usize> {
    let mut reporting = Reporting::new(sender);
    if let Some(result) = out_of_archive(source, target_dir, kind, &mut reporting, conflict) {
        return result;
    }
    match kind {
        ncrs_core::TransferKind::Copy => {
            transfer(source, target_dir, &Copy, &mut reporting, conflict)
        }
        ncrs_core::TransferKind::Move => {
            transfer(source, target_dir, &Move, &mut reporting, conflict)
        }
    }
}

/// As `run_reporting`, but the source lands at exactly `target`, under another
/// name than its own. Only for sources and targets on this computer's disk.
pub fn run_reporting_as(
    source: &Path,
    target: &Path,
    kind: ncrs_core::TransferKind,
    conflict: OnConflict,
    sender: &mpsc::UnboundedSender<Tick>,
) -> io::Result<usize> {
    let mut reporting = Reporting::new(sender);
    match kind {
        ncrs_core::TransferKind::Copy => {
            transfer_to(source, target, &Copy, &mut reporting, conflict)
        }
        ncrs_core::TransferKind::Move => {
            transfer_to(source, target, &Move, &mut reporting, conflict)
        }
    }
}

/// A source that is not on disk but is a path inside an archive is copied out
/// of it. `None` for every other source.
fn out_of_archive<P: Progress>(
    source: &Path,
    target_dir: &Path,
    kind: ncrs_core::TransferKind,
    progress: &mut P,
    conflict: OnConflict,
) -> Option<io::Result<usize>> {
    if std::fs::symlink_metadata(source).is_ok() || archive::locate_in_archive(source).is_none() {
        return None;
    }
    Some(match kind {
        ncrs_core::TransferKind::Copy => archive::copy_out(source, target_dir, progress, conflict),
        ncrs_core::TransferKind::Move => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "moving out of an archive is not supported; copy instead",
        )),
    })
}

/// Counts but does not report. For a caller that only wants the work done —
/// `run_reporting` is what the app uses, and this is what its own tests use to
/// check that a plain run stays quiet.
#[allow(dead_code)]
pub fn run(
    source: &Path,
    target_dir: &Path,
    kind: ncrs_core::TransferKind,
    conflict: OnConflict,
) -> io::Result<usize> {
    if let Some(result) = out_of_archive(source, target_dir, kind, &mut Counting, conflict) {
        return result;
    }
    match kind {
        ncrs_core::TransferKind::Copy => {
            transfer(source, target_dir, &Copy, &mut Counting, conflict)
        }
        ncrs_core::TransferKind::Move => {
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
mod tests;

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod reporting_tests;
