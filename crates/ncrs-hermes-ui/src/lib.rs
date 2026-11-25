//! The Hermes pages: the sources, targets and rules of sorting, the filing
//! status, the overview, the saved queries, the tags and the activity. Their
//! state types, messages and views, and their update logic.

pub mod action;
pub mod condition_editor;
pub mod ctx;
pub mod cursor_roles;
pub mod documents_facts;
pub mod activity_page;
pub mod filing_setup;
pub mod filing_status;
pub mod hermes_column;
pub mod hermes_overview;
pub mod hermes_status;
pub mod keys;
pub mod message;
pub mod reading_again;
pub mod rule_form;
pub mod rules_page;
pub mod saved_queries;
pub mod sorting_page;
pub mod source_dialog;
pub mod state;
pub mod suggest;
pub mod tags_page;
pub mod target_dialog;
pub mod ui;
pub mod update;
pub mod view;
pub mod watching;

pub use action::{Action, ProgressJobBuilder, ProtectionStep, StartProgressJob};
pub use ctx::{Bin, Ctx};
pub use cursor_roles::CursorRoles;
pub use message::{
    ActivityMessage, FilingStatusMessage, HermesColumnMessage, HermesFileMessage,
    HermesOverviewMessage, HermesPage, Message, RuleFormMessage,
    RulesMessage, SavedQueriesMessage, SortingMessage, SuggestMessage, TagsMessage,
    TargetsMessage,
};
pub use source_dialog::SourceDialogKind;
pub use state::{Hermes, KeyStoreHandle};
pub use update::{
    apply_ai_change, apply_saved_filing, change_protection, close_dialog, closing,
    continuous_filed, dialog_is_open, exclude_from_hermes, filing_restarted, leave_pages,
    open_filing_status_page, read_paths_again, refresh_hermes_column, request_hermes_column,
    request_hermes_counts, restart_hermes_module, role_save_failed, run_filing_work,
    run_filing_work_after, show_page, start_filing_module,
    update, update_activity, update_filing, update_filing_status, update_hermes_column,
    open_rule_form, open_source_dialog, open_suggest_rule, open_suggest_scheme, open_target_dialog, update_hermes_overview, update_rules, update_saved_queries,
    update_sorting, update_tags, Outcome, SavedFilingApplies,
};
pub use watching::{SourceRun, Watching};
pub use view::{
    confirm_again_dialog, dialogs, filing_badge, hermes_line, hermes_status, overview_facts,
    page_content, page_keys, FilingBadge, ViewCtx,
};
