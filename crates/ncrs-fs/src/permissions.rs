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
    let mut outcome = Outcome::default();
    let work = collect_work(chosen, plan, &mut outcome.failures);
    let total = work.len();
    for (index, (path, edit)) in work.iter().enumerate() {
        if sender.send(Tick { done: index, total }).is_err() {
            outcome.cancelled = true;
            return outcome;
        }
        match apply(path, edit) {
            Ok(true) => outcome.changed += 1,
            Ok(false) => {}
            Err(err) => outcome.failures.push(Failure {
                path: path.clone(),
                reason: err.to_string(),
            }),
        }
    }
    // Ignored on purpose: the work is done whether or not anyone still listens.
    let _ = sender.send(Tick { done: total, total });
    outcome
}

fn collect_work(
    chosen: &[PathBuf],
    plan: &PermissionPlan,
    failures: &mut Vec<Failure>,
) -> Vec<(PathBuf, PermissionEdit)> {
    let mut work = Vec::new();
    for path in chosen {
        work.push((path.clone(), plan.edit));
        if let Some((for_files, for_folders)) = plan.recurse {
            let is_real_folder = fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
            if is_real_folder {
                let files = match for_files {
                    ForFiles::AsAbove => Some(plan.edit),
                    ForFiles::WithoutExecute => Some(plan.edit.without_execute()),
                    ForFiles::Unchanged => None,
                };
                let folders = match for_folders {
                    ForFolders::AsAbove => Some(plan.edit),
                    ForFolders::Unchanged => None,
                };
                walk(path, files, folders, &mut work, failures);
            }
        }
    }
    work
}

fn walk(
    folder: &Path,
    files: Option<PermissionEdit>,
    folders: Option<PermissionEdit>,
    work: &mut Vec<(PathBuf, PermissionEdit)>,
    failures: &mut Vec<Failure>,
) {
    let children = match fs::read_dir(folder) {
        Ok(children) => children,
        Err(err) => {
            failures.push(Failure {
                path: folder.to_path_buf(),
                reason: err.to_string(),
            });
            return;
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
        if kind.is_dir() {
            if let Some(edit) = folders {
                work.push((path.clone(), edit));
            }
            walk(&path, files, folders, work, failures);
        } else if let Some(edit) = files {
            work.push((path, edit));
        }
    }
}

/// `Ok(true)` when something was written, `Ok(false)` when the entry already
/// had the wanted permissions or is a link or another kind of file.
///
/// The entry is opened without following links and changed through that
/// handle: a path-based check followed by a path-based chmod would let someone
/// who can write in the folder swap a file for a link in between.
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

#[cfg(all(test, unix))]
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
            &[a.clone()],
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
            &[top.clone()],
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
            &[top.clone()],
            PermissionPlan {
                edit: edit(0o004, 0),
                recurse: Some((ForFiles::AsAbove, ForFolders::Unchanged)),
            },
        );
        assert_eq!(mode(&top), 0o704);
        assert_eq!(mode(&inner), 0o700);
        assert_eq!(mode(&file), 0o604);
        assert_eq!(outcome.changed, 2);

        let outcome = run(
            &[top.clone()],
            PermissionPlan {
                edit: edit(0o040, 0),
                recurse: Some((ForFiles::Unchanged, ForFolders::AsAbove)),
            },
        );
        assert_eq!(mode(&inner), 0o740);
        assert_eq!(mode(&file), 0o604);
        assert_eq!(outcome.changed, 2);
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
    fn a_failure_is_listed_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone");
        let outcome = run(
            &[missing.clone()],
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
            &[a.clone()],
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

#[cfg(all(test, unix))]
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
            &[path.clone()],
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
