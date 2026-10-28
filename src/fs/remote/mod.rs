//! SFTP servers browsed like folders. A path on a server is the address as
//! text (`sftp://user@host[:port]/abs/path`) inside a `PathBuf`, so the panels
//! need no path type of their own for it, and it is told apart from a local
//! path by its text alone, never by asking the filesystem.
//!
//! A server path must never be built with `Path::join`, `parent` or
//! `components`: on Unix the parent of the root would be `sftp:`, `//` would be
//! collapsed, and on Windows `join` would add a backslash. [`RemotePath`] does
//! it as text. Nothing here reaches `std::fs` with a server path.
//!
//! Login is by password only, and the host key is checked before the password
//! is sent.

// The layer is built up in steps; what the app does not call yet is not dead
// for long.
#![allow(
    dead_code,
    reason = "the app uses the SFTP layer from the next commits on"
)]

pub mod credentials;
pub mod known_hosts;
pub mod list;
pub mod session;
#[cfg(test)]
pub mod test_server;
pub mod transfer;

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zeroize::Zeroizing;

pub use known_hosts::KnownHostsFiles;

const SCHEME: &str = "sftp://";
const DEFAULT_PORT: u16 = 22;

/// How much of a block is moved in one SFTP request.
pub const CHUNK: usize = 256 * 1024;

/// How long the app waits for a server, by what it waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// TCP, key exchange and login together.
    pub connect: Duration,
    /// Every single SFTP request.
    pub request: Duration,
    /// Each block of a file in motion.
    pub block: Duration,
    /// A whole directory listing.
    pub list: Duration,
    /// All the links of one listing together: what is not found out by then
    /// stays a link.
    pub links: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            request: Duration::from_secs(20),
            block: Duration::from_secs(30),
            list: Duration::from_secs(30),
            links: Duration::from_secs(5),
        }
    }
}

/// A password. Its text is reachable only through [`Secret::expose`], it is
/// wiped from memory when dropped, and it prints as `***`: it must never end up
/// in a log, an error text or a file.
///
/// The wiping covers this value and its clones. The login hands the password
/// to russh as a `String`, which that library owns from then on and does not
/// wipe; the keychain gets the bytes and keeps them where the system keeps
/// them.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(text: String) -> Self {
        Self(Zeroizing::new(text))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

impl Eq for Secret {}

impl Default for Secret {
    fn default() -> Self {
        Self::new(String::new())
    }
}

/// Whether `c` lets a name say something other than what it is: a control
/// character (a newline, a bell, an escape sequence) or one of the bidirectional
/// controls that reorder the text around them, so that `gnp.exe` can look like
/// `exe.png`. A name with one is not listed, and not created here.
pub fn is_deceptive(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// One account on one server: what a login and a saved password belong to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerId {
    pub user: String,
    /// Lower case, without the brackets of an IPv6 address.
    pub host: String,
    pub port: u16,
}

impl ServerId {
    pub fn new(user: &str, host: &str, port: u16) -> Self {
        Self {
            user: user.to_owned(),
            host: host.to_lowercase(),
            port,
        }
    }

    /// The root folder of the server as a panel path.
    pub fn root_path(&self) -> PathBuf {
        PathBuf::from(format!("{SCHEME}{}/", self.authority()))
    }

    /// The keychain account: `user@host:port`, with the port always there.
    pub fn account(&self) -> String {
        format!("{}@{}:{}", self.user, self.host_text(), self.port)
    }

    /// `user@host`, and `:port` unless it is 22.
    pub fn authority(&self) -> String {
        if self.port == DEFAULT_PORT {
            format!("{}@{}", self.user, self.host_text())
        } else {
            format!("{}@{}:{}", self.user, self.host_text(), self.port)
        }
    }

    fn host_text(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        }
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.authority())
    }
}

/// What is wrong with a typed address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlProblem {
    NotSftp,
    NoHost,
    BadPort,
    /// `user:password@`: a password is typed into its own field, never into an
    /// address that ends up in the history and the config.
    PasswordInAddress,
    BadCharacter,
}

/// A parsed `sftp://` address. The user is optional here, the dialog asks for
/// it when it is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUrl {
    pub user: Option<String>,
    pub host: String,
    pub port: u16,
    /// Absolute and normalised; `/` when the address names none.
    pub path: String,
}

impl RemoteUrl {
    pub fn server(&self) -> Option<ServerId> {
        self.user
            .as_deref()
            .map(|user| ServerId::new(user, &self.host, self.port))
    }

    pub fn remote_path(&self) -> Option<RemotePath> {
        Some(RemotePath {
            server: self.server()?,
            path: self.path.clone(),
        })
    }
}

/// Reads `sftp://[user@]host[:port][/path]`. No percent-decoding: what is typed
/// is what is meant.
pub fn parse_url(text: &str) -> Result<RemoteUrl, UrlProblem> {
    let rest = text
        .get(..SCHEME.len())
        .filter(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
        .and_then(|_| text.get(SCHEME.len()..))
        .ok_or(UrlProblem::NotSftp)?;
    let (authority, path) = match rest.find('/') {
        Some(slash) => rest.split_at(slash),
        None => (rest, "/"),
    };
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((user, host_port)) => (Some(user), host_port),
        None => (None, authority),
    };
    if user.is_some_and(|name| name.contains(':')) {
        return Err(UrlProblem::PasswordInAddress);
    }
    if user.is_some_and(|name| !plain_word(name)) {
        return Err(UrlProblem::BadCharacter);
    }
    let (host, port) = split_host_port(host_port)?;
    if host.is_empty() {
        return Err(UrlProblem::NoHost);
    }
    if !plain_word(&host.replace(['[', ']'], "")) || host.contains('@') {
        return Err(UrlProblem::BadCharacter);
    }
    Ok(RemoteUrl {
        user: user.map(str::to_owned),
        host: host.to_lowercase(),
        port,
        path: normalise(path),
    })
}

/// Not empty, no white space and no control characters.
fn plain_word(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn split_host_port(text: &str) -> Result<(&str, u16), UrlProblem> {
    let (host, port_text) = if let Some(inner) = text.strip_prefix('[') {
        let (host, after) = inner.split_once(']').ok_or(UrlProblem::NoHost)?;
        match after {
            "" => (host, None),
            _ => (
                host,
                Some(after.strip_prefix(':').ok_or(UrlProblem::BadCharacter)?),
            ),
        }
    } else {
        match text.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (text, None),
        }
    };
    let port = match port_text {
        None => DEFAULT_PORT,
        Some(digits) => digits
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or(UrlProblem::BadPort)?,
    };
    Ok((host, port))
}

/// An absolute server path with `.` and `..` resolved as text: `..` never
/// climbs above the root.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(|c| c == '/' || (cfg!(windows) && c == '\\')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    format!("/{}", parts.join("/"))
}

/// A place on a server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemotePath {
    server: ServerId,
    /// Absolute, normalised, `/` as the only separator.
    path: String,
}

impl RemotePath {
    /// `Some` when `path` is the text of a server address with a user. Looks at
    /// the text only.
    pub fn parse(path: &Path) -> Option<Self> {
        parse_url(path.to_str()?).ok()?.remote_path()
    }

    pub fn new(server: ServerId, path: &str) -> Self {
        Self {
            server,
            path: normalise(path),
        }
    }

    pub fn server(&self) -> &ServerId {
        &self.server
    }

    /// The path on the server.
    pub fn remote_str(&self) -> &str {
        &self.path
    }

    /// The panel path: the address as text.
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf::from(format!("{SCHEME}{}{}", self.server.authority(), self.path))
    }

    pub fn is_root(&self) -> bool {
        self.path == "/"
    }

    /// The folder above, `None` at the root.
    pub fn parent(&self) -> Option<Self> {
        let (above, _) = self.path.rsplit_once('/')?;
        if self.is_root() {
            return None;
        }
        Some(Self {
            server: self.server.clone(),
            path: if above.is_empty() {
                "/".to_owned()
            } else {
                above.to_owned()
            },
        })
    }

    /// The last name; `None` at the root.
    pub fn file_name(&self) -> Option<&str> {
        self.path
            .rsplit_once('/')
            .map(|(_, name)| name)
            .filter(|name| !name.is_empty())
    }

    /// `name` below this folder. A name is one component; anything else is
    /// resolved like a typed path.
    pub fn join(&self, name: &str) -> Self {
        Self {
            server: self.server.clone(),
            path: normalise(&format!("{}/{name}", self.path)),
        }
    }

    /// What was typed into the path field while the panel shows this folder:
    /// absolute, `~` for the home folder of the login, or relative to here.
    pub fn resolve_typed(&self, typed: &str, home: &str) -> Self {
        let words = typed.trim();
        let path = match words.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
            _ if words.starts_with('/') => words.to_owned(),
            _ => format!("{}/{words}", self.path),
        };
        Self {
            server: self.server.clone(),
            path: normalise(&path),
        }
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
    use super::*;

    fn parsed(text: &str) -> RemotePath {
        RemotePath::parse(Path::new(text)).unwrap()
    }

    #[test]
    fn parses_user_host_port_and_path() {
        let path = parsed("sftp://marc@Example.org:2222/srv/files");
        assert_eq!(path.server, ServerId::new("marc", "example.org", 2222));
        assert_eq!(path.remote_str(), "/srv/files");
        assert_eq!(path.file_name(), Some("files"));
        let url = parse_url("SFTP://example.org").unwrap();
        assert_eq!(url.user, None);
        assert_eq!(url.port, 22);
        assert_eq!(url.path, "/");
    }

    #[test]
    fn default_port_is_left_out_of_the_canonical_text() {
        assert_eq!(
            parsed("sftp://marc@host:22/a").to_path_buf(),
            PathBuf::from("sftp://marc@host/a")
        );
        assert_eq!(
            parsed("sftp://marc@host:2200/a").to_path_buf(),
            PathBuf::from("sftp://marc@host:2200/a")
        );
        assert_eq!(ServerId::new("m", "h", 22).account(), "m@h:22");
    }

    #[test]
    fn dot_dot_never_climbs_above_the_server_root() {
        let path = parsed("sftp://m@h/a/b");
        assert_eq!(
            path.resolve_typed("../../../..", "/home/m").remote_str(),
            "/"
        );
        assert_eq!(path.resolve_typed("../c", "/home/m").remote_str(), "/a/c");
        assert_eq!(parsed("sftp://m@h/../../x").remote_str(), "/x");
        assert_eq!(
            path.resolve_typed("~/docs", "/home/m").remote_str(),
            "/home/m/docs"
        );
        assert_eq!(path.resolve_typed("~", "/home/m").remote_str(), "/home/m");
        assert_eq!(path.resolve_typed("/etc", "/home/m").remote_str(), "/etc");
        assert_eq!(
            path.resolve_typed("sub//./x", "/h").remote_str(),
            "/a/b/sub/x"
        );
    }

    #[test]
    fn a_local_absolute_path_is_never_remote() {
        let unix = if cfg!(windows) {
            r"C:\tmp\sftp:\x"
        } else {
            "/tmp/sftp:/x"
        };
        assert_eq!(RemotePath::parse(Path::new(unix)), None);
        assert_eq!(RemotePath::parse(Path::new(r"C:\sftp:")), None);
        assert_eq!(RemotePath::parse(Path::new("/sftp://m@h/a")), None);
        assert_eq!(
            RemotePath::parse(Path::new("sftp://host/a")),
            None,
            "no user"
        );
    }

    #[test]
    fn backslash_is_a_separator_only_on_windows() {
        let path = parsed("sftp://m@h/a");
        let joined = path.resolve_typed(r"b\c", "/h");
        if cfg!(windows) {
            assert_eq!(joined.remote_str(), "/a/b/c");
        } else {
            assert_eq!(joined.remote_str(), r"/a/b\c");
        }
    }

    #[test]
    fn parent_of_the_root_is_none() {
        let root = parsed("sftp://m@h/");
        assert!(root.is_root());
        assert_eq!(root.parent(), None);
        assert_eq!(root.file_name(), None);
        assert_eq!(parsed("sftp://m@h/a").parent().unwrap().remote_str(), "/");
        assert_eq!(
            parsed("sftp://m@h/a/b").parent().unwrap().remote_str(),
            "/a"
        );
    }

    #[test]
    fn ipv6_host_in_brackets() {
        let path = parsed("sftp://m@[::1]:2222/x");
        assert_eq!(path.server.host, "::1");
        assert_eq!(path.server.port, 2222);
        assert_eq!(path.to_path_buf(), PathBuf::from("sftp://m@[::1]:2222/x"));
        assert_eq!(path.server.account(), "m@[::1]:2222");
    }

    #[test]
    fn names_that_hide_what_they_are_are_deceptive() {
        for bad in [
            '\u{202E}', '\u{202A}', '\u{2066}', '\u{2069}', '\u{200F}', '\n', '\u{7}', '\0',
            '\u{1b}',
        ] {
            assert!(is_deceptive(bad), "{bad:?}");
        }
        for fine in ['a', ' ', 'ä', '日', '\u{200D}', '\u{FE0F}', '-'] {
            assert!(!is_deceptive(fine), "{fine:?}");
        }
    }

    #[test]
    fn a_bad_address_names_its_problem() {
        assert_eq!(parse_url("http://h/").unwrap_err(), UrlProblem::NotSftp);
        assert_eq!(parse_url("sftp://m@/x").unwrap_err(), UrlProblem::NoHost);
        assert_eq!(parse_url("sftp://m@h:0/").unwrap_err(), UrlProblem::BadPort);
        assert_eq!(
            parse_url("sftp://m@h:99999/").unwrap_err(),
            UrlProblem::BadPort
        );
        assert_eq!(
            parse_url("sftp://m:secret@h/").unwrap_err(),
            UrlProblem::PasswordInAddress
        );
        assert_eq!(
            parse_url("sftp://m@h h/").unwrap_err(),
            UrlProblem::BadCharacter
        );
    }

    #[test]
    fn join_adds_one_component_as_text() {
        let path = parsed("sftp://m@h/a");
        assert_eq!(path.join("b c").remote_str(), "/a/b c");
        assert_eq!(path.join("..").remote_str(), "/");
        assert_eq!(
            parsed("sftp://m@h/").join("x").to_path_buf(),
            PathBuf::from("sftp://m@h/x")
        );
    }

    #[test]
    fn secret_debug_is_redacted() {
        let secret = Secret::new("hunter2".to_owned());
        assert_eq!(format!("{secret:?}"), "***");
        assert_eq!(secret, Secret::new("hunter2".to_owned()));
        assert_ne!(secret, Secret::default());
    }
}
