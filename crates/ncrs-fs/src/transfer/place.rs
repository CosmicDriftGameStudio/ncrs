//! Putting a source at a target: the conflict rules, merging folders, and the
//! staged copy that keeps a half-written file from ever carrying a real name.

use std::io;
use std::path::{Path, PathBuf};

use super::{partial_prefix, OnConflict, Progress, Source};
use crate::place_file::{copy_xattrs, rename_no_replace};

/// How much of a file is written between two looks at the progress channel.
const CHUNK: usize = 1024 * 1024;

pub(super) fn place<S: Source, P: Progress>(
    source: &Path,
    target: &Path,
    effect: &S,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    // `symlink_metadata`, not `exists`: a dangling symlink in the way is still
    // something in the way, and `exists` follows it and says no.
    if lstat(target)?.is_some() {
        match conflict {
            OnConflict::Fail => return Err(already_exists(target)),
            OnConflict::Skip => return Ok(0),
            OnConflict::Overwrite => {}
        }
    }

    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let mut run = Run {
        effect,
        progress,
        done: 0,
        total: 0,
    };
    run.entry(source, target)?;
    // A renamed folder counts as one item inside a merge, so the bar is filled
    // here rather than left short.
    if run.done < run.total {
        run.done = run.total;
        run.progress.advanced(run.done, run.total)?;
    }
    Ok(run.done)
}

struct Run<'a, S, P> {
    effect: &'a S,
    progress: &'a mut P,
    done: usize,
    /// 0 while unknown: a plain rename never needs to count the tree.
    total: usize,
}

impl<S: Source, P: Progress> Run<'_, S, P> {
    /// Puts one entry at `target`. What is already there is replaced when both
    /// are files or links, merged when both are folders, and a file against a
    /// folder is refused: neither side is a replacement for the other.
    fn entry(&mut self, source: &Path, target: &Path) -> io::Result<()> {
        let source_meta = std::fs::symlink_metadata(source)?;
        let replacing = match lstat(target)? {
            Some(existing) if source_meta.is_dir() && existing.is_dir() => {
                return self.merge(source, target);
            }
            Some(existing) if source_meta.is_dir() != existing.is_dir() => {
                // Not `AlreadyExists`: that would offer "overwrite" again for a
                // pair that overwriting can never resolve.
                let kind = if source_meta.is_dir() {
                    io::ErrorKind::NotADirectory
                } else {
                    io::ErrorKind::IsADirectory
                };
                return Err(io::Error::new(
                    kind,
                    format!("{} is in the way", target.display()),
                ));
            }
            Some(_) => true,
            None => false,
        };

        if self.effect.removes_source() {
            let moved = if replacing {
                self.effect.relocate_replacing(source, target)?
            } else {
                self.effect.relocate(source, target)?
            };
            if moved {
                // A rename is one step, however large the tree.
                self.done += 1;
                if self.total == 0 {
                    self.total = 1;
                }
                return self.progress.advanced(self.done, self.total);
            }
        }

        self.ensure_total(source);
        let staged = self.claim_partial(source, &source_meta, target)?;
        let finished = self
            .descend(source, &staged, &source_meta)
            .and_then(|()| {
                if self.effect.removes_source() {
                    verify_copy(source, &staged)
                } else {
                    Ok(())
                }
            })
            .and_then(|()| {
                if replacing {
                    std::fs::rename(&staged, target)
                } else {
                    rename_no_replace(&staged, target)
                }
            });
        if let Err(e) = finished {
            let _best_effort = remove_path(&staged);
            return Err(e);
        }
        if self.effect.removes_source() {
            self.effect.remove(source)?;
        }
        Ok(())
    }

    /// A folder onto a folder: every entry of the source is placed into the
    /// target one by one, so nothing that only the target holds is touched.
    fn merge(&mut self, source: &Path, target: &Path) -> io::Result<()> {
        self.ensure_total(source);
        self.done += 1;
        self.progress.advanced(self.done, self.total)?;
        for listed in std::fs::read_dir(source)? {
            let entry = listed?;
            self.entry(&entry.path(), &target.join(entry.file_name()))?;
        }
        if self.effect.removes_source() {
            // Not `remove_dir_all`: the folder is empty because everything went
            // out of it, and one that is not must stay.
            std::fs::remove_dir(source)?;
        }
        Ok(())
    }

    fn ensure_total(&mut self, source: &Path) {
        if self.total == 0 {
            self.total = count_items(source);
        }
    }

    /// Creates the first entry of a copy under a hidden name nobody else holds.
    fn claim_partial(
        &mut self,
        source: &Path,
        meta: &std::fs::Metadata,
        target: &Path,
    ) -> io::Result<PathBuf> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate = target.with_file_name(format!(
                "{}{}-{n}",
                partial_prefix(target),
                std::process::id()
            ));
            match create_entry(source, meta, &candidate, self.progress) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Counts the entry just created and walks into it if it is a directory.
    fn descend(
        &mut self,
        source: &Path,
        target: &Path,
        meta: &std::fs::Metadata,
    ) -> io::Result<()> {
        self.done += 1;
        // A cancel arrives as an error here and stops the walk.
        self.progress.advanced(self.done, self.total)?;

        if meta.is_dir() {
            for listed in std::fs::read_dir(source)? {
                let entry = listed?;
                let child = target.join(entry.file_name());
                let child_meta = std::fs::symlink_metadata(entry.path())?;
                create_entry(&entry.path(), &child_meta, &child, self.progress)?;
                self.descend(&entry.path(), &child, &child_meta)?;
            }
            finish_dir(source, meta, target);
        }
        Ok(())
    }
}

fn lstat(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn already_exists(target: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} already exists", target.display()),
    )
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

fn remove_path(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
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

/// Makes one new entry at `target`, failing if the name is already taken.
pub(super) fn create_entry<P: Progress>(
    source: &Path,
    meta: &std::fs::Metadata,
    target: &Path,
    progress: &mut P,
) -> io::Result<()> {
    if meta.is_dir() {
        std::fs::create_dir(target)
    } else if meta.file_type().is_symlink() {
        // A symlink is copied as a symlink, not followed: following it would
        // copy whatever it points at, possibly outside the tree the user marked.
        let link = std::fs::read_link(source)?;
        symlink(source, &link, target)
    } else {
        refuse_special_file(meta)?;
        let mut from = open_regular(source)?;
        // `create_new`, not `fs::copy`: copy opens an existing path and writes
        // through whatever is there, including a symlink. Private while it is
        // filled; the original's permissions come with the last step.
        let mut to = create_private(target)?;
        let filled = copy_in_chunks(&mut from, &mut to, progress).and_then(|()| {
            copy_xattrs(source, target);
            // The copy keeps the date of the original, as in the Finder: a
            // folder comparison after a sync must not find every copy newer.
            if let Ok(modified) = meta.modified() {
                to.set_modified(modified)?;
            }
            to.set_permissions(copied_permissions(meta))
        });
        if filled.is_err() {
            // The name was created just now by this call, so it is ours to remove.
            let _best_effort = std::fs::remove_file(target);
        }
        filled
    }
}

fn create_private(target: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(target)
}

/// Opens the source for reading without following a link that replaced it
/// since the listing, and refuses anything that is not a regular file by the
/// handle itself, so a swap for a FIFO cannot block the job.
fn open_regular(source: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(source)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}

fn copy_in_chunks<P: Progress>(
    from: &mut std::fs::File,
    to: &mut std::fs::File,
    progress: &mut P,
) -> io::Result<()> {
    use io::{Read as _, Write as _};
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        progress.keep_going()?;
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(buffer.get(..read).unwrap_or_default())?;
    }
}

/// A finished folder gets what the original had: attributes, date and
/// permissions. Last, because the date changes with every entry added and a
/// read-only folder cannot be filled. A folder that cannot take them is still
/// a complete copy.
fn finish_dir(source: &Path, meta: &std::fs::Metadata, target: &Path) {
    copy_xattrs(source, target);
    #[cfg(unix)]
    if let (Ok(modified), Ok(handle)) = (meta.modified(), std::fs::File::open(target)) {
        let _skipped = handle.set_modified(modified);
    }
    let _skipped = std::fs::set_permissions(target, copied_permissions(meta));
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
