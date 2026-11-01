//! An SSH and SFTP server for the tests, served from a temporary folder and
//! reached through an in-memory pipe: no socket, no network, no user files.

// A failing assertion is the signal in a test, and this file is test code only.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashMap;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::PrivateKey;
use russh::server::{Auth, ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
};

use super::known_hosts::KnownHostsFiles;
use super::session::{AsyncReadWrite, BoxFuture, Dial, Pool};
use super::{ServerId, Timeouts};

pub const USER: &str = "tester";
pub const PASSWORD: &str = "correct horse";
/// The login's home folder on the server.
pub const HOME: &str = "/home/tester";

const KEY_SEED: [u8; 32] = [7; 32];
const OTHER_KEY_SEED: [u8; 32] = [9; 32];

/// What the fixtures use unless a test says otherwise: long, so that a slow
/// runner or a debug build never turns a test into a timeout by accident.
pub fn timeouts() -> Timeouts {
    Timeouts {
        connect: Duration::from_secs(10),
        request: Duration::from_secs(10),
        block: Duration::from_secs(10),
        list: Duration::from_secs(10),
        links: Duration::from_secs(5),
    }
}

/// For the tests that wait for a server that never answers: they pass these,
/// and only they.
pub fn short_timeouts() -> Timeouts {
    Timeouts {
        request: Duration::from_millis(300),
        block: Duration::from_millis(300),
        list: Duration::from_millis(300),
        ..timeouts()
    }
}

/// An ECDSA host key made once for the tests with `ssh-keygen`; it protects
/// nothing and belongs to no machine.
const ECDSA_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAaAAAABNlY2RzYS
1zaGEyLW5pc3RwMjU2AAAACG5pc3RwMjU2AAAAQQRbcwcXCKTXAMcSfZyVsBTkLh4WsvmM
ABZnjIuJSHfmIlR6Ot0dmeeK9M6UGXQssoGuel9H/tGE0gfKpukCzuvVAAAAqHjwx6B48M
egAAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFtzBxcIpNcAxxJ9
nJWwFOQuHhay+YwAFmeMi4lId+YiVHo63R2Z54r0zpQZdCyyga56X0f+0YTSB8qm6QLO69
UAAAAgLx1IdK6bbUOxgOA2KGgJWKLrO7KLTBjialF5Y/hqwcMAAAAJbmNycy10ZXN0AQID
BAUGBw==
-----END OPENSSH PRIVATE KEY-----
";

/// Its public half, as a known_hosts file spells it.
pub const ECDSA_PUBLIC: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFtzBxcIpNcAxxJ9nJWwFOQuHhay+YwAFmeMi4lId+YiVHo63R2Z54r0zpQZdCyyga56X0f+0YTSB8qm6QLO69U=";

pub fn ecdsa_host_key() -> PrivateKey {
    PrivateKey::from_openssh(ECDSA_KEY).unwrap()
}

pub fn host_key(seed: &[u8; 32]) -> PrivateKey {
    PrivateKey::from(Ed25519Keypair::from_seed(seed))
}

pub fn public_key_text() -> String {
    host_key(&KEY_SEED)
        .public_key()
        .to_openssh()
        .unwrap_or_default()
}

pub fn other_public_key_text() -> String {
    host_key(&OTHER_KEY_SEED)
        .public_key()
        .to_openssh()
        .unwrap_or_default()
}

/// What the server saw and how it misbehaves on request.
#[derive(Debug, Default)]
pub struct Control {
    /// "password" is recorded when a client sends one, so a test can tell that
    /// none was sent to a server it did not trust.
    events: Mutex<Vec<&'static str>>,
    served: AtomicU64,
    stall_after: AtomicU64,
    stall_listing: std::sync::atomic::AtomicBool,
    stall_stat: std::sync::atomic::AtomicBool,
    dials: AtomicUsize,
    handles: Mutex<Vec<russh::server::Handle>>,
}

impl Control {
    fn log(&self, event: &'static str) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    pub fn events(&self) -> Vec<&'static str> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// After this many bytes were read from files, a read never answers.
    pub fn stall_after(&self, bytes: u64) {
        self.stall_after.store(bytes + 1, Ordering::SeqCst);
    }

    pub fn stall_listing(&self) {
        self.stall_listing.store(true, Ordering::SeqCst);
    }

    /// `stat` (what following a link asks) never answers.
    pub fn stall_stat(&self) {
        self.stall_stat.store(true, Ordering::SeqCst);
    }

    pub fn dials(&self) -> usize {
        self.dials.load(Ordering::SeqCst)
    }

    /// Ends every connection the way a dropped network does.
    pub async fn cut(&self) {
        let handles: Vec<_> =
            std::mem::take(&mut *self.handles.lock().unwrap_or_else(PoisonError::into_inner));
        for handle in handles {
            let _ = handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "cut".to_owned(),
                    "en".to_owned(),
                )
                .await;
        }
    }
}

#[derive(Debug)]
pub struct TestServer {
    pub root: tempfile::TempDir,
    pub control: Arc<Control>,
    pub password: Arc<Mutex<String>>,
    /// The host keys it has, the first one being its own preference.
    pub keys: Vec<PrivateKey>,
    /// Whether it takes a password; without, only keyboard-interactive.
    pub password_login: bool,
}

impl TestServer {
    pub fn new() -> Arc<Self> {
        Self::with_keys(vec![host_key(&KEY_SEED)], true)
    }

    /// A server that offers another host key than the usual one.
    pub fn with_other_key() -> Arc<Self> {
        Self::with_keys(vec![host_key(&OTHER_KEY_SEED)], true)
    }

    /// A server with an ed25519 and an ECDSA host key: which one it offers
    /// depends on what the client asks for first.
    pub fn with_two_keys() -> Arc<Self> {
        Self::with_keys(vec![host_key(&KEY_SEED), ecdsa_host_key()], true)
    }

    /// A server that does not take a password.
    pub fn without_password_login() -> Arc<Self> {
        Self::with_keys(vec![host_key(&KEY_SEED)], false)
    }

    fn with_keys(keys: Vec<PrivateKey>, password_login: bool) -> Arc<Self> {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("a scratch folder: {error}"));
        std::fs::create_dir_all(root.path().join("home/tester"))
            .unwrap_or_else(|error| panic!("the home folder: {error}"));
        Arc::new(Self {
            root,
            control: Arc::new(Control::default()),
            password: Arc::new(Mutex::new(PASSWORD.to_owned())),
            keys,
            password_login,
        })
    }

    pub fn server_id() -> ServerId {
        ServerId::new(USER, "test.invalid", 22)
    }

    /// The local file behind a server path.
    pub fn local(&self, remote: &str) -> PathBuf {
        self.root.path().join(remote.trim_start_matches('/'))
    }

    pub fn pool(self: &Arc<Self>, known: KnownHostsFiles) -> Arc<Pool> {
        self.pool_with(known, timeouts())
    }

    pub fn pool_with(self: &Arc<Self>, known: KnownHostsFiles, limits: Timeouts) -> Arc<Pool> {
        Arc::new(Pool::new(
            Arc::new(DuplexDial(Arc::clone(self))),
            known,
            limits,
        ))
    }
}

struct DuplexDial(Arc<TestServer>);

impl Dial for DuplexDial {
    fn dial<'a>(
        &'a self,
        _server: &'a ServerId,
    ) -> BoxFuture<'a, std::io::Result<Box<dyn AsyncReadWrite>>> {
        Box::pin(async move {
            let server = &self.0;
            server.control.dials.fetch_add(1, Ordering::SeqCst);
            let (client, served) = tokio::io::duplex(1 << 16);
            let config = Arc::new(russh::server::Config {
                keys: server.keys.clone(),
                methods: if server.password_login {
                    russh::MethodSet::from(
                        &[
                            russh::MethodKind::Password,
                            russh::MethodKind::PublicKey,
                            russh::MethodKind::KeyboardInteractive,
                        ][..],
                    )
                } else {
                    russh::MethodSet::from(&[russh::MethodKind::KeyboardInteractive][..])
                },
                auth_rejection_time: Duration::from_millis(1),
                auth_rejection_time_initial: Some(Duration::ZERO),
                ..russh::server::Config::default()
            });
            let handler = Ssh {
                root: server.root.path().to_path_buf(),
                control: Arc::clone(&server.control),
                password: Arc::clone(&server.password),
                channels: HashMap::new(),
            };
            // The server waits for the client's greeting before it returns, so
            // it runs on a task of its own while the client is handed out.
            let control = Arc::clone(&server.control);
            tokio::spawn(async move {
                if let Ok(running) = russh::server::run_stream(config, served, handler).await {
                    control
                        .handles
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(running.handle());
                    let _ended = running.await;
                    control.log("closed");
                }
            });
            Ok(Box::new(client) as Box<dyn AsyncReadWrite>)
        })
    }
}

struct Ssh {
    root: PathBuf,
    control: Arc<Control>,
    password: Arc<Mutex<String>>,
    channels: HashMap<ChannelId, Channel<Msg>>,
}

impl russh::server::Handler for Ssh {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.control.log("password");
        let expected = self
            .password
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if user == USER && password == expected {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        match (name, self.channels.remove(&channel)) {
            ("sftp", Some(opened)) => {
                session.channel_success(channel)?;
                let fs = Fs {
                    root: self.root.clone(),
                    control: Arc::clone(&self.control),
                    handles: HashMap::new(),
                    next: 0,
                };
                tokio::spawn(russh_sftp::server::run(opened.into_stream(), fs));
            }
            _ => session.channel_failure(channel)?,
        }
        Ok(())
    }
}

enum Open {
    Dir(Option<Vec<File>>),
    File(std::fs::File),
}

struct Fs {
    root: PathBuf,
    control: Arc<Control>,
    handles: HashMap<String, Open>,
    next: u64,
}

fn status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_owned(),
        language_tag: "en-US".to_owned(),
    }
}

fn code(error: &std::io::Error) -> StatusCode {
    match error.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

impl Fs {
    fn path(&self, remote: &str) -> PathBuf {
        let relative = if remote == "." { HOME } else { remote };
        self.root.join(relative.trim_start_matches('/'))
    }

    fn remember(&mut self, open: Open) -> String {
        self.next += 1;
        let handle = format!("h{}", self.next);
        self.handles.insert(handle.clone(), open);
        handle
    }
}

fn attrs_of(meta: &std::fs::Metadata) -> FileAttributes {
    FileAttributes::from(meta)
}

impl russh_sftp::server::Handler for Fs {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        self.control.log("open");
        let mut options = std::fs::OpenOptions::new();
        options
            .read(pflags.contains(OpenFlags::READ))
            .write(pflags.contains(OpenFlags::WRITE));
        if pflags.contains(OpenFlags::CREATE) && pflags.contains(OpenFlags::EXCLUDE) {
            options.create_new(true);
        } else if pflags.contains(OpenFlags::CREATE) {
            options.create(true);
        }
        if pflags.contains(OpenFlags::TRUNCATE) {
            options.truncate(true);
        }
        let file = options.open(self.path(&filename)).map_err(|e| code(&e))?;
        Ok(Handle {
            id,
            handle: self.remember(Open::File(file)),
        })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.handles.remove(&handle);
        Ok(status(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let limit = self.control.stall_after.load(Ordering::SeqCst);
        if limit > 0 && self.control.served.load(Ordering::SeqCst) >= limit - 1 {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        let Some(Open::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset)).map_err(|e| code(&e))?;
        let mut data = vec![0; len as usize];
        let read = file.read(&mut data).map_err(|e| code(&e))?;
        if read == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(read);
        self.control.served.fetch_add(read as u64, Ordering::SeqCst);
        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let Some(Open::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset)).map_err(|e| code(&e))?;
        file.write_all(&data).map_err(|e| code(&e))?;
        Ok(status(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let meta = std::fs::symlink_metadata(self.path(&path)).map_err(|e| code(&e))?;
        Ok(Attrs {
            id,
            attrs: attrs_of(&meta),
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        if self.control.stall_stat.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        let meta = std::fs::metadata(self.path(&path)).map_err(|e| code(&e))?;
        Ok(Attrs {
            id,
            attrs: attrs_of(&meta),
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        self.control.log("opendir");
        let mut files = vec![
            File::new(".", FileAttributes::dummy()),
            File::new("..", FileAttributes::dummy()),
        ];
        for item in std::fs::read_dir(self.path(&path)).map_err(|e| code(&e))? {
            let found = item.map_err(|e| code(&e))?;
            let meta = std::fs::symlink_metadata(found.path()).map_err(|e| code(&e))?;
            files.push(File::new(
                found.file_name().to_string_lossy().into_owned(),
                attrs_of(&meta),
            ));
        }
        Ok(Handle {
            id,
            handle: self.remember(Open::Dir(Some(files))),
        })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if self.control.stall_listing.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        match self.handles.get_mut(&handle) {
            Some(Open::Dir(pending)) => match pending.take() {
                Some(files) => Ok(Name { id, files }),
                None => Err(StatusCode::Eof),
            },
            _ => Err(StatusCode::Failure),
        }
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        self.control.log("mkdir");
        std::fs::create_dir(self.path(&path)).map_err(|e| code(&e))?;
        Ok(status(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        std::fs::remove_dir(self.path(&path)).map_err(|e| code(&e))?;
        Ok(status(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        self.control.log("remove");
        std::fs::remove_file(self.path(&filename)).map_err(|e| code(&e))?;
        Ok(status(id))
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        self.control.log("rename");
        let to = self.path(&newpath);
        if to.exists() {
            return Err(StatusCode::Failure);
        }
        std::fs::rename(self.path(&oldpath), to).map_err(|e| code(&e))?;
        Ok(status(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let resolved = if path == "." { HOME.to_owned() } else { path };
        Ok(Name {
            id,
            files: vec![File::dummy(resolved)],
        })
    }
}

/// A server, a pool in front of it and the files for the host keys, all in a
/// scratch folder.
#[derive(Debug)]
pub struct Fixture {
    pub server: Arc<TestServer>,
    pub pool: Arc<Pool>,
    pub scratch: tempfile::TempDir,
    pub known: KnownHostsFiles,
}

impl Fixture {
    /// Its host key is in the file of ncrs already.
    pub fn trusted() -> Self {
        let fixture = Self::untrusted();
        std::fs::write(
            fixture.known.own.as_deref().unwrap(),
            format!("test.invalid {}\n", public_key_text()),
        )
        .unwrap();
        fixture
    }

    /// No file knows the host key.
    pub fn untrusted() -> Self {
        Self::over(TestServer::new())
    }

    pub fn over(server: Arc<TestServer>) -> Self {
        Self::over_with(server, timeouts())
    }

    /// Trusted, with the limits of the test.
    pub fn trusted_with(limits: Timeouts) -> Self {
        let fixture = Self::over_with(TestServer::new(), limits);
        std::fs::write(
            fixture.known.own.as_deref().unwrap(),
            format!("test.invalid {}\n", public_key_text()),
        )
        .unwrap();
        fixture
    }

    /// Trusted, logged in, with the limits of the test.
    pub async fn connected_with(limits: Timeouts) -> Self {
        let fixture = Self::trusted_with(limits);
        fixture
            .pool
            .connect(&Self::id(), &Self::password())
            .await
            .unwrap();
        fixture
    }

    pub fn over_with(server: Arc<TestServer>, limits: Timeouts) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let known = KnownHostsFiles {
            ssh: Some(scratch.path().join("ssh_known_hosts")),
            own: Some(scratch.path().join("known_hosts")),
        };
        let pool = server.pool_with(known.clone(), limits);
        Self {
            server,
            pool,
            scratch,
            known,
        }
    }

    pub fn id() -> ServerId {
        TestServer::server_id()
    }

    pub fn password() -> super::Secret {
        super::Secret::new(PASSWORD.to_owned())
    }

    /// Trusted, and logged in.
    pub async fn connected() -> Self {
        let fixture = Self::trusted();
        fixture
            .pool
            .connect(&Self::id(), &Self::password())
            .await
            .unwrap();
        fixture
    }

    /// A place on the server.
    pub fn at(path: &str) -> super::RemotePath {
        super::RemotePath::new(Self::id(), path)
    }
}

/// A link at `at` that leads to `target`. A no-op where links need rights.
#[cfg(unix)]
pub fn symlink(target: &Path, at: &Path) {
    std::os::unix::fs::symlink(target, at).unwrap()
}

#[cfg(not(unix))]
pub fn symlink(_target: &Path, _at: &Path) {}

/// Lists the names directly in `dir`, sorted. For the tests' assertions.
pub fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}
