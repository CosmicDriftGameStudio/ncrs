//! Copying between a server and this computer. All of it blocks and belongs on
//! the blocking pool; the requests run on the async runtime through a handle
//! to it, each under a time limit, and the progress callback is asked after
//! every block, so a cancel stops a large file within a moment.
//!
//! What lands under its real name is complete: downloads are unpacked into a
//! hidden folder next to the target and moved into place at the end, uploads
//! are written under a hidden name on the server and renamed. A server cannot
//! make ncrs create a file outside the target: every name it sends goes through
//! the same rule as a name from an archive, and its links are never followed.

use std::fmt;
use std::fs::OpenOptions;
use std::future::Future;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::Metadata;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::list::is_plain_name;
use super::session::{Pool, RemoteError, Session};
use super::{is_deceptive, RemotePath, ServerId, Timeouts, CHUNK};
use crate::archive::{restrict_to_owner, safe_components};
use crate::transfer::{
    resolve_conflict, transfer, Counting, Move, OnConflict, Progress, Reporting, Tick,
};
use ncrs_core::TransferKind;

/// A name from a server that cannot be created here. Carried inside an
/// `io::Error` of kind `InvalidData`, so the UI can word it in its language.
#[derive(Debug)]
pub struct UnsafeName(pub String);

impl fmt::Display for UnsafeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the name {} from the server cannot be created here",
            self.0
        )
    }
}

impl std::error::Error for UnsafeName {}

fn unsafe_name(name: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, UnsafeName(name.to_owned()))
}

/// A name a server sent must be one plain component, whatever it looks like,
/// and must not hide what it is behind control or bidirectional characters.
fn ensure_safe(name: &str) -> io::Result<()> {
    let plain = matches!(
        safe_components(name),
        Some(parts) if parts.len() == 1 && parts.first().is_some_and(|part| part == name)
    );
    if plain && !name.chars().any(is_deceptive) {
        Ok(())
    } else {
        Err(unsafe_name(name))
    }
}

/// One connection, used from a blocking thread.
struct Link<'a> {
    pool: &'a Pool,
    server: &'a ServerId,
    session: Arc<Session>,
    runtime: Handle,
    timeouts: Timeouts,
}

impl<'a> Link<'a> {
    fn new(pool: &'a Pool, server: &'a ServerId) -> io::Result<Self> {
        let session = pool
            .session(server)
            .ok_or_else(|| RemoteError::Lost("not connected".to_owned()).into_io())?;
        Ok(Self {
            pool,
            server,
            session,
            runtime: Handle::current(),
            timeouts: pool.timeouts(),
        })
    }

    fn fail(&self, error: RemoteError) -> io::Error {
        self.pool.note(self.server, &error);
        error.into_io()
    }

    /// An SFTP request under `limit`.
    fn call<T>(
        &self,
        limit: Duration,
        request: impl Future<Output = Result<T, SftpError>>,
    ) -> io::Result<T> {
        match self.runtime.block_on(timeout(limit, request)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(self.fail(RemoteError::of_sftp(&error))),
            Err(_) => Err(self.fail(RemoteError::Timeout)),
        }
    }

    /// A read or write on an open file under `limit`.
    fn call_io<T>(
        &self,
        limit: Duration,
        request: impl Future<Output = io::Result<T>>,
    ) -> io::Result<T> {
        match self.runtime.block_on(timeout(limit, request)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(self.fail(RemoteError::of_io(&error))),
            Err(_) => Err(self.fail(RemoteError::Timeout)),
        }
    }

    /// What is at `path`, without following a link; `None` when nothing is.
    fn lstat(&self, path: &RemotePath) -> io::Result<Option<Metadata>> {
        let sftp = self.session.sftp();
        match self.call(
            self.timeouts.request,
            sftp.symlink_metadata(path.remote_str()),
        ) {
            Ok(meta) => Ok(Some(meta)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn names_in(&self, dir: &RemotePath) -> io::Result<Vec<(String, Metadata)>> {
        let sftp = self.session.sftp();
        self.call(self.timeouts.list, async {
            let entries = sftp.read_dir(dir.remote_str()).await?;
            Ok(entries
                .map(|entry| (entry.file_name(), entry.metadata()))
                .collect())
        })
    }
}

/// Copies the file or folder at `source` into the local folder `target_dir`,
/// keeping its name, and returns how many files it wrote.
pub fn download<P: Progress>(
    pool: &Pool,
    source: &RemotePath,
    target_dir: &Path,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    let name = source.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the root of a server is not an entry in it",
        )
    })?;
    ensure_safe(name)?;
    if !resolve_conflict(&target_dir.join(name), conflict)? {
        return Ok(0);
    }
    let link = Link::new(pool, source.server())?;
    let meta = link
        .lstat(source)?
        .ok_or_else(|| RemoteError::NotFound.into_io())?;
    let kind = meta.file_type();
    if !kind.is_dir() && !kind.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "links and special files on a server are not copied",
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(".ncrs-partial-")
        .tempdir_in(target_dir)?;
    let staged = staging.path().join(name);
    let mut done = 0;
    if kind.is_dir() {
        fetch_folder(&link, source, &staged, progress, &mut done)?;
    } else {
        fetch_file(&link, source, &staged, meta.mtime, progress, &mut done)?;
    }
    let when_taken = if conflict == OnConflict::Overwrite {
        OnConflict::Overwrite
    } else {
        OnConflict::Fail
    };
    transfer(&staged, target_dir, &Move, &mut Counting, when_taken)?;
    Ok(done)
}

fn fetch_folder<P: Progress>(
    link: &Link<'_>,
    remote: &RemotePath,
    local: &Path,
    progress: &mut P,
    done: &mut usize,
) -> io::Result<()> {
    std::fs::create_dir(local)?;
    for (name, meta) in link.names_in(remote)? {
        progress.keep_going()?;
        ensure_safe(&name)?;
        let child = remote.join(&name);
        let kind = meta.file_type();
        if kind.is_dir() {
            fetch_folder(link, &child, &local.join(&name), progress, done)?;
        } else if kind.is_file() {
            fetch_file(link, &child, &local.join(&name), meta.mtime, progress, done)?;
        }
    }
    Ok(())
}

fn fetch_file<P: Progress>(
    link: &Link<'_>,
    remote: &RemotePath,
    local: &Path,
    mtime: Option<u32>,
    progress: &mut P,
    done: &mut usize,
) -> io::Result<()> {
    progress.keep_going()?;
    let sftp = link.session.sftp();
    let mut input = link.call(link.timeouts.request, sftp.open(remote.remote_str()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(local)?;
    let mut block: Vec<u8> = vec![0; CHUNK];
    loop {
        progress.keep_going()?;
        let read = link.call_io(link.timeouts.block, input.read(&mut block))?;
        let Some(data) = block.get(..read).filter(|data| !data.is_empty()) else {
            break;
        };
        output.write_all(data)?;
    }
    // A close that fails changes nothing: every block is here.
    let _closed = link.call_io(link.timeouts.request, input.close());
    if let Some(secs) = mtime {
        // A time that cannot be set is a nicety lost, not a failed copy.
        let _dated = output.set_modified(UNIX_EPOCH + Duration::from_secs(u64::from(secs)));
    }
    *done += 1;
    progress.advanced(*done, 0)
}

/// Copies the file at `source` into `slot` for the viewer and returns the path
/// of the copy, readable by its owner only. Whatever goes wrong, `slot` is gone
/// when this returns an error.
pub fn download_for_view<P: Progress>(
    pool: &Pool,
    source: &RemotePath,
    slot: &Path,
    progress: &mut P,
) -> io::Result<PathBuf> {
    let result = fetch_for_view(pool, source, slot, progress);
    if result.is_err() {
        let _best_effort = std::fs::remove_dir_all(slot);
    }
    result
}

fn fetch_for_view<P: Progress>(
    pool: &Pool,
    source: &RemotePath,
    slot: &Path,
    progress: &mut P,
) -> io::Result<PathBuf> {
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the entry has no name"))?;
    download(pool, source, slot, progress, OnConflict::Fail)?;
    let copy = slot.join(name);
    restrict_to_owner(&copy, 0o600)?;
    // A cancel that came while the last block was written ends here.
    progress.keep_going()?;
    Ok(copy)
}

/// What a transfer between `from` and `to` does when a server is on either
/// side, reporting progress as `fs::transfer::run_reporting` does. `None` when
/// both are paths on this computer: the caller does it then. A move across a
/// server is not offered, and neither is a copy from one server to another.
pub fn run_reporting(
    pool: &Pool,
    from: &Path,
    to: &Path,
    kind: TransferKind,
    conflict: OnConflict,
    sender: &mpsc::UnboundedSender<Tick>,
) -> Option<io::Result<usize>> {
    let (source, target) = (RemotePath::parse(from), RemotePath::parse(to));
    if source.is_none() && target.is_none() {
        return None;
    }
    let unsupported = |what: &str| {
        Some(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            what.to_owned(),
        )))
    };
    if kind == TransferKind::Move {
        return unsupported("moving to or from a server is not supported; copy instead");
    }
    let mut reporting = Reporting::new(sender);
    match (source, target) {
        (Some(_), Some(_)) => unsupported("copying between servers is not supported"),
        (Some(there), None) => Some(download(pool, &there, to, &mut reporting, conflict)),
        (None, Some(there)) => Some(upload(pool, from, &there, &mut reporting, conflict)),
        (None, None) => None,
    }
}

/// A hidden name for what is being written, unlikely to be taken. The open
/// that creates it is exclusive anyway.
fn unique() -> String {
    use std::hash::{BuildHasher as _, Hasher as _, RandomState};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", hasher.finish())
}

/// Copies the local file or folder `source` into `target_dir` on the server,
/// keeping its name, and returns how many files it wrote.
///
/// A new folder is built under a hidden name and renamed in one step. A file
/// that replaces another is removed from the way first, because the SFTP
/// library offers no atomic rename over an existing file: a failure between
/// the two steps leaves the new copy under its hidden name and says so.
pub fn upload<P: Progress>(
    pool: &Pool,
    source: &Path,
    target_dir: &RemotePath,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    let name = source
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| is_plain_name(name))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the name cannot be sent to a server",
            )
        })?;
    let meta = std::fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "links are not copied to a server",
        ));
    }
    let target = target_dir.join(name);
    let link = Link::new(pool, target_dir.server())?;
    let existing = link.lstat(&target)?;
    if existing.is_some() {
        match conflict {
            OnConflict::Fail => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists", target.to_path_buf().display()),
                ))
            }
            OnConflict::Skip => return Ok(0),
            OnConflict::Overwrite => {}
        }
    }
    let mut upload = Upload {
        link: &link,
        progress,
        done: 0,
        partials: Vec::new(),
    };
    let result = upload.place(
        source,
        meta.is_dir(),
        target_dir,
        &target,
        existing.as_ref(),
    );
    if result.is_err() {
        upload.clean_up();
    }
    result.map(|()| upload.done)
}

struct Upload<'a, 'b, P> {
    link: &'a Link<'b>,
    progress: &'a mut P,
    done: usize,
    /// Hidden names this upload created on the server and has not yet renamed.
    partials: Vec<RemotePath>,
}

impl<P: Progress> Upload<'_, '_, P> {
    fn place(
        &mut self,
        source: &Path,
        is_dir: bool,
        target_dir: &RemotePath,
        target: &RemotePath,
        existing: Option<&Metadata>,
    ) -> io::Result<()> {
        let taken_by_folder = existing.is_some_and(|found| found.file_type().is_dir());
        match (is_dir, existing) {
            (false, _) if taken_by_folder => Err(in_the_way("a folder")),
            (false, _) => self.file(source, target, existing.is_some()),
            (true, Some(_)) if !taken_by_folder => Err(in_the_way("a file")),
            (true, Some(_)) => self.folder(source, target),
            (true, None) => {
                let hidden = target_dir.join(&format!(".ncrs-partial-{}", unique()));
                self.make_folder(&hidden)?;
                self.partials.push(hidden.clone());
                self.folder(source, &hidden)?;
                self.rename(&hidden, target)?;
                self.partials.retain(|kept| *kept != hidden);
                Ok(())
            }
        }
    }

    fn make_folder(&self, folder: &RemotePath) -> io::Result<()> {
        let sftp = self.link.session.sftp();
        self.link.call(
            self.link.timeouts.request,
            sftp.create_dir(folder.remote_str()),
        )
    }

    fn rename(&self, from: &RemotePath, to: &RemotePath) -> io::Result<()> {
        let sftp = self.link.session.sftp();
        self.link.call(
            self.link.timeouts.request,
            sftp.rename(from.remote_str(), to.remote_str()),
        )
    }

    /// Puts the content of the local folder into the folder on the server,
    /// making the folders that are missing and replacing the files that are
    /// there.
    fn folder(&mut self, local: &Path, remote: &RemotePath) -> io::Result<()> {
        for listed in std::fs::read_dir(local)? {
            self.progress.keep_going()?;
            let item = listed?;
            let meta = std::fs::symlink_metadata(item.path())?;
            if meta.file_type().is_symlink() || !(meta.is_dir() || meta.is_file()) {
                continue;
            }
            let name = item
                .file_name()
                .to_str()
                .filter(|name| is_plain_name(name))
                .map(str::to_owned)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a name cannot be sent to a server",
                    )
                })?;
            let child = remote.join(&name);
            let there = self.link.lstat(&child)?;
            let is_folder = there
                .as_ref()
                .is_some_and(|found| found.file_type().is_dir());
            if meta.is_dir() {
                match there {
                    None => self.make_folder(&child)?,
                    Some(_) if is_folder => {}
                    Some(_) => return Err(in_the_way("a file")),
                }
                self.folder(&item.path(), &child)?;
            } else if is_folder {
                return Err(in_the_way("a folder"));
            } else {
                self.file(&item.path(), &child, there.is_some())?;
            }
        }
        Ok(())
    }

    fn file(&mut self, local: &Path, remote: &RemotePath, replaces: bool) -> io::Result<()> {
        let name = remote.file_name().unwrap_or_default();
        let parent = remote
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder to write in"))?;
        let hidden = parent.join(&format!(".ncrs-partial-{}-{name}", unique()));
        let link = self.link;
        let sftp = link.session.sftp();
        let mut input = std::fs::File::open(local)?;
        let mut output = link.call(
            link.timeouts.request,
            sftp.open_with_flags(
                hidden.remote_str(),
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
            ),
        )?;
        self.partials.push(hidden.clone());
        let mut block: Vec<u8> = vec![0; CHUNK];
        loop {
            self.progress.keep_going()?;
            let read = input.read(&mut block)?;
            let Some(data) = block.get(..read).filter(|data| !data.is_empty()) else {
                break;
            };
            link.call_io(link.timeouts.block, output.write_all(data))?;
        }
        link.call_io(link.timeouts.request, output.shutdown())?;
        if replaces {
            let removed = link.call(link.timeouts.request, sftp.remove_file(remote.remote_str()));
            match removed {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        if let Err(error) = self.rename(&hidden, remote) {
            if replaces {
                // The old file is gone, so the new copy is all that is left.
                self.partials.retain(|kept| *kept != hidden);
                return Err(io::Error::other(format!(
                    "the new copy stays on the server as {}: {error}",
                    hidden.remote_str()
                )));
            }
            return Err(error);
        }
        self.partials.retain(|kept| *kept != hidden);
        self.done += 1;
        self.progress.advanced(self.done, 0)
    }

    /// Removes what this upload left under hidden names. Best effort: the
    /// connection may be what failed.
    fn clean_up(&mut self) {
        if !self.link.pool.is_connected(self.link.server) {
            return;
        }
        for hidden in std::mem::take(&mut self.partials) {
            remove_tree(self.link, &hidden);
        }
    }
}

fn in_the_way(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{what} of that name is in the way on the server"),
    )
}

/// Removes `path` and what is below it, links as links. Only called on hidden
/// names this process made.
fn remove_tree(link: &Link<'_>, path: &RemotePath) {
    let Ok(Some(meta)) = link.lstat(path) else {
        return;
    };
    let sftp = link.session.sftp();
    if meta.file_type().is_dir() {
        for (name, _) in link.names_in(path).unwrap_or_default() {
            if is_plain_name(&name) {
                remove_tree(link, &path.join(&name));
            }
        }
        let _best_effort = link.call(link.timeouts.request, sftp.remove_dir(path.remote_str()));
    } else {
        let _best_effort = link.call(link.timeouts.request, sftp.remove_file(path.remote_str()));
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
