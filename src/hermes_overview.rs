//! What the Hermes overview says: from the state of the module, the counts,
//! the AI and the folders, one set of facts the page shows as a status pill,
//! four tiles and the numbers of the flow picture.
//!
//! The state only. Nothing here touches the disk or the module; the app
//! gathers the facts and the page draws them.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use ncrs_index::{DocumentFields, HermesCounts};
use ncrs_intake::{pattern_is_valid, render_pattern, DEFAULT_PATTERNS};

use crate::filing::{AiStatus, KeyProblem, Phase, ProtectionNote};

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
pub enum FolderTile {
    Ready,
    Missing,
}

impl FolderTile {
    pub fn tone(self) -> Tone {
        match self {
            Self::Ready => Tone::Good,
            Self::Missing => Tone::Needs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrTile {
    Ready,
    Missing,
}

impl OcrTile {
    pub fn tone(self) -> Tone {
        match self {
            Self::Ready => Tone::Good,
            Self::Missing => Tone::Needs,
        }
    }
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

impl AiTile {
    pub fn tone(&self) -> Tone {
        match self {
            Self::Off => Tone::Quiet,
            Self::NotLoaded | Self::Downloading { .. } | Self::Failed(_) => Tone::Needs,
            Self::Ready | Self::OwnServer { .. } | Self::TakesEffectAtRestart => Tone::Good,
        }
    }

    /// The model can fill in fields now.
    pub fn reads(&self) -> bool {
        matches!(self, Self::Ready | Self::OwnServer { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewTile {
    Waiting(u64),
    Clear,
}

impl ReviewTile {
    pub fn tone(self) -> Tone {
        match self {
            Self::Waiting(_) => Tone::Needs,
            Self::Clear => Tone::Good,
        }
    }
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

    pub fn folder_tile(&self) -> FolderTile {
        if self.has_locations {
            FolderTile::Ready
        } else {
            FolderTile::Missing
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

    pub fn review_tile(&self) -> ReviewTile {
        match self.open_reviews() {
            0 => ReviewTile::Clear,
            waiting => ReviewTile::Waiting(waiting),
        }
    }

    /// The reviews can be read again with the AI: it is ready and some wait.
    pub fn can_read_reviews_again(&self) -> bool {
        self.module == ModuleState::Ready && self.ai_tile().reads() && self.open_reviews() > 0
    }

}

/// The pattern the field starts with when the config has none: the first of
/// the built-in ones, which Hermes tries before the poorer ones.
pub fn built_in_pattern() -> &'static str {
    DEFAULT_PATTERNS[0]
}

/// Why a typed pattern was not written, or that the write failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternNote {
    Invalid,
    WriteFailed(String),
}

/// What Enter in the pattern field asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternCommit {
    Unchanged,
    /// The text is no pattern Hermes can render; nothing is written.
    Rejected,
    /// `None` drops the line, which brings back the built-in patterns.
    Write(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewState {
    pattern_draft: String,
    /// The pattern the config has now.
    configured_pattern: Option<String>,
    pattern_note: Option<PatternNote>,
    pattern_writing: bool,
    /// The pattern field has the focus, so the letters are its and not the page's.
    editing: bool,
    ocr_models_present: bool,
}

impl OverviewState {
    pub fn new(configured_pattern: Option<&str>, ocr_models_present: bool) -> Self {
        Self {
            pattern_draft: configured_pattern.unwrap_or(built_in_pattern()).to_owned(),
            configured_pattern: configured_pattern.map(str::to_owned),
            pattern_note: None,
            pattern_writing: false,
            editing: false,
            ocr_models_present,
        }
    }

    pub fn pattern_draft(&self) -> &str {
        &self.pattern_draft
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

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn set_editing(&mut self, editing: bool) {
        self.editing = editing;
    }

    pub fn edit_pattern(&mut self, text: String) {
        self.editing = true;
        self.pattern_draft = text;
        self.pattern_note = None;
    }

    /// What to do with the text in the field. Valid patterns and the empty
    /// text go to the config; the first built-in pattern means "no pattern of
    /// my own".
    pub fn commit_pattern(&mut self) -> PatternCommit {
        if self.pattern_writing {
            return PatternCommit::Unchanged;
        }
        let typed = self.pattern_draft.trim();
        let wanted = if typed.is_empty() || typed == built_in_pattern() {
            None
        } else if pattern_is_valid(typed) {
            Some(typed.to_owned())
        } else {
            self.pattern_note = Some(PatternNote::Invalid);
            return PatternCommit::Rejected;
        };
        if wanted == self.configured_pattern {
            self.pattern_draft = wanted.as_deref().unwrap_or(built_in_pattern()).to_owned();
            return PatternCommit::Unchanged;
        }
        self.pattern_writing = true;
        PatternCommit::Write(wanted)
    }

    /// The config was written: what it now says is the pattern.
    pub fn pattern_written(&mut self, configured: Option<&str>) {
        self.pattern_writing = false;
        self.configured_pattern = configured.map(str::to_owned);
        self.pattern_draft = configured.unwrap_or(built_in_pattern()).to_owned();
        self.pattern_note = None;
    }

    pub fn pattern_write_failed(&mut self, reason: String) {
        self.pattern_writing = false;
        self.pattern_note = Some(PatternNote::WriteFailed(reason));
    }

    /// The pattern the example is drawn with: the typed one while it works,
    /// else the one Hermes uses.
    fn example_pattern(&self) -> &str {
        let typed = self.pattern_draft.trim();
        if pattern_is_valid(typed) {
            typed
        } else {
            self.configured_pattern
                .as_deref()
                .unwrap_or(built_in_pattern())
        }
    }

    /// Where a made-up invoice of a made-up sender would be filed, under the
    /// filing location.
    pub fn example_target(&self, filing_location: &Path) -> Option<PathBuf> {
        let fields = DocumentFields {
            sender: Some("Frost".to_owned()),
            kind: Some("Rechnung".to_owned()),
            number: Some("3318".to_owned()),
            date: NaiveDate::from_ymd_opt(2021, 5, 18),
            ..DocumentFields::default()
        };
        let relative = render_pattern(self.example_pattern(), &fields)?;
        let mut name = filing_location.join(relative).into_os_string();
        name.push(".pdf");
        Some(PathBuf::from(name))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use ncrs_index::StatusCounts;

    use super::*;

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
    fn the_review_tile_counts_what_waits() {
        assert_eq!(facts(ModuleState::Ready).review_tile(), ReviewTile::Clear);
        let tile = with_open_reviews(facts(ModuleState::Ready), 5).review_tile();
        assert_eq!(tile, ReviewTile::Waiting(5));
        assert_eq!(tile.tone(), Tone::Needs);
        assert_eq!(ReviewTile::Clear.tone(), Tone::Good);
    }

    #[test]
    fn the_folder_tile_needs_folders() {
        let mut facts = facts(ModuleState::Ready);
        assert_eq!(facts.folder_tile().tone(), Tone::Good);
        facts.has_locations = false;
        assert_eq!(facts.folder_tile().tone(), Tone::Needs);
    }

    #[test]
    fn scans_need_models_unless_the_system_reads_them() {
        let mut facts = facts(ModuleState::Ready);
        assert_eq!(facts.ocr_tile(), OcrTile::Ready);
        facts.ocr = OcrFacts::of(false, true);
        assert_eq!(facts.ocr_tile(), OcrTile::Ready);
        facts.ocr = OcrFacts::of(false, false);
        assert_eq!(facts.ocr_tile(), OcrTile::Missing);
        assert_eq!(facts.ocr_tile().tone(), Tone::Needs);
    }

    #[test]
    fn the_ai_tile_follows_the_switch_the_download_and_the_model() {
        let mut facts = facts(ModuleState::Ready);
        assert_eq!(facts.ai_tile(), AiTile::NotLoaded);
        assert_eq!(facts.ai_tile().tone(), Tone::Needs);

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
        assert_eq!(facts.ai_tile().tone(), Tone::Good);

        facts.ai_status = AiStatus::Failed("the model file is damaged");
        assert!(matches!(facts.ai_tile(), AiTile::Failed(_)));

        facts.ai_status = AiStatus::Off;
        assert_eq!(facts.ai_tile(), AiTile::TakesEffectAtRestart);

        facts.ai_on = false;
        assert_eq!(facts.ai_tile(), AiTile::Off);
        assert_eq!(facts.ai_tile().tone(), Tone::Quiet);
    }

    #[test]
    fn reading_again_is_offered_only_with_a_ready_ai_and_open_reviews() {
        let ready = |reviews| {
            let mut facts = with_open_reviews(facts(ModuleState::Ready), reviews);
            facts.ai_status = AiStatus::NotLoadedYet;
            facts
        };
        assert!(ready(3).can_read_reviews_again());
        assert!(!ready(0).can_read_reviews_again());
        assert!(!with_open_reviews(facts(ModuleState::Ready), 3).can_read_reviews_again());
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
        assert_eq!(ModuleState::of(&Phase::PlainFileWarning), ModuleState::WaitsForYou);
    }

    #[test]
    fn the_pattern_field_starts_with_the_first_built_in_pattern() {
        let state = OverviewState::new(None, true);
        assert_eq!(state.pattern_draft(), built_in_pattern());
        let own = OverviewState::new(Some("{year}/{sender}"), true);
        assert_eq!(own.pattern_draft(), "{year}/{sender}");
    }

    #[test]
    fn a_valid_pattern_is_written_once() {
        let mut state = OverviewState::new(None, true);
        state.edit_pattern("  {year}/{sender}  ".to_owned());
        assert_eq!(
            state.commit_pattern(),
            PatternCommit::Write(Some("{year}/{sender}".to_owned()))
        );
        assert_eq!(state.commit_pattern(), PatternCommit::Unchanged);
        state.pattern_written(Some("{year}/{sender}"));
        assert_eq!(state.commit_pattern(), PatternCommit::Unchanged);
        assert_eq!(state.pattern_draft(), "{year}/{sender}");
    }

    #[test]
    fn an_invalid_pattern_is_refused_and_nothing_is_written() {
        let mut state = OverviewState::new(None, true);
        state.edit_pattern("{nothing}/x".to_owned());
        assert_eq!(state.commit_pattern(), PatternCommit::Rejected);
        assert_eq!(state.pattern_note(), Some(&PatternNote::Invalid));
        state.edit_pattern("{year}".to_owned());
        assert_eq!(state.pattern_note(), None);
    }

    #[test]
    fn an_empty_field_or_the_first_built_in_pattern_drops_the_own_pattern() {
        let mut state = OverviewState::new(Some("{year}/{sender}"), true);
        state.edit_pattern("   ".to_owned());
        assert_eq!(state.commit_pattern(), PatternCommit::Write(None));
        state.pattern_written(None);
        assert_eq!(state.pattern_draft(), built_in_pattern());

        let mut own = OverviewState::new(Some("{year}/{sender}"), true);
        own.edit_pattern(built_in_pattern().to_owned());
        assert_eq!(own.commit_pattern(), PatternCommit::Write(None));

        let mut none = OverviewState::new(None, true);
        none.edit_pattern(String::new());
        assert_eq!(none.commit_pattern(), PatternCommit::Unchanged);
        assert_eq!(none.pattern_draft(), built_in_pattern());
    }

    #[test]
    fn a_failed_write_says_why_and_lets_the_user_try_again() {
        let mut state = OverviewState::new(None, true);
        state.edit_pattern("{year}/{sender}".to_owned());
        assert!(matches!(state.commit_pattern(), PatternCommit::Write(_)));
        state.pattern_write_failed("disk full".to_owned());
        assert_eq!(
            state.pattern_note(),
            Some(&PatternNote::WriteFailed("disk full".to_owned()))
        );
        assert!(matches!(state.commit_pattern(), PatternCommit::Write(_)));
    }

    #[test]
    fn the_example_shows_where_a_made_up_invoice_would_go() {
        let state = OverviewState::new(None, true);
        let location = PathBuf::from("/home/user/tmp");
        assert_eq!(
            state.example_target(&location),
            Some(location.join("2021").join("20210518_Frost_3318.pdf"))
        );
        let mut typed = OverviewState::new(None, true);
        typed.edit_pattern("{kind}/{sender}".to_owned());
        assert_eq!(
            typed.example_target(&location),
            Some(location.join("Rechnung").join("Frost.pdf"))
        );
        typed.edit_pattern("{broken".to_owned());
        assert_eq!(
            typed.example_target(&location),
            Some(location.join("2021").join("20210518_Frost_3318.pdf")),
            "a pattern that does not work leaves the example as it was"
        );
    }
}
