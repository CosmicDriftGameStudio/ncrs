//! State of the dialog that connects to an SFTP server. The app owns it and only
//! `update` changes it; the view is `view`.
//!
//! The password is a [`Secret`]: this type prints without it, so no `{:?}` of
//! the dialog or of a message that carries it can leak it.

use std::fmt;
use std::path::{Path, PathBuf};

use russh::keys::PublicKey;

use ncrs_core::PanelSide;
use ncrs_fs::remote::session::Login;
use ncrs_fs::remote::{plain_word, RemoteUrl, Secret, ServerId};

use crate::message::ConnectField;

/// Where a folder that was asked for goes once the connection is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Then {
    pub side: PanelSide,
    pub path: PathBuf,
    pub select: Option<String>,
    /// The load the panel started for it, so a dialog that outlives it does not
    /// leave the panel waiting.
    pub request_id: u64,
}

/// What the dialog is doing.
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    Editing,
    /// A connection is being made; the fields are locked.
    Connecting,
    /// The server is unknown. The user compares the fingerprint and decides.
    HostKey {
        fingerprint: String,
        key: Box<PublicKey>,
    },
    /// The server asks for a key file that is locked. The user types its
    /// passphrase.
    Passphrase {
        key_file: PathBuf,
    },
}

/// Why the typed values cannot be used yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    Host,
    Port,
    User,
}

/// What `~/.ssh/config` says about the typed host: the dialog tells the user
/// what a connection will use that the fields do not show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshHint {
    pub host_name: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
}

#[derive(Clone)]
pub struct ConnectDialog {
    attempt: u64,
    host: String,
    port: String,
    user: String,
    password: Secret,
    remember: bool,
    save: bool,
    key_file: String,
    passphrase: Secret,
    remember_passphrase: bool,
    start_path: String,
    stage: Stage,
    error: Option<String>,
    then: Option<Then>,
    ssh_hint: SshHint,
    /// A login name is known without the user field: `~/.ssh/config` gives
    /// one for the host, or the account of this computer.
    login_user_known: bool,
    /// The App Store build: no ssh agent, so the key file has to be chosen.
    sandboxed: bool,
    home: Option<PathBuf>,
}

impl fmt::Debug for ConnectDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectDialog")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"***")
            .field("remember", &self.remember)
            .field("save", &self.save)
            .field("key_file", &self.key_file)
            .field("passphrase", &"***")
            .field("start_path", &self.start_path)
            .field("stage", &self.stage)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl ConnectDialog {
    /// An empty dialog.
    pub fn new(attempt: u64) -> Self {
        Self {
            attempt,
            host: String::new(),
            port: String::new(),
            user: String::new(),
            password: Secret::default(),
            remember: false,
            save: false,
            key_file: String::new(),
            passphrase: Secret::default(),
            remember_passphrase: true,
            start_path: String::new(),
            stage: Stage::Editing,
            error: None,
            then: None,
            ssh_hint: SshHint::default(),
            login_user_known: false,
            sandboxed: false,
            home: None,
        }
    }

    /// A dialog for a server that is known, with the folder to go on to.
    pub fn for_server(attempt: u64, server: &ServerId, then: Option<Then>) -> Self {
        let mut dialog = Self::new(attempt);
        dialog.host.clone_from(&server.host);
        if server.port != 22 {
            dialog.port = server.port.to_string();
        }
        dialog.user.clone_from(&server.user);
        dialog.then = then;
        dialog
    }

    /// A dialog for an address that was typed without a user.
    pub fn for_address(attempt: u64, url: &RemoteUrl) -> Self {
        let mut dialog = Self::new(attempt);
        dialog.host.clone_from(&url.host);
        if url.port != 22 {
            dialog.port = url.port.to_string();
        }
        if url.path != "/" {
            dialog.start_path.clone_from(&url.path);
        }
        dialog
    }

    pub fn attempt(&self) -> u64 {
        self.attempt
    }

    /// A new attempt id, so an answer to an earlier one is dropped.
    pub fn next_attempt(&mut self, id: u64) {
        self.attempt = id;
    }

    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// A passphrase does not outlive the stage that asked for it.
    pub fn set_stage(&mut self, stage: Stage) {
        if !matches!(stage, Stage::Passphrase { .. } | Stage::Connecting) {
            self.passphrase = Secret::default();
        }
        self.stage = stage;
    }

    /// What the dialog knows of the machine: whether this is the App Store
    /// build and the home folder, which paths are shown relative to.
    pub fn set_surroundings(&mut self, sandboxed: bool, home: Option<PathBuf>) {
        self.sandboxed = sandboxed;
        self.home = home;
    }

    /// What `~/.ssh/config` and the account give for the typed host.
    pub fn set_ssh(&mut self, hint: SshHint, login_user_known: bool) {
        self.ssh_hint = hint;
        self.login_user_known = login_user_known;
    }

    pub const fn ssh_hint(&self) -> &SshHint {
        &self.ssh_hint
    }

    pub const fn sandboxed(&self) -> bool {
        self.sandboxed
    }

    /// `path` with `~` for the home folder.
    pub fn shown_path(&self, path: &Path) -> String {
        match self
            .home
            .as_deref()
            .and_then(|home| path.strip_prefix(home).ok())
        {
            Some(rest) => PathBuf::from("~").join(rest).display().to_string(),
            None => path.display().to_string(),
        }
    }

    /// The typed key file, if there is one.
    pub fn key_file(&self) -> Option<PathBuf> {
        match self.key_file.trim() {
            "" => None,
            typed => Some(PathBuf::from(typed)),
        }
    }

    pub fn passphrase(&self) -> &Secret {
        &self.passphrase
    }

    /// Only the passphrase stage takes a passphrase.
    pub fn set_passphrase(&mut self, passphrase: Secret) {
        if matches!(self.stage, Stage::Passphrase { .. }) {
            self.passphrase = passphrase;
        }
    }

    pub const fn remember_passphrase(&self) -> bool {
        self.remember_passphrase
    }

    pub fn toggle_remember_passphrase(&mut self) {
        if matches!(self.stage, Stage::Passphrase { .. }) {
            self.remember_passphrase = !self.remember_passphrase;
        }
    }

    /// `base` with what the dialog adds: the typed key file over the saved one
    /// and the passphrase, which leaves the dialog with it. Call it while the
    /// passphrase stage is shown: the passphrase belongs to the key file of
    /// that stage.
    pub fn take_login(&mut self, base: Login) -> Login {
        let passphrase = std::mem::take(&mut self.passphrase);
        let for_key_file = match &self.stage {
            Stage::Passphrase { key_file } if !passphrase.is_empty() => {
                Some((key_file.clone(), passphrase))
            }
            _ => None,
        };
        Login {
            key_file: self.key_file().or(base.key_file),
            passphrase: for_key_file,
            remember_passphrase: self.remember_passphrase,
            ..base
        }
    }

    pub fn is_locked(&self) -> bool {
        self.stage != Stage::Editing
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn then(&self) -> Option<&Then> {
        self.then.as_ref()
    }

    pub fn take_then(&mut self) -> Option<Then> {
        self.then.take()
    }

    pub fn value(&self, field: ConnectField) -> &str {
        match field {
            ConnectField::Host => &self.host,
            ConnectField::Port => &self.port,
            ConnectField::User => &self.user,
            ConnectField::Password => self.password.expose(),
            ConnectField::KeyFile => &self.key_file,
            ConnectField::StartPath => &self.start_path,
        }
    }

    /// Takes what was typed. A locked dialog takes nothing.
    pub fn set(&mut self, field: ConnectField, text: String) {
        if self.is_locked() {
            return;
        }
        self.error = None;
        match field {
            ConnectField::Host => self.host = text,
            ConnectField::Port => self.port = text,
            ConnectField::User => self.user = text,
            ConnectField::Password => self.password = Secret::new(text),
            ConnectField::KeyFile => self.key_file = text,
            ConnectField::StartPath => self.start_path = text,
        }
    }

    pub fn set_password(&mut self, password: Secret) {
        if !self.is_locked() {
            self.error = None;
            self.password = password;
        }
    }

    pub fn password(&self) -> &Secret {
        &self.password
    }

    /// The password the keychain held, put in for a trust decision that comes
    /// before the connection is tried again.
    pub fn keep_password(&mut self, password: Secret) {
        self.password = password;
    }

    pub fn remember(&self) -> bool {
        self.remember
    }

    /// Whether the server goes into the list of saved servers.
    pub fn save(&self) -> bool {
        self.save
    }

    pub fn with_save(mut self, save: bool) -> Self {
        self.save = save;
        self
    }

    pub fn toggle_save(&mut self) {
        if !self.is_locked() {
            self.save = !self.save;
        }
    }

    pub fn toggle_remember(&mut self) {
        if !self.is_locked() {
            self.remember = !self.remember;
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// The server and the folder to open, or what is still missing.
    pub fn target(&self) -> Result<(ServerId, Option<String>), Missing> {
        let typed_host = self.host.trim();
        let host = typed_host
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .unwrap_or(typed_host);
        if !plain_word(host) || host.contains(['/', '@']) {
            return Err(Missing::Host);
        }
        let port = match self.port.trim() {
            "" => 22,
            digits => digits
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
                .ok_or(Missing::Port)?,
        };
        let user = self.user.trim();
        let user_asked_elsewhere = user.is_empty() && self.login_user_known;
        if !user_asked_elsewhere && (!plain_word(user) || user.contains(['/', '@', ':'])) {
            return Err(Missing::User);
        }
        let path = match self.start_path.trim() {
            "" => None,
            typed => Some(typed.to_owned()),
        };
        Ok((ServerId::new(user, host, port), path))
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

    fn filled() -> ConnectDialog {
        let mut dialog = ConnectDialog::new(1);
        dialog.set(ConnectField::Host, "Example.org".to_owned());
        dialog.set(ConnectField::User, "marc".to_owned());
        dialog.set_password(Secret::new("hunter2".to_owned()));
        dialog
    }

    #[test]
    fn connect_dialog_debug_hides_password() {
        let dialog = filled();
        let shown = format!("{dialog:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("marc") && shown.contains("***"));
    }

    #[test]
    fn a_bracketed_ipv6_host_loses_its_brackets_and_control_characters_are_refused() {
        let mut dialog = filled();
        dialog.set(ConnectField::Host, "[::1]".to_owned());
        let (server, _) = dialog.target().unwrap();
        assert_eq!(server, ServerId::new("marc", "::1", 22));
        dialog.set(ConnectField::Host, "a\u{1b}b".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Host);
    }

    #[test]
    fn the_typed_values_make_a_server() {
        let mut dialog = filled();
        let (server, path) = dialog.target().unwrap();
        assert_eq!(server, ServerId::new("marc", "example.org", 22));
        assert_eq!(path, None);

        dialog.set(ConnectField::Port, " 2222 ".to_owned());
        dialog.set(ConnectField::StartPath, "/srv/files".to_owned());
        let (moved, folder) = dialog.target().unwrap();
        assert_eq!(moved.port, 2222);
        assert_eq!(folder.as_deref(), Some("/srv/files"));
    }

    #[test]
    fn what_is_missing_is_named() {
        let mut dialog = ConnectDialog::new(1);
        assert_eq!(dialog.target().unwrap_err(), Missing::Host);
        dialog.set(ConnectField::Host, "h".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::User);
        dialog.set(ConnectField::User, "u".to_owned());
        assert!(dialog.target().is_ok(), "no password is needed to try keys");
        dialog.set(ConnectField::Port, "x".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Port);
        dialog.set(ConnectField::Port, "70000".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Port);
        dialog.set(ConnectField::Host, "a b".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Host);
    }

    #[test]
    fn no_user_is_needed_when_the_ssh_config_or_the_account_gives_one() {
        let mut dialog = ConnectDialog::new(1);
        dialog.set(ConnectField::Host, "alias".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::User);
        dialog.set_ssh(SshHint::default(), true);
        let (server, _) = dialog.target().unwrap();
        assert_eq!(server, ServerId::new("", "alias", 22));
        dialog.set(ConnectField::User, "a b".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::User);
    }

    #[test]
    fn the_typed_key_file_goes_over_the_saved_one_and_the_passphrase_leaves_with_the_login() {
        let mut dialog = filled();
        let saved = Login {
            key_file: Some(PathBuf::from("/saved")),
            ..Login::default()
        };
        assert_eq!(
            dialog.take_login(saved.clone()).key_file,
            Some(PathBuf::from("/saved"))
        );
        dialog.set(ConnectField::KeyFile, " /typed ".to_owned());
        dialog.set_stage(Stage::Passphrase {
            key_file: PathBuf::from("/typed"),
        });
        dialog.set_passphrase(Secret::new("open sesame".to_owned()));
        let login = dialog.take_login(saved);
        assert_eq!(login.key_file, Some(PathBuf::from("/typed")));
        assert_eq!(login.passphrase.unwrap().1.expose(), "open sesame");
        assert!(dialog.passphrase().is_empty());
        assert!(!format!("{dialog:?}").contains("open sesame"));
    }

    #[test]
    fn a_passphrase_is_typed_only_in_its_stage_and_wiped_when_the_stage_ends() {
        let mut dialog = filled();
        dialog.set_passphrase(Secret::new("early".to_owned()));
        assert!(dialog.passphrase().is_empty());
        dialog.set_stage(Stage::Passphrase {
            key_file: PathBuf::from("/k"),
        });
        dialog.set_passphrase(Secret::new("late".to_owned()));
        dialog.set_stage(Stage::Editing);
        assert!(dialog.passphrase().is_empty());
    }

    #[test]
    fn a_locked_dialog_takes_no_typing() {
        let mut dialog = filled();
        dialog.set_stage(Stage::Connecting);

        dialog.set(ConnectField::Host, "other".to_owned());
        dialog.toggle_remember();
        dialog.toggle_save();
        dialog.set_password(Secret::new("changed".to_owned()));

        assert_eq!(dialog.value(ConnectField::Host), "Example.org");
        assert!(!dialog.remember() && !dialog.save());
        assert_eq!(dialog.password().expose(), "hunter2");
    }

    #[test]
    fn a_dialog_for_a_known_server_starts_with_its_values() {
        let server = ServerId::new("marc", "example.org", 2222);
        let dialog = ConnectDialog::for_server(3, &server, None);
        assert_eq!(dialog.value(ConnectField::Host), "example.org");
        assert_eq!(dialog.value(ConnectField::Port), "2222");
        assert_eq!(dialog.value(ConnectField::User), "marc");
        assert_eq!(dialog.value(ConnectField::Password), "");
        let plain = ConnectDialog::for_server(3, &ServerId::new("m", "h", 22), None);
        assert_eq!(plain.value(ConnectField::Port), "");
    }
}
