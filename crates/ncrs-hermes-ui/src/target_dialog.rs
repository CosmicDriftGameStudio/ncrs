//! The dialog of one target, the questions about a target and the messages of
//! the targets part of the page Einsortieren.

use std::path::{Path, PathBuf};

use ncrs_config::config::{Filing, FilingWriteError, TargetSetting};
use ncrs_filing::sorting::TargetState;
use ncrs_index::TargetId;
use ncrs_intake::SortTarget;

use crate::sorting_page::typed_path;

#[derive(Debug, Clone, PartialEq)]
pub enum TargetsMessage {
    /// Folds the rules of a target in or out.
    ToggleFold(TargetId),
    /// "Schema vorschlagen" and "Regeln vorschlagen" on a target.
    Suggest(TargetId),
    Add,
    /// The target at this place of the list.
    Change(usize),
    /// "Bestätigen": the folder is the right one, remember its volume.
    Confirm(usize),
    /// What the disk says of the folder of the dialog.
    FolderChecked {
        generation: u64,
        exists: bool,
    },
    /// The states of the targets, read off the UI thread.
    StatesLoaded(Vec<(TargetId, TargetState)>),
    /// What the disk and the index say of a target that is being confirmed.
    ConfirmChecked(Box<ConfirmCheck>),
    AskRemove(usize),
    ConfirmRemove,
    /// "Trotzdem bestätigen".
    ConfirmAnyway,
    CancelQuestion,
    /// Enter: the button of the question that has the focus.
    AnswerQuestion,
    /// The other button of the question gets the focus.
    SwitchAnswer,
    Name(String),
    Folder(String),
    PickFolder,
    FolderPicked(Option<PathBuf>),
    Save,
    Cancel,
    Written(Box<Result<Filing, FilingWriteError>>),
    Removed(Box<Removed>),
}

/// The target as it was when the disk was asked.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmCheck {
    pub target: SortTarget,
    pub own_volume: Option<bool>,
    /// Documents the index knows in the folder while none of them is there.
    pub gone: Result<usize, String>,
}

/// What removing a target made of the config and of its rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    pub filing: Result<Filing, FilingWriteError>,
    /// Why the rules of the target are still in the index, if they are.
    pub rules_failure: Option<String>,
}

/// What is wrong with the draft, shown under the field it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetProblem {
    FolderMissing,
    /// The folder lies in another target or holds one; its name.
    Overlaps(String),
}

/// The draft of a target; nothing is written before Save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDialog {
    /// The target this changes; `None` for a new one.
    pub replacing: Option<TargetId>,
    pub name: String,
    pub folder: String,
    pub saving: bool,
    /// What the disk said of the folder; `None` while it is being asked.
    pub folder_exists: Option<bool>,
    /// What opens once a new target is saved.
    pub then_suggest: Option<SuggestAfter>,
    /// Counts the looks at the disk, so an old answer is dropped.
    checking: u64,
}

/// The proposals for the target the dialog adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestAfter {
    /// The pattern the user asked for.
    pub wish: Option<String>,
    /// The targets there were when the dialog opened.
    known: Vec<TargetId>,
}

impl SuggestAfter {
    pub fn new(wish: Option<String>, filing: &Filing) -> Self {
        Self {
            wish,
            known: filing
                .targets
                .iter()
                .map(|setting| setting.target.id.clone())
                .collect(),
        }
    }

    /// The target that came since the dialog opened.
    pub fn added(&self, filing: &Filing) -> Option<TargetId> {
        filing
            .targets
            .iter()
            .map(|setting| &setting.target.id)
            .find(|id| !self.known.contains(id))
            .cloned()
    }
}

impl TargetDialog {
    pub fn new(existing: Option<&TargetSetting>) -> Self {
        let (replacing, name, folder) = match existing {
            Some(setting) => (
                Some(setting.target.id.clone()),
                setting.target.name.clone(),
                setting.target.path.to_string_lossy().into_owned(),
            ),
            None => (None, String::new(), String::new()),
        };
        Self {
            replacing,
            name,
            folder,
            saving: false,
            folder_exists: None,
            then_suggest: None,
            checking: 0,
        }
    }

    pub fn path(&self, home: &Path) -> PathBuf {
        typed_path(&self.folder, home)
    }

    /// Whether the draft may be saved: the disk said the folder is there and
    /// nothing else is wrong.
    pub fn can_save(&self, filing: &Filing, home: &Path) -> bool {
        !self.saving && self.folder_exists == Some(true) && self.problems(filing, home).is_empty()
    }

    /// Takes the answer of the disk for the folder if it is for the text as
    /// it stands now.
    pub fn folder_checked(&mut self, generation: u64, exists: bool) {
        if generation == self.checking {
            self.folder_exists = Some(exists);
        }
    }

    /// Starts a new look at the disk: the old answer no longer counts.
    pub fn begin_check(&mut self) -> u64 {
        self.checking = self.checking.wrapping_add(1);
        self.folder_exists = None;
        self.checking
    }

    /// What is wrong with the draft; the folder is only reported missing
    /// once the disk has said so, and the disk is never asked here.
    pub fn problems(&self, filing: &Filing, home: &Path) -> Vec<TargetProblem> {
        let path = self.path(home);
        if self.folder_exists == Some(false) {
            return vec![TargetProblem::FolderMissing];
        }
        filing
            .targets
            .iter()
            .filter(|other| Some(&other.target.id) != self.replacing.as_ref())
            .find(|other| {
                path.starts_with(&other.target.path) || other.target.path.starts_with(&path)
            })
            .map(|other| vec![TargetProblem::Overlaps(target_name(&other.target))])
            .unwrap_or_default()
    }

    /// The target the draft stands for; `own_volume` is asked of the disk by
    /// the caller.
    pub fn target(&self, id: TargetId, path: PathBuf, own_volume: Option<bool>) -> SortTarget {
        SortTarget {
            id,
            name: self.name.trim().to_owned(),
            path,
            own_volume,
        }
    }
}

/// The name of a target, or the name of its folder when it has none.
pub fn target_name(target: &SortTarget) -> String {
    if target.name.is_empty() {
        target
            .path
            .file_name()
            .map_or_else(String::new, |last| last.to_string_lossy().into_owned())
    } else {
        target.name.clone()
    }
}

/// The question about a target that is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetQuestion {
    Remove(TargetRemoval),
    Confirm(TargetConfirmation),
}

/// The question before a target is confirmed whose known documents are all
/// not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetConfirmation {
    pub target: SortTarget,
    pub gone: usize,
    /// The focus is on "Trotzdem bestätigen"; it starts on "Abbrechen".
    pub confirm_focused: bool,
}

impl TargetConfirmation {
    pub fn new(target: SortTarget, gone: usize) -> Self {
        Self {
            target,
            gone,
            confirm_focused: false,
        }
    }
}

/// The question before a target is removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRemoval {
    pub id: TargetId,
    pub name: String,
    /// Rules of the target, which go with it.
    pub rules: usize,
    pub busy: bool,
}

#[cfg(test)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
mod tests {
    use super::*;

    fn setting(id: &str, name: &str, path: &Path) -> TargetSetting {
        TargetSetting {
            target: SortTarget {
                id: TargetId::new(id).unwrap(),
                name: name.to_owned(),
                path: path.to_path_buf(),
                own_volume: None,
            },
            paused: None,
        }
    }

    fn filing_with(targets: Vec<TargetSetting>) -> Filing {
        Filing {
            targets,
            ..Filing::default()
        }
    }

    #[test]
    fn a_folder_inside_another_target_is_an_error_of_the_folder_field() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("2026");
        std::fs::create_dir(&inner).unwrap();
        let filing = filing_with(vec![setting("main", "Documents", root.path())]);
        let mut dialog = TargetDialog::new(None);
        dialog.folder = inner.to_string_lossy().into_owned();

        assert_eq!(
            dialog.problems(&filing, root.path()),
            vec![TargetProblem::Overlaps("Documents".to_owned())]
        );
    }

    #[test]
    fn the_target_being_changed_does_not_overlap_itself() {
        let root = tempfile::tempdir().unwrap();
        let existing = setting("main", "Documents", root.path());
        let filing = filing_with(vec![existing.clone()]);
        let dialog = TargetDialog::new(Some(&existing));

        assert_eq!(dialog.problems(&filing, root.path()), Vec::new());
    }

    #[test]
    fn saving_waits_for_the_answer_of_the_disk_and_drops_old_answers() {
        let root = tempfile::tempdir().unwrap();
        let mut dialog = TargetDialog::new(None);
        dialog.folder = root.path().to_string_lossy().into_owned();
        let old = dialog.begin_check();
        let current = dialog.begin_check();
        assert!(!dialog.can_save(&Filing::default(), root.path()));

        dialog.folder_checked(old, false);
        assert_eq!(dialog.folder_exists, None);
        dialog.folder_checked(current, true);
        assert!(dialog.can_save(&Filing::default(), root.path()));
    }

    #[test]
    fn a_folder_that_is_not_there_is_the_only_error_reported() {
        let root = tempfile::tempdir().unwrap();
        let mut dialog = TargetDialog::new(None);
        dialog.folder = root.path().join("nothing").to_string_lossy().into_owned();
        let generation = dialog.begin_check();
        dialog.folder_checked(generation, false);

        assert_eq!(
            dialog.problems(&Filing::default(), root.path()),
            vec![TargetProblem::FolderMissing]
        );
    }
}
