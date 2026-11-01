use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_config::config::Filing;
use ncrs_filing::filing::{FilingModule, Phase};
use ncrs_filing::replan_work::ReplanItem;
use ncrs_index::KeyStore;

use crate::filing_setup::FilingSetup;
use crate::filing_status::FilingStatus;
use crate::hermes_column::{ColumnStage, HermesColumn};
use crate::hermes_overview::OverviewState;
use crate::hermes_setup::{ExistingIndex, HermesSetup, SetupFocus, SetupOffer, StartPlan};
use crate::message::HermesPage;
use crate::quick_fix::QuickFix;
use crate::reading_again::ReadingAgain;
use crate::reorganisation_plan::ReorganisationView;
use crate::review_list::ReviewList;
use crate::rule_suggestions::RuleSuggestions;
use crate::rules_page::RulesPage;
use crate::saved_queries::SavedQueries;
use crate::tags_page::TagsPage;

/// What only the Hermes pages use: the pages that are up. Each page has a
/// generation, so the answer of work that was started for a page that was
/// closed and opened again is not taken for the answer of the new one.
#[derive(Debug, Default)]
pub struct Hermes {
    /// The overview of Hermes.
    pub overview: Option<OverviewState>,
    /// The overview comes back when the card that covers it is gone.
    pub return_to_overview: bool,
    /// The card that lists the folders Hermes works on.
    pub filing_setup: Option<FilingSetup>,
    /// The card that sets Hermes up the first time.
    pub hermes_setup: Option<HermesSetup>,
    pub review_list: Option<ReviewList>,
    pub review_list_generation: u64,
    /// The fixes for an inquiry; at most one of it and the context menu.
    pub quick_fix: Option<QuickFix>,
    pub quick_fix_generation: u64,
    pub reorganisation: Option<ReorganisationView>,
    pub reorganisation_generation: u64,
    pub rule_suggestions: Option<RuleSuggestions>,
    pub rule_suggestions_generation: u64,
    pub rules: Option<RulesPage>,
    pub rules_generation: u64,
    pub saved_queries: Option<SavedQueries>,
    pub saved_queries_generation: u64,
    pub tags: Option<TagsPage>,
    pub tags_generation: u64,
    /// The planned documents wait for a free queue; the next tick asks again.
    pub filing_due: bool,
    /// The folder and the documents of a confirmed replan, waiting for a free
    /// queue.
    pub pending_refile: Option<(PathBuf, Vec<ReplanItem>)>,
    /// What the filing status page and the status bar show of the pipeline.
    pub filing_status: FilingStatus,
    /// What the index says about the files and folders the panels show.
    pub hermes_column: HermesColumn,
    /// Reads of the column that are under way or gave up, until something
    /// is invalidated.
    pub hermes_reads: HashSet<(PathBuf, ColumnStage)>,
    /// Files whose status bar detail is being read.
    pub hermes_detail_reads: HashSet<PathBuf>,
    /// How far the spinner of a file being read has turned, in radians.
    pub hermes_spinner_angle: f32,
    /// The folder the PDF library is missing from, as of the last pipeline start.
    pub pdf_library_missing: Option<PathBuf>,
    /// The card opened by itself for a missing model; once per start, so a
    /// "Cancel" is not asked again.
    pub ai_model_asked: bool,
    /// Files the user asked to read again and whose end is not yet reported.
    pub reading_again: ReadingAgain,
    /// The filing settings of the config file, as last loaded or written.
    pub filing_config: Filing,
    /// The filing module, when the config switches it on.
    pub filing: Option<FilingModule>,
    /// Started once the filing module is ready; dropping it stops the workers.
    pub pipeline: Option<ncrs_pipeline::Runner>,
    /// What "Hermes starten" still has to do once the module gets there.
    pub pending_hermes_start: Option<StartPlan>,
    /// `Snapshot::plans_made` the filing job was last asked for.
    pub plans_filed_for: usize,
    /// The module is being rebuilt after an AI setting changed: its
    /// generation and the filing config it starts with.
    pub filing_restart: Option<(u64, Filing)>,
    pub filing_restart_generation: u64,
    /// Where the index lives; `None` without a data directory.
    pub index_path: Option<PathBuf>,
    /// Opens the key of the index; the module cannot start without it.
    pub key_store: Option<KeyStoreHandle>,
}

/// The key store of the index, shared with the module that opens it.
#[derive(Clone)]
pub struct KeyStoreHandle(pub Arc<dyn KeyStore + Send + Sync>);

impl std::fmt::Debug for KeyStoreHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyStoreHandle")
    }
}

impl Hermes {
    pub fn is_ready(&self) -> bool {
        self.filing
            .as_ref()
            .is_some_and(|module| matches!(module.phase(), Phase::Ready))
    }

    /// The module is ready and has places to look at.
    pub fn is_active(&self) -> bool {
        self.is_ready()
            && self
                .filing
                .as_ref()
                .is_some_and(FilingModule::has_configured_locations)
    }

    /// The module is rebuilt after a setting changed; it is not off then.
    pub fn is_restarting(&self) -> bool {
        self.filing_restart.is_some()
    }

    pub fn is_off(&self) -> bool {
        self.filing.is_none() && !self.is_restarting()
    }

    /// The column asks again for everything that depends on `path`.
    pub fn invalidate_hermes_path(&mut self, path: &Path) {
        self.hermes_column.invalidate_path(path);
        self.hermes_reads.clear();
        self.hermes_detail_reads.remove(path);
    }

    /// A page of this state stands in for the panels.
    pub fn page_is_open(&self) -> bool {
        self.open_page().is_some()
    }

    /// The page of this state that is up, if any.
    pub fn open_page(&self) -> Option<HermesPage> {
        if self.overview.is_some() {
            Some(HermesPage::Overview)
        } else if self.review_list.is_some() {
            Some(HermesPage::Review)
        } else if self.saved_queries.is_some() {
            Some(HermesPage::SavedQueries)
        } else if self.rules.is_some() {
            Some(HermesPage::Rules)
        } else if self.rule_suggestions.is_some() {
            Some(HermesPage::RuleSuggestions)
        } else if self.tags.is_some() {
            Some(HermesPage::Tags)
        } else if self.reorganisation.is_some() {
            Some(HermesPage::Reorganisation)
        } else {
            None
        }
    }

    pub fn close_pages(&mut self) {
        self.overview = None;
        self.review_list = None;
        self.reorganisation = None;
        self.rule_suggestions = None;
        self.rules = None;
        self.saved_queries = None;
        self.tags = None;
    }

    /// The card that sets Hermes up cannot be changed while the folders are
    /// written or the module starts and creates the index.
    pub fn setup_is_busy(&self) -> bool {
        self.pending_hermes_start.is_some()
            || self.hermes_setup.as_ref().is_some_and(HermesSetup::is_busy)
            || self
                .filing
                .as_ref()
                .is_some_and(FilingModule::is_creating_index)
    }

    /// The elements of the setup card the keys go through, in order.
    pub fn setup_focus_order(&self) -> Vec<SetupFocus> {
        self.hermes_setup
            .as_ref()
            .map(|setup| setup.focus_order(setup.can_start() && !self.setup_is_busy()))
            .unwrap_or_default()
    }

    /// What the setup card is told about the index and the model on disk.
    pub fn setup_offer(&self, model_is_installed: fn(&Path) -> bool) -> SetupOffer {
        let models_dir = self
            .index_path
            .as_deref()
            .and_then(ncrs_filing::filing::ai_models_dir);
        SetupOffer {
            existing_index: self.index_path.as_deref().and_then(ExistingIndex::of_file),
            model_download: models_dir.is_some_and(|dir| !model_is_installed(&dir)),
            hermes_was_off: self.is_off(),
        }
    }
}
