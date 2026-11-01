use std::sync::Arc;

use ncrs_core::PanelSide;
use ncrs_fs::remote::credentials::PasswordStore;
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::ServerId;
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
    pub left_request_id: u64,
    pub right_request_id: u64,
}

impl Ctx<'_> {
    pub fn request_id(&self, side: PanelSide) -> u64 {
        match side {
            PanelSide::Left => self.left_request_id,
            PanelSide::Right => self.right_request_id,
        }
    }
}
