//! The Hermes pages: the filing setup and status, the overview, the review
//! list, the rules, the rule suggestions, the reorganisation plan, the saved
//! queries, the tags and the quick-fix menu. Their state types, messages and
//! views; the update logic is still in the app.

pub mod condition_editor;
pub mod document_preview;
pub mod filing_pieces;
pub mod filing_setup;
pub mod filing_status;
pub mod hermes_column;
pub mod hermes_overview;
pub mod hermes_setup;
pub mod hermes_status;
pub mod message;
pub mod quick_fix;
pub mod reading_again;
pub mod reorganisation_plan;
pub mod review_list;
pub mod rule_form;
pub mod rule_suggestions;
pub mod rules_page;
pub mod saved_queries;
pub mod tags_page;
pub mod ui;

pub use message::{
    FilingSetupMessage, FilingStatusMessage, HermesColumnMessage, HermesFileMessage, HermesFocus,
    HermesOverviewMessage, HermesPage, HermesSetupMessage, Message, QuickFixMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleFormMessage, RuleSuggestionsMessage,
    RulesMessage, SavedQueriesMessage, TagsMessage,
};
