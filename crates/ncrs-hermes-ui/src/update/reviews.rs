//! The reviews page: reads the open reviews and carries out the answer a
//! person gives to a question, for a whole group or for one file. Every answer
//! that files or removes documents runs on a blocking thread through the
//! journaled paths of the review work, and the list is read again after it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::operation;
use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::review_work::{AcceptSummary, NotMoved, ReviewAccess, ReviewItem};
use ncrs_i18n::{fill_template, Msg};
use ncrs_index::DocumentId;

use super::quick_fix::{accept_notice, exclude_from_hermes, settle_column};
use super::rules::open_rule_form;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, ReviewsMessage};
use crate::quick_fix::rule_prefill_of;
use crate::reviews_page::{Answer, Question, ReviewsLoad, ReviewsPage};
use crate::rule_form::RuleForm;
use crate::state::Hermes;

type Work = Box<dyn Fn(&ReviewAccess, &[ReviewItem]) -> AcceptSummary + Send>;

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
        tokio::task::spawn_blocking(move || read_all(&access)),
        move |joined| {
            Message::Reviews(ReviewsMessage::Loaded {
                generation,
                result: joined.unwrap_or_else(|err| Err(err.to_string())),
            })
        },
    )
}

fn read_all(access: &ReviewAccess) -> Result<ReviewsLoad, String> {
    Ok(ReviewsLoad {
        items: access.load()?,
        already_filed: access.load_already_filed()?,
    })
}

/// Looks at the files in the taken places on a blocking thread: hashing is
/// slow, so the list shows before it is done.
fn compare_taken(hermes: &Hermes) -> Task<Message> {
    let Some(page) = hermes.reviews.as_ref() else {
        return Task::none();
    };
    let pairs = page.taken_to_compare();
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::review_access)
        .filter(|_| !pairs.is_empty())
    else {
        return Task::none();
    };
    let generation = hermes.reviews_generation;
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let places: Vec<(PathBuf, PathBuf)> = pairs
                .iter()
                .map(|(_, incoming, relative)| (incoming.clone(), relative.clone()))
                .collect();
            let found = access.compare_targets(&places);
            pairs
                .into_iter()
                .map(|(document, _, _)| document)
                .zip(found)
                .collect::<Vec<_>>()
        }),
        move |joined| {
            Message::Reviews(ReviewsMessage::Compared {
                generation,
                result: joined.unwrap_or_default(),
            })
        },
    )
}

fn is_busy(hermes: &Hermes) -> bool {
    hermes
        .reviews
        .as_ref()
        .is_some_and(|page| page.busy_with().is_some())
}

fn is_answer(message: &ReviewsMessage) -> bool {
    matches!(
        message,
        ReviewsMessage::Answer(..)
            | ReviewsMessage::AnswerFile(..)
            | ReviewsMessage::AnswerRepeat(..)
    )
}

pub fn update(
    hermes: &mut Hermes,
    message: ReviewsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if is_answer(&message) && is_busy(hermes) {
        return Task::none();
    }
    match message {
        ReviewsMessage::Loaded { generation, result } => {
            if generation != hermes.reviews_generation {
                return Task::none();
            }
            let Some(page) = hermes.reviews.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(load) => {
                    page.loaded(load.items, ctx.kinds);
                    page.already_filed_loaded(load.already_filed);
                    compare_taken(hermes)
                }
                Err(detail) => {
                    page.failed(detail);
                    Task::none()
                }
            }
        }
        ReviewsMessage::Compared { generation, result } => {
            if generation == hermes.reviews_generation {
                if let Some(page) = hermes.reviews.as_mut() {
                    page.compared(result);
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
            answer_question(hermes, (question, answer), None, ctx, actions)
        }
        ReviewsMessage::AnswerFile(question, answer, document) => {
            answer_question(hermes, (question, answer), Some(document), ctx, actions)
        }
        ReviewsMessage::AnswerRepeat(answer, path) => {
            answer_already_filed(hermes, answer, Some(path), ctx, actions)
        }
        ReviewsMessage::InputChanged(document, text) => {
            if let Some(page) = hermes.reviews.as_mut() {
                page.type_into(document, text);
            }
            Task::none()
        }
        ReviewsMessage::OpenFile(path) => {
            actions.push(Action::PreviewFile(path));
            Task::none()
        }
        ReviewsMessage::FolderPicked(picked_folder) => {
            let Some(items) = hermes
                .reviews
                .as_mut()
                .map(ReviewsPage::take_awaiting_folder)
            else {
                return Task::none();
            };
            let Some(folder) = picked_folder else {
                return Task::none();
            };
            let picked = move |access: &ReviewAccess, batch: &[ReviewItem]| {
                access.accept_into(batch, &folder)
            };
            run(hermes, items, ctx, actions, Box::new(picked))
        }
        ReviewsMessage::Filed(summary) => {
            answered(hermes, &summary, ctx, actions);
            actions.push(Action::RequestHermesCounts);
            if let Some(page) = hermes.reviews.as_mut() {
                page.work_finished();
                page.loading_again();
            }
            request(hermes)
        }
    }
}

/// Carries out what the user chose, for the group of `question` or for the
/// one `document` of it.
fn answer_question(
    hermes: &mut Hermes,
    (question, answer): (Question, Answer),
    document: Option<DocumentId>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if question == Question::AlreadyFiled {
        return answer_already_filed(hermes, answer, None, ctx, actions);
    }
    let Some(page) = hermes.reviews.as_mut() else {
        return Task::none();
    };
    let items = match (answer, document) {
        (Answer::FileTyped, Some(single)) => page.typed_for(question, single).into_iter().collect(),
        _ => page.answer_items(question, answer, document),
    };
    match answer {
        Answer::EnterValue => {
            if !page.is_unfolded(question) {
                page.toggle(question);
            }
            page.first_input(question).map_or_else(Task::none, |first| {
                operation::focus(crate::ui::reviews::input_id(first))
            })
        }
        Answer::SetFields => {
            actions.push(Action::OpenChatAbout(items));
            Task::none()
        }
        Answer::CreateRule => {
            let Some(item) = items.first() else {
                return Task::none();
            };
            let prefill = rule_prefill_of(
                item,
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
            let documents: HashSet<DocumentId> =
                items.iter().map(|item| item.document_id).collect();
            let paths = items.iter().map(|item| item.path.clone()).collect();
            page.remove(&documents);
            exclude_from_hermes(hermes, paths, actions)
        }
        Answer::PickFolder => {
            let Some(access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
                actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
                return Task::none();
            };
            if items.is_empty() {
                return Task::none();
            }
            page.await_folder(items);
            actions.push(Action::PickReviewFolder(access.target_root().to_path_buf()));
            Task::none()
        }
        Answer::Accept | Answer::FileTyped => {
            run(hermes, items, ctx, actions, Box::new(ReviewAccess::accept))
        }
        Answer::DeleteDuplicate => run(
            hermes,
            items,
            ctx,
            actions,
            Box::new(ReviewAccess::delete_duplicates),
        ),
        Answer::Overwrite => run(
            hermes,
            items,
            ctx,
            actions,
            Box::new(ReviewAccess::overwrite),
        ),
        Answer::KeepBoth => run(
            hermes,
            items,
            ctx,
            actions,
            Box::new(ReviewAccess::keep_both),
        ),
    }
}

/// Carries out what the user chose for the inbox files the archive already
/// holds: all of the group, or the one at `path`. Deleting is the journaled
/// trash run of the duplicates; ignoring is the same as in the other groups.
fn answer_already_filed(
    hermes: &mut Hermes,
    answer: Answer,
    path: Option<PathBuf>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(page) = hermes.reviews.as_mut() else {
        return Task::none();
    };
    let files = page.answer_already_filed(path.as_deref());
    match answer {
        Answer::DeleteDuplicate => {
            let count = files.len();
            let work = move |access: &ReviewAccess| access.delete_already_filed(&files);
            run_with(hermes, count, ctx, actions, work)
        }
        Answer::Exclude => {
            let paths: Vec<PathBuf> = files.into_iter().map(|found| found.path).collect();
            page.remove_already_filed(&paths);
            exclude_from_hermes(hermes, paths, actions)
        }
        _ => Task::none(),
    }
}

/// Runs `work` on the documents, one after the other: each can fail on its
/// own, and the summary says how it went for all.
fn run(
    hermes: &mut Hermes,
    items: Vec<ReviewItem>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
    work: Work,
) -> Task<Message> {
    if items.is_empty() {
        return Task::none();
    }
    let count = items.len();
    run_with(hermes, count, ctx, actions, move |access| {
        work(access, &items)
    })
}

fn run_with(
    hermes: &mut Hermes,
    count: usize,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
    work: impl FnOnce(&ReviewAccess) -> AcceptSummary + Send + 'static,
) -> Task<Message> {
    let Some(bare_access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    let access = bare_access.with_trash(Arc::clone(&ctx.trash.0));
    if let Some(page) = hermes.reviews.as_mut() {
        page.work_started(count);
    }
    Task::perform(
        tokio::task::spawn_blocking(move || work(&access)),
        |joined| {
            Message::Reviews(ReviewsMessage::Filed(joined.unwrap_or_else(|err| {
                AcceptSummary {
                    failed: 1,
                    first_failure: Some(NotMoved::other(err.to_string())),
                    ..AcceptSummary::default()
                }
            })))
        },
    )
}

/// What the status bar says about an answer; failures go through the error
/// line, the rest is a notice. The column and the panels read again.
fn answered(
    hermes: &mut Hermes,
    summary: &AcceptSummary,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    settle_column(hermes);
    if summary.failed > 0 {
        let total = summary.total();
        actions.push(Action::ShowError(fill_template(
            ctx.lang.text(Msg::StatusReviewsFailed),
            &[
                ("{done}", &(total - summary.failed).to_string()),
                ("{total}", &total.to_string()),
                ("{failed}", &summary.failed.to_string()),
                (
                    "{reason}",
                    &summary
                        .first_failure
                        .as_ref()
                        .map(|failure| failure.text(ctx.lang))
                        .unwrap_or_default(),
                ),
            ],
        )));
    } else if summary.trashed > 0 {
        actions.push(Action::ShowNotice(fill_template(
            ctx.lang.text(Msg::StatusReviewsTrashed),
            &[("{count}", &summary.trashed.to_string())],
        )));
    } else if let Some(text) = accept_notice(summary, ctx.lang) {
        actions.push(Action::ShowNotice(text));
    }
    actions.push(Action::ReloadPanels);
}
