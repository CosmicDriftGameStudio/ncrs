//! The path field of the status bar: what it holds while it is edited, and the
//! words for a path that cannot be opened.

use std::path::Path;

use ncrs_fs::PathProblem;
use ncrs_i18n::{fill_template, Language, Msg};

use crate::state::Panels;

impl Panels {
    /// Sets what the path field shows, or ends the editing with `None`. Any
    /// change makes an answer still on its way stale.
    pub fn set_path_field(&mut self, value: Option<String>) {
        self.path_check_id = self.path_check_id.wrapping_add(1);
        self.path_field = value;
    }
}

/// Why `target` cannot be opened, for the status bar.
pub fn problem_text(lang: Language, problem: &PathProblem, target: &Path) -> String {
    let shown = target.display().to_string();
    match problem {
        PathProblem::Missing => {
            fill_template(lang.text(Msg::StatusPathMissing), &[("{path}", &shown)])
        }
        PathProblem::Unreadable { reason } => fill_template(
            lang.text(Msg::StatusPathUnreadable),
            &[("{path}", &shown), ("{reason}", reason)],
        ),
    }
}
