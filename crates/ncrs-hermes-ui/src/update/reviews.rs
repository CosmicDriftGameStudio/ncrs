//! The reviews page: reads the open reviews, files a group in one go through
//! the review's own confirm, and sends the rest to the chat or the panel.

use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::review_work::{AcceptSummary, ReviewItem};

use super::quick_fix::{exclude_from_hermes, filed};
use super::rules::open_rule_form;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, ReviewsMessage};
use crate::quick_fix::rule_prefill_of;
use crate::reviews_page::{Answer, Question, ReviewsPage};
use crate::rule_form::RuleForm;
use crate::state::Hermes;

pub fn open(hermes: &mut Hermes, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.reviews.is_some() {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.reviews = Some(ReviewsPage::new());
    request(hermes)
}

/// Reads the reviews on a blocking thread; the answer of an older read is
/// dropped.
fn request(hermes: &mut Hermes) -> Task<Message> {
    hermes.reviews_generation = hermes.reviews_generation.wrapping_add(1);
    let generation = hermes.reviews_generation;
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
        if let Some(page) = hermes.reviews.as_mut() {
            page.failed(INDEX_NOT_OPEN.to_owned());
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.load()),
        move |joined| {
            Message::Reviews(ReviewsMessage::Loaded {
                generation,
                result: joined.unwrap_or_else(|err| Err(err.to_string())),
            })
        },
    )
}

pub fn update(
    hermes: &mut Hermes,
    message: ReviewsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        ReviewsMessage::Loaded { generation, result } => {
            if generation != hermes.reviews_generation {
                return Task::none();
            }
            if let Some(page) = hermes.reviews.as_mut() {
                match result {
                    Ok(items) => page.loaded(items, ctx.kinds),
                    Err(detail) => page.failed(detail),
                }
            }
            Task::none()
        }
        ReviewsMessage::ToggleGroup(question) => {
            if let Some(page) = hermes.reviews.as_mut() {
                page.toggle(question);
            }
            Task::none()
        }
        ReviewsMessage::Answer(question, answer) => {
            answer_question(hermes, question, answer, ctx, actions)
        }
        ReviewsMessage::ShowFile(path) => {
            actions.push(Action::RevealFile(path));
            Task::none()
        }
        ReviewsMessage::Filed(summary) => {
            filed(hermes, &summary, ctx, actions);
            actions.push(Action::RequestHermesCounts);
            if let Some(page) = hermes.reviews.as_mut() {
                page.loading_again();
            }
            request(hermes)
        }
    }
}

/// Carries out what the user chose for the group of `question`, with the
/// actions the quick-fix menu has.
fn answer_question(
    hermes: &mut Hermes,
    question: Question,
    answer: Answer,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(page) = hermes.reviews.as_mut() else {
        return Task::none();
    };
    match answer {
        Answer::Accept => {
            let items = page.fileable(question);
            file_group(hermes, items, actions)
        }
        Answer::SetFields => {
            actions.push(Action::OpenChat);
            Task::none()
        }
        Answer::CreateRule => {
            let Some(item) = page.members(question).first().map(|item| (*item).clone()) else {
                return Task::none();
            };
            let prefill = rule_prefill_of(
                &item,
                hermes.filing_config.default_pattern.as_deref(),
                ctx.kinds,
            );
            open_rule_form(
                hermes,
                RuleForm::from_document(prefill, ctx.kinds),
                ctx,
                actions,
            )
        }
        Answer::Exclude => {
            let paths = page
                .members(question)
                .into_iter()
                .map(|item| item.path.clone())
                .collect();
            page.put_off(question);
            exclude_from_hermes(hermes, paths, actions)
        }
        Answer::Later => {
            page.put_off(question);
            Task::none()
        }
    }
}

/// Files the documents one after the other, each through the review's own
/// confirm: never a plain move.
fn file_group(
    hermes: &mut Hermes,
    items: Vec<ReviewItem>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if items.is_empty() {
        return Task::none();
    }
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.accept(&items)),
        |joined| {
            Message::Reviews(ReviewsMessage::Filed(joined.unwrap_or_else(|err| {
                AcceptSummary {
                    failed: 1,
                    first_failure: Some(err.to_string()),
                    ..AcceptSummary::default()
                }
            })))
        },
    )
}
