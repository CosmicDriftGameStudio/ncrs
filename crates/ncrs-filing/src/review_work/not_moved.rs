//! Why a document was not filed, kept apart from the words so that the page
//! that shows it says it in the language of the person.

use std::path::Path;

use ncrs_i18n::{fill_template, Language, Msg};

use crate::execute::SkipReason;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotMovedReason {
    /// What the operating system said, as it said it.
    Os(String),
    OutsideFolders,
    Changed,
    Missing,
    NameTaken,
    Stopped,
    /// A reason that has no words of its own yet.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotMoved {
    /// The file name; empty when the failure is not about one file.
    pub file: String,
    pub reason: NotMovedReason,
}

impl NotMoved {
    pub fn other(reason: impl Into<String>) -> Self {
        Self {
            file: String::new(),
            reason: NotMovedReason::Other(reason.into()),
        }
    }

    pub fn text(&self, lang: Language) -> String {
        let with_file = |msg: Msg| fill_template(lang.text(msg), &[("{file}", &self.file)]);
        let raw = match &self.reason {
            NotMovedReason::OutsideFolders => return with_file(Msg::NotMovedOutsideFolders),
            NotMovedReason::Changed => return with_file(Msg::NotMovedChanged),
            NotMovedReason::Missing => return with_file(Msg::NotMovedMissing),
            NotMovedReason::NameTaken => return with_file(Msg::NotMovedNameTaken),
            NotMovedReason::Stopped => return with_file(Msg::NotMovedStopped),
            NotMovedReason::Os(text) | NotMovedReason::Other(text) => text,
        };
        if self.file.is_empty() {
            return raw.clone();
        }
        fill_template(
            lang.text(Msg::NotMovedWithReason),
            &[("{file}", &self.file), ("{reason}", raw)],
        )
    }
}

impl From<String> for NotMoved {
    fn from(reason: String) -> Self {
        Self::other(reason)
    }
}

impl NotMoved {
    pub(super) fn skipped(path: &Path, skip: &SkipReason) -> Self {
        let reason = match skip {
            SkipReason::Failed(detail) => NotMovedReason::Os(detail.clone()),
            SkipReason::OutsideLocation | SkipReason::LeavesLocation => {
                NotMovedReason::OutsideFolders
            }
            SkipReason::SourceChanged => NotMovedReason::Changed,
            SkipReason::SourceMissing => NotMovedReason::Missing,
            SkipReason::TargetExists => NotMovedReason::NameTaken,
            SkipReason::Cancelled | SkipReason::Interrupted => NotMovedReason::Stopped,
        };
        Self {
            file: file_name_of(path),
            reason,
        }
    }

    /// The run came back with no word about the file.
    pub(super) fn unsettled(path: &Path) -> Self {
        Self {
            file: file_name_of(path),
            reason: NotMovedReason::Stopped,
        }
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
