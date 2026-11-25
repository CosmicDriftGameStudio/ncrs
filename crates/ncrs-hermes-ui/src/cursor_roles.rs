//! What the configured folders say about a path: which Hermes commands the
//! context menu may offer for it.

use std::path::Path;

use ncrs_config::config::same_folder;

use crate::hermes_column::HermesFileState;
use crate::state::Hermes;

/// How a path relates to the folders Hermes takes files from and files into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorRoles {
    pub is_source: bool,
    pub is_target: bool,
    /// The path lies in a folder Hermes reads, so it can be read again.
    pub lies_in_an_indexed_folder: bool,
}

impl Hermes {
    pub fn cursor_roles(&self, path: &Path) -> CursorRoles {
        let filing = &self.filing_config;
        CursorRoles {
            is_source: filing
                .sources
                .iter()
                .any(|setting| same_folder(&setting.source.path, path)),
            is_target: filing
                .targets
                .iter()
                .any(|setting| same_folder(&setting.target.path, path)),
            lies_in_an_indexed_folder: filing
                .locations
                .iter()
                .any(|location| path.starts_with(&location.path)),
        }
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
