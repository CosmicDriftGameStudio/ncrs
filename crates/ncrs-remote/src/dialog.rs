//! State of the dialog that connects to an SFTP server. The app owns it and only
//! `update` changes it; the view is `view`.
//!
//! The password is a [`Secret`]: this type prints without it, so no `{:?}` of
//! the dialog or of a message that carries it can leak it.

use std::fmt;
use std::path::PathBuf;

use russh::keys::PublicKey;

use ncrs_core::PanelSide;
use ncrs_fs::remote::{RemoteUrl, Secret, ServerId};

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
}

/// Why the typed values cannot be used yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    Host,
    Port,
    User,
    Password,
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
    start_path: String,
    stage: Stage,
    error: Option<String>,
    then: Option<Then>,
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
            start_path: String::new(),
            stage: Stage::Editing,
            error: None,
            then: None,
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

    pub fn set_stage(&mut self, stage: Stage) {
        self.stage = stage;
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
        let host = self.host.trim();
        if host.is_empty()
            || host
                .chars()
                .any(|c| c.is_whitespace() || c == '/' || c == '@')
        {
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
        if user.is_empty()
            || user
                .chars()
                .any(|c| c.is_whitespace() || c == '/' || c == '@' || c == ':')
        {
            return Err(Missing::User);
        }
        if self.password.is_empty() {
            return Err(Missing::Password);
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
        assert_eq!(dialog.target().unwrap_err(), Missing::Password);
        dialog.set_password(Secret::new("p".to_owned()));
        dialog.set(ConnectField::Port, "x".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Port);
        dialog.set(ConnectField::Port, "70000".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Port);
        dialog.set(ConnectField::Host, "a b".to_owned());
        assert_eq!(dialog.target().unwrap_err(), Missing::Host);
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
