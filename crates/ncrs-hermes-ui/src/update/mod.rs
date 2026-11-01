//! The state changes of the Hermes pages. What reaches outside them comes back
//! as actions. Each page has its own entry; the app routes the messages.

mod column;
mod filing_job;
mod filing_setup;
mod hermes_file;
mod hermes_setup;
mod lifecycle;
mod overview;
mod quick_fix;
mod refile;
mod reorganisation;
mod review_list;
mod rule_suggestions;
mod rules;
mod saved_queries;
mod status;
mod tags;

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::FilingWriteError;
use ncrs_filing::execute::UndoReport;
use ncrs_filing::filing_work::FilingSummary;
use ncrs_filing::reorganisation_work::{ExecutionSummary, PlannedItem};
use ncrs_filing::replan_work::ReplanSummary;
use ncrs_index::IncludeOutcome;
use ncrs_ui_kit::focus::release_focus;

use crate::action::{Action, ProtectionStep};
use crate::ctx::Ctx;
use crate::hermes_column::FolderRole;
use crate::message::{
    FilingSetupMessage, HermesColumnMessage, HermesOverviewMessage, HermesPage, HermesSetupMessage,
    Message, QuickFixMessage, ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage,
    RulesMessage, SavedQueriesMessage, TagsMessage,
};
use crate::state::Hermes;
use ncrs_config::config::Filing;
use ncrs_filing::filing::FilingMessage;
use ncrs_index::RuleId;

pub use self::column::{refresh_hermes_column, request_hermes_column};
pub use self::filing_job::{run_filing_work, run_filing_work_after};
pub use self::lifecycle::SavedFilingApplies;
pub use self::review_list::request_review_preview;
pub use self::status::{
    open_filing_status_page, request_hermes_counts, request_index_figures, update_filing_status,
};

pub type Outcome = (Task<Message>, Vec<Action>);

/// The pages, the menu and the setup card of the Hermes state count, as they
/// are now; the status page and the dialogs come from the context.
fn blocking_overlay_is_open(hermes: &Hermes, ctx: &Ctx<'_>) -> bool {
    ctx.status_page_open
        || ctx.overlay_apart_from_ablage_tab_open
        || hermes.page_is_open()
        || hermes.quick_fix.is_some()
        || hermes.hermes_setup.is_some()
}

/// The Hermes page that is up, in the order the sidebar knows them.
fn open_page(hermes: &Hermes, ctx: &Ctx<'_>) -> Option<HermesPage> {
    ctx.status_page_open
        .then_some(HermesPage::Inspection)
        .or_else(|| hermes.open_page())
}

type Opener = fn(&mut Hermes, &Ctx<'_>, &mut Vec<Action>) -> Task<Message>;

/// A click in the sidebar: the page that is up closes without its Escape
/// steps, the chosen page opens. Nothing happens when it is the open one, or
/// when no Hermes page is up. The status page is the app's: it goes there.
fn go_to_page(
    hermes: &mut Hermes,
    page: HermesPage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match open_page(hermes, ctx) {
        None => return Task::none(),
        Some(open) if open == page => return Task::none(),
        Some(_) => {}
    }
    let open: Opener = match page {
        HermesPage::Review => review_list::open,
        HermesPage::SavedQueries => saved_queries::open,
        HermesPage::Rules => rules::open,
        HermesPage::RuleSuggestions => rule_suggestions::open,
        HermesPage::Tags => tags::open,
        HermesPage::Reorganisation => reorganisation::open,
        HermesPage::Overview => overview::open,
        HermesPage::Inspection => {
            actions.push(Action::GoToHermesPage(page));
            return Task::none();
        }
    };
    actions.push(Action::CloseStatusPage);
    hermes.close_pages();
    let without_ablage_tab = Ctx {
        status_page_open: false,
        ..ctx.clone()
    };
    Task::batch([release_focus(), open(hermes, &without_ablage_tab, actions)])
}

/// Leaves the plan for the page of the rule behind an entry: the Rules page
/// for a confirmed rule, the suggestions for one that is not.
fn open_rule_of_plan(
    hermes: &mut Hermes,
    id: RuleId,
    page: HermesPage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let open = go_to_page(hermes, page, ctx, actions);
    if let Some(rules) = hermes.rules.as_mut() {
        rules.select_on_load(id);
    }
    if let Some(suggestions) = hermes.rule_suggestions.as_mut() {
        suggestions.select_on_load(id);
    }
    open
}

pub fn update_hermes_overview(
    hermes: &mut Hermes,
    message: HermesOverviewMessage,
    ctx: &Ctx<'_>,
    write_failure: Option<String>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = overview::update(hermes, message, ctx, write_failure, &mut actions);
    (task, actions)
}

pub fn open_overview(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = overview::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn leave_overview(hermes: &mut Hermes) -> Task<Message> {
    overview::leave(hermes)
}

/// A click in the sidebar, or any other way to a page.
pub fn go_to_hermes_page(hermes: &mut Hermes, page: HermesPage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = go_to_page(hermes, page, ctx, &mut actions);
    (task, actions)
}

pub fn update_review_list(
    hermes: &mut Hermes,
    message: ReviewListMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = review_list::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_quick_fix(hermes: &mut Hermes, message: QuickFixMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = quick_fix::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_reorganisation(
    hermes: &mut Hermes,
    message: ReorganisationPlanMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_rule_suggestions(
    hermes: &mut Hermes,
    message: RuleSuggestionsMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = rule_suggestions::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_rules(hermes: &mut Hermes, message: RulesMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = rules::update(hermes, message, ctx, &mut actions);
    (task, actions)
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

pub fn update_filing_setup(
    hermes: &mut Hermes,
    message: FilingSetupMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = filing_setup::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_hermes_setup(
    hermes: &mut Hermes,
    message: HermesSetupMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_setup::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_tags(hermes: &mut Hermes, message: TagsMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = tags::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn open_saved_queries(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = saved_queries::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn open_tags(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = tags::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn run_reorganisation(hermes: &Hermes, selection: Vec<PlannedItem>, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    reorganisation::run_selection(hermes, selection, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn exclude_from_hermes(hermes: &Hermes, paths: Vec<PathBuf>) -> Outcome {
    let mut actions = Vec::new();
    let task = quick_fix::exclude_from_hermes(hermes, paths, &mut actions);
    (task, actions)
}

pub fn reorganisation_executed(
    hermes: &mut Hermes,
    result: Result<ExecutionSummary, String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::executed(hermes, result, ctx, &mut actions);
    (task, actions)
}

pub fn undo_reorganisation(hermes: &Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::undo(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn reorganisation_undone(
    hermes: &mut Hermes,
    result: Result<Option<UndoReport>, String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::undone(hermes, result, ctx, &mut actions);
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

pub fn run_refile(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    refile::run_refile(hermes, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn reprocess_folder(hermes: &mut Hermes, folder: PathBuf, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = refile::reprocess_folder(hermes, folder, ctx, &mut actions);
    (task, actions)
}

pub fn documents_refiled(
    hermes: &mut Hermes,
    result: Result<ReplanSummary, String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    refile::documents_refiled(hermes, result, ctx)
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

pub fn open_hermes_setup(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    lifecycle::open_hermes_setup(hermes, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn activate_hermes_focus(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_setup::activate_focus(hermes, ctx, &mut actions);
    (task, actions)
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

/// Opens the inspection page, at the file's error if the page lists one.
pub fn show_in_inspection(hermes: &Hermes, path: &Path, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes_file::show_in_inspection(hermes, path, ctx, &mut actions);
    (Task::none(), actions)
}

/// Takes the next step of "Hermes starten" the module's phase allows.
pub fn advance_hermes_plan(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let _ = ctx;
    let mut actions = Vec::new();
    let task = lifecycle::advance_hermes_plan(hermes, &mut actions);
    (task, actions)
}
