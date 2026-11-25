//! The dialog of one source, and of the one-off filing of a folder.

use std::path::{Path, PathBuf};

use ncrs_config::config::{same_folder, Filing, SourceSetting};
use ncrs_filing::sorting::is_on_own_volume;
use ncrs_fs::search::PathFilter;
use ncrs_index::TargetId;
use ncrs_intake::{LocationMode, LocationTransfer, OnNoText, OnSeveralTargets, SortSource};

use crate::sorting_page::{own_volume_to_save, typed_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDialogKind {
    Watched,
    Once,
}

/// The fields the keyboard walks through, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceField {
    Folder,
    Include,
    Exclude,
    /// The target at this place of the list.
    Target(usize),
    AddTarget,
    When,
    Files,
    NoText,
    Several,
}

/// What the disk says of the folder of the draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderCheck {
    Looking,
    Missing,
    /// There, with its resolved path.
    Found(PathBuf),
}

/// The fields of a dialog that lists this many targets.
fn fields(targets: usize) -> Vec<SourceField> {
    let mut fields = vec![
        SourceField::Folder,
        SourceField::Include,
        SourceField::Exclude,
    ];
    fields.extend((0..targets).map(SourceField::Target));
    fields.extend([
        SourceField::AddTarget,
        SourceField::When,
        SourceField::Files,
        SourceField::NoText,
        SourceField::Several,
    ]);
    fields
}

/// What is wrong with the draft, shown under the field it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProblem {
    FolderMissing,
    InsideTarget,
    NoTarget,
    /// The pattern as typed, and the field it is in.
    Glob {
        field: SourceField,
        pattern: String,
    },
}

/// The draft of a source; nothing is written before Save.
#[derive(Debug, Clone)]
pub struct SourceDialog {
    pub kind: SourceDialogKind,
    /// The folder of the source this edits; `None` for a new one.
    pub replacing: Option<PathBuf>,
    /// What is saved for the folder of the source this edits.
    pub own_volume: Option<bool>,
    pub folder: String,
    pub include: String,
    pub exclude: String,
    pub targets: Vec<TargetId>,
    pub watch: bool,
    pub transfer: LocationTransfer,
    pub no_text: OnNoText,
    pub several: OnSeveralTargets,
    pub focus: SourceField,
    pub folder_check: FolderCheck,
    pub saving: bool,
    /// The targets there were when the dialog of a new target opened over this
    /// one; the one it writes is told by them.
    pub known_targets: Option<Vec<TargetId>>,
}

impl SourceDialog {
    /// A draft for `folder`; the settings of a source at the same folder are
    /// taken over. A once dialog never watches.
    pub fn new(kind: SourceDialogKind, folder: Option<&Path>, filing: &Filing) -> Self {
        let existing = folder.and_then(|path| {
            filing
                .sources
                .iter()
                .find(|setting| same_folder(&setting.source.path, path))
        });
        let mut dialog = Self {
            kind,
            replacing: None,
            own_volume: None,
            folder: folder
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            include: String::new(),
            exclude: String::new(),
            targets: Vec::new(),
            watch: kind == SourceDialogKind::Watched,
            transfer: LocationTransfer::default(),
            no_text: OnNoText::default(),
            several: OnSeveralTargets::default(),
            focus: SourceField::Folder,
            folder_check: FolderCheck::Looking,
            saving: false,
            known_targets: None,
        };
        if let Some(setting) = existing {
            dialog.take_over(setting);
        }
        dialog
    }

    fn take_over(&mut self, setting: &SourceSetting) {
        let source = &setting.source;
        self.replacing = Some(source.path.clone());
        self.own_volume = source.own_volume;
        self.include = source.include.clone();
        self.exclude = source.exclude.clone();
        self.targets = source.targets.clone();
        self.watch = self.kind == SourceDialogKind::Watched && source.watch;
        self.transfer = source.transfer;
        self.no_text = source.no_text;
        self.several = source.several;
    }

    pub fn move_focus(&mut self, forward: bool, targets: usize) {
        let fields = fields(targets);
        let at = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let step = if forward { 1 } else { fields.len() - 1 };
        self.focus = fields[(at + step) % fields.len()];
    }

    /// Left or Right on the focused choice. `watch` is not changed in a once
    /// dialog: there the choice is fixed.
    pub fn step_choice(&mut self) {
        fn flip<T: Copy + PartialEq>(value: &mut T, one: T, other: T) {
            *value = if *value == one { other } else { one };
        }
        match self.focus {
            SourceField::When if self.kind == SourceDialogKind::Watched => self.watch = !self.watch,
            SourceField::Files => flip(
                &mut self.transfer,
                LocationTransfer::Move,
                LocationTransfer::Copy,
            ),
            SourceField::NoText => flip(&mut self.no_text, OnNoText::Ask, OnNoText::Skip),
            SourceField::Several => {
                flip(
                    &mut self.several,
                    OnSeveralTargets::Ask,
                    OnSeveralTargets::CopyToAll,
                );
            }
            _ => {}
        }
    }

    pub fn toggle_target(&mut self, id: &TargetId) {
        match self.targets.iter().position(|chosen| chosen == id) {
            Some(at) => {
                self.targets.remove(at);
            }
            None => self.targets.push(id.clone()),
        }
    }

    /// The dialog of a new target opens.
    pub fn remember_targets(&mut self, filing: &Filing) {
        self.known_targets = Some(target_ids(filing).cloned().collect());
    }

    /// The dialog of a new target wrote `filing`: the new one is ticked. The
    /// settings in memory may know it already, the file is watched.
    pub fn tick_new_targets(&mut self, filing: &Filing) {
        let Some(known) = self.known_targets.take() else {
            return;
        };
        for id in target_ids(filing) {
            if !known.contains(id) && !self.targets.contains(id) {
                self.targets.push(id.clone());
            }
        }
    }

    pub fn path(&self, home: &Path) -> PathBuf {
        typed_path(&self.folder, home)
    }

    /// What is wrong with the draft as far as it is known; the folder counts
    /// once the disk answered.
    pub fn problems(&self, filing: &Filing) -> Vec<SourceProblem> {
        let mut problems = Vec::new();
        match &self.folder_check {
            FolderCheck::Missing if !self.folder.trim().is_empty() => {
                problems.push(SourceProblem::FolderMissing);
            }
            FolderCheck::Found(path) if self.watch && inside_a_target(path, filing) => {
                problems.push(SourceProblem::InsideTarget);
            }
            FolderCheck::Looking | FolderCheck::Missing | FolderCheck::Found(_) => {}
        }
        if self.targets.is_empty() {
            problems.push(SourceProblem::NoTarget);
        }
        let typed = [
            (SourceField::Include, &self.include),
            (SourceField::Exclude, &self.exclude),
        ];
        problems.extend(typed.into_iter().filter_map(|(field, text)| {
            let pattern = broken_pattern(text)?.to_owned();
            Some(SourceProblem::Glob { field, pattern })
        }));
        problems
    }

    pub fn can_be_saved(&self, filing: &Filing) -> bool {
        !self.saving
            && matches!(self.folder_check, FolderCheck::Found(_))
            && self.problems(filing).is_empty()
    }

    /// Watched, the folder holds a target or a folder of the index: the pipeline
    /// takes only folders that do not overlap, so nothing in it is read by
    /// itself.
    pub fn is_not_read_by_itself(&self, filing: &Filing) -> bool {
        let FolderCheck::Found(folder) = &self.folder_check else {
            return false;
        };
        let targets = filing.targets.iter().map(|setting| &setting.target.path);
        let indexed = filing
            .locations
            .iter()
            .filter(|location| location.mode == LocationMode::Archive)
            .map(|location| &location.path);
        self.watch
            && targets
                .chain(indexed)
                .any(|inner| inner != folder && inner.starts_with(folder))
    }

    /// The source the draft stands for. Blocking: `own_volume` is asked of the
    /// disk.
    pub fn source(&self, home: &Path) -> SortSource {
        let path = self.path(home);
        let before = self.replacing.as_deref().map(|old| (old, self.own_volume));
        SortSource {
            own_volume: own_volume_to_save(before, &path, is_on_own_volume(&path)),
            path,
            include: self.include.trim().to_owned(),
            exclude: self.exclude.trim().to_owned(),
            targets: self.targets.clone(),
            watch: self.watch,
            transfer: self.transfer,
            no_text: self.no_text,
            several: self.several,
        }
    }
}

fn target_ids(filing: &Filing) -> impl Iterator<Item = &TargetId> {
    filing.targets.iter().map(|setting| &setting.target.id)
}

/// The first of the patterns in `typed` that cannot be used, as it is there.
fn broken_pattern(typed: &str) -> Option<&str> {
    typed
        .split(';')
        .map(str::trim)
        .find(|pattern| PathFilter::new(PathBuf::new(), pattern, "", false).is_err())
}

/// Blocking: the resolved path of the folder, if it is there.
pub fn look_up_folder(path: &Path) -> Option<PathBuf> {
    path.is_dir()
        .then(|| ncrs_intake::canonical_path(path).unwrap_or_else(|_| path.to_path_buf()))
}

fn inside_a_target(path: &Path, filing: &Filing) -> bool {
    filing
        .targets
        .iter()
        .any(|setting| path.starts_with(&setting.target.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_config::config::{
        add_sort_target_with_home, load, upsert_sort_source_with_home, TargetSetting,
    };
    use ncrs_intake::SortTarget;

    /// What the background task does: looks for the folder of the draft.
    fn look(draft: &mut SourceDialog, home: &Path) {
        draft.folder_check = match look_up_folder(&draft.path(home)) {
            Some(path) => FolderCheck::Found(path),
            None => FolderCheck::Missing,
        };
    }

    fn filing_with_target(target_path: &Path) -> (Filing, TargetId) {
        let id = TargetId::new("docs").expect("valid id");
        let mut filing = Filing::default();
        filing.targets.push(TargetSetting {
            target: SortTarget {
                id: id.clone(),
                name: "Docs".to_owned(),
                path: target_path.to_path_buf(),
                own_volume: None,
            },
            paused: None,
        });
        (filing, id)
    }

    #[test]
    fn a_draft_without_target_or_with_a_missing_folder_has_problems() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (filing, id) = filing_with_target(&dir.path().join("docs"));
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, None, &filing);
        draft.folder = dir.path().join("nowhere").to_string_lossy().into_owned();
        draft.include = "[".to_owned();
        look(&mut draft, dir.path());
        let problems = draft.problems(&filing);
        assert!(problems.contains(&SourceProblem::FolderMissing));
        assert!(problems.contains(&SourceProblem::NoTarget));
        draft.folder = dir.path().to_string_lossy().into_owned();
        draft.toggle_target(&id);
        look(&mut draft, dir.path());
        let broken = |field| SourceProblem::Glob {
            field,
            pattern: "[".to_owned(),
        };
        assert_eq!(draft.problems(&filing), [broken(SourceField::Include)]);
        draft.include = "*.pdf".to_owned();
        draft.exclude = "*.tmp; [".to_owned();
        assert_eq!(draft.problems(&filing), [broken(SourceField::Exclude)]);
        draft.exclude.clear();
        assert!(draft.can_be_saved(&filing));
    }

    #[test]
    fn a_draft_is_not_saved_before_the_disk_answered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (filing, id) = filing_with_target(&dir.path().join("docs"));
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, Some(dir.path()), &filing);
        draft.toggle_target(&id);
        assert!(draft.problems(&filing).is_empty());
        assert!(!draft.can_be_saved(&filing));
        look(&mut draft, dir.path());
        assert!(draft.can_be_saved(&filing));
    }

    #[test]
    fn a_new_source_without_a_folder_shows_no_folder_problem_yet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (filing, id) = filing_with_target(&dir.path().join("docs"));
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, None, &filing);
        draft.toggle_target(&id);
        look(&mut draft, dir.path());
        assert!(draft.problems(&filing).is_empty());
        assert!(!draft.can_be_saved(&filing));
    }

    #[test]
    fn a_watched_folder_inside_a_target_is_refused_but_not_a_once_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("docs");
        std::fs::create_dir_all(target.join("in")).expect("folders");
        let target = target.canonicalize().expect("a real path");
        let (filing, id) = filing_with_target(&target);
        let mut draft =
            SourceDialog::new(SourceDialogKind::Watched, Some(&target.join("in")), &filing);
        draft.toggle_target(&id);
        look(&mut draft, dir.path());
        assert_eq!(draft.problems(&filing), vec![SourceProblem::InsideTarget]);
        let mut once = SourceDialog::new(SourceDialogKind::Once, Some(&target.join("in")), &filing);
        look(&mut once, dir.path());
        assert!(!once.watch);
        assert!(!once
            .problems(&filing)
            .contains(&SourceProblem::InsideTarget));
    }

    #[test]
    fn a_watched_folder_that_holds_a_target_says_nothing_in_it_is_read_by_itself() {
        let folder = Path::new("/home/me/Downloads");
        let (filing, _) = filing_with_target(&folder.join("Filed"));
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, Some(folder), &filing);
        assert!(!draft.is_not_read_by_itself(&filing));
        draft.folder_check = FolderCheck::Found(folder.to_path_buf());
        assert!(draft.is_not_read_by_itself(&filing));
        draft.watch = false;
        assert!(!draft.is_not_read_by_itself(&filing));
        draft.watch = true;
        draft.folder_check = FolderCheck::Found(PathBuf::from("/home/me/Scans"));
        assert!(!draft.is_not_read_by_itself(&filing));
    }

    #[test]
    fn saving_a_draft_writes_exactly_one_source_with_all_options() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&inbox).expect("folder");
        let docs = dir.path().join("docs");
        std::fs::create_dir_all(&docs).expect("folder");
        let config_path = dir.path().join("config.toml");
        let (id, config) =
            add_sort_target_with_home(&config_path, "Docs", &docs, None, Some(dir.path()))
                .expect("a target in the config");
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, Some(&inbox), &config.filing);
        draft.toggle_target(&id);
        draft.include = "*.pdf".to_owned();
        draft.exclude = "tmp*".to_owned();
        draft.transfer = LocationTransfer::Copy;
        draft.no_text = OnNoText::Skip;
        draft.several = OnSeveralTargets::CopyToAll;
        let source = draft.source(dir.path());
        upsert_sort_source_with_home(&config_path, &source, None, Some(dir.path()))
            .expect("written");
        let read = load(&config_path).expect("read back").filing;
        assert_eq!(read.targets.len(), 1);
        assert_eq!(read.sources.len(), 1);
        assert_eq!(read.sources[0].paused, None);
        let written = &read.sources[0].source;
        assert_eq!(written.include, "*.pdf");
        assert_eq!(written.exclude, "tmp*");
        assert_eq!(written.targets, vec![id]);
        assert_eq!(written.transfer, LocationTransfer::Copy);
        assert_eq!(written.no_text, OnNoText::Skip);
        assert_eq!(written.several, OnSeveralTargets::CopyToAll);
        assert!(written.watch);
    }

    #[test]
    fn a_folder_of_an_existing_source_brings_its_settings_along() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut filing, id) = filing_with_target(&dir.path().join("docs"));
        filing.sources.push(SourceSetting {
            source: SortSource {
                path: dir.path().to_path_buf(),
                include: "*.jpg".to_owned(),
                exclude: String::new(),
                targets: vec![id.clone()],
                watch: true,
                transfer: LocationTransfer::Copy,
                no_text: OnNoText::Skip,
                several: OnSeveralTargets::Ask,
                own_volume: None,
            },
            paused: None,
        });
        let once = SourceDialog::new(SourceDialogKind::Once, Some(dir.path()), &filing);
        assert_eq!(once.include, "*.jpg");
        assert_eq!(once.targets, vec![id]);
        assert_eq!(once.transfer, LocationTransfer::Copy);
        assert!(!once.watch, "a once dialog never watches");
    }

    #[test]
    fn tab_walks_the_fields_round_and_arrows_flip_the_focused_choice() {
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, None, &Filing::default());
        draft.move_focus(false, 0);
        assert_eq!(draft.focus, SourceField::Several);
        draft.step_choice();
        assert_eq!(draft.several, OnSeveralTargets::CopyToAll);
        draft.move_focus(true, 0);
        assert_eq!(draft.focus, SourceField::Folder);
        draft.step_choice();
        assert!(draft.watch, "a text field has no choice to flip");
    }

    #[test]
    fn tab_reaches_every_target_and_the_button_that_adds_one() {
        let mut draft = SourceDialog::new(SourceDialogKind::Watched, None, &Filing::default());
        draft.focus = SourceField::Exclude;
        let mut walked = Vec::new();
        for _ in 0..4 {
            draft.move_focus(true, 2);
            walked.push(draft.focus);
        }
        assert_eq!(
            walked,
            vec![
                SourceField::Target(0),
                SourceField::Target(1),
                SourceField::AddTarget,
                SourceField::When,
            ]
        );
    }
}
