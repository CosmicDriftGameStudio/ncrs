use std::path::PathBuf;
use std::sync::Arc;

use ncrs_core::PanelSide;
use ncrs_fs::remote::credentials::PasswordStore;
use ncrs_fs::remote::session::{Login, LoginEnv, Pool};
use ncrs_fs::remote::{Secret, ServerId};
use ncrs_i18n::Language;

/// What `update` reads from the app.
#[derive(Debug)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub pool: &'a Arc<Pool>,
    pub passwords: &'a Arc<dyn PasswordStore>,
    /// A page of the Ablage or a dialog is up that nothing opens over; the
    /// connect dialog itself counts.
    pub blocking_overlay_open: bool,
    pub active_side: PanelSide,
    /// The server the active panel shows, when it shows one.
    pub active_panel_server: Option<ServerId>,
    /// The key files of the saved servers that name one.
    pub key_files: Vec<(ServerId, PathBuf)>,
    /// The home folder, the agent and the user name a login is made with: what
    /// the dialog tells the user ~/.ssh/config and the login name provide.
    pub login_env: LoginEnv,
    /// The App Store build: no ssh agent, key files only by a grant.
    pub sandboxed: bool,
    pub left_request_id: u64,
    pub right_request_id: u64,
}

impl Ctx<'_> {
    /// What a connection to `server` is made with: the password and the key
    /// file of its saved entry, if it has one.
    pub fn login(&self, server: &ServerId, password: Secret) -> Login {
        Login {
            password,
            key_file: self.key_file_of(server).cloned(),
            ..Login::default()
        }
    }

    /// The key file the saved entry of `server` names.
    pub fn key_file_of(&self, server: &ServerId) -> Option<&PathBuf> {
        self.key_files
            .iter()
            .find(|(saved, _file)| saved == server)
            .map(|(_saved, file)| file)
    }

    pub fn request_id(&self, side: PanelSide) -> u64 {
        match side {
            PanelSide::Left => self.left_request_id,
            PanelSide::Right => self.right_request_id,
        }
    }
}
