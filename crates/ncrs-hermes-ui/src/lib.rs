//! The Hermes pages: the filing setup and status, the overview, the review
//! list, the rules, the rule suggestions, the reorganisation plan, the saved
//! queries, the tags and the quick-fix menu. Their state types, messages and
//! views, and the update logic of the pages that have moved.

pub mod action;
pub mod condition_editor;
pub mod ctx;
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
pub mod state;
pub mod tags_page;
pub mod ui;
pub mod update;

pub use action::{Action, ProgressJobBuilder, SetupCard, StartProgressJob};
pub use ctx::Ctx;
pub use message::{
    FilingSetupMessage, FilingStatusMessage, HermesColumnMessage, HermesFileMessage, HermesFocus,
    HermesOverviewMessage, HermesPage, HermesSetupMessage, Message, QuickFixMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleFormMessage, RuleSuggestionsMessage,
    RulesMessage, SavedQueriesMessage, TagsMessage,
};
pub use state::Hermes;
pub use update::{
    documents_filed, documents_refiled, exclude_from_hermes, open_reorganisation, open_review_list,
    open_rule_suggestions, open_rules, open_saved_queries, open_tags, reorganisation_executed,
    reorganisation_undone, reprocess_folder, request_review_preview, run_filing, run_filing_work,
    run_filing_work_after, run_refile, run_reorganisation, undo_reorganisation,
    update_filing_setup, update_hermes_setup, update_quick_fix, update_reorganisation,
    update_review_list, update_rule_suggestions, update_rules, update_saved_queries, update_tags,
    Outcome,
};
