use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::suggestion_work::{SuggestedRule, SuggestionAccess};
use ncrs_ui_kit::focus::release_focus;

use super::blocking_overlay_is_open;
use super::rules::open_rule_form;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{HermesPage, Message, RuleSuggestionsMessage};
use crate::rule_form::RuleForm;
use crate::rule_suggestions::RuleSuggestions;
use crate::state::Hermes;

pub fn update(
    hermes: &mut Hermes,
    message: RuleSuggestionsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        RuleSuggestionsMessage::Open => open(hermes, ctx, actions),
        RuleSuggestionsMessage::Close => close(hermes),
        RuleSuggestionsMessage::Reanalyse => {
            let Some(view) = hermes.rule_suggestions.as_mut() else {
                return Task::none();
            };
            if view.is_busy() {
                return Task::none();
            }
            view.begin_analysis();
            run_suggestion_work(hermes, SuggestionAccess::analyse)
        }
        RuleSuggestionsMessage::Loaded { generation, result } => {
            if generation != hermes.rule_suggestions_generation {
                return Task::none();
            }
            let Some(view) = hermes.rule_suggestions.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(rows) => view.loaded(rows),
                Err(detail) => view.failed(detail),
            }
            look_up_suggestion_matches(hermes)
        }
        RuleSuggestionsMessage::Move(delta) => {
            if let Some(view) = hermes.rule_suggestions.as_mut() {
                view.move_cursor(delta);
            }
            look_up_suggestion_matches(hermes)
        }
        RuleSuggestionsMessage::Scrolled(delta) => {
            if let Some(view) = hermes.rule_suggestions.as_mut() {
                view.scroll(delta);
            }
            look_up_suggestion_matches(hermes)
        }
        RuleSuggestionsMessage::Select(index) => {
            if let Some(view) = hermes.rule_suggestions.as_mut() {
                view.select(index);
            }
            look_up_suggestion_matches(hermes)
        }
        RuleSuggestionsMessage::Edit => {
            let Some(view) = hermes.rule_suggestions.as_ref() else {
                return Task::none();
            };
            let Some(rule) = view.rule_to_edit() else {
                return Task::none();
            };
            open_rule_form(
                hermes,
                RuleForm::from_suggestion(&rule),
                Some(HermesPage::RuleSuggestions),
                ctx,
                actions,
            )
        }
        RuleSuggestionsMessage::Confirm => {
            let Some((id, spec)) = hermes
                .rule_suggestions
                .as_mut()
                .and_then(RuleSuggestions::begin_confirm)
            else {
                return Task::none();
            };
            run_suggestion_work(hermes, move |access| access.confirm(id, &spec))
        }
        RuleSuggestionsMessage::Discard => {
            let Some(id) = hermes
                .rule_suggestions
                .as_mut()
                .and_then(RuleSuggestions::begin_discard)
            else {
                return Task::none();
            };
            run_suggestion_work(hermes, move |access| access.discard(id))
        }
        RuleSuggestionsMessage::MatchesFound { id, result } => {
            if let (Some(view), Ok(paths)) = (hermes.rule_suggestions.as_mut(), result) {
                view.matches_found(id, paths);
            }
            Task::none()
        }
    }
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.rule_suggestions.is_some() || blocking_overlay_is_open(hermes, ctx) {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.rule_suggestions_generation = hermes.rule_suggestions_generation.wrapping_add(1);
    hermes.rule_suggestions = Some(RuleSuggestions::new());
    run_suggestion_work(hermes, SuggestionAccess::analyse)
}

fn close(hermes: &mut Hermes) -> Task<Message> {
    if hermes.rule_suggestions.take().is_none() {
        return Task::none();
    }
    release_focus()
}

/// Runs `work` on a blocking thread: the index is shared with the intake,
/// which may hold it while it hashes a large file.
fn run_suggestion_work(
    hermes: &mut Hermes,
    work: impl FnOnce(&SuggestionAccess) -> Result<Vec<SuggestedRule>, String> + Send + 'static,
) -> Task<Message> {
    let generation = hermes.rule_suggestions_generation;
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::suggestion_access)
    else {
        if let Some(view) = hermes.rule_suggestions.as_mut() {
            view.failed(INDEX_NOT_OPEN.to_owned());
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || work(&access)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::RuleSuggestions(RuleSuggestionsMessage::Loaded { generation, result })
        },
    )
}

/// Looks up the documents of the selected suggestion, once per selection.
fn look_up_suggestion_matches(hermes: &mut Hermes) -> Task<Message> {
    let Some((id, condition)) = hermes
        .rule_suggestions
        .as_mut()
        .and_then(RuleSuggestions::matches_wanted)
    else {
        return Task::none();
    };
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::suggestion_access)
    else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.matching_documents(&condition)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::RuleSuggestions(RuleSuggestionsMessage::MatchesFound { id, result })
        },
    )
}
