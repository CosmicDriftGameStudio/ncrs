//! The state changes of the Hermes pages. What reaches outside them comes back
//! as actions. Each page has its own entry; the app routes the messages.

mod activity;
mod column;
mod filing_job;
mod hermes_file;
mod lifecycle;
mod overview;
mod rules;
mod saved_queries;
mod sorting;
mod status;
mod suggest;
mod tags;
mod targets;
mod watching;

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::FilingWriteError;
use ncrs_ui_kit::focus::release_focus;

use crate::action::{Action, ProtectionStep};
use crate::ctx::Ctx;
use crate::message::{
    ActivityMessage, HermesColumnMessage, HermesFileMessage, HermesOverviewMessage, HermesPage,
    Message, RulesMessage, SavedQueriesMessage, SortingMessage, TagsMessage, TargetsMessage,
};
use crate::rule_form::RuleForm;
use crate::rules_page::RulesPage;
use crate::saved_queries::SavedQueries;
use crate::source_dialog::SourceDialogKind;
use crate::state::Hermes;
use crate::tags_page::TagsPage;
use crate::watching::SourceRun;
use ncrs_config::config::Filing;
use ncrs_filing::filing::FilingMessage;
use ncrs_index::{DocumentId, TargetId};

pub use self::column::{refresh_hermes_column, request_hermes_column};
pub use self::filing_job::{run_filing_work, run_filing_work_after};
pub use self::lifecycle::SavedFilingApplies;
pub use self::status::{open_filing_status_page, request_hermes_counts, update_filing_status};
pub use self::suggest::{open_rule as open_suggest_rule, open_scheme as open_suggest_scheme};
pub use self::targets::open_dialog_then_suggest as open_target_dialog;

pub type Outcome = (Task<Message>, Vec<Action>);

/// The settings show `page`: what the other Hermes pages held is dropped and
/// the data of this one is read.
pub fn show_page(hermes: &mut Hermes, page: HermesPage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes.close_pages();
    let task = match page {
        HermesPage::SavedQueries => saved_queries::open(hermes, ctx, &mut actions),
        HermesPage::Sorting => {
            let rules_task = rules::open(hermes, ctx, &mut actions);
            Task::batch([sorting::open(hermes, ctx, &mut actions), rules_task])
        }
        HermesPage::Tags => tags::open(hermes, ctx, &mut actions),
        HermesPage::Activity => activity::open(hermes, &mut actions),
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

/// The app is closing: a file being read now must not count as a crash.
pub fn closing(hermes: &Hermes) {
    if let Some(module) = &hermes.filing {
        module.forget_reading_marker();
    }
}

/// A dialog of the page is up, and the next Escape closes it.
pub fn dialog_is_open(hermes: &Hermes) -> bool {
    hermes.source_dialog.is_some()
        || hermes.target_dialog.is_some()
        || hermes.target_removal.is_some()
        || hermes.suggest.is_some()
        || hermes.rules.as_ref().is_some_and(RulesPage::dialog_is_open)
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
    // The dialog of a target opens over the one of a source and goes first.
    if hermes.target_dialog.take().is_some()
        || hermes.source_dialog.take().is_some()
        || hermes.target_removal.take().is_some()
    {
        return Some(Task::none());
    }
    // The rule form opens over the dialog of the proposals and goes first.
    if !hermes.rules.as_ref().is_some_and(RulesPage::dialog_is_open) && hermes.suggest.is_some() {
        suggest::close(hermes);
        return Some(Task::none());
    }
    if dialog_holds_unsaved_input(hermes) {
        return Some(Task::none());
    }
    let closed = Task::batch([
        rules::close_dialog(hermes),
        tags::close_dialog(hermes),
        saved_queries::close_dialog(hermes),
    ]);
    suggest::after_rules(hermes);
    Some(closed)
}

/// The one entry of the Hermes pages for the messages that need nothing but
/// the state and the context. `None` for the messages the app answers itself:
/// the file messages that need the cursor and the settings switch.
pub fn update(hermes: &mut Hermes, message: Message, ctx: &Ctx<'_>) -> Option<Outcome> {
    Some(match message {
        Message::Filing(inner) => update_filing(hermes, inner, ctx),
        Message::FilingStatus(inner) => update_filing_status(hermes, inner, ctx),
        Message::HermesOverview(inner) => update_hermes_overview(hermes, inner, ctx),
        Message::Rules(inner) => update_rules(hermes, inner, ctx),
        Message::Sorting(inner) => update_sorting(hermes, inner, ctx),
        Message::Targets(inner) => update_targets(hermes, inner, ctx),
        Message::Suggest(inner) => suggest::update(hermes, inner, ctx),
        Message::SavedQueries(inner) => update_saved_queries(hermes, inner, ctx),
        Message::Tags(inner) => update_tags(hermes, inner, ctx),
        Message::Activity(inner) => update_activity(hermes, inner),
        Message::ContinuousFiled(result) => continuous_filed(hermes, result, ctx),
        Message::HermesColumn(inner) => update_hermes_column(hermes, inner, ctx),
        Message::HermesFile(inner) => update_hermes_file(hermes, inner, ctx),
        Message::ToggleFileListTags => return None,
    })
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

fn update_hermes_file(hermes: &mut Hermes, message: HermesFileMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_file::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_rules(hermes: &mut Hermes, message: RulesMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = rules::update(hermes, message, ctx, &mut actions);
    suggest::after_rules(hermes);
    (task, actions)
}

/// Opens the rule dialog over whatever is shown; see `rules::open_rule_form`.
pub fn open_rule_form(
    hermes: &mut Hermes,
    target: Option<TargetId>,
    document: Option<DocumentId>,
    path: Option<&Path>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = rules::open_rule_form(hermes, target, document, path, ctx, &mut actions);
    (task, actions)
}

/// Opens the dialog of the source at `folder`, or of a new one, or of the
/// one-off filing of the folder; it works over every tab.
pub fn open_source_dialog(
    hermes: &mut Hermes,
    folder: Option<PathBuf>,
    kind: SourceDialogKind,
) -> Task<Message> {
    sorting::open_dialog(hermes, folder, kind)
}

fn update_targets(hermes: &mut Hermes, message: TargetsMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    sorting::tick_new_targets(hermes, &message);
    let task = targets::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn update_sorting(hermes: &mut Hermes, message: SortingMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = sorting::update(hermes, message, ctx, &mut actions);
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

pub fn update_activity(hermes: &mut Hermes, message: ActivityMessage) -> Outcome {
    let mut actions = Vec::new();
    let task = activity::update(hermes, message, &mut actions);
    (task, actions)
}

pub fn update_tags(hermes: &mut Hermes, message: TagsMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = tags::update(hermes, message, ctx, &mut actions);
    (task, actions)
}

pub fn exclude_from_hermes(hermes: &Hermes, paths: Vec<PathBuf>) -> Outcome {
    let mut actions = Vec::new();
    let task = hermes_file::exclude_from_hermes(hermes, paths, &mut actions);
    (task, actions)
}

pub fn continuous_filed(
    hermes: &mut Hermes,
    result: Result<Vec<SourceRun>, String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    watching::continuous_filed(hermes, result, ctx, &mut actions);
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

pub fn role_save_failed(hermes: &Hermes, error: &FilingWriteError, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    hermes_file::role_save_failed(hermes, error, ctx, &mut actions);
    (Task::none(), actions)
}

pub fn update_filing(hermes: &mut Hermes, message: FilingMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = lifecycle::update_filing(hermes, message, ctx, &mut actions);
    (Task::batch([task, suggest::resume(hermes, ctx)]), actions)
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
