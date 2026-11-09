//! Connections to servers, one per account, kept for the run. A connection is
//! made in steps, each of them under a time limit, and the host key is judged
//! before anything secret is sent: an unknown or changed key stops the login
//! with nothing sent.

mod login;

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::{Algorithm, HashAlg, PublicKey, PublicKeyOrCertificate};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::StatusCode;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::timeout;

use super::credentials::{KeyringPassphrases, PassphraseStore};
use super::known_hosts::{self, HostVerdict, KnownHosts};
use super::ssh_config::{self, HostSettings};
use super::{KnownHostsFiles, ServerId, Timeouts};
pub use login::{Login, LoginEnv};

pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How a connection to a server is opened. A trait so the tests can put a
/// server of their own at the other end of an in-memory pipe.
pub trait Dial: Send + Sync {
    fn dial<'a>(
        &'a self,
        server: &'a ServerId,
    ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>>;
}

/// TCP to the host and port of the server.
#[derive(Debug)]
pub struct TcpDial {
    pub timeout: Duration,
}

impl Dial for TcpDial {
    fn dial<'a>(
        &'a self,
        server: &'a ServerId,
    ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>> {
        Box::pin(async move {
            let address = (server.host.as_str(), server.port);
            let stream = timeout(self.timeout, tokio::net::TcpStream::connect(address))
                .await
                .map_err(|_elapsed| {
                    io::Error::new(io::ErrorKind::TimedOut, "connect timed out")
                })??;
            stream.set_nodelay(true)?;
            Ok(Box::new(stream) as Box<dyn AsyncReadWrite>)
        })
    }
}

/// The dial of the tests that were not given a server: it refuses, so no test
/// reaches a network.
#[derive(Debug)]
struct NoNetwork;

impl Dial for NoNetwork {
    fn dial<'a>(
        &'a self,
        _server: &'a ServerId,
    ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>> {
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "no network in tests",
            ))
        })
    }
}

/// Why a connection could not be made.
#[derive(Clone, PartialEq)]
pub enum ConnectFailure {
    /// The server's key is in no file. Nothing was sent; the user may trust it.
    UnknownHost {
        fingerprint: String,
        key: Box<PublicKey>,
    },
    HostKeyChanged {
        fingerprint: String,
        file: PathBuf,
        line: usize,
    },
    /// The server offers a key of another type than the one on record.
    HostKeyOtherType {
        fingerprint: String,
        file: PathBuf,
        line: usize,
    },
    /// A file with host keys could not be read, so the server cannot be
    /// checked.
    KnownHostsUnreadable {
        file: PathBuf,
        reason: String,
    },
    HostKeyRevoked,
    AuthFailed,
    /// A key file is encrypted and no passphrase was given or kept for it.
    PassphraseNeeded {
        key_file: PathBuf,
    },
    /// The passphrase that was given does not open the key file.
    WrongPassphrase {
        key_file: PathBuf,
    },
    /// The server does not take a password at all (key or keyboard-interactive
    /// login only), so a wrong one is not the matter.
    PasswordNotOffered,
    Timeout,
    NoCommonAlgorithm,
    Unreachable(String),
    Keychain(String),
}

impl fmt::Debug for ConnectFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHost { fingerprint, .. } => write!(f, "UnknownHost({fingerprint})"),
            Self::HostKeyChanged {
                fingerprint,
                file,
                line,
            } => {
                write!(
                    f,
                    "HostKeyChanged({fingerprint}, {}:{line})",
                    file.display()
                )
            }
            Self::HostKeyOtherType {
                fingerprint,
                file,
                line,
            } => {
                write!(
                    f,
                    "HostKeyOtherType({fingerprint}, {}:{line})",
                    file.display()
                )
            }
            Self::KnownHostsUnreadable { file, reason } => {
                write!(f, "KnownHostsUnreadable({}: {reason})", file.display())
            }
            Self::HostKeyRevoked => f.write_str("HostKeyRevoked"),
            Self::AuthFailed => f.write_str("AuthFailed"),
            Self::PassphraseNeeded { key_file } => {
                write!(f, "PassphraseNeeded({})", key_file.display())
            }
            Self::WrongPassphrase { key_file } => {
                write!(f, "WrongPassphrase({})", key_file.display())
            }
            Self::PasswordNotOffered => f.write_str("PasswordNotOffered"),
            Self::Timeout => f.write_str("Timeout"),
            Self::NoCommonAlgorithm => f.write_str("NoCommonAlgorithm"),
            Self::Unreachable(reason) => write!(f, "Unreachable({reason})"),
            Self::Keychain(reason) => write!(f, "Keychain({reason})"),
        }
    }
}

/// What a request to a server came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    NotFound,
    /// The server answered no. The connection is fine.
    Refused(String),
    Timeout,
    /// The connection is gone or broke.
    Lost(String),
}

impl RemoteError {
    pub fn of_sftp(error: &SftpError) -> Self {
        match error {
            SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile => {
                Self::NotFound
            }
            SftpError::Status(status) => Self::Refused(if status.error_message.is_empty() {
                status.status_code.to_string()
            } else {
                status.error_message.clone()
            }),
            SftpError::Timeout => Self::Timeout,
            SftpError::Limited(reason) => Self::Refused(reason.clone()),
            SftpError::IO(_) | SftpError::UnexpectedPacket | SftpError::UnexpectedBehavior(_) => {
                Self::Lost(error.to_string())
            }
        }
    }

    pub fn of_io(error: &io::Error) -> Self {
        if let Some(sftp) = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<SftpError>())
        {
            return Self::of_sftp(sftp);
        }
        match error.kind() {
            io::ErrorKind::TimedOut => Self::Timeout,
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof => Self::Lost(error.to_string()),
            _ => Self::Refused(error.to_string()),
        }
    }

    /// Whether the connection has to be dropped and made again. A request that
    /// ran out of time does not say so: the server may only be slow, and the
    /// keepalive finds a connection that is gone.
    pub fn breaks_connection(&self) -> bool {
        matches!(self, Self::Lost(_))
    }

    pub fn into_io(self) -> io::Error {
        match self {
            Self::NotFound => io::Error::new(io::ErrorKind::NotFound, "not found on the server"),
            Self::Refused(reason) => io::Error::other(reason),
            Self::Timeout => io::Error::new(io::ErrorKind::TimedOut, "the server did not answer"),
            Self::Lost(reason) => io::Error::new(io::ErrorKind::ConnectionAborted, reason),
        }
    }
}

/// An open connection with its SFTP channel.
pub struct Session {
    handle: Handle<HostCheck>,
    sftp: SftpSession,
    home: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn sftp(&self) -> &SftpSession {
        &self.sftp
    }

    /// The home folder of the login, where `~` leads.
    pub fn home(&self) -> &str {
        &self.home
    }

    fn is_alive(&self) -> bool {
        !self.handle.is_closed()
    }
}

/// What the host key check saw, for the failure to report afterwards.
struct Seen {
    verdict: HostVerdict,
    key: PublicKey,
}

/// Judges the key of the server during the key exchange, before any login.
struct HostCheck {
    known: Arc<KnownHosts>,
    host: String,
    port: u16,
    seen: Arc<Mutex<Option<Seen>>>,
}

impl client::Handler for HostCheck {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(certificate) => {
                PublicKey::from(certificate.public_key().clone())
            }
        };
        let verdict = self.known.check(&self.host, self.port, &key);
        let trusted = verdict == HostVerdict::Known;
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(Seen { verdict, key });
        Ok(trusted)
    }
}

/// A step of the connection that went wrong, before it is named for the user.
enum Step {
    PasswordNotOffered,
    PassphraseNeeded(PathBuf),
    WrongPassphrase(PathBuf),
    Io(io::Error),
    Ssh(russh::Error),
    Sftp(SftpError),
    AuthFailed,
}

impl From<io::Error> for Step {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<russh::Error> for Step {
    fn from(error: russh::Error) -> Self {
        Self::Ssh(error)
    }
}

impl From<SftpError> for Step {
    fn from(error: SftpError) -> Self {
        Self::Sftp(error)
    }
}

/// The connections of the run.
pub struct Pool {
    sessions: Mutex<HashMap<ServerId, Arc<Session>>>,
    dial: Arc<dyn Dial>,
    known: KnownHostsFiles,
    timeouts: Timeouts,
    env: LoginEnv,
    passphrases: Arc<dyn PassphraseStore>,
    /// The runtime the connections run on, noted when one is made, so that
    /// closing them can send its goodbye from a thread that is not in it.
    runtime: Mutex<Option<tokio::runtime::Handle>>,
}

impl fmt::Debug for Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("known", &self.known)
            .finish_non_exhaustive()
    }
}

impl Pool {
    pub fn new(dial: Arc<dyn Dial>, known: KnownHostsFiles, timeouts: Timeouts) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            dial,
            known,
            timeouts,
            // In the tests nothing reads the machine of the developer: a test
            // that wants a home folder or an agent puts one in.
            env: if cfg!(any(test, feature = "test-support")) {
                LoginEnv::default()
            } else {
                LoginEnv::from_process()
            },
            passphrases: Arc::new(KeyringPassphrases::new()),
            runtime: Mutex::new(None),
        }
    }

    /// The same pool reading its home folder, agent and user name from `env`.
    #[must_use]
    pub fn with_login_env(mut self, env: LoginEnv) -> Self {
        self.env = env;
        self
    }

    /// The same pool keeping key passphrases in `passphrases`.
    #[must_use]
    pub fn with_passphrases(mut self, passphrases: Arc<dyn PassphraseStore>) -> Self {
        self.passphrases = passphrases;
        self
    }

    /// The pool the app runs with: real sockets and the limits of
    /// [`Timeouts::default`]. In the tests there is no network: whatever tries
    /// to connect is refused, and a test that wants a server puts one in with
    /// [`Pool::new`].
    pub fn over_tcp(known: KnownHostsFiles) -> Self {
        let timeouts = Timeouts::default();
        let dial: Arc<dyn Dial> = if cfg!(any(test, feature = "test-support")) {
            Arc::new(NoNetwork)
        } else {
            Arc::new(TcpDial {
                timeout: timeouts.connect,
            })
        };
        Self::new(dial, known, timeouts)
    }

    pub fn timeouts(&self) -> Timeouts {
        self.timeouts
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<ServerId, Arc<Session>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The live connection to `server`, if there is one. One that has died is
    /// dropped on the way.
    pub fn session(&self, server: &ServerId) -> Option<Arc<Session>> {
        let mut sessions = self.sessions();
        match sessions.get(server) {
            Some(session) if session.is_alive() => Some(Arc::clone(session)),
            Some(_) => {
                sessions.remove(server);
                None
            }
            None => None,
        }
    }

    pub fn is_connected(&self, server: &ServerId) -> bool {
        self.session(server).is_some()
    }

    /// The servers with a live connection, in no particular order.
    pub fn connected(&self) -> Vec<ServerId> {
        let mut sessions = self.sessions();
        sessions.retain(|_, session| session.is_alive());
        sessions.keys().cloned().collect()
    }

    /// Drops the connection, so the next use connects again. The server is told
    /// that the client is leaving, as far as a runtime is there to send it.
    pub fn forget(&self, server: &ServerId) {
        let removed = self.sessions().remove(server);
        self.say_goodbye(removed);
    }

    /// Drops what a failed request says is broken.
    pub fn note(&self, server: &ServerId, error: &RemoteError) {
        if error.breaks_connection() {
            self.forget(server);
        }
    }

    /// Drops every connection and tells the servers. Best effort and never
    /// waiting: when the app quits, the goodbye may not get out.
    pub fn close_all(&self) {
        let all: Vec<Arc<Session>> = self
            .sessions()
            .drain()
            .map(|(_, session)| session)
            .collect();
        self.say_goodbye(all);
    }

    fn say_goodbye(&self, sessions: impl IntoIterator<Item = Arc<Session>>) {
        let noted = self
            .runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(runtime) = noted.or_else(|| tokio::runtime::Handle::try_current().ok()) else {
            return;
        };
        for session in sessions {
            runtime.spawn(async move {
                // The connection may be gone already, which is what this ends.
                let _gone = session
                    .handle
                    .disconnect(russh::Disconnect::ByApplication, "", "")
                    .await;
            });
        }
    }

    /// Runs `request` on the connection under the time limit for requests.
    pub async fn request<T>(
        &self,
        server: &ServerId,
        limit: Duration,
        request: impl Future<Output = Result<T, SftpError>>,
    ) -> Result<T, RemoteError> {
        let error = match timeout(limit, request).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) => RemoteError::of_sftp(&error),
            Err(_) => RemoteError::Timeout,
        };
        self.note(server, &error);
        Err(error)
    }

    /// Connects and logs in. The address may be an alias of `~/.ssh/config`;
    /// the connection is kept under `server` as given. The host key is judged
    /// first, by the host and port the alias resolves to; nothing secret is
    /// sent to a server whose key is not known.
    pub async fn connect(&self, server: &ServerId, login: &Login) -> Result<(), ConnectFailure> {
        if self.is_connected(server) {
            return Ok(());
        }
        let (files, home, alias) = (
            self.known.clone(),
            self.env.home.clone(),
            server.host.clone(),
        );
        let (known, settings) = tokio::task::spawn_blocking(move || {
            (
                KnownHosts::load(&files),
                ssh_config::read_settings(home.as_deref(), &alias),
            )
        })
        .await
        .map_err(|error| ConnectFailure::Unreachable(error.to_string()))?;
        let Some(target) = login::effective_server(server, &settings, self.env.os_user.as_deref())
        else {
            return Err(ConnectFailure::AuthFailed);
        };
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(tokio::runtime::Handle::current());
        let seen = Arc::new(Mutex::new(None));
        let on_record = known.algorithms_for(&target.host, target.port);
        let check = HostCheck {
            known: Arc::new(known),
            host: target.host.clone(),
            port: target.port,
            seen: Arc::clone(&seen),
        };
        let opened = timeout(
            self.timeouts.connect,
            self.open(&target, login, &settings, check, &on_record),
        )
        .await;
        match opened {
            Err(_) => Err(ConnectFailure::Timeout),
            Ok(Ok(opened_session)) => {
                let session = Arc::new(opened_session);
                // A concurrent connect may have got in first; its connection
                // keeps serving requests, so the newcomer is the one to leave.
                let loser = {
                    let mut sessions = self.sessions();
                    match sessions.get(server) {
                        Some(existing) if existing.is_alive() => Some(session),
                        _ => sessions.insert(server.clone(), session),
                    }
                };
                self.say_goodbye(loser);
                Ok(())
            }
            Ok(Err(step)) => Err(failure_of(step, &seen)),
        }
    }

    async fn open(
        &self,
        server: &ServerId,
        login: &Login,
        settings: &HostSettings,
        check: HostCheck,
        on_record: &[Algorithm],
    ) -> Result<Session, Step> {
        let mut preferred = russh::Preferred::default();
        preferred.key = ask_for_known_first(&preferred.key, on_record).into();
        let config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(10)),
            keepalive_max: 3,
            inactivity_timeout: None,
            preferred,
            ..client::Config::default()
        });
        let stream = self.dial.dial(server).await?;
        let mut handle = client::connect_stream(config, stream, check).await?;
        // Ask what the server takes before anything secret is sent: a server
        // that offers no password login gets none, and the user hears why
        // instead of "wrong password". A server that names no methods is tried
        // with everything all the same.
        let probe = handle.authenticate_none(server.user.clone()).await?;
        if let client::AuthResult::Failure {
            remaining_methods, ..
        } = &probe
        {
            let attempt = login::Attempt {
                user: &server.user,
                login,
                env: &self.env,
                identity_files: &settings.identity_files,
                passphrases: &self.passphrases,
            };
            login::sign_in(&mut handle, remaining_methods, &attempt).await?;
        }
        let channel = handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await?;
        sftp.set_timeout(self.timeouts.request.as_secs().max(1));
        let home = sftp.canonicalize(".").await?;
        Ok(Session { handle, sftp, home })
    }

    /// Writes the key the user confirmed to the file of ncrs, under the host
    /// and port the address resolves to: the ones `connect` judges the key by.
    /// Blocking.
    pub fn trust(&self, server: &ServerId, key: &PublicKey) -> io::Result<()> {
        let own = self
            .known
            .own
            .as_deref()
            .ok_or_else(|| io::Error::other("there is no place to keep the host key"))?;
        let settings = ssh_config::read_settings(self.env.home.as_deref(), &server.host);
        let (host, port) = login::effective_server(server, &settings, self.env.os_user.as_deref())
            .map_or_else(
                || (server.host.clone(), server.port),
                |at| (at.host, at.port),
            );
        known_hosts::learn(own, &host, port, key)
    }
}

/// The key types russh offers, with those on record for the host first: a
/// server that has several answers with the first one it shares, and that must
/// be the one that is known. Types russh does not offer cannot be asked for.
fn ask_for_known_first(offered: &[Algorithm], on_record: &[Algorithm]) -> Vec<Algorithm> {
    let mut order: Vec<Algorithm> = on_record
        .iter()
        .filter(|known| offered.contains(known))
        .cloned()
        .collect();
    for algorithm in offered {
        if !order.contains(algorithm) {
            order.push(algorithm.clone());
        }
    }
    order
}

fn failure_of(step: Step, seen: &Mutex<Option<Seen>>) -> ConnectFailure {
    match step {
        Step::AuthFailed => ConnectFailure::AuthFailed,
        Step::PasswordNotOffered => ConnectFailure::PasswordNotOffered,
        Step::PassphraseNeeded(key_file) => ConnectFailure::PassphraseNeeded { key_file },
        Step::WrongPassphrase(key_file) => ConnectFailure::WrongPassphrase { key_file },
        Step::Ssh(russh::Error::UnknownKey) => {
            let offered = seen.lock().unwrap_or_else(PoisonError::into_inner).take();
            match offered {
                Some(Seen { verdict, key }) => {
                    let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
                    match verdict {
                        HostVerdict::Changed { file, line } => ConnectFailure::HostKeyChanged {
                            fingerprint,
                            file,
                            line,
                        },
                        HostVerdict::OtherTypeKnown { file, line } => {
                            ConnectFailure::HostKeyOtherType {
                                fingerprint,
                                file,
                                line,
                            }
                        }
                        HostVerdict::Unreadable { file, reason } => {
                            ConnectFailure::KnownHostsUnreadable { file, reason }
                        }
                        HostVerdict::Revoked => ConnectFailure::HostKeyRevoked,
                        HostVerdict::Known | HostVerdict::Unknown => ConnectFailure::UnknownHost {
                            fingerprint,
                            key: Box::new(key),
                        },
                    }
                }
                None => ConnectFailure::Unreachable("the host key was not offered".to_owned()),
            }
        }
        Step::Ssh(russh::Error::NoCommonAlgo { .. }) => ConnectFailure::NoCommonAlgorithm,
        Step::Ssh(error) => ConnectFailure::Unreachable(error.to_string()),
        Step::Io(error) if error.kind() == io::ErrorKind::TimedOut => ConnectFailure::Timeout,
        Step::Io(error) => ConnectFailure::Unreachable(error.to_string()),
        Step::Sftp(error) => ConnectFailure::Unreachable(error.to_string()),
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod login_tests;
