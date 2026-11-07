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
        match apply(path, &plan.edit) {
            Ok(true) => run.outcome.changed += 1,
            Ok(false) => {}
            Err(err) => run.fail(path.clone(), &err),
        }
        if let Some(edits) = &below {
            if !walk_root(&mut run, path, edits) {
                return run.outcome;
            }
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
    let names: Vec<std::ffi::CString> = dir
        .iter()
        .filter_map(Result::ok)
        .map(|child| child.file_name().to_owned())
        .filter(|name| !matches!(name.to_bytes(), b"." | b".."))
        .collect();
    for name in names {
        let Ok(stat) = fstatat(&dir, name.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW) else {
            continue;
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
        let path = folder.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        let (changed, opened) = change_in(dir.as_fd(), &name, is_folder, edit.as_ref());
        if changed {
            run.outcome.changed += 1;
        }
        match opened {
            Ok(Some(child)) if is_folder => {
                if !walk_folder(run, child, &path, edits) {
                    return false;
                }
            }
            Ok(_) => {}
            Err(err) => run.fail(path, &err),
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
    let Some(after) = mode_after(u32::from(stat.st_mode), edit) else {
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
    let Some(after) = mode_after(u32::from(stat.st_mode), edit) else {
        return (false, Err(open_error()));
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
#[cfg(unix)]
fn mode_after(before: u32, edit: &PermissionEdit) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut permissions = fs::Permissions::from_mode(before & 0o7777);
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
        let edit = if kind.is_dir() {
            edits.folders
        } else {
            edits.files
        };
        if kind.is_symlink() || !(kind.is_dir() || kind.is_file()) {
            continue;
        }
        if let Some(edit) = edit {
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
    use nix::sys::stat::{fchmodat, FchmodatFlags, Mode};

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Ok(false);
    }
    let mut permissions = metadata.permissions();
    if !edited(&mut permissions, edit) {
        return Ok(false);
    }
    // mode_t is u16 on macOS and u32 on Linux; the mask keeps the cast lossless.
    let mode = Mode::from_bits_truncate((permissions.mode() & 0o7777) as libc::mode_t);
    match fchmodat(AT_FDCWD, path, mode, FchmodatFlags::NoFollowSymlink) {
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
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    fn put(path: &Path, bits: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
    }

    fn run(chosen: &[PathBuf], plan: PermissionPlan) -> Outcome {
        let (sender, _receiver) = mpsc::unbounded_channel();
        change_permissions(chosen, &plan, &sender)
    }

    fn edit(set: u32, clear: u32) -> PermissionEdit {
        PermissionEdit {
            set,
            clear,
            read_only: None,
        }
    }

    fn file_with(dir: &Path, name: &str, bits: u32) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "x").unwrap();
        put(&path, bits);
        path
    }

    #[test]
    fn sets_and_clears_only_the_named_bits() {
        let dir = tempfile::tempdir().unwrap();
        let a = file_with(dir.path(), "a", 0o640);
        let b = file_with(dir.path(), "b", 0o666);
        let outcome = run(
            &[a.clone(), b.clone()],
            PermissionPlan {
                edit: edit(0o100, 0o002),
                recurse: None,
            },
        );
        assert_eq!(mode(&a), 0o740);
        assert_eq!(mode(&b), 0o764);
        assert_eq!(outcome.changed, 2);
        assert!(outcome.failures.is_empty());
    }

    #[test]
    fn leaves_an_entry_alone_when_the_mode_already_fits() {
        let dir = tempfile::tempdir().unwrap();
        let a = file_with(dir.path(), "a", 0o644);
        let outcome = run(
            std::slice::from_ref(&a),
            PermissionPlan {
                edit: edit(0o004, 0o002),
                recurse: None,
            },
        );
        assert_eq!(outcome.changed, 0);
        assert_eq!(mode(&a), 0o644);
    }

    fn tree() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("top");
        fs::create_dir(&top).unwrap();
        put(&top, 0o700);
        let inner = top.join("inner");
        fs::create_dir(&inner).unwrap();
        put(&inner, 0o700);
        let file = file_with(&inner, "f", 0o600);
        (dir, top, inner, file)
    }

    #[test]
    fn recursion_applies_to_files_and_folders_and_without_execute_clears_x() {
        let (_dir, top, inner, file) = tree();
        put(&file, 0o700);
        let plan = |files, folders| PermissionPlan {
            edit: edit(0o044, 0),
            recurse: Some((files, folders)),
        };
        let outcome = run(
            std::slice::from_ref(&top),
            plan(ForFiles::WithoutExecute, ForFolders::AsAbove),
        );
        assert_eq!(mode(&top), 0o744);
        assert_eq!(mode(&inner), 0o744);
        assert_eq!(mode(&file), 0o644);
        assert_eq!(outcome.changed, 3);
    }

    #[test]
    fn unchanged_files_and_folders_are_skipped_but_still_walked() {
        let (_dir, top, inner, file) = tree();
        let outcome = run(
            std::slice::from_ref(&top),
            PermissionPlan {
                edit: edit(0o004, 0),
                recurse: Some((ForFiles::AsAbove, ForFolders::Unchanged)),
            },
        );
        assert_eq!(mode(&top), 0o704);
        assert_eq!(mode(&inner), 0o700);
        assert_eq!(mode(&file), 0o604);
        assert_eq!(outcome.changed, 2);

        let second = run(
            std::slice::from_ref(&top),
            PermissionPlan {
                edit: edit(0o040, 0),
                recurse: Some((ForFiles::Unchanged, ForFolders::AsAbove)),
            },
        );
        assert_eq!(mode(&inner), 0o740);
        assert_eq!(mode(&file), 0o604);
        assert_eq!(second.changed, 2);
    }

    #[test]
    fn links_are_neither_followed_nor_changed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let hidden = file_with(&outside, "f", 0o600);
        put(&outside, 0o700);
        let top = dir.path().join("top");
        fs::create_dir(&top).unwrap();
        put(&top, 0o700);
        let folder_link = top.join("to_folder");
        let file_link = top.join("to_file");
        symlink(&outside, &folder_link).unwrap();
        symlink(&hidden, &file_link).unwrap();

        let plan = PermissionPlan {
            edit: edit(0o004, 0),
            recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
        };
        run(&[top.clone(), folder_link.clone()], plan);
        assert_eq!(mode(&hidden), 0o600);
        assert_eq!(mode(&outside), 0o700);
        assert_eq!(mode(&top), 0o704);
    }

    #[test]
    fn a_folder_swapped_for_a_link_after_the_count_is_not_followed() {
        let (dir, top, inner, _file) = tree();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let hidden = file_with(&outside, "f", 0o600);
        put(&outside, 0o700);
        let plan = PermissionPlan {
            edit: edit(0o004, 0),
            recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
        };
        let chosen = [top.clone()];
        let total = count_work(&chosen, &plan);
        assert_eq!(total, 3);

        fs::remove_dir_all(&inner).unwrap();
        symlink(&outside, &inner).unwrap();
        let (sender, _receiver) = mpsc::unbounded_channel();
        let outcome = apply_counted(&chosen, &plan, total, &sender);

        assert_eq!(mode(&hidden), 0o600);
        assert_eq!(mode(&outside), 0o700);
        assert_eq!(mode(&top), 0o704);
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    }
    #[test]
    fn a_failure_is_listed_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone");
        let outcome = run(
            std::slice::from_ref(&missing),
            PermissionPlan {
                edit: edit(0o004, 0),
                recurse: None,
            },
        );
        assert_eq!(outcome.changed, 0);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].path, missing);
    }

    #[test]
    fn dropping_the_receiver_cancels() {
        let dir = tempfile::tempdir().unwrap();
        let a = file_with(dir.path(), "a", 0o600);
        let (sender, receiver) = mpsc::unbounded_channel();
        drop(receiver);
        let outcome = change_permissions(
            std::slice::from_ref(&a),
            &PermissionPlan {
                edit: edit(0o004, 0),
                recurse: None,
            },
            &sender,
        );
        assert!(outcome.cancelled);
        assert_eq!(mode(&a), 0o600);
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
mod unreadable_tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn a_file_with_mode_000_can_be_made_readable_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked");
        fs::write(&path, "x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let (sender, _receiver) = mpsc::unbounded_channel();
        let outcome = change_permissions(
            std::slice::from_ref(&path),
            &PermissionPlan {
                edit: PermissionEdit {
                    set: 0o004,
                    clear: 0,
                    read_only: None,
                },
                recurse: None,
            },
            &sender,
        );
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(mode, 0o004);
    }
}

#[cfg(test)]
#[cfg(unix)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod unreadable_folder_tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn a_folder_with_mode_000_is_made_readable_and_walked() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("top");
        let locked = top.join("locked");
        fs::create_dir_all(&locked).unwrap();
        let file = locked.join("f");
        fs::write(&file, "x").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let (sender, _receiver) = mpsc::unbounded_channel();
        let outcome = change_permissions(
            std::slice::from_ref(&top),
            &PermissionPlan {
                edit: PermissionEdit {
                    set: 0o700,
                    clear: 0,
                    read_only: None,
                },
                recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
            },
            &sender,
        );
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(mode(&locked), 0o700);
        assert_eq!(mode(&file), 0o700);
    }
}
