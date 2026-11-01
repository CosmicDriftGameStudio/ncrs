use crate::message::HermesPage;
use crate::quick_fix::QuickFix;
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
}

impl Hermes {
    /// A page of this state stands in for the panels.
    pub fn page_is_open(&self) -> bool {
        self.open_page().is_some()
    }

    /// The page of this state that is up, if any.
    pub fn open_page(&self) -> Option<HermesPage> {
        if self.review_list.is_some() {
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
        self.review_list = None;
        self.reorganisation = None;
        self.rule_suggestions = None;
        self.rules = None;
        self.saved_queries = None;
        self.tags = None;
    }
}
