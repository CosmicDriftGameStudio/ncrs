//! The state changes of the Hermes pages. What reaches outside them comes back
//! as actions. Each page has its own entry; the app routes the messages.

mod column;
mod filing_job;
mod hermes_file;
mod lifecycle;
mod overview;
mod quick_fix;
mod rules;
mod saved_queries;
mod status;
mod tags;

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::FilingWriteError;
use ncrs_filing::filing_work::FilingSummary;
use ncrs_index::IncludeOutcome;
use ncrs_ui_kit::focus::release_focus;

use crate::action::{Action, ProtectionStep};
use crate::ctx::Ctx;
use crate::hermes_column::FolderRole;
use crate::message::{
    HermesColumnMessage, HermesOverviewMessage, HermesPage, Message, QuickFixMessage, RulesMessage,
    SavedQueriesMessage, TagsMessage,
};
use crate::rule_form::RuleForm;
use crate::rules_page::RulesPage;
use crate::saved_queries::SavedQueries;
use crate::state::Hermes;
use crate::tags_page::TagsPage;
use ncrs_config::config::Filing;
use ncrs_filing::filing::FilingMessage;

pub use self::column::{refresh_hermes_column, request_hermes_column};
pub use self::filing_job::{run_filing_work, run_filing_work_after};
pub use self::lifecycle::SavedFilingApplies;
pub use self::status::{open_filing_status_page, request_hermes_counts, update_filing_status};

pub type Outcome = (Task<Message>, Vec<Action>);

/// The settings show `page`: what the other Hermes pages held is dropped and
/// the data of this one is read.
pub fn show_page(hermes: &mut Hermes, page: HermesPage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes.close_pages();
    let task = match page {
        HermesPage::SavedQueries => saved_queries::open(hermes, ctx, &mut actions),
        HermesPage::Rules => rules::open(hermes, ctx, &mut actions),
        HermesPage::Tags => tags::open(hermes, ctx, &mut actions),
        HermesPage::Inspection => {
            let (task, opened) = open_filing_status_page(hermes, ctx);
            actions.extend(opened);
            task
        }
    };
    (Task::batch([release_focus(), task]), actions)
}

/// The settings show none of the Hermes pages any more.
pub fn leave_pages(hermes: &mut Hermes) -> Task<Message> {
    hermes.close_pages();
    release_focus()
}

/// A dialog of the page is up, and the next Escape closes it.
pub fn dialog_is_open(hermes: &Hermes) -> bool {
    hermes.rules.as_ref().is_some_and(RulesPage::dialog_is_open)
        || hermes.tags.as_ref().is_some_and(TagsPage::dialog_is_open)
        || hermes
            .saved_queries
            .as_ref()
            .is_some_and(SavedQueries::dialog_is_open)
}

/// Closing the dialog that is up would throw away typed input.
fn dialog_holds_unsaved_input(hermes: &Hermes) -> bool {
    hermes
        .rules
        .as_ref()
        .and_then(RulesPage::form)
        .is_some_and(RuleForm::is_edited)
        || hermes
            .saved_queries
            .as_ref()
            .is_some_and(SavedQueries::holds_unsaved_input)
}

/// Closes the dialog that is up; `None` when there was none. A dialog with
/// typed input stays up, its own Cancel button is the way to drop it.
pub fn close_dialog(hermes: &mut Hermes) -> Option<Task<Message>> {
    if !dialog_is_open(hermes) {
        return None;
    }
    if dialog_holds_unsaved_input(hermes) {
        return Some(Task::none());
    }
    Some(Task::batch([
        rules::close_dialog(hermes),
        tags::close_dialog(hermes),
        saved_queries::close_dialog(hermes),
    ]))
}

pub fn update_hermes_overview(
    hermes: &mut Hermes,
    message: HermesOverviewMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = overview::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_quick_fix(hermes: &mut Hermes, message: QuickFixMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = quick_fix::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_rules(hermes: &mut Hermes, message: RulesMessage) -> Outcome {
    (rules::update(hermes, message), Vec::new())
}

pub fn update_saved_queries(
    hermes: &mut Hermes,
    message: SavedQueriesMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = saved_queries::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_tags(hermes: &mut Hermes, message: TagsMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = tags::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn exclude_from_hermes(hermes: &Hermes, paths: Vec<PathBuf>) -> Outcome {
    let mut actions = Vec::new();
    let task = quick_fix::exclude_from_hermes(hermes, paths, &mut actions);
    (task, actions)
}

pub fn documents_filed(result: Result<FilingSummary, String>, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    filing_job::documents_filed(result, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn run_filing(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    filing_job::run_filing(hermes, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn update_hermes_column(
    hermes: &mut Hermes,
    message: HermesColumnMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = column::update_hermes_column(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn read_paths_again(hermes: &mut Hermes, paths: Vec<PathBuf>, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes_file::read_paths_again(hermes, paths, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn process_again(hermes: &mut Hermes, path: &Path, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes_file::process_again(hermes, path, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn set_folder_role(hermes: &Hermes, folder: &Path, role: FolderRole, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_file::set_folder_role(hermes, folder, role, ctx, &mut actions);
    (task, actions)
}

pub fn role_save_failed(hermes: &Hermes, error: &FilingWriteError, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes_file::role_save_failed(hermes, error, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn toggle_hermes_exclusion(hermes: &mut Hermes, path: PathBuf, excluded: bool) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_file::toggle_hermes_exclusion(hermes, path, excluded, &mut actions);
    (task, actions)
}

pub fn hermes_included(
    hermes: &mut Hermes,
    path: &Path,
    result: Result<IncludeOutcome, String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    hermes_file::hermes_included(hermes, path, result, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn update_filing(hermes: &mut Hermes, message: FilingMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = lifecycle::update_filing(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn start_filing_module(hermes: &mut Hermes, filing: &Filing, ctx: &Ctx<'_>) -> Outcome {
    (
        lifecycle::start_filing_module(hermes, filing, ctx),
        Vec::new(),
    )
}

pub fn restart_hermes_module(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    (lifecycle::restart_hermes_module(hermes, ctx), Vec::new())
}

pub fn filing_restarted(hermes: &mut Hermes, generation: u64, ctx: &Ctx<'_>) -> Outcome {
    (
        lifecycle::filing_restarted(hermes, generation, ctx),
        Vec::new(),
    )
}

pub fn change_protection(hermes: &mut Hermes, begin: ProtectionStep, ctx: &Ctx<'_>) -> Outcome {
    (lifecycle::change_protection(hermes, begin, ctx), Vec::new())
}

/// The saved filing list takes over, now or at the next start.
pub fn apply_saved_filing(
    hermes: &mut Hermes,
    filing: Filing,
    ctx: &Ctx<'_>,
) -> (SavedFilingApplies, Outcome) {
    let (applies, task) = lifecycle::apply_saved_filing(hermes, filing, ctx);
    (applies, (task, Vec::new()))
}

/// An AI setting changed: the module is rebuilt now or at the next start.
pub fn apply_ai_change(
    hermes: &mut Hermes,
    filing: Filing,
    ctx: &Ctx<'_>,
) -> (SavedFilingApplies, Outcome) {
    let (applies, task) = lifecycle::apply_ai_change(hermes, filing, ctx);
    (applies, (task, Vec::new()))
}

pub fn hermes_role_saved(
    hermes: &mut Hermes,
    result: Result<Filing, FilingWriteError>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_file::role_saved(hermes, result, ctx, &mut actions);
    (task, actions)
}

/// Opens the settings on the Read errors page.
pub fn show_in_inspection() -> Outcome {
    let mut actions = Vec::new();
    hermes_file::show_in_inspection(&mut actions);
    (Task::none(), actions)
}
