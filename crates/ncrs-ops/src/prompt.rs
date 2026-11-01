//! State of the modal prompt.
//!
//! Lives next to the flows rather than in the view, because it is state: the
//! app owns it and only its update changes it. The matching `view` function is
//! in `prompt_view`.
//!
//! Not yet in the roadmap's T10 (config): the prompt remembers the name the
//! user typed while the operation runs, so a failure can be shown next to the
//! text that caused it.
use std::ffi::OsString;
use std::path::PathBuf;

use ncrs_fs::archive;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    kind: PromptKind,
    /// The directory the new entry goes into, and the panel to reload.
    parent: PathBuf,
    /// What the user typed. Kept across a failed attempt so the error sits
    /// next to the text that caused it.
    name: String,
    /// What a pack prompt packs; empty for the others.
    sources: Vec<PathBuf>,
    /// Set while the operation runs.
    busy: bool,
    /// Why the last attempt failed, in the active language.
    error: Option<String>,
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
            name: String::new(),
            sources: Vec::new(),
            busy: false,
            error: None,
            hits: None,
        }
    }

    pub fn create_dir(parent: PathBuf) -> Self {
        Self::new(PromptKind::CreateDir, parent)
    }

    pub fn create_file(parent: PathBuf) -> Self {
        Self::new(PromptKind::CreateFile, parent)
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

    /// Replaces the typed name and clears any error: a new keystroke means the
    /// previous complaint no longer applies.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
        self.error = None;
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
    pub fn set_validation_error(&mut self, reason: &'static str) {
        self.error = Some(reason.to_string());
    }

    /// Shows a failure from the filesystem.
    pub fn set_error(&mut self, message: String) {
        self.error = Some(message);
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether the typed name could be a directory or file name. Empty, `.` and `..`
    /// are rejected before the filesystem is touched, so the error appears while
    /// the dialog is still open instead of after a round trip. A pack name also
    /// needs a known archive ending and must not be taken in the folder.
    pub fn validate(&self) -> Result<(), &'static str> {
        let name = self.name().trim();
        if self.kind == PromptKind::Pack {
            return self.validate_pack(name);
        }
        if name.is_empty() {
            return Err("empty_name");
        }
        if name == "." || name == ".." {
            return Err("reserved_name");
        }
        if name.contains('/') || name.contains('\\') {
            return Err("separator_not_allowed");
        }
        Ok(())
    }

    fn validate_pack(&self, typed: &str) -> Result<(), &'static str> {
        if typed.is_empty() {
            return Err("empty_name");
        }
        let target = self.archive_path();
        let Some(file_name) = target.file_name() else {
            return Err("reserved_name");
        };
        if !archive::is_archive_name(file_name) {
            return Err("archive_extension");
        }
        if !target.parent().is_some_and(std::path::Path::is_dir) {
            return Err("archive_folder_missing");
        }
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err("archive_exists");
        }
        Ok(())
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_with(name: &str) -> Prompt {
        let mut prompt = Prompt::create_dir(PathBuf::from("/tmp"));
        prompt.set_name(name.to_string());
        prompt
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
        assert_eq!(prompt_with("").validate(), Err("empty_name"));
        assert_eq!(prompt_with("   ").validate(), Err("empty_name"));
        assert_eq!(prompt_with(".").validate(), Err("reserved_name"));
        assert_eq!(prompt_with("..").validate(), Err("reserved_name"));
        assert_eq!(prompt_with("a/b").validate(), Err("separator_not_allowed"));
        assert_eq!(prompt_with(r"a\b").validate(), Err("separator_not_allowed"));
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
        assert_eq!(relative.validate(), Ok(()));

        let own_file = own.path().join("mine.7z");
        let absolute = pack_prompt_in(other.path(), own_file.to_str().unwrap());
        assert_eq!(absolute.archive_path(), own_file);
        assert_eq!(absolute.validate(), Ok(()));
    }

    #[test]
    fn a_pack_name_needs_a_known_ending_a_free_name_and_a_folder() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("taken.zip"), b"x").unwrap();

        assert_eq!(
            pack_prompt_in(folder.path(), "new.tar.zst").validate(),
            Ok(())
        );
        assert_eq!(pack_prompt_in(folder.path(), "New.ZIP").validate(), Ok(()));
        assert_eq!(
            pack_prompt_in(folder.path(), "notes.txt").validate(),
            Err("archive_extension")
        );
        assert_eq!(
            pack_prompt_in(folder.path(), "noending").validate(),
            Err("archive_extension")
        );
        assert_eq!(
            pack_prompt_in(folder.path(), "taken.zip").validate(),
            Err("archive_exists")
        );
        assert_eq!(
            pack_prompt_in(folder.path(), "missing/b.zip").validate(),
            Err("archive_folder_missing")
        );
        assert_eq!(
            pack_prompt_in(folder.path(), "").validate(),
            Err("empty_name")
        );
        assert_eq!(
            pack_prompt_in(folder.path(), "..").validate(),
            Err("reserved_name")
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
