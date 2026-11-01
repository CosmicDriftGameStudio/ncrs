//! The state changes of the Hermes pages. What reaches outside them comes back
//! as actions. Each page has its own entry; the app routes the messages.

mod filing_job;
mod filing_setup;
mod hermes_setup;
mod quick_fix;
mod refile;
mod reorganisation;
mod review_list;
mod rule_suggestions;
mod rules;
mod saved_queries;
mod tags;

use std::path::PathBuf;

use iced::Task;
use ncrs_filing::execute::UndoReport;
use ncrs_filing::filing_work::FilingSummary;
use ncrs_filing::reorganisation_work::{ExecutionSummary, PlannedItem};
use ncrs_filing::replan_work::ReplanSummary;
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{
    FilingSetupMessage, HermesPage, HermesSetupMessage, Message, QuickFixMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage, RulesMessage,
    SavedQueriesMessage, TagsMessage,
};
use crate::state::Hermes;
use ncrs_index::RuleId;

pub use self::filing_job::{run_filing_work, run_filing_work_after};
pub use self::review_list::request_review_preview;

pub type Outcome = (Task<Message>, Vec<Action>);

/// The pages, the menu and the setup card of the Hermes state count, as they
/// are now.
fn blocking_overlay_is_open(hermes: &Hermes, ctx: &Ctx<'_>) -> bool {
    ctx.ablage_tab_apart_from_hermes_open
        || ctx.overlay_apart_from_ablage_tab_open
        || hermes.page_is_open()
        || hermes.quick_fix.is_some()
        || hermes.hermes_setup.is_some()
}

/// The Hermes page that is up, in the order the sidebar knows them.
fn open_page(hermes: &Hermes, ctx: &Ctx<'_>) -> Option<HermesPage> {
    ctx.page_apart_from_hermes.or_else(|| hermes.open_page())
}

type Opener = fn(&mut Hermes, &Ctx<'_>, &mut Vec<Action>) -> Task<Message>;

/// A click in the sidebar: the page that is up closes without its Escape
/// steps, the chosen page opens. Nothing happens when it is the open one, or
/// when no Hermes page is up. The overview and the status page are the app's:
/// it goes there.
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
        HermesPage::Overview | HermesPage::Inspection => {
            actions.push(Action::GoToHermesPage(page));
            return Task::none();
        }
    };
    actions.push(Action::CloseOverviewAndStatus);
    hermes.close_pages();
    let without_ablage_tab = Ctx {
        ablage_tab_apart_from_hermes_open: false,
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

pub fn open_rules(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = rules::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn open_rule_suggestions(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = rule_suggestions::open(hermes, ctx, &mut actions);
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

pub fn open_review_list(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = review_list::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn open_reorganisation(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::open(hermes, ctx, &mut actions);
    (task, actions)
}

pub fn run_reorganisation(selection: Vec<PlannedItem>, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    reorganisation::run_selection(selection, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn exclude_from_hermes(paths: Vec<PathBuf>, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = quick_fix::exclude_from_hermes(paths, ctx, &mut actions);
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

pub fn undo_reorganisation(ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = reorganisation::undo(ctx, &mut actions);
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
