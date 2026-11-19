//! What the configured folders say about a path: which Hermes commands the
//! context menu may offer for it.

use std::path::Path;

use ncrs_intake::LocationMode;

use crate::hermes_column::{role_of, FolderRole};
use crate::state::Hermes;

/// How a path relates to the folders Hermes reads and files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorRoles {
    /// The path lies outside the inbox, so it can be handed to it.
    pub can_be_handed_to_inbox: bool,
    pub lies_in_a_filing_location: bool,
    pub inbox_role_is_free: bool,
    pub filing_location_role_is_free: bool,
    pub archive_role_is_free: bool,
}

impl Hermes {
    pub fn cursor_roles(&self, path: &Path) -> CursorRoles {
        let locations = &self.filing_config.locations;
        let role_free = |role| role_of(path, locations) != Some(role);
        let inbox = locations
            .iter()
            .find(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.as_path());
        CursorRoles {
            can_be_handed_to_inbox: inbox.is_some_and(|inbox| !path.starts_with(inbox)),
            lies_in_a_filing_location: locations
                .iter()
                .any(|location| path.starts_with(&location.path)),
            inbox_role_is_free: role_free(FolderRole::Inbox),
            filing_location_role_is_free: role_free(FolderRole::FilingLocation),
            archive_role_is_free: role_free(FolderRole::Archive),
        }
    }

    /// The setting of the load changed: the pipeline runs by it from now on.
    /// A load setting leaves the thread count of the local model to the system.
    pub fn apply_load_setting(&mut self, load: ncrs_pipeline::Load, now: std::time::SystemTime) {
        self.filing_config.load = load;
        if let ncrs_config::config::AiSetting::Local { threads } = &mut self.filing_config.ai {
            *threads = None;
        }
        self.set_pipeline_load(load, now);
    }
}
