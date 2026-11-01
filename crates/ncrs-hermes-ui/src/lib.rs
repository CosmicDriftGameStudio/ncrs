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
pub mod keys;
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
pub mod view;

pub use action::{Action, ProgressJobBuilder, ProtectionStep, SetupCard, StartProgressJob};
pub use ctx::Ctx;
pub use message::{
    FilingSetupMessage, FilingStatusMessage, HermesColumnMessage, HermesFileMessage, HermesFocus,
    HermesOverviewMessage, HermesPage, HermesSetupMessage, Message, QuickFixMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleFormMessage, RuleSuggestionsMessage,
    RulesMessage, SavedQueriesMessage, TagsMessage,
};
pub use state::{Hermes, KeyStoreHandle};
pub use update::{
    activate_hermes_focus, advance_hermes_plan, apply_ai_change, apply_saved_filing,
    change_protection, documents_filed, documents_refiled, exclude_from_hermes, filing_restarted,
    go_to_hermes_page, hermes_included, hermes_role_saved, leave_overview, open_filing_status_page,
    open_hermes_setup, open_overview, open_saved_queries, open_tags, process_again,
    read_paths_again, refresh_hermes_column, reorganisation_executed, reorganisation_undone,
    reprocess_folder, request_hermes_column, request_hermes_counts, request_index_figures,
    request_review_preview, restart_hermes_module, role_save_failed, run_filing, run_filing_work,
    run_filing_work_after, run_refile, run_reorganisation, set_folder_role, show_in_inspection,
    start_filing_module, toggle_hermes_exclusion, undo_reorganisation, update_filing,
    update_filing_setup, update_filing_status, update_hermes_column, update_hermes_overview,
    update_hermes_setup, update_quick_fix, update_reorganisation, update_review_list,
    update_rule_suggestions, update_rules, update_saved_queries, update_tags, Outcome,
    SavedFilingApplies,
};
pub use view::{
    ablage_area, filing_badge, hermes_badges, hermes_chip, hermes_line, hermes_status,
    overview_facts, FilingBadge, ViewCtx,
};
