//! What the configured folders say about a path: which Hermes commands the
//! context menu may offer for it.

use std::path::Path;

use ncrs_intake::LocationMode;

use crate::hermes_column::{role_of, FolderRole, HermesFileState};
use crate::state::Hermes;

/// How a path relates to the folders Hermes reads and files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorRoles {
    /// The path lies outside the inbox, so it can be handed to it.
    pub can_be_handed_to_inbox: bool,
    pub lies_in_a_filing_location: bool,
    /// The path is the folder the files are filed into.
    pub is_write_folder: bool,
    /// The path lies below the write folder.
    pub lies_in_write_folder: bool,
    /// The path is a read folder that files its new files by itself.
    pub is_continuous: bool,
    pub filing_location_role_is_free: bool,
    pub archive_role_is_free: bool,
}

impl Hermes {
    pub fn cursor_roles(&self, path: &Path) -> CursorRoles {
        let locations = &self.filing_config.locations;
        let role_free = |role| role_of(path, locations) != Some(role);
        let inbox = self.inbox_location();
        let write_folder = self.write_folder();
        CursorRoles {
            can_be_handed_to_inbox: inbox.is_some_and(|inbox_path| !path.starts_with(inbox_path)),
            lies_in_a_filing_location: locations
                .iter()
                .any(|location| path.starts_with(&location.path)),
            is_write_folder: write_folder == Some(path),
            lies_in_write_folder: write_folder
                .is_some_and(|folder| path != folder && path.starts_with(folder)),
            is_continuous: locations
                .iter()
                .any(|location| location.path == path && location.continuous),
            filing_location_role_is_free: role_free(FolderRole::FilingLocation),
            archive_role_is_free: role_free(FolderRole::Archive),
        }
    }

    /// The inbox the configured folders name, if any.
    pub fn inbox_location(&self) -> Option<&Path> {
        self.filing_config
            .locations
            .iter()
            .find(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.as_path())
    }

    /// The folder the files are filed into, if one is configured.
    pub fn write_folder(&self) -> Option<&Path> {
        self.filing_config
            .locations
            .iter()
            .find(|location| location.is_target)
            .map(|location| location.path.as_path())
    }

    /// Hermes no longer looks at the file, or at the whole folder.
    pub fn cursor_is_excluded(&self, path: &Path, is_dir: bool) -> bool {
        if is_dir {
            self.hermes_column
                .summary_of(path)
                .is_some_and(|summary| summary.excluded)
        } else {
            self.hermes_column.state_of(path) == Some(HermesFileState::Excluded)
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
