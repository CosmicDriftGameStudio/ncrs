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
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::archive;

mod place;
mod skipped;
mod sweep;
#[cfg(all(test, unix))]
use place::create_entry;
use place::place;
pub use place::same_bytes;
pub use skipped::{SkipKind, Skipped};
#[cfg(any(test, feature = "test-support"))]
pub use sweep::let_time_pass_for_sweep;
pub use sweep::{is_staging_name, sweep_partials};

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

    /// As `relocate`, for a `to` that is a file or link to be replaced by a
    /// file or link. Never called for a folder.
    fn relocate_replacing(&self, _from: &Path, _to: &Path) -> io::Result<bool> {
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
    /// Only a rename across devices falls back to copy and remove. It refuses a
    /// name that is taken, so a file that appeared since the check survives.
    fn relocate(&self, from: &Path, to: &Path) -> io::Result<bool> {
        match crate::place_file::rename_no_replace(from, to) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn relocate_replacing(&self, from: &Path, to: &Path) -> io::Result<bool> {
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

    /// Something was left out of the copy while the rest went on. The default
    /// drops it; `Reporting` collects it for the caller.
    fn skipped(&mut self, _skipped: Skipped) {}
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
    /// When the last tick went out. Between two ticks at least `TICK_EVERY`
    /// lie, except for the one that finishes the run.
    last_sent_at: Option<Instant>,
    skipped: Vec<Skipped>,
}

/// A hundred thousand small files would otherwise be a hundred thousand
/// messages for a window that can show ten changes a second.
const TICK_EVERY: Duration = Duration::from_millis(100);

impl<'a> Reporting<'a> {
    pub fn new(sender: &'a mpsc::UnboundedSender<Tick>) -> Self {
        Self {
            sender,
            last_sent: 0,
            last_sent_at: None,
            skipped: Vec::new(),
        }
    }

    /// What the run left out, in the order it met it. Empties the list.
    pub fn take_skipped(&mut self) -> Vec<Skipped> {
        std::mem::take(&mut self.skipped)
    }
}

impl Progress for Reporting<'_> {
    fn advanced(&mut self, done: usize, total: usize) -> io::Result<()> {
        if done == self.last_sent {
            return Ok(());
        }
        let finishing = total > 0 && done >= total;
        let due = finishing
            || self
                .last_sent_at
                .is_none_or(|sent| sent.elapsed() >= TICK_EVERY);
        if !due {
            return self.keep_going();
        }
        self.last_sent = done;
        self.last_sent_at = Some(Instant::now());
        // A closed channel means the app is gone, and that is the one case that
        // should stop a copy — it is also how a stopped app reaches the blocking
        // thread.
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

    fn skipped(&mut self, skipped: Skipped) {
        self.skipped.push(skipped);
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
    /// Replace what is there. A folder onto a folder is merged: files with the
    /// same name are replaced, everything else in the target stays.
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
    refuse_same_entry(source, &target)?;
    sweep_partials(target_dir);
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
    if let Some(folder) = target.parent() {
        sweep_partials(folder);
    }
    place(source, target, effect, progress, conflict)
}

/// `path` with its folder resolved (links, `..`, the spelling the file system
/// keeps) and its own name left alone, so a link as the entry itself is not
/// followed. A folder that does not exist yet is left as given.
fn with_resolved_folder(path: &Path) -> PathBuf {
    match (
        path.parent().filter(|p| !p.as_os_str().is_empty()),
        path.file_name(),
    ) {
        (Some(folder), Some(name)) => std::fs::canonicalize(folder)
            .map_or_else(|_| path.to_path_buf(), |real| real.join(name)),
        _ => path.to_path_buf(),
    }
}

/// Another spelling of the same entry (`Foo` and `foo` on a case-blind file
/// system, a second hard link) would be merged or replaced with itself. Links
/// are not followed: one that points at the other is a different entry.
fn refuse_same_entry(source: &Path, target: &Path) -> io::Result<()> {
    let is_plain = |path: &Path| {
        std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.file_type().is_symlink())
    };
    if is_plain(source)
        && is_plain(target)
        && same_file::is_same_file(source, target).unwrap_or(false)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is already there", source.display()),
        ));
    }
    Ok(())
}

fn refuse_target_in_the_way(given_source: &Path, given_target: &Path) -> io::Result<()> {
    let refused = |text: String| Err(io::Error::new(io::ErrorKind::InvalidInput, text));
    let (resolved_source, resolved_target) = (
        with_resolved_folder(given_source),
        with_resolved_folder(given_target),
    );
    let (source, target) = (resolved_source.as_path(), resolved_target.as_path());
    if target == source {
        return refused(format!("{} is already there", source.display()));
    }
    refuse_same_entry(source, target)?;
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
    /// A file name may be 255 bytes; the prefix and the counters come on top
    /// of the name, so a long name is cut to leave room for them.
    const NAME_BYTES: usize = 100;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let mut end = name.len().min(NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!(".ncrs-partial-{}-", name.get(..end).unwrap_or_default())
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
    run_with(
        source,
        target_dir,
        kind,
        conflict,
        &mut Reporting::new(sender),
    )
}

/// As `run_reporting`, with the caller's own `Progress`, so it can read what
/// the run left out ([`Reporting::take_skipped`]) afterwards.
pub fn run_with<P: Progress>(
    source: &Path,
    target_dir: &Path,
    kind: ncrs_core::TransferKind,
    conflict: OnConflict,
    progress: &mut P,
) -> io::Result<usize> {
    if let Some(result) = out_of_archive(source, target_dir, kind, progress, conflict) {
        return result;
    }
    match kind {
        ncrs_core::TransferKind::Copy => transfer(source, target_dir, &Copy, progress, conflict),
        ncrs_core::TransferKind::Move => transfer(source, target_dir, &Move, progress, conflict),
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
    run_as_with(source, target, kind, conflict, &mut Reporting::new(sender))
}

/// As `run_reporting_as`, with the caller's own `Progress`.
pub fn run_as_with<P: Progress>(
    source: &Path,
    target: &Path,
    kind: ncrs_core::TransferKind,
    conflict: OnConflict,
    progress: &mut P,
) -> io::Result<usize> {
    match kind {
        ncrs_core::TransferKind::Copy => transfer_to(source, target, &Copy, progress, conflict),
        ncrs_core::TransferKind::Move => transfer_to(source, target, &Move, progress, conflict),
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

/// Counts but does not report; tests use it to check that a plain run stays quiet.
#[cfg(test)]
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reporting_tests;
