//! Changing the permissions of files and folders, one level or all the way down.
//! Runs on a blocking thread and reports like a transfer does.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use crate::transfer::Tick;

/// What to change on one entry. Bits that are in neither `set` nor `clear`
/// stay as they are on each file, so a mixed selection keeps what the user did
/// not touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermissionEdit {
    /// Unix permission bits (`0o7777`) to turn on.
    pub set: u32,
    /// Unix permission bits to turn off.
    pub clear: u32,
    /// Windows read-only attribute; `None` leaves it alone. Hidden and system
    /// are not editable: the standard library cannot write them.
    pub read_only: Option<bool>,
}

impl PermissionEdit {
    const EXECUTE: u32 = 0o111;

    fn without_execute(self) -> Self {
        Self {
            set: self.set & !Self::EXECUTE,
            clear: self.clear | Self::EXECUTE,
            ..self
        }
    }

    /// The edit as two: turning bits on, and turning bits off. A folder gets the
    /// first before it is walked and the second after, so removing its own
    /// read or execute bit does not lock the walk out of it.
    fn split_set_and_clear(self) -> (Self, Self) {
        let turn_on = Self { clear: 0, ..self };
        let turn_off = Self {
            set: 0,
            clear: self.clear & !self.set,
            read_only: None,
        };
        (turn_on, turn_off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForFiles {
    AsAbove,
    /// As above, but no execute bit is set and all are removed.
    WithoutExecute,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForFolders {
    AsAbove,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionPlan {
    /// Applied to the chosen entries themselves.
    pub edit: PermissionEdit,
    /// `Some` goes into the chosen folders. Links are neither followed nor
    /// changed.
    pub recurse: Option<(ForFiles, ForFolders)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Entries whose permissions were actually written.
    pub changed: usize,
    pub failures: Vec<Failure>,
    /// The receiving end of the progress channel went away before the end.
    pub cancelled: bool,
}

/// Applies `plan` to `chosen`. Progress goes out as [`Tick`]s; dropping the
/// receiver stops the run, which is how a cancel arrives.
pub fn change_permissions(
    chosen: &[PathBuf],
    plan: &PermissionPlan,
    sender: &mpsc::UnboundedSender<Tick>,
) -> Outcome {
    let total = count_work(chosen, plan);
    apply_counted(chosen, plan, total, sender)
}

#[derive(Clone, Copy)]
struct Edits {
    files: Option<PermissionEdit>,
    folders: Option<PermissionEdit>,
}

fn edits_below(plan: &PermissionPlan) -> Option<Edits> {
    let (for_files, for_folders) = plan.recurse?;
    Some(Edits {
        files: match for_files {
            ForFiles::AsAbove => Some(plan.edit),
            ForFiles::WithoutExecute => Some(plan.edit.without_execute()),
            ForFiles::Unchanged => None,
        },
        folders: match for_folders {
            ForFolders::AsAbove => Some(plan.edit),
            ForFolders::Unchanged => None,
        },
    })
}

/// How many entries [`apply_counted`] will touch, for the progress total. Only
/// counts; nothing is remembered, so what is applied is decided while walking.
fn count_work(chosen: &[PathBuf], plan: &PermissionPlan) -> usize {
    let below = edits_below(plan);
    chosen
        .iter()
        .map(|path| {
            let is_real_folder = fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
            1 + below
                .filter(|_| is_real_folder)
                .map_or(0, |edits| count_below(path, &edits))
        })
        .sum()
}

fn count_below(folder: &Path, edits: &Edits) -> usize {
    let Ok(children) = fs::read_dir(folder) else {
        return 0;
    };
    let mut count = 0;
    for child in children.filter_map(Result::ok) {
        let Ok(kind) = child.file_type() else {
            continue;
        };
        if kind.is_dir() {
            count += usize::from(edits.folders.is_some()) + count_below(&child.path(), edits);
        } else if !kind.is_symlink() {
            count += usize::from(edits.files.is_some());
        }
    }
    count
}

struct Run<'a> {
    sender: &'a mpsc::UnboundedSender<Tick>,
    total: usize,
    done: usize,
    outcome: Outcome,
}

impl Run<'_> {
    /// Reports the entry about to be changed; `false` once the receiver is gone.
    fn tick(&mut self) -> bool {
        let tick = Tick {
            done: self.done.min(self.total),
            total: self.total,
        };
        self.done += 1;
        if self.sender.send(tick).is_err() {
            self.outcome.cancelled = true;
            return false;
        }
        true
    }

    fn fail(&mut self, path: PathBuf, err: &io::Error) {
        self.outcome.failures.push(Failure {
            path,
            reason: err.to_string(),
        });
    }
}

/// The run itself, with `total` already counted. The folders are walked while
/// they are changed, through the handle of each folder, so a folder that is
/// swapped for a link after the count is not followed.
fn apply_counted(
    chosen: &[PathBuf],
    plan: &PermissionPlan,
    total: usize,
    sender: &mpsc::UnboundedSender<Tick>,
) -> Outcome {
    let below = edits_below(plan);
    let mut run = Run {
        sender,
        total,
        done: 0,
        outcome: Outcome::default(),
    };
    for path in chosen {
        if !run.tick() {
            return run.outcome;
        }
        let (first, last) = match below {
            Some(_) => plan.edit.split_set_and_clear(),
            None => (plan.edit, PermissionEdit::default()),
        };
        let mut changed = false;
        match apply(path, &first) {
            Ok(did) => changed = did,
            Err(err) => run.fail(path.clone(), &err),
        }
        if let Some(edits) = &below {
            if !walk_root(&mut run, path, edits) {
                return run.outcome;
            }
            match apply(path, &last) {
                Ok(did) => changed |= did,
                Err(err) => run.fail(path.clone(), &err),
            }
        }
        if changed {
            run.outcome.changed += 1;
        }
    }
    // Ignored on purpose: the work is done whether or not anyone still listens.
    let _ = sender.send(Tick { done: total, total });
    run.outcome
}

/// `false` when the run was cancelled.
#[cfg(unix)]
fn walk_root(run: &mut Run<'_>, root: &Path, edits: &Edits) -> bool {
    use nix::errno::Errno;
    use nix::fcntl::{open, OFlag};
    use nix::sys::stat::Mode;

    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    match open(root, flags, Mode::empty()) {
        Ok(handle) => walk_folder(run, handle, root, edits),
        // Not a real folder: a file, or a link, which is not followed.
        Err(Errno::ENOTDIR | Errno::ELOOP) => true,
        Err(errno) => {
            run.fail(root.to_path_buf(), &io::Error::from(errno));
            true
        }
    }
}

/// Lists `handle` and changes its children through it, going into the folders
/// with the handle each of them is opened as. Only regular files and folders
/// are touched; links, devices, pipes and sockets are not.
#[cfg(unix)]
fn walk_folder(
    run: &mut Run<'_>,
    handle: std::os::fd::OwnedFd,
    folder: &Path,
    edits: &Edits,
) -> bool {
    use std::os::fd::AsFd as _;
    use std::os::unix::ffi::OsStrExt as _;

    use nix::dir::Dir;
    use nix::fcntl::AtFlags;
    use nix::sys::stat::{fstatat, SFlag};

    let mut dir = match Dir::from_fd(handle) {
        Ok(dir) => dir,
        Err(errno) => {
            run.fail(folder.to_path_buf(), &io::Error::from(errno));
            return true;
        }
    };
    let mut names: Vec<std::ffi::CString> = Vec::new();
    for child in dir.iter() {
        match child {
            Ok(child) => {
                let name = child.file_name();
                if !matches!(name.to_bytes(), b"." | b"..") {
                    names.push(name.to_owned());
                }
            }
            Err(errno) => {
                run.fail(folder.to_path_buf(), &io::Error::from(errno));
                break;
            }
        }
    }
    for name in names {
        let path = folder.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        let stat = match fstatat(&dir, name.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            // Gone since the listing.
            Err(nix::errno::Errno::ENOENT) => continue,
            Err(errno) => {
                run.fail(path, &io::Error::from(errno));
                continue;
            }
        };
        let kind = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
        let is_folder = match kind {
            SFlag::S_IFDIR => true,
            SFlag::S_IFREG => false,
            _ => continue,
        };
        let edit = if is_folder {
            edits.folders
        } else {
            edits.files
        };
        if edit.is_none() && !is_folder {
            continue;
        }
        if edit.is_some() && !run.tick() {
            return false;
        }
        let (first, last) = match edit {
            Some(whole) if is_folder => {
                let (turn_on, turn_off) = whole.split_set_and_clear();
                (Some(turn_on), Some(turn_off))
            }
            other => (other, None),
        };
        let (mut changed, opened) = change_in(dir.as_fd(), &name, is_folder, first.as_ref());
        match opened {
            Ok(Some(child)) if is_folder => {
                let held = last.map(|_| child.try_clone());
                if !walk_folder(run, child, &path, edits) {
                    return false;
                }
                if let (Some(turn_off), Some(held)) = (last, held) {
                    match held.and_then(|held| change_handle(&held, true, &turn_off)) {
                        Ok(did) => changed |= did,
                        Err(err) => run.fail(path, &err),
                    }
                }
            }
            Ok(_) => {}
            Err(err) => run.fail(path, &err),
        }
        if changed {
            run.outcome.changed += 1;
        }
    }
    true
}

#[cfg(unix)]
fn open_flags(is_folder: bool) -> nix::fcntl::OFlag {
    use nix::fcntl::OFlag;

    let base = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    // O_NONBLOCK: opening a pipe must not wait for a writer.
    base | if is_folder {
        OFlag::O_DIRECTORY
    } else {
        OFlag::O_NONBLOCK
    }
}

/// Whether anything was written, and the handle the entry was opened as
/// (`None` when it is a link or another kind of entry by now). Without an
/// `edit` the entry is only opened.
///
/// The change goes through that handle, never through a path: a path-based
/// check followed by a path-based chmod would let someone who can write in the
/// folder swap the entry for a link in between.
#[cfg(unix)]
fn change_in(
    dirfd: std::os::fd::BorrowedFd<'_>,
    name: &std::ffi::CStr,
    is_folder: bool,
    wanted: Option<&PermissionEdit>,
) -> (bool, io::Result<Option<std::os::fd::OwnedFd>>) {
    use nix::errno::Errno;
    use nix::fcntl::openat;
    use nix::sys::stat::Mode;

    match openat(dirfd, name, open_flags(is_folder), Mode::empty()) {
        Ok(handle) => {
            let Some(edit) = wanted else {
                return (false, Ok(Some(handle)));
            };
            match change_handle(&handle, is_folder, edit) {
                Ok(changed) => (changed, Ok(Some(handle))),
                Err(err) => (false, Err(err)),
            }
        }
        Err(Errno::ELOOP | Errno::ENOTDIR) => (false, Ok(None)),
        Err(Errno::EACCES) => match wanted {
            Some(edit) => change_unopenable(dirfd, name, is_folder, edit),
            None => (false, Err(io::Error::from(Errno::EACCES))),
        },
        Err(errno) => (false, Err(io::Error::from(errno))),
    }
}

#[cfg(unix)]
fn change_handle(
    handle: &std::os::fd::OwnedFd,
    is_folder: bool,
    edit: &PermissionEdit,
) -> io::Result<bool> {
    use nix::sys::stat::{fchmod, fstat, SFlag};

    let stat = fstat(handle)?;
    let wanted_kind = if is_folder {
        SFlag::S_IFDIR
    } else {
        SFlag::S_IFREG
    };
    if SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT != wanted_kind {
        return Ok(false);
    }
    let Some(after) = mode_after(stat.st_mode, edit) else {
        return Ok(false);
    };
    fchmod(handle, mode_of_bits(after))?;
    Ok(true)
}

/// For an entry the user owns but cannot open (mode 000): making it readable
/// again is the typical chmod. `fchmodat` with `NoFollowSymlink` does not follow
/// the last path part either, so it is as safe as the `O_NOFOLLOW` open. Where
/// the system cannot do that (`EOPNOTSUPP`), the open error is the result. A
/// folder is opened again afterwards so that it can be walked.
#[cfg(unix)]
fn change_unopenable(
    dirfd: std::os::fd::BorrowedFd<'_>,
    name: &std::ffi::CStr,
    is_folder: bool,
    edit: &PermissionEdit,
) -> (bool, io::Result<Option<std::os::fd::OwnedFd>>) {
    use nix::errno::Errno;
    use nix::fcntl::{openat, AtFlags};
    use nix::sys::stat::{fchmodat, fstatat, FchmodatFlags, Mode, SFlag};

    let open_error = || io::Error::from(Errno::EACCES);
    let stat = match fstatat(dirfd, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(errno) => return (false, Err(io::Error::from(errno))),
    };
    let wanted_kind = if is_folder {
        SFlag::S_IFDIR
    } else {
        SFlag::S_IFREG
    };
    if SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT != wanted_kind {
        return (false, Ok(None));
    }
    let Some(after) = mode_after(stat.st_mode, edit) else {
        // A file whose bits already fit is done; a folder still cannot be walked.
        let fitting = if is_folder {
            Err(open_error())
        } else {
            Ok(None)
        };
        return (false, fitting);
    };
    match fchmodat(
        dirfd,
        name,
        mode_of_bits(after),
        FchmodatFlags::NoFollowSymlink,
    ) {
        Ok(()) => {}
        Err(Errno::EOPNOTSUPP) => return (false, Err(open_error())),
        Err(errno) => return (false, Err(io::Error::from(errno))),
    }
    if !is_folder {
        return (true, Ok(None));
    }
    match openat(dirfd, name, open_flags(true), Mode::empty()) {
        Ok(handle) => (true, Ok(Some(handle))),
        Err(Errno::ELOOP | Errno::ENOTDIR) => (true, Ok(None)),
        Err(errno) => (true, Err(io::Error::from(errno))),
    }
}

/// The permission bits after `edit`; `None` when they already fit.
/// Generic because `st_mode` is u16 on macOS and u32 on Linux: a `u32::from`
/// at the call is a useless conversion on Linux.
#[cfg(unix)]
fn mode_after(before: impl Into<u32>, edit: &PermissionEdit) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut permissions = fs::Permissions::from_mode(before.into() & 0o7777);
    edited(&mut permissions, edit).then(|| permissions.mode() & 0o7777)
}

/// mode_t is u16 on macOS and u32 on Linux; the mask keeps the cast lossless.
#[cfg(unix)]
fn mode_of_bits(bits: u32) -> nix::sys::stat::Mode {
    nix::sys::stat::Mode::from_bits_truncate((bits & 0o7777) as libc::mode_t)
}

/// Without file handles to hold on to, the folders are walked by path.
#[cfg(not(unix))]
fn walk_root(run: &mut Run<'_>, root: &Path, edits: &Edits) -> bool {
    if !fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return true;
    }
    let children = match fs::read_dir(root) {
        Ok(children) => children,
        Err(err) => {
            run.fail(root.to_path_buf(), &err);
            return true;
        }
    };
    for child in children.filter_map(Result::ok) {
        let path = child.path();
        let Ok(kind) = child.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let kind_edit = if kind.is_dir() {
            edits.folders
        } else {
            edits.files
        };
        if let Some(edit) = kind_edit {
            if !run.tick() {
                return false;
            }
            match apply(&path, &edit) {
                Ok(true) => run.outcome.changed += 1,
                Ok(false) => {}
                Err(err) => run.fail(path.clone(), &err),
            }
        }
        if kind.is_dir() && !walk_root(run, &path, edits) {
            return false;
        }
    }
    true
}

/// `Ok(true)` when something was written, `Ok(false)` when the entry already
/// had the wanted permissions or is a link or another kind of file.
///
/// For the chosen entries themselves, by path: the entry is opened without
/// following a link and changed through that handle, but a link in an earlier
/// part of the path is followed. What lies below a chosen folder is changed by
/// `walk_folder` through the handles of its folders instead.
#[cfg(unix)]
fn apply(path: &Path, edit: &PermissionEdit) -> io::Result<bool> {
    use std::os::unix::fs::OpenOptionsExt as _;

    // O_NONBLOCK: opening a pipe must not wait for a writer. A file that cannot
    // be opened (no read right) is reported, not changed by path: not changing
    // is safe, changing by path is not.
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path);
    let handle = match opened {
        Ok(handle) => handle,
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => return Ok(false),
        Err(err) if err.raw_os_error() == Some(libc::EACCES) => {
            return apply_without_opening(path, edit, err)
        }
        Err(err) => return Err(err),
    };
    let metadata = handle.metadata()?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Ok(false);
    }
    let mut permissions = metadata.permissions();
    if !edited(&mut permissions, edit) {
        return Ok(false);
    }
    handle.set_permissions(permissions)?;
    Ok(true)
}

/// For an entry the user owns but cannot open (mode 000): making it readable
/// again is the typical chmod. `fchmodat` with `NoFollowSymlink` does not follow
/// the last path part either, so it is as safe as the `O_NOFOLLOW` open. Where
/// the system cannot do that (`EOPNOTSUPP`), the open error is the result.
#[cfg(unix)]
fn apply_without_opening(
    path: &Path,
    edit: &PermissionEdit,
    open_error: io::Error,
) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt as _;

    use nix::fcntl::AT_FDCWD;
    use nix::sys::stat::{fchmodat, FchmodatFlags};

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Ok(false);
    }
    let Some(after) = mode_after(metadata.permissions().mode(), edit) else {
        return Ok(false);
    };
    match fchmodat(
        AT_FDCWD,
        path,
        mode_of_bits(after),
        FchmodatFlags::NoFollowSymlink,
    ) {
        Ok(()) => Ok(true),
        Err(nix::errno::Errno::EOPNOTSUPP) => Err(open_error),
        Err(errno) => Err(io::Error::from(errno)),
    }
}

#[cfg(not(unix))]
fn apply(path: &Path, edit: &PermissionEdit) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let mut permissions = metadata.permissions();
    if !edited(&mut permissions, edit) {
        return Ok(false);
    }
    fs::set_permissions(path, permissions)?;
    Ok(true)
}

/// Writes the wanted bits into `permissions`; `false` when they were there.
#[cfg(unix)]
fn edited(permissions: &mut fs::Permissions, edit: &PermissionEdit) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    let before = permissions.mode() & 0o7777;
    let after = (before & !edit.clear) | edit.set;
    permissions.set_mode(after);
    after != before
}

#[cfg(not(unix))]
fn edited(permissions: &mut fs::Permissions, edit: &PermissionEdit) -> bool {
    match edit.read_only {
        Some(read_only) if permissions.readonly() != read_only => {
            permissions.set_readonly(read_only);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
#[cfg(unix)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;

#[cfg(test)]
#[cfg(unix)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod unreadable_tests;

#[cfg(test)]
#[cfg(unix)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod unreadable_folder_tests;
