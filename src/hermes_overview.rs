//! What the Hermes overview says: from the state of the module, the counts,
//! the AI and the folders, one set of facts the page shows as a status pill,
//! four tiles and the numbers of the flow picture.
//!
//! The state only. Nothing here touches the disk or the module; the app
//! gathers the facts and the page draws them.

use ncrs_index::HermesCounts;
use ncrs_intake::DEFAULT_PATTERNS;

use crate::filing::{AiStatus, KeyProblem, Phase, ProtectionNote};
use crate::filing_pieces::{TargetEdit, TargetEditor};

/// Where the module stands, as far as the overview tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    /// Switched off in the config, or not built.
    Off,
    /// Rebuilt after a setting changed, or still opening the index.
    Starting,
    Ready,
    /// One of its own cards waits for an answer: encryption, the recovery key
    /// shown, the unencrypted file.
    WaitsForYou,
    KeyMissing,
    Failed,
    Dismissed,
    /// The index is being encrypted.
    Encrypting,
    /// The index and its key are being removed.
    StartingOver,
}

impl ModuleState {
    pub fn of(phase: &Phase) -> Self {
        match phase {
            Phase::Starting => Self::Starting,
            Phase::Ready => Self::Ready,
            Phase::AskEncryption { .. }
            | Phase::ShowRecoveryKey { .. }
            | Phase::PlainFileWarning => Self::WaitsForYou,
            Phase::AskRecoveryKey { .. } => Self::KeyMissing,
            Phase::Failed(_) => Self::Failed,
            Phase::Dismissed => Self::Dismissed,
            Phase::Encrypting => Self::Encrypting,
            Phase::StartingOver => Self::StartingOver,
        }
    }
}

/// How the text recognition stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrFacts {
    /// The system reads scans itself.
    BuiltIn,
    ModelsPresent,
    ModelsMissing,
}

impl OcrFacts {
    pub fn of(built_in: bool, models_present: bool) -> Self {
        if built_in {
            Self::BuiltIn
        } else if models_present {
            Self::ModelsPresent
        } else {
            Self::ModelsMissing
        }
    }
}

/// How the index is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    Encrypted,
    Plain,
    /// The file is being encrypted now.
    Encrypting,
    /// The file is being removed now.
    StartingOver,
    /// There is no open index to tell by.
    Unknown,
}

impl Protection {
    /// `index_encrypted` is known once an index is open; a missing key means
    /// there is an encrypted one.
    pub fn of(module: ModuleState, index_encrypted: Option<bool>) -> Self {
        match (module, index_encrypted) {
            (ModuleState::Encrypting, _) => Self::Encrypting,
            (ModuleState::StartingOver, _) => Self::StartingOver,
            (ModuleState::KeyMissing, _) | (_, Some(true)) => Self::Encrypted,
            (_, Some(false)) => Self::Plain,
            (_, None) => Self::Unknown,
        }
    }
}

/// The status pill beside the headline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    Running,
    NeedsYou,
    Off,
    Starting,
}

/// The color of a tile: green when fine, orange when it needs something,
/// quiet when the user chose to leave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Needs,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrTile {
    Ready,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiTile {
    Off,
    /// On, but the model file is not on this machine.
    NotLoaded,
    Downloading {
        received: u64,
        total: u64,
    },
    Ready,
    OwnServer {
        model: String,
    },
    /// On in the config; the module takes it up when it restarts.
    TakesEffectAtRestart,
    Failed(String),
}

/// Everything the page needs to know, gathered by the app.
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewFacts {
    pub module: ModuleState,
    /// Why the module failed, while it did.
    pub failure: Option<String>,
    pub has_locations: bool,
    pub counts: Option<HermesCounts>,
    pub ai_on: bool,
    pub ai_status: AiStatus,
    /// Bytes on disk and in all, while the model downloads.
    pub ai_download: Option<(u64, u64)>,
    pub ocr: OcrFacts,
    pub protection: Protection,
    /// The key of the index is missing or wrong, and the user has not solved it.
    pub key_problem: Option<KeyProblem>,
    pub can_encrypt: bool,
    pub can_start_over: bool,
    /// Why the last change of the protection did not go through.
    pub protection_note: Option<ProtectionNote>,
}

impl OverviewFacts {
    pub fn open_reviews(&self) -> u64 {
        self.counts.map_or(0, |counts| counts.open_reviews)
    }

    pub fn pill(&self) -> Pill {
        match self.module {
            ModuleState::Off => Pill::Off,
            ModuleState::Starting | ModuleState::Encrypting | ModuleState::StartingOver => {
                Pill::Starting
            }
            ModuleState::WaitsForYou
            | ModuleState::KeyMissing
            | ModuleState::Failed
            | ModuleState::Dismissed => Pill::NeedsYou,
            ModuleState::Ready if !self.has_locations || self.open_reviews() > 0 => Pill::NeedsYou,
            ModuleState::Ready => Pill::Running,
        }
    }

    pub fn ocr_tile(&self) -> OcrTile {
        match self.ocr {
            OcrFacts::BuiltIn | OcrFacts::ModelsPresent => OcrTile::Ready,
            OcrFacts::ModelsMissing => OcrTile::Missing,
        }
    }

    pub fn ai_tile(&self) -> AiTile {
        if !self.ai_on || self.module == ModuleState::Off {
            return AiTile::Off;
        }
        if let Some((received, total)) = self.ai_download {
            return AiTile::Downloading { received, total };
        }
        match &self.ai_status {
            AiStatus::Off => AiTile::TakesEffectAtRestart,
            AiStatus::NotDownloaded => AiTile::NotLoaded,
            AiStatus::NotLoadedYet | AiStatus::Loaded { .. } => AiTile::Ready,
            AiStatus::Http { model, .. } => AiTile::OwnServer {
                model: model.clone(),
            },
            AiStatus::Failed(reason) => AiTile::Failed((*reason).to_owned()),
        }
    }
}

/// The pattern the default target starts with when the config has none: the
/// first of the built-in ones, which Hermes tries before the poorer ones.
pub fn built_in_pattern() -> &'static str {
    DEFAULT_PATTERNS[0]
}

/// That the write of the default target failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternNote {
    WriteFailed(String),
}

/// What an edit of the default target asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternCommit {
    Unchanged,
    /// `None` drops the line, which brings back the built-in patterns.
    Write(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewState {
    target: TargetEditor,
    /// The pattern the config has now.
    configured_pattern: Option<String>,
    pattern_note: Option<PatternNote>,
    pattern_writing: bool,
    ocr_models_present: bool,
}

impl OverviewState {
    pub fn new(configured_pattern: Option<&str>, ocr_models_present: bool) -> Self {
        Self {
            target: TargetEditor::new(configured_pattern.unwrap_or(built_in_pattern())),
            configured_pattern: configured_pattern.map(str::to_owned),
            pattern_note: None,
            pattern_writing: false,
            ocr_models_present,
        }
    }

    pub fn target(&self) -> &TargetEditor {
        &self.target
    }

    pub fn pattern_note(&self) -> Option<&PatternNote> {
        self.pattern_note.as_ref()
    }

    pub fn ocr_models_present(&self) -> bool {
        self.ocr_models_present
    }

    pub fn set_ocr_models_present(&mut self, present: bool) {
        self.ocr_models_present = present;
    }

    /// The field for the name of a folder has the focus, so the letters are
    /// its and not the page's.
    pub fn is_editing(&self) -> bool {
        self.target.own_name.is_some()
    }

    pub fn edit_target(&mut self, edit: TargetEdit) {
        self.target.apply(edit);
        self.pattern_note = None;
    }

    /// What to do with the target as it stands. A target that cannot file
    /// anything is not written; the first built-in pattern means "no pattern
    /// of my own".
    pub fn commit_pattern(&mut self) -> PatternCommit {
        if self.pattern_writing || !self.target.is_usable() {
            return PatternCommit::Unchanged;
        }
        let pattern = self.target.pattern();
        let wanted = (pattern != built_in_pattern()).then_some(pattern);
        if wanted == self.configured_pattern {
            return PatternCommit::Unchanged;
        }
        self.pattern_writing = true;
        PatternCommit::Write(wanted)
    }

    /// The config was written: what it now says is the pattern.
    pub fn pattern_written(&mut self, configured: Option<&str>) {
        self.pattern_writing = false;
        self.configured_pattern = configured.map(str::to_owned);
        self.pattern_note = None;
    }

    pub fn pattern_write_failed(&mut self, reason: String) {
        self.pattern_writing = false;
        self.pattern_note = Some(PatternNote::WriteFailed(reason));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use ncrs_index::StatusCounts;

    use super::*;
    use crate::filing_pieces::FolderField;

    fn facts(module: ModuleState) -> OverviewFacts {
        OverviewFacts {
            module,
            failure: None,
            has_locations: true,
            counts: Some(HermesCounts::default()),
            ai_on: true,
            ai_status: AiStatus::NotDownloaded,
            ai_download: None,
            ocr: OcrFacts::BuiltIn,
            protection: Protection::Encrypted,
            key_problem: None,
            can_encrypt: false,
            can_start_over: false,
            protection_note: None,
        }
    }

    fn with_open_reviews(mut facts: OverviewFacts, open: u64) -> OverviewFacts {
        facts.counts = Some(HermesCounts {
            open_reviews: open,
            inbox: StatusCounts::default(),
            ..HermesCounts::default()
        });
        facts
    }

    #[test]
    fn the_pill_is_green_only_for_a_module_that_works_and_has_nothing_open() {
        assert_eq!(facts(ModuleState::Ready).pill(), Pill::Running);
        assert_eq!(
            with_open_reviews(facts(ModuleState::Ready), 5).pill(),
            Pill::NeedsYou
        );
        let mut without_folders = facts(ModuleState::Ready);
        without_folders.has_locations = false;
        assert_eq!(without_folders.pill(), Pill::NeedsYou);
    }

    #[test]
    fn the_pill_asks_for_the_user_when_the_module_cannot_go_on_by_itself() {
        for state in [
            ModuleState::KeyMissing,
            ModuleState::Failed,
            ModuleState::Dismissed,
            ModuleState::WaitsForYou,
        ] {
            assert_eq!(facts(state).pill(), Pill::NeedsYou, "{state:?}");
        }
    }

    #[test]
    fn the_pill_says_off_and_starting_for_a_module_that_is_not_there() {
        assert_eq!(facts(ModuleState::Off).pill(), Pill::Off);
        assert_eq!(facts(ModuleState::Starting).pill(), Pill::Starting);
        assert_eq!(facts(ModuleState::Encrypting).pill(), Pill::Starting);
        assert_eq!(facts(ModuleState::StartingOver).pill(), Pill::Starting);
    }

    #[test]
    fn scans_need_models_unless_the_system_reads_them() {
        let mut facts = facts(ModuleState::Ready);
        assert_eq!(facts.ocr_tile(), OcrTile::Ready);
        facts.ocr = OcrFacts::of(false, true);
        assert_eq!(facts.ocr_tile(), OcrTile::Ready);
        facts.ocr = OcrFacts::of(false, false);
        assert_eq!(facts.ocr_tile(), OcrTile::Missing);
    }

    #[test]
    fn the_ai_tile_follows_the_switch_the_download_and_the_model() {
        let mut facts = facts(ModuleState::Ready);
        assert_eq!(facts.ai_tile(), AiTile::NotLoaded);

        facts.ai_download = Some((312_000_000, 639_000_000));
        assert_eq!(
            facts.ai_tile(),
            AiTile::Downloading {
                received: 312_000_000,
                total: 639_000_000,
            }
        );

        facts.ai_download = None;
        facts.ai_status = AiStatus::NotLoadedYet;
        assert_eq!(facts.ai_tile(), AiTile::Ready);

        facts.ai_status = AiStatus::Failed("the model file is damaged");
        assert!(matches!(facts.ai_tile(), AiTile::Failed(_)));

        facts.ai_status = AiStatus::Off;
        assert_eq!(facts.ai_tile(), AiTile::TakesEffectAtRestart);

        facts.ai_on = false;
        assert_eq!(facts.ai_tile(), AiTile::Off);
    }

    #[test]
    fn a_missing_key_means_an_encrypted_index() {
        assert_eq!(
            Protection::of(ModuleState::KeyMissing, None),
            Protection::Encrypted
        );
        assert_eq!(
            Protection::of(ModuleState::Ready, Some(false)),
            Protection::Plain
        );
        assert_eq!(Protection::of(ModuleState::Off, None), Protection::Unknown);
        assert_eq!(
            Protection::of(ModuleState::Encrypting, None),
            Protection::Encrypting
        );
        assert_eq!(
            Protection::of(ModuleState::StartingOver, Some(true)),
            Protection::StartingOver
        );
        assert_eq!(ModuleState::of(&Phase::Encrypting), ModuleState::Encrypting);
    }

    #[test]
    fn the_phases_of_the_module_map_to_what_the_page_calls_them() {
        assert_eq!(ModuleState::of(&Phase::Ready), ModuleState::Ready);
        assert_eq!(ModuleState::of(&Phase::Dismissed), ModuleState::Dismissed);
        assert_eq!(
            ModuleState::of(&Phase::Failed("no".to_owned())),
            ModuleState::Failed
        );
        assert_eq!(
            ModuleState::of(&Phase::PlainFileWarning),
            ModuleState::WaitsForYou
        );
    }

    #[test]
    fn the_default_target_starts_with_the_first_built_in_pattern() {
        let state = OverviewState::new(None, true);
        assert_eq!(state.target().pattern(), built_in_pattern());
        let own = OverviewState::new(Some("{year}/{sender}/"), true);
        assert_eq!(own.target().pattern(), "{year}/{sender}/");
    }

    #[test]
    fn a_changed_target_is_written_once() {
        let mut state = OverviewState::new(None, true);
        state.edit_target(TargetEdit::Remove(0));
        assert_eq!(
            state.commit_pattern(),
            PatternCommit::Write(Some("{year}/{sender}/".to_owned()))
        );
        assert_eq!(state.commit_pattern(), PatternCommit::Unchanged);
        state.pattern_written(Some("{year}/{sender}/"));
        assert_eq!(state.commit_pattern(), PatternCommit::Unchanged);
        assert_eq!(state.target().pattern(), "{year}/{sender}/");
    }

    #[test]
    fn a_target_without_a_folder_is_not_written() {
        let mut state = OverviewState::new(Some("{year}/"), true);
        state.edit_target(TargetEdit::Remove(0));

        assert_eq!(state.commit_pattern(), PatternCommit::Unchanged);
        assert!(!state.target().is_usable());
    }

    #[test]
    fn the_first_built_in_pattern_drops_the_own_pattern() {
        let mut state = OverviewState::new(Some("{year}/{sender}/"), true);
        state.edit_target(TargetEdit::Remove(0));
        state.edit_target(TargetEdit::Remove(0));
        state.edit_target(TargetEdit::AddField(FolderField::Recipient));
        state.edit_target(TargetEdit::AddField(FolderField::Year));
        state.edit_target(TargetEdit::AddField(FolderField::Sender));

        assert_eq!(state.commit_pattern(), PatternCommit::Write(None));
        state.pattern_written(None);
        assert_eq!(state.target().pattern(), built_in_pattern());
    }

    #[test]
    fn a_failed_write_says_why_and_lets_the_user_try_again() {
        let mut state = OverviewState::new(None, true);
        state.edit_target(TargetEdit::Remove(0));
        assert!(matches!(state.commit_pattern(), PatternCommit::Write(_)));
        state.pattern_write_failed("disk full".to_owned());
        assert_eq!(
            state.pattern_note(),
            Some(&PatternNote::WriteFailed("disk full".to_owned()))
        );
        assert!(matches!(state.commit_pattern(), PatternCommit::Write(_)));
    }

    #[test]
    fn the_name_field_for_a_folder_has_the_keys_of_the_page_while_it_is_open() {
        let mut state = OverviewState::new(None, true);
        assert!(!state.is_editing());
        state.edit_target(TargetEdit::StartOwnName);
        assert!(state.is_editing());
        state.edit_target(TargetEdit::OwnNameTyped("Privat".to_owned()));
        state.edit_target(TargetEdit::OwnNameSubmit);
        assert!(!state.is_editing());
        assert_eq!(
            state.target().pattern(),
            "{recipient}/{year}/{sender}/Privat/"
        );
    }
}
