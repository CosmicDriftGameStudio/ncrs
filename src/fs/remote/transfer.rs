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
use tokio::time::timeout;

use super::list::is_plain_name;
use super::session::{Pool, RemoteError, Session};
use super::{is_deceptive, RemotePath, ServerId, Timeouts, CHUNK};
use crate::fs::archive::{restrict_to_owner, safe_components};
use crate::fs::transfer::{resolve_conflict, transfer, Counting, Move, OnConflict, Progress};

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
mod tests {
    use super::super::test_server::{names_in, short_timeouts, symlink, Fixture};
    use super::*;

    /// Stops the transfer at the `after`th check.
    struct StopAt {
        after: usize,
        calls: usize,
        saw: Option<Box<dyn FnMut(usize) + Send>>,
    }

    impl StopAt {
        fn never() -> Self {
            Self {
                after: usize::MAX,
                calls: 0,
                saw: None,
            }
        }

        fn at(after: usize) -> Self {
            Self {
                after,
                calls: 0,
                saw: None,
            }
        }
    }

    impl Progress for StopAt {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            Ok(())
        }

        fn keep_going(&mut self) -> io::Result<()> {
            self.calls += 1;
            if let Some(saw) = self.saw.as_mut() {
                saw(self.calls);
            }
            if self.calls >= self.after {
                return Err(io::Error::other("stopped"));
            }
            Ok(())
        }
    }

    fn bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect()
    }

    async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
        tokio::task::spawn_blocking(work).await.unwrap()
    }

    async fn pull(fixture: &Fixture, remote: &str, target_dir: &Path) -> io::Result<usize> {
        pull_with(fixture, remote, target_dir, OnConflict::Fail).await
    }

    async fn pull_with(
        fixture: &Fixture,
        remote: &str,
        target_dir: &Path,
        how: OnConflict,
    ) -> io::Result<usize> {
        let pool = Arc::clone(&fixture.pool);
        let (into, source) = (target_dir.to_path_buf(), Fixture::at(remote));
        blocking(move || download(&pool, &source, &into, &mut Counting, how)).await
    }

    async fn push(
        fixture: &Fixture,
        local: &Path,
        remote_dir: &str,
        how: OnConflict,
    ) -> io::Result<usize> {
        let pool = Arc::clone(&fixture.pool);
        let (from, dir) = (local.to_path_buf(), Fixture::at(remote_dir));
        blocking(move || upload(&pool, &from, &dir, &mut Counting, how)).await
    }

    fn out_dir(fixture: &Fixture) -> PathBuf {
        let out = fixture.scratch.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        out
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn download_file_streams_in_blocks_and_lands_under_its_name() {
        let fixture = Fixture::connected().await;
        let content = bytes(CHUNK * 2 + 1000);
        std::fs::write(fixture.server.local("/home/tester/big.bin"), &content).unwrap();
        let out = out_dir(&fixture);
        let (pool, target) = (Arc::clone(&fixture.pool), out.clone());
        let source = Fixture::at("/home/tester/big.bin");

        let (written, calls) = blocking(move || {
            let mut progress = StopAt::never();
            let written =
                download(&pool, &source, &target, &mut progress, OnConflict::Fail).unwrap();
            (written, progress.calls)
        })
        .await;

        assert_eq!(written, 1);
        assert_eq!(std::fs::read(out.join("big.bin")).unwrap(), content);
        assert!(
            calls >= 4,
            "asked to go on once per block, not once per file: {calls}"
        );
        assert_eq!(names_in(&out), ["big.bin"], "no staging folder is left");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn download_folder_skips_server_symlinks() {
        let fixture = Fixture::connected().await;
        let docs = fixture.server.local("/home/tester/docs");
        std::fs::create_dir_all(docs.join("sub")).unwrap();
        std::fs::write(docs.join("a.txt"), "a").unwrap();
        std::fs::write(docs.join("sub/b.txt"), "b").unwrap();
        std::fs::write(fixture.server.local("/secret.txt"), "secret").unwrap();
        symlink(&fixture.server.local("/secret.txt"), &docs.join("link"));
        symlink(&fixture.server.local("/home"), &docs.join("dirlink"));
        let out = out_dir(&fixture);

        let written = pull(&fixture, "/home/tester/docs", &out).await.unwrap();

        assert_eq!(written, 2);
        assert_eq!(names_in(&out.join("docs")), ["a.txt", "sub"]);
        assert_eq!(std::fs::read(out.join("docs/sub/b.txt")).unwrap(), b"b");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_link_asked_for_directly_is_not_followed() {
        let fixture = Fixture::connected().await;
        std::fs::write(fixture.server.local("/secret.txt"), "secret").unwrap();
        symlink(
            &fixture.server.local("/secret.txt"),
            &fixture.server.local("/home/tester/link"),
        );
        let out = out_dir(&fixture);

        let error = pull(&fixture, "/home/tester/link", &out).await.unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(names_in(&out).is_empty());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn download_unsafe_name_is_refused_and_leaves_nothing() {
        for unsafe_one in ["a\\b", "C:", "x\u{202E}y", "bell\u{7}"] {
            let fixture = Fixture::connected().await;
            let docs = fixture.server.local("/home/tester/docs");
            std::fs::create_dir_all(&docs).unwrap();
            std::fs::write(docs.join("fine.txt"), "ok").unwrap();
            std::fs::write(docs.join(unsafe_one), "bad").unwrap();
            let out = out_dir(&fixture);

            let error = pull(&fixture, "/home/tester/docs", &out).await.unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{unsafe_one}");
            let named = error
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<UnsafeName>());
            assert_eq!(named.map(|name| name.0.as_str()), Some(unsafe_one));
            assert!(
                names_in(&out).is_empty(),
                "{unsafe_one}: {:?}",
                names_in(&out)
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn download_abort_leaves_no_partial_file() {
        let fixture = Fixture::connected().await;
        std::fs::write(
            fixture.server.local("/home/tester/big.bin"),
            bytes(CHUNK * 3),
        )
        .unwrap();
        let out = out_dir(&fixture);
        let (pool, target) = (Arc::clone(&fixture.pool), out.clone());
        let source = Fixture::at("/home/tester/big.bin");

        let result = blocking(move || {
            download(
                &pool,
                &source,
                &target,
                &mut StopAt::at(3),
                OnConflict::Fail,
            )
        })
        .await;

        assert_eq!(result.unwrap_err().to_string(), "stopped");
        assert!(names_in(&out).is_empty(), "{:?}", names_in(&out));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_download_over_an_existing_name_follows_the_conflict_rule() {
        let fixture = Fixture::connected().await;
        std::fs::write(fixture.server.local("/home/tester/x.txt"), "new").unwrap();
        let out = out_dir(&fixture);
        std::fs::write(out.join("x.txt"), "old").unwrap();

        let error = pull(&fixture, "/home/tester/x.txt", &out)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(out.join("x.txt")).unwrap(), b"old");

        let skipped = pull_with(&fixture, "/home/tester/x.txt", &out, OnConflict::Skip)
            .await
            .unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(std::fs::read(out.join("x.txt")).unwrap(), b"old");

        pull_with(&fixture, "/home/tester/x.txt", &out, OnConflict::Overwrite)
            .await
            .unwrap();
        assert_eq!(std::fs::read(out.join("x.txt")).unwrap(), b"new");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn upload_file_goes_through_partial_and_rename() {
        let fixture = Fixture::connected().await;
        let content = bytes(CHUNK + 500);
        let local = fixture.scratch.path().join("up.bin");
        std::fs::write(&local, &content).unwrap();
        let home = fixture.server.local("/home/tester");
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
        let (pool, dir) = (Arc::clone(&fixture.pool), Fixture::at("/home/tester"));
        let (watched, folder) = (Arc::clone(&seen), home.clone());

        blocking(move || {
            let mut progress = StopAt::never();
            progress.saw = Some(Box::new(move |_| {
                watched.lock().unwrap().push(names_in(&folder));
            }));
            upload(&pool, &local, &dir, &mut progress, OnConflict::Fail).unwrap()
        })
        .await;

        assert_eq!(std::fs::read(home.join("up.bin")).unwrap(), content);
        assert_eq!(names_in(&home), ["up.bin"], "no hidden file is left");
        let snapshots = seen.lock().unwrap().clone();
        assert!(
            snapshots.iter().any(|names| {
                names.len() == 1
                    && names[0].starts_with(".ncrs-partial-")
                    && names[0].ends_with("-up.bin")
            }),
            "while it was written it had a hidden name: {snapshots:?}"
        );
        assert!(snapshots
            .iter()
            .all(|names| !names.contains(&"up.bin".to_owned())));
        let events = fixture.server.control.events();
        let open = events.iter().position(|event| *event == "open").unwrap();
        let rename = events.iter().position(|event| *event == "rename").unwrap();
        assert!(open < rename);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn upload_conflict_fail_skip_overwrite() {
        let fixture = Fixture::connected().await;
        let home = fixture.server.local("/home/tester");
        std::fs::write(home.join("same.txt"), "old").unwrap();
        let local = fixture.scratch.path().join("same.txt");
        std::fs::write(&local, "new").unwrap();

        let failed = push(&fixture, &local, "/home/tester", OnConflict::Fail)
            .await
            .unwrap_err();
        assert_eq!(failed.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(home.join("same.txt")).unwrap(), b"old");

        let skipped = push(&fixture, &local, "/home/tester", OnConflict::Skip)
            .await
            .unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(std::fs::read(home.join("same.txt")).unwrap(), b"old");

        let written = push(&fixture, &local, "/home/tester", OnConflict::Overwrite)
            .await
            .unwrap();
        assert_eq!(written, 1);
        assert_eq!(std::fs::read(home.join("same.txt")).unwrap(), b"new");
        assert_eq!(names_in(&home), ["same.txt"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn upload_abort_removes_partial_on_server() {
        let fixture = Fixture::connected().await;
        let local = fixture.scratch.path().join("big.bin");
        std::fs::write(&local, bytes(CHUNK * 3)).unwrap();
        let home = fixture.server.local("/home/tester");
        let (pool, dir) = (Arc::clone(&fixture.pool), Fixture::at("/home/tester"));

        let result =
            blocking(move || upload(&pool, &local, &dir, &mut StopAt::at(3), OnConflict::Fail))
                .await;

        assert_eq!(result.unwrap_err().to_string(), "stopped");
        assert!(names_in(&home).is_empty(), "{:?}", names_in(&home));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn upload_folder_is_built_hidden_and_renamed_once() {
        let fixture = Fixture::connected().await;
        let tree = fixture.scratch.path().join("tree");
        std::fs::create_dir_all(tree.join("inner")).unwrap();
        std::fs::write(tree.join("a.txt"), "a").unwrap();
        std::fs::write(tree.join("inner/b.txt"), "b").unwrap();
        let home = fixture.server.local("/home/tester");

        let written = push(&fixture, &tree, "/home/tester", OnConflict::Fail)
            .await
            .unwrap();

        assert_eq!(written, 2);
        assert_eq!(names_in(&home), ["tree"]);
        assert_eq!(std::fs::read(home.join("tree/inner/b.txt")).unwrap(), b"b");
        let renames = fixture
            .server
            .control
            .events()
            .iter()
            .filter(|e| **e == "rename")
            .count();
        assert!(renames >= 3, "two files and the folder: {renames}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn upload_folder_over_a_folder_merges_when_overwriting() {
        let fixture = Fixture::connected().await;
        let home = fixture.server.local("/home/tester");
        std::fs::create_dir_all(home.join("tree")).unwrap();
        std::fs::write(home.join("tree/keep.txt"), "keep").unwrap();
        std::fs::write(home.join("tree/a.txt"), "old").unwrap();
        let tree = fixture.scratch.path().join("tree");
        std::fs::create_dir_all(tree.join("new")).unwrap();
        std::fs::write(tree.join("a.txt"), "new").unwrap();
        std::fs::write(tree.join("new/c.txt"), "c").unwrap();

        let refused = push(&fixture, &tree, "/home/tester", OnConflict::Fail)
            .await
            .unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        push(&fixture, &tree, "/home/tester", OnConflict::Overwrite)
            .await
            .unwrap();

        assert_eq!(names_in(&home.join("tree")), ["a.txt", "keep.txt", "new"]);
        assert_eq!(std::fs::read(home.join("tree/a.txt")).unwrap(), b"new");
        assert_eq!(std::fs::read(home.join("tree/new/c.txt")).unwrap(), b"c");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn upload_folder_does_not_follow_local_symlinks() {
        let fixture = Fixture::connected().await;
        let outside = fixture.scratch.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        let tree = fixture.scratch.path().join("tree");
        std::fs::create_dir(&tree).unwrap();
        std::fs::write(tree.join("a.txt"), "a").unwrap();
        std::os::unix::fs::symlink(&outside, tree.join("dirlink")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), tree.join("filelink")).unwrap();

        push(&fixture, &tree, "/home/tester", OnConflict::Fail)
            .await
            .unwrap();

        let home = fixture.server.local("/home/tester");
        assert_eq!(names_in(&home.join("tree")), ["a.txt"]);

        let alone = push(
            &fixture,
            &tree.join("filelink"),
            "/home/tester",
            OnConflict::Fail,
        )
        .await
        .unwrap_err();
        assert_eq!(alone.kind(), io::ErrorKind::Unsupported);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn view_copy_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = Fixture::connected().await;
        std::fs::write(fixture.server.local("/home/tester/doc.txt"), "text").unwrap();
        let slot = fixture.scratch.path().join("slot");
        std::fs::create_dir(&slot).unwrap();
        let (pool, to) = (Arc::clone(&fixture.pool), slot.clone());
        let source = Fixture::at("/home/tester/doc.txt");

        let copy = blocking(move || download_for_view(&pool, &source, &to, &mut Counting))
            .await
            .unwrap();

        assert_eq!(copy, slot.join("doc.txt"));
        assert_eq!(std::fs::read(&copy).unwrap(), b"text");
        assert_eq!(
            std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_view_copy_removes_its_slot() {
        let fixture = Fixture::connected().await;
        let slot = fixture.scratch.path().join("slot");
        std::fs::create_dir(&slot).unwrap();
        let (pool, to) = (Arc::clone(&fixture.pool), slot.clone());
        let source = Fixture::at("/home/tester/missing.txt");

        let error = blocking(move || download_for_view(&pool, &source, &to, &mut Counting))
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!slot.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stalled_block_times_out_and_the_connection_stays() {
        let fixture = Fixture::connected_with(short_timeouts()).await;
        std::fs::write(
            fixture.server.local("/home/tester/big.bin"),
            bytes(CHUNK * 2),
        )
        .unwrap();
        fixture.server.control.stall_after(0);
        let out = out_dir(&fixture);

        let error = pull(&fixture, "/home/tester/big.bin", &out)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(fixture.pool.is_connected(&Fixture::id()));
        assert!(names_in(&out).is_empty());
    }
}
