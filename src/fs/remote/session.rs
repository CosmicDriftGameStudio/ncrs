//! Connections to servers, one per account, kept for the run. A connection is
//! made in steps, each of them under a time limit, and the host key is judged
//! before the password is sent: an unknown or changed key stops the login with
//! nothing sent.

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

use super::known_hosts::{self, HostVerdict, KnownHosts};
use super::{KnownHostsFiles, Secret, ServerId, Timeouts};

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
            runtime: Mutex::new(None),
        }
    }

    /// The pool the app runs with: real sockets and the limits of
    /// [`Timeouts::default`].
    pub fn over_tcp(known: KnownHostsFiles) -> Self {
        let timeouts = Timeouts::default();
        Self::new(
            Arc::new(TcpDial {
                timeout: timeouts.connect,
            }),
            known,
            timeouts,
        )
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

    /// Connects and logs in. The host key is judged first; the password is sent
    /// only to a server whose key is known.
    pub async fn connect(
        &self,
        server: &ServerId,
        password: &Secret,
    ) -> Result<(), ConnectFailure> {
        let files = self.known.clone();
        let known = tokio::task::spawn_blocking(move || KnownHosts::load(&files))
            .await
            .map_err(|error| ConnectFailure::Unreachable(error.to_string()))?;
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(tokio::runtime::Handle::current());
        let seen = Arc::new(Mutex::new(None));
        let on_record = known.algorithms_for(&server.host, server.port);
        let check = HostCheck {
            known: Arc::new(known),
            host: server.host.clone(),
            port: server.port,
            seen: Arc::clone(&seen),
        };
        let opened = timeout(
            self.timeouts.connect,
            self.open(server, password, check, &on_record),
        )
        .await;
        match opened {
            Err(_) => Err(ConnectFailure::Timeout),
            Ok(Ok(session)) => {
                self.sessions().insert(server.clone(), Arc::new(session));
                Ok(())
            }
            Ok(Err(step)) => Err(failure_of(step, &seen)),
        }
    }

    async fn open(
        &self,
        server: &ServerId,
        password: &Secret,
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
        // with the password all the same.
        let probe = handle.authenticate_none(server.user.clone()).await?;
        if let client::AuthResult::Failure {
            remaining_methods, ..
        } = &probe
        {
            if !remaining_methods.is_empty()
                && !remaining_methods.contains(&russh::MethodKind::Password)
            {
                return Err(Step::PasswordNotOffered);
            }
            let login = handle
                .authenticate_password(server.user.clone(), password.expose().to_owned())
                .await?;
            if !login.success() {
                return Err(Step::AuthFailed);
            }
        }
        let channel = handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await?;
        sftp.set_timeout(self.timeouts.request.as_secs().max(1));
        let home = sftp.canonicalize(".").await?;
        Ok(Session { handle, sftp, home })
    }

    /// Writes the key the user confirmed to the file of ncrs. Blocking.
    pub fn trust(&self, server: &ServerId, key: &PublicKey) -> io::Result<()> {
        let own = self
            .known
            .own
            .as_deref()
            .ok_or_else(|| io::Error::other("there is no place to keep the host key"))?;
        known_hosts::learn(own, &server.host, server.port, key)
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
mod tests {
    use super::super::test_server::{other_public_key_text, public_key_text, Fixture, TestServer};
    use super::*;
    use crate::fs::reader::Listing;
    use crate::fs::{ReadError, SortKey};

    async fn listing(fixture: &Fixture) -> Listing {
        super::super::list::list(&fixture.pool, Fixture::at("/"), SortKey::default()).await
    }

    #[tokio::test]
    async fn host_key_is_checked_before_the_password_is_sent() {
        let unknown = Fixture::untrusted();
        let refused = unknown
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await;
        assert!(
            matches!(refused, Err(ConnectFailure::UnknownHost { .. })),
            "{refused:?}"
        );
        assert!(
            !unknown.server.control.events().contains(&"password"),
            "no password went to a server whose key is unknown"
        );

        let known = Fixture::trusted();
        known
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap();
        assert!(known.server.control.events().contains(&"password"));
        assert!(known.pool.is_connected(&Fixture::id()));
    }

    #[tokio::test]
    async fn unknown_host_reports_sha256_fingerprint() {
        let fixture = Fixture::untrusted();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap_err();

        let ConnectFailure::UnknownHost { fingerprint, key } = refused else {
            panic!("{refused:?}");
        };
        let expected = PublicKey::from_openssh(&public_key_text())
            .unwrap()
            .fingerprint(HashAlg::Sha256)
            .to_string();
        assert!(fingerprint.starts_with("SHA256:"));
        assert_eq!(fingerprint, expected);
        assert_eq!(*key, PublicKey::from_openssh(&public_key_text()).unwrap());
        assert!(!fixture.pool.is_connected(&Fixture::id()));
    }

    #[tokio::test]
    async fn trusting_the_key_lets_the_next_connect_through() {
        let fixture = Fixture::untrusted();
        let ConnectFailure::UnknownHost { key, .. } = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap_err()
        else {
            panic!("expected an unknown host");
        };

        fixture.pool.trust(&Fixture::id(), &key).unwrap();
        fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap();

        assert!(fixture.pool.is_connected(&Fixture::id()));
        assert!(!std::fs::exists(fixture.known.ssh.as_deref().unwrap()).unwrap());
    }

    #[tokio::test]
    async fn changed_host_key_is_refused_without_auth() {
        let fixture = Fixture::over(TestServer::with_other_key());
        let ssh = fixture.known.ssh.clone().unwrap();
        std::fs::write(&ssh, format!("# old\ntest.invalid {}\n", public_key_text())).unwrap();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap_err();

        let ConnectFailure::HostKeyChanged {
            fingerprint,
            file,
            line,
        } = refused
        else {
            panic!("{refused:?}");
        };
        assert_eq!(file, ssh);
        assert_eq!(line, 2);
        let offered = PublicKey::from_openssh(&other_public_key_text()).unwrap();
        assert_eq!(
            fingerprint,
            offered.fingerprint(HashAlg::Sha256).to_string()
        );
        assert!(!fixture.server.control.events().contains(&"password"));
    }

    #[tokio::test]
    async fn a_revoked_key_is_refused_without_auth() {
        let fixture = Fixture::untrusted();
        std::fs::write(
            fixture.known.ssh.as_deref().unwrap(),
            format!("@revoked test.invalid {}\n", public_key_text()),
        )
        .unwrap();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await;

        assert_eq!(refused, Err(ConnectFailure::HostKeyRevoked));
        assert!(!fixture.server.control.events().contains(&"password"));
    }

    #[tokio::test]
    async fn wrong_password_is_auth_failed() {
        let fixture = Fixture::trusted();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Secret::new("not it".to_owned()))
            .await;

        assert_eq!(refused, Err(ConnectFailure::AuthFailed));
        assert!(!fixture.pool.is_connected(&Fixture::id()));
    }

    #[tokio::test]
    async fn the_home_folder_is_the_one_the_server_names() {
        let fixture = Fixture::connected().await;

        let session = fixture.pool.session(&Fixture::id()).unwrap();

        assert_eq!(session.home(), super::super::test_server::HOME);
    }

    struct Hanging;

    impl Dial for Hanging {
        fn dial<'a>(
            &'a self,
            _server: &'a ServerId,
        ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>> {
            Box::pin(std::future::pending())
        }
    }

    struct Refusing;

    impl Dial for Refusing {
        fn dial<'a>(
            &'a self,
            _server: &'a ServerId,
        ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>> {
            Box::pin(async { Err(io::Error::new(io::ErrorKind::ConnectionRefused, "refused")) })
        }
    }

    #[tokio::test]
    async fn a_server_that_never_answers_is_a_timeout() {
        let pool = Pool::new(
            Arc::new(Hanging),
            KnownHostsFiles::default(),
            Timeouts {
                connect: Duration::from_millis(200),
                ..super::super::test_server::timeouts()
            },
        );

        let result = pool.connect(&Fixture::id(), &Fixture::password()).await;

        assert_eq!(result, Err(ConnectFailure::Timeout));
    }

    #[tokio::test]
    async fn a_refused_connection_is_unreachable_with_the_reason() {
        let pool = Pool::new(
            Arc::new(Refusing),
            KnownHostsFiles::default(),
            super::super::test_server::timeouts(),
        );

        let result = pool.connect(&Fixture::id(), &Fixture::password()).await;

        assert!(
            matches!(&result, Err(ConnectFailure::Unreachable(reason)) if reason.contains("refused")),
            "{result:?}"
        );
    }

    /// Every way a connection fails, as the result of a real attempt, with a
    /// password that is easy to find in any text it might leak into.
    async fn failures_with(secret: &Secret) -> Vec<Result<(), ConnectFailure>> {
        let id = Fixture::id();
        let mut results = Vec::new();
        // Refused before anything: unreachable.
        let refused = Pool::new(
            Arc::new(Refusing),
            KnownHostsFiles::default(),
            super::super::test_server::timeouts(),
        );
        results.push(refused.connect(&id, secret).await);
        // The server says the password is wrong.
        results.push(Fixture::trusted().pool.connect(&id, secret).await);
        // The server is not known: the password stays here.
        results.push(Fixture::untrusted().pool.connect(&id, secret).await);
        // The server takes no password.
        let keyboard_only = Fixture::over(TestServer::without_password_login());
        std::fs::write(
            keyboard_only.known.own.as_deref().unwrap(),
            format!("test.invalid {}\n", public_key_text()),
        )
        .unwrap();
        results.push(keyboard_only.pool.connect(&id, secret).await);
        results
    }

    /// The results of those attempts, as a `{:?}` shows them, are the text a
    /// log or a bug report would carry.
    #[tokio::test]
    async fn the_password_appears_in_no_result_of_a_failed_attempt() {
        let secret = Secret::new("hunter2-the-password".to_owned());

        let results = failures_with(&secret).await;

        assert_eq!(results.len(), 4);
        assert!(
            matches!(results.first(), Some(Err(ConnectFailure::Unreachable(_)))),
            "{results:?}"
        );
        assert!(matches!(
            results.get(1),
            Some(Err(ConnectFailure::AuthFailed))
        ));
        assert!(matches!(
            results.get(2),
            Some(Err(ConnectFailure::UnknownHost { .. }))
        ));
        assert!(matches!(
            results.get(3),
            Some(Err(ConnectFailure::PasswordNotOffered))
        ));
        let shown = format!("{results:?}{secret:?}");
        assert!(
            !shown.contains("hunter2"),
            "the password leaked into {shown}"
        );
        assert!(shown.contains("***"), "the secret itself shows as stars");
    }

    #[tokio::test]
    async fn a_server_that_offers_no_password_login_gets_no_password_and_is_named() {
        let fixture = Fixture::over(TestServer::without_password_login());
        std::fs::write(
            fixture.known.own.as_deref().unwrap(),
            format!("test.invalid {}\n", public_key_text()),
        )
        .unwrap();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await;

        assert_eq!(refused, Err(ConnectFailure::PasswordNotOffered));
        assert!(!fixture.server.control.events().contains(&"password"));
    }

    #[tokio::test]
    async fn a_key_of_another_type_than_the_one_on_record_is_refused_without_auth() {
        let fixture = Fixture::untrusted();
        let ssh = fixture.known.ssh.clone().unwrap();
        std::fs::write(
            &ssh,
            format!(
                "# the ECDSA key was recorded\ntest.invalid {}\n",
                super::super::test_server::ECDSA_PUBLIC
            ),
        )
        .unwrap();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap_err();

        let ConnectFailure::HostKeyOtherType { file, line, .. } = refused else {
            panic!("{refused:?}");
        };
        assert_eq!((file, line), (ssh, 2));
        assert!(!fixture.server.control.events().contains(&"password"));
    }

    #[tokio::test]
    async fn the_server_is_asked_for_the_key_type_on_record_first() {
        // It has both an ed25519 and an ECDSA key and offers the first type
        // the client names that it has; ed25519 would come first by default.
        let fixture = Fixture::over(TestServer::with_two_keys());
        std::fs::write(
            fixture.known.own.as_deref().unwrap(),
            format!("test.invalid {}\n", super::super::test_server::ECDSA_PUBLIC),
        )
        .unwrap();

        fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap();

        assert!(fixture.pool.is_connected(&Fixture::id()));
    }

    #[test]
    fn the_types_on_record_lead_the_list_and_those_russh_lacks_are_left_out() {
        let offered = russh::Preferred::default().key.to_vec();
        assert!(offered.len() > 1);
        let last = offered.last().unwrap().clone();

        let order = ask_for_known_first(&offered, &[last.clone(), Algorithm::Dsa]);

        assert_eq!(order.first(), Some(&last));
        assert!(!order.contains(&Algorithm::Dsa) || offered.contains(&Algorithm::Dsa));
        let mut sorted_in: Vec<String> = offered.iter().map(ToString::to_string).collect();
        let mut sorted_out: Vec<String> = order.iter().map(ToString::to_string).collect();
        sorted_in.sort();
        sorted_out.sort();
        assert_eq!(sorted_in, sorted_out, "nothing is lost, nothing is added");
    }

    #[tokio::test]
    async fn a_host_key_file_that_cannot_be_read_gives_no_way_to_trust_and_sends_nothing() {
        let fixture = Fixture::untrusted();
        let ssh = fixture.known.ssh.clone().unwrap();
        std::fs::create_dir(&ssh).unwrap();

        let refused = fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap_err();

        assert!(
            matches!(&refused, ConnectFailure::KnownHostsUnreadable { file, .. } if *file == ssh),
            "{refused:?}"
        );
        assert!(!fixture.server.control.events().contains(&"password"));
    }

    async fn wait_for_closed(fixture: &Fixture) -> bool {
        for _ in 0..150 {
            if fixture.server.control.events().contains(&"closed") {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn forgetting_a_connection_tells_the_server_we_are_leaving() {
        let fixture = Fixture::connected().await;
        // Another holder keeps the connection from ending just by being dropped.
        let held = fixture.pool.session(&Fixture::id()).unwrap();

        fixture.pool.forget(&Fixture::id());

        assert!(wait_for_closed(&fixture).await, "the server saw no goodbye");
        drop(held);
    }

    #[tokio::test]
    async fn closing_all_tells_every_server_and_never_waits() {
        let fixture = Fixture::connected().await;
        let held = fixture.pool.session(&Fixture::id()).unwrap();

        fixture.pool.close_all();

        assert!(!fixture.pool.is_connected(&Fixture::id()));
        assert!(wait_for_closed(&fixture).await, "the server saw no goodbye");
        drop(held);
    }

    #[test]
    fn closing_without_a_runtime_is_quiet() {
        let pool = Pool::new(
            Arc::new(Refusing),
            KnownHostsFiles::default(),
            super::super::test_server::timeouts(),
        );
        pool.close_all();
        pool.forget(&Fixture::id());
    }

    #[test]
    fn a_request_that_ran_out_of_time_does_not_end_the_connection_but_a_lost_one_does() {
        assert!(!RemoteError::Timeout.breaks_connection());
        assert!(!RemoteError::Refused("no".into()).breaks_connection());
        assert!(!RemoteError::NotFound.breaks_connection());
        assert!(RemoteError::Lost("gone".into()).breaks_connection());
    }

    #[tokio::test]
    async fn cut_connection_reports_disconnected_then_reconnects() {
        let fixture = Fixture::connected().await;
        assert_eq!(fixture.server.control.dials(), 1);

        fixture.server.control.cut().await;
        for _ in 0..100 {
            if !fixture.pool.is_connected(&Fixture::id()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let after_cut = listing(&fixture).await;
        assert!(
            matches!(after_cut.error, Some(ReadError::RemoteDisconnected { .. })),
            "{:?}",
            after_cut.error
        );

        fixture
            .pool
            .connect(&Fixture::id(), &Fixture::password())
            .await
            .unwrap();
        assert_eq!(fixture.server.control.dials(), 2);
        assert_eq!(listing(&fixture).await.error, None);
    }

    #[tokio::test]
    async fn closing_all_drops_every_connection() {
        let fixture = Fixture::connected().await;

        fixture.pool.close_all();

        assert!(!fixture.pool.is_connected(&Fixture::id()));
    }

    #[tokio::test]
    async fn a_second_account_on_the_same_host_is_its_own_session() {
        let fixture = Fixture::connected().await;
        let other = ServerId::new("tester", "test.invalid", 2222);

        assert!(fixture.pool.is_connected(&Fixture::id()));
        assert!(!fixture.pool.is_connected(&other));
    }
}
