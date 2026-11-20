//! State of the modal prompt.
//!
//! Lives next to the flows rather than in the view, because it is state: the
//! app owns it and only its update changes it. The matching `view` function is
//! in `prompt_view`.
//!
//! The prompt remembers the name the user typed while the operation runs, so a
//! failure can be shown next to the text that caused it.
use std::ffi::OsString;
use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_fs::archive;
use ncrs_fs::remote::RemotePath;

use crate::rename_flow::{validate_name, NameProblem};

/// Why a typed name is refused before anything is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptProblem {
    EmptyName,
    ReservedName,
    SeparatorNotAllowed,
    CharacterNotAllowed,
    ControlNotAllowed,
    TrailingNotAllowed,
    NameTooLong,
    DeviceName,
    ArchiveExtension,
    ArchiveExists,
    ArchiveFolderMissing,
    ArchiveInsideSource,
}

/// What the prompt shows under the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptError {
    Problem(PromptProblem),
    /// A failure of the file system, already worded.
    Failed(String),
}

/// Which operation the prompt is collecting input for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// F7 — ask for a directory name to create in the active panel.
    CreateDir,
    /// Shift+F4 — ask for a file name to create in the active panel and edit.
    CreateFile,
    /// Alt+F5 — ask for the file name of a new archive in the other panel's
    /// folder and pack the selection into it.
    Pack,
    /// Cmd+V with a picture or text on the clipboard — ask for the name of the
    /// file it is saved as in the active panel.
    Paste,
    /// Ask for a pattern of names and tag the entries of the panel that match.
    TagByPattern,
    /// The same, to take the tag off the entries that match.
    UntagByPattern,
    /// Cmd/Ctrl+G: ask for a path and open it in the active panel.
    GoToPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    kind: PromptKind,
    /// The directory the new entry goes into, and the panel to reload.
    parent: PathBuf,
    /// The panel the prompt was opened for, so a switch of panels while it is
    /// open cannot put the entry into the wrong one. Set when it is opened.
    side: Option<PanelSide>,
    /// What the user typed. Kept across a failed attempt so the error sits
    /// next to the text that caused it.
    name: String,
    /// What a pack prompt packs; empty for the others.
    sources: Vec<PathBuf>,
    /// Set while the operation runs.
    busy: bool,
    /// Why the last attempt failed, in the active language.
    error: Option<PromptError>,
    /// Set for a pack prompt that packs a hit list.
    hits: Option<PackHits>,
}

/// What a pack prompt knows about the hit list it packs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackHits {
    /// The folder the search ran in; the folders below it are what "Keep
    /// folders" keeps.
    pub scope: PathBuf,
    pub keep_folders: bool,
    /// Files that share a name, with the name each gets in the archive.
    pub numbered: Vec<(PathBuf, OsString)>,
}

impl Prompt {
    fn new(kind: PromptKind, parent: PathBuf) -> Self {
        Self {
            kind,
            parent,
            side: None,
            name: String::new(),
            sources: Vec::new(),
            busy: false,
            error: None,
            hits: None,
        }
    }

    /// This prompt, belonging to the panel on `side`.
    pub fn on_side(mut self, side: PanelSide) -> Self {
        self.side = Some(side);
        self
    }

    /// The panel the prompt was opened for.
    pub fn side(&self) -> Option<PanelSide> {
        self.side
    }

    pub fn create_dir(parent: PathBuf) -> Self {
        Self::new(PromptKind::CreateDir, parent)
    }

    pub fn create_file(parent: PathBuf) -> Self {
        Self::new(PromptKind::CreateFile, parent)
    }

    /// The pattern prompt of the panel folder `parent`, starting with every name.
    pub fn tag_pattern(parent: PathBuf, untag: bool) -> Self {
        let kind = if untag {
            PromptKind::UntagByPattern
        } else {
            PromptKind::TagByPattern
        };
        Self {
            name: "*.*".to_owned(),
            ..Self::new(kind, parent)
        }
    }

    /// The "Go to path" prompt, empty, for the panel whose folder is `parent`.
    pub fn go_to_path(parent: PathBuf) -> Self {
        Self::new(PromptKind::GoToPath, parent)
    }

    /// The paste prompt for the folder `parent`, with the suggested file name.
    pub fn paste(parent: PathBuf, name: String) -> Self {
        Self {
            name,
            ..Self::new(PromptKind::Paste, parent)
        }
    }

    /// The pack prompt for the folder `parent`. The field starts as the full
    /// path of the archive-to-be, so the user sees where it will be made.
    pub fn pack(parent: PathBuf, sources: Vec<PathBuf>, name: &str) -> Self {
        Self {
            sources,
            name: parent.join(name).to_string_lossy().into_owned(),
            ..Self::new(PromptKind::Pack, parent)
        }
    }

    /// The pack prompt for the files of a hit list that ran in `scope`.
    pub fn pack_hits(parent: PathBuf, sources: Vec<PathBuf>, scope: PathBuf, name: &str) -> Self {
        let numbered = ncrs_fs::flat_names::members_of_shared_names(&sources);
        Self {
            hits: Some(PackHits {
                scope,
                keep_folders: false,
                numbered,
            }),
            ..Self::pack(parent, sources, name)
        }
    }

    pub fn hits(&self) -> Option<&PackHits> {
        self.hits.as_ref()
    }

    pub fn toggle_keep_folders(&mut self) {
        if let Some(hits) = self.hits.as_mut() {
            hits.keep_folders = !hits.keep_folders;
        }
    }

    /// The folder entry names are made relative to, when "Keep folders" is on.
    pub fn kept_folders_under(&self) -> Option<&std::path::Path> {
        self.hits
            .as_ref()
            .filter(|hits| hits.keep_folders)
            .map(|hits| hits.scope.as_path())
    }

    /// Where a pack prompt writes: the typed text, a relative one taken from
    /// `parent`, an absolute one as it is.
    pub fn archive_path(&self) -> PathBuf {
        self.parent.join(self.name.trim()).components().collect()
    }

    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    pub fn kind(&self) -> PromptKind {
        self.kind
    }

    /// The prompt asks for a pattern of names, not for a name to create.
    pub fn is_pattern(&self) -> bool {
        matches!(
            self.kind,
            PromptKind::TagByPattern | PromptKind::UntagByPattern
        )
    }

    /// Replaces the typed name and clears any error: a new keystroke means the
    /// previous complaint no longer applies.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
        self.error = None;
    }

    /// The format the typed ending selects; `None` while it is none we know.
    pub fn archive_format(&self) -> Option<archive::ArchiveFormat> {
        archive::ArchiveFormat::of_name(std::ffi::OsStr::new(self.name.trim()))
    }

    /// Puts the ending of `format` on the typed name in place of the one it
    /// has, or after the name when it has none.
    pub fn set_archive_format(&mut self, format: archive::ArchiveFormat) {
        let typed = self.name.trim();
        let stem = archive::without_archive_ending(typed);
        self.set_name(format!("{stem}{}", format.ending()));
    }

    /// The text the user typed.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Marks the operation as running or finished.
    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Shows a validation failure, in the language the UI is in.
    pub fn set_validation_error(&mut self, problem: PromptProblem) {
        self.error = Some(PromptError::Problem(problem));
    }

    /// Takes the shown complaint away, before the next attempt.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Shows a failure from the filesystem.
    pub fn set_error(&mut self, message: String) {
        self.error = Some(PromptError::Failed(message));
    }

    pub fn error(&self) -> Option<&PromptError> {
        self.error.as_ref()
    }

    /// Whether the typed name could be a directory or file name. Empty, `.` and `..`
    /// are rejected before the filesystem is touched, so the error appears while
    /// the dialog is still open instead of after a round trip. A pack name also
    /// needs a known archive ending and must not be taken in the folder.
    pub fn validate(&self) -> Result<(), PromptProblem> {
        let name = self.name().trim();
        if self.kind == PromptKind::Pack {
            return self.validate_pack(name);
        }
        if self.is_pattern() || self.kind == PromptKind::GoToPath {
            return if name.is_empty() {
                Err(PromptProblem::EmptyName)
            } else {
                Ok(())
            };
        }
        if name.contains('\\') {
            return Err(PromptProblem::SeparatorNotAllowed);
        }
        let on_server = RemotePath::parse(&self.parent).is_some();
        validate_name(name, cfg!(windows) && !on_server).map_err(|problem| match problem {
            NameProblem::Empty => PromptProblem::EmptyName,
            NameProblem::Dots => PromptProblem::ReservedName,
            NameProblem::Separator => PromptProblem::SeparatorNotAllowed,
            NameProblem::Character(_) => PromptProblem::CharacterNotAllowed,
            NameProblem::Control => PromptProblem::ControlNotAllowed,
            NameProblem::TrailingDotOrSpace => PromptProblem::TrailingNotAllowed,
            NameProblem::TooLong => PromptProblem::NameTooLong,
            NameProblem::Device => PromptProblem::DeviceName,
        })
    }

    /// The checks of a pack name that need no disk: the rest is
    /// [`PackTargetCheck::run`], which may block on a slow folder.
    fn validate_pack(&self, typed: &str) -> Result<(), PromptProblem> {
        if typed.is_empty() {
            return Err(PromptProblem::EmptyName);
        }
        let target = self.archive_path();
        let Some(file_name) = target.file_name() else {
            return Err(PromptProblem::ReservedName);
        };
        if !archive::is_archive_name(file_name) {
            return Err(PromptProblem::ArchiveExtension);
        }
        Ok(())
    }

    /// What the disk has to say about the archive the pack prompt would write.
    pub fn pack_target_check(&self) -> PackTargetCheck {
        PackTargetCheck {
            target: self.archive_path(),
            sources: self.sources.clone(),
        }
    }

    /// The folder the new entry goes into.
    pub fn parent(&self) -> &std::path::Path {
        &self.parent
    }
}

/// The checks of a pack target that ask the file system. They block, so they
/// run on the blocking pool.
#[derive(Debug, Clone)]
pub struct PackTargetCheck {
    target: PathBuf,
    sources: Vec<PathBuf>,
}

impl PackTargetCheck {
    pub fn run(&self) -> Result<(), PromptProblem> {
        let Some(parent_folder) = self.target.parent().filter(|folder| folder.is_dir()) else {
            return Err(PromptProblem::ArchiveFolderMissing);
        };
        let target_folder = parent_folder
            .canonicalize()
            .unwrap_or_else(|_| parent_folder.to_path_buf());
        let inside_source = self.sources.iter().any(|source| {
            let resolved_source = source.canonicalize().unwrap_or_else(|_| source.clone());
            target_folder.starts_with(resolved_source)
        });
        if inside_source {
            return Err(PromptProblem::ArchiveInsideSource);
        }
        if std::fs::symlink_metadata(&self.target).is_ok() {
            return Err(PromptProblem::ArchiveExists);
        }
        Ok(())
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[cfg(test)]
mod tests {
    use super::*;

    fn fully_validated(prompt: &Prompt) -> Result<(), PromptProblem> {
        prompt.validate()?;
        if prompt.kind() == PromptKind::Pack {
            return prompt.pack_target_check().run();
        }
        Ok(())
    }

    fn prompt_with(name: &str) -> Prompt {
        let mut prompt = Prompt::create_dir(PathBuf::from("/tmp"));
        prompt.set_name(name.to_string());
        prompt
    }

    #[test]
    fn a_pattern_may_hold_wildcards_and_separators_but_not_nothing() {
        let mut prompt = Prompt::tag_pattern(PathBuf::from("/tmp"), false);
        assert!(prompt.is_pattern());
        assert_eq!(prompt.name(), "*.*");
        assert_eq!(prompt.validate(), Ok(()));
        prompt.set_name("*.jpg; IMG_??.png".to_owned());
        assert_eq!(prompt.validate(), Ok(()));
        prompt.set_name("  ".to_owned());
        assert_eq!(prompt.validate(), Err(PromptProblem::EmptyName));
        assert!(!Prompt::create_dir(PathBuf::from("/tmp")).is_pattern());
    }

    #[test]
    fn the_go_to_path_prompt_starts_empty_and_takes_any_path_text() {
        let mut prompt = Prompt::go_to_path(PathBuf::from("/tmp"));
        assert_eq!(prompt.name(), "");
        assert_eq!(prompt.validate(), Err(PromptProblem::EmptyName));
        prompt.set_name("~/Dokumente/Rechnung 1/2.pdf".to_owned());
        assert_eq!(prompt.validate(), Ok(()));
        prompt.set_error("Not found".to_owned());
        prompt.clear_error();
        assert_eq!(prompt.error(), None);
    }

    #[test]
    fn choosing_a_format_swaps_the_ending_and_typing_one_selects_it() {
        use archive::ArchiveFormat;
        let mut prompt = Prompt::pack(PathBuf::from("/x"), vec![], "fotos.zip");
        assert_eq!(prompt.archive_format(), Some(ArchiveFormat::Zip));
        let stem = PathBuf::from("/x").join("fotos");

        for format in [
            ArchiveFormat::TarGz,
            ArchiveFormat::SevenZip,
            ArchiveFormat::TarZst,
            ArchiveFormat::Tar,
            ArchiveFormat::Zip,
        ] {
            prompt.set_archive_format(format);
            assert_eq!(prompt.archive_format(), Some(format));
            assert_eq!(
                archive::without_archive_ending(prompt.name()),
                stem.to_string_lossy()
            );
        }

        prompt.set_name("/x/fotos.tar.zst".to_string());
        assert_eq!(prompt.archive_format(), Some(ArchiveFormat::TarZst));
        prompt.set_name("/x/fotos".to_string());
        assert_eq!(prompt.archive_format(), None);
        prompt.set_archive_format(ArchiveFormat::SevenZip);
        assert_eq!(prompt.name(), "/x/fotos.7z");
    }

    #[test]
    fn an_ordinary_name_is_accepted() {
        assert_eq!(prompt_with("Projekte").validate(), Ok(()));
        // A name with spaces is normal on every platform here.
        assert_eq!(prompt_with("Meine Dateien").validate(), Ok(()));
        // Non-ASCII must work; this is a file manager, not a shell.
        assert_eq!(prompt_with("Übersicht").validate(), Ok(()));
    }

    /// The three rejections that a user can type but a filesystem would answer
    /// late and less clearly.
    #[test]
    fn names_that_cannot_be_created_are_rejected() {
        assert_eq!(prompt_with("").validate(), Err(PromptProblem::EmptyName));
        assert_eq!(prompt_with("   ").validate(), Err(PromptProblem::EmptyName));
        assert_eq!(
            prompt_with(".").validate(),
            Err(PromptProblem::ReservedName)
        );
        assert_eq!(
            prompt_with("..").validate(),
            Err(PromptProblem::ReservedName)
        );
        assert_eq!(
            prompt_with("a/b").validate(),
            Err(PromptProblem::SeparatorNotAllowed)
        );
        assert_eq!(
            prompt_with(r"a\b").validate(),
            Err(PromptProblem::SeparatorNotAllowed)
        );
    }

    /// A rejected name must never reach the filesystem, because `..` would
    /// create a directory above the active panel. The name and the parent are
    /// joined by the caller, so the validation is the only thing standing
    /// between a keystroke and the parent directory.
    #[test]
    fn a_traversal_name_is_rejected_before_it_can_be_joined() {
        assert!(prompt_with("..").validate().is_err());
        assert!(prompt_with("a/b").validate().is_err());
    }

    /// The field of a pack prompt as the user left it.
    fn pack_prompt_in(folder: &std::path::Path, typed: &str) -> Prompt {
        let mut prompt = Prompt::pack(folder.to_path_buf(), Vec::new(), "start.zip");
        prompt.set_name(typed.to_string());
        prompt
    }

    #[test]
    fn a_pack_prompt_starts_with_the_full_path() {
        let full = PathBuf::from("/other").join("fotos.zip");
        let prompt = Prompt::pack(PathBuf::from("/other"), Vec::new(), "fotos.zip");
        assert_eq!(prompt.name(), full.to_string_lossy());
        assert_eq!(prompt.archive_path(), full);
    }

    #[test]
    fn a_relative_pack_name_is_taken_from_the_other_panel_and_an_absolute_path_as_it_is() {
        let other = tempfile::tempdir().unwrap();
        let own = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join("sub")).unwrap();

        let relative = pack_prompt_in(other.path(), "sub/new.zip");
        assert_eq!(relative.archive_path(), other.path().join("sub/new.zip"));
        assert_eq!(fully_validated(&relative), Ok(()));

        let own_file = own.path().join("mine.7z");
        let absolute = pack_prompt_in(other.path(), own_file.to_str().unwrap());
        assert_eq!(absolute.archive_path(), own_file);
        assert_eq!(fully_validated(&absolute), Ok(()));
    }

    #[test]
    fn a_pack_name_needs_a_known_ending_a_free_name_and_a_folder() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("taken.zip"), b"x").unwrap();

        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "new.tar.zst")),
            Ok(())
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "New.ZIP")),
            Ok(())
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "notes.txt")),
            Err(PromptProblem::ArchiveExtension)
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "noending")),
            Err(PromptProblem::ArchiveExtension)
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "taken.zip")),
            Err(PromptProblem::ArchiveExists)
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "missing/b.zip")),
            Err(PromptProblem::ArchiveFolderMissing)
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "")),
            Err(PromptProblem::EmptyName)
        );
        assert_eq!(
            fully_validated(&pack_prompt_in(folder.path(), "..")),
            Err(PromptProblem::ReservedName)
        );
    }

    #[test]
    fn a_pack_target_inside_a_packed_folder_is_rejected() {
        let folder = tempfile::tempdir().unwrap();
        let packed = folder.path().join("fotos");
        std::fs::create_dir_all(packed.join("deep")).unwrap();
        let mut prompt = Prompt::pack(folder.path().to_path_buf(), vec![packed.clone()], "x.zip");
        assert_eq!(fully_validated(&prompt), Ok(()));

        prompt.set_name(packed.join("fotos.zip").to_string_lossy().into_owned());
        assert_eq!(
            fully_validated(&prompt),
            Err(PromptProblem::ArchiveInsideSource)
        );

        prompt.set_name("fotos/deep/../fotos.zip".to_string());
        assert_eq!(
            fully_validated(&prompt),
            Err(PromptProblem::ArchiveInsideSource)
        );
    }

    #[test]
    fn a_name_that_is_no_archive_is_fine_for_the_other_prompts() {
        assert_eq!(prompt_with("notes.txt").validate(), Ok(()));
    }

    /// Whitespace around a name is easy to type and should not create a
    /// directory called " x".
    #[test]
    fn surrounding_whitespace_is_accepted_and_left_to_the_target() {
        let p = prompt_with("  spaced  ");
        assert_eq!(p.validate(), Ok(()));
        assert_eq!(p.name(), "  spaced  ");
    }
}
