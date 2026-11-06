//! The Hermes pages: the filing setup and status, the overview, the review
//! list, the rules, the rule suggestions, the reorganisation plan, the saved
//! queries, the tags and the quick-fix menu. Their state types, messages and
//! views, and the update logic of the pages that have moved.

pub mod action;
pub mod condition_editor;
pub mod ctx;
pub mod document_preview;
pub use ncrs_intake::filing_pieces;
pub mod filing_setup;
pub mod filing_status;
pub mod hermes_column;
pub mod hermes_overview;
pub mod hermes_status;
pub mod keys;
pub mod message;
pub mod quick_fix;
pub mod reading_again;
pub mod rule_form;
pub mod rules_page;
pub mod saved_queries;
pub mod state;
pub mod tags_page;
pub mod ui;
pub mod update;
pub mod view;

pub use action::{Action, ProgressJobBuilder, ProtectionStep, StartProgressJob};
pub use ctx::Ctx;
pub use message::{
    FilingStatusMessage, HermesColumnMessage, HermesFileMessage, HermesOverviewMessage, HermesPage,
    Message, QuickFixMessage, RuleFormMessage, RulesMessage, SavedQueriesMessage, TagsMessage,
};
pub use state::{Hermes, KeyStoreHandle};
pub use update::{
    apply_ai_change, apply_saved_filing, change_protection, close_dialog, dialog_is_open,
    documents_filed, exclude_from_hermes, filing_restarted, hermes_included, hermes_role_saved,
    leave_pages, open_filing_status_page, process_again, read_paths_again, refresh_hermes_column,
    request_hermes_column, request_hermes_counts, restart_hermes_module, role_save_failed,
    run_filing, run_filing_work, run_filing_work_after, set_folder_role, show_in_inspection,
    show_page, start_filing_module, toggle_hermes_exclusion, update_filing, update_filing_status,
    update_hermes_column, update_hermes_overview, update_quick_fix, update_rules,
    update_saved_queries, update_tags, Outcome, SavedFilingApplies,
};
pub use view::{
    dialogs, filing_badge, hermes_chip, hermes_line, hermes_status, overview_facts, page_content,
    FilingBadge, ViewCtx,
};
