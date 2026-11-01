use iced::widget::operation;
use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::replan_work::ReplanScope;
use ncrs_filing::review_work::ReviewAccess;
use ncrs_ui_kit::focus::release_focus;

use super::blocking_overlay_is_open;
use super::rules::open_rule_form;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::document_preview;
use crate::filing_pieces::TargetEdit;
use crate::message::{HermesPage, Message, ReviewListMessage};
use crate::review_list::ReviewList;
use crate::rule_form::RuleForm;
use crate::state::Hermes;
use crate::ui::review_list as review_list_ui;

pub fn update(
    hermes: &mut Hermes,
    message: ReviewListMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        ReviewListMessage::Open => open(hermes, ctx, actions),
        ReviewListMessage::Close => close(hermes),
        ReviewListMessage::ReadAgainWithAi => read_again_with_ai(hermes, actions),
        ReviewListMessage::Loaded { generation, result } => {
            if generation != hermes.review_list_generation {
                return Task::none();
            }
            let Some(view) = hermes.review_list.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(rows) => view.loaded(rows),
                Err(detail) => view.failed(detail),
            }
            view.take_focus_request().map_or_else(Task::none, |field| {
                operation::focus(review_list_ui::field_id(field))
            })
        }
        ReviewListMessage::Move(delta) => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.move_cursor(delta);
            }
            Task::none()
        }
        ReviewListMessage::Scrolled(delta) => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.scroll(delta);
            }
            Task::none()
        }
        ReviewListMessage::Select(index) => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.select(index);
            }
            Task::none()
        }
        ReviewListMessage::Edit => {
            let Some(view) = hermes.review_list.as_mut() else {
                return Task::none();
            };
            view.start_edit();
            if view.is_editing() {
                operation::focus(review_list_ui::FIRST_FIELD_ID)
            } else {
                Task::none()
            }
        }
        ReviewListMessage::FieldChanged(field, text) => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.set_text(field, text);
            }
            Task::none()
        }
        ReviewListMessage::TogglePreview => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.toggle_preview();
            }
            Task::none()
        }
        ReviewListMessage::OpenTagInput => {
            let Some(view) = hermes.review_list.as_mut() else {
                return Task::none();
            };
            view.open_tag_input();
            if view.is_tag_input_open() {
                operation::focus(review_list_ui::TAG_INPUT_ID)
            } else {
                Task::none()
            }
        }
        ReviewListMessage::TargetEdited(edit) => {
            let starts_own_name = edit == TargetEdit::StartOwnName;
            if let Some(view) = hermes.review_list.as_mut() {
                view.edit_target(edit);
            }
            if starts_own_name {
                operation::focus(review_list_ui::FOLDER_NAME_FIELD_ID)
            } else {
                Task::none()
            }
        }
        ReviewListMessage::AddTag => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.add_tag();
            }
            Task::none()
        }
        ReviewListMessage::RemoveTag(tag) => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.remove_tag(&tag);
            }
            Task::none()
        }
        ReviewListMessage::FocusField { backwards } => {
            if backwards {
                operation::focus_previous()
            } else {
                operation::focus_next()
            }
        }
        ReviewListMessage::Confirm => {
            let Some(request) = hermes
                .review_list
                .as_mut()
                .and_then(ReviewList::begin_confirm)
            else {
                return Task::none();
            };
            let generation = hermes.review_list_generation;
            let Some(access) = review_access(hermes, ctx) else {
                return Task::none();
            };
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    access.confirm(
                        request.entry,
                        request.document_id,
                        &request.fields,
                        &request.pattern,
                    )
                }),
                move |joined| {
                    let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                    Message::ReviewList(ReviewListMessage::Confirmed { generation, result })
                },
            )
        }
        ReviewListMessage::Confirmed { generation, result } => {
            if generation != hermes.review_list_generation {
                return Task::none();
            }
            let Some(view) = hermes.review_list.as_mut() else {
                return Task::none();
            };
            match result {
                Ok(outcome) => view.confirmed(outcome),
                Err(detail) => {
                    view.failed(detail);
                    return Task::none();
                }
            }
            run_review_load(hermes, ctx)
        }
        ReviewListMessage::CreateRule => {
            let Some(prefill) = hermes.review_list.as_ref().and_then(ReviewList::rule_offer) else {
                return Task::none();
            };
            open_rule_form(
                hermes,
                RuleForm::from_document(prefill, ctx.kinds),
                Some(HermesPage::Review),
                ctx,
                actions,
            )
        }
        ReviewListMessage::ReplanOpenCases => {
            start_replan(hermes, ReplanScope::OpenCases, ctx, actions)
        }
        ReviewListMessage::Previewed {
            generation,
            serial,
            result,
        } => {
            if generation != hermes.review_list_generation {
                return Task::none();
            }
            if let Some(view) = hermes.review_list.as_mut() {
                match result {
                    Ok(items) => view.previewed(serial, items),
                    Err(detail) => view.failed(detail),
                }
            }
            Task::none()
        }
        ReviewListMessage::ReplanConfirm => {
            if hermes.pending_refile.is_some() {
                return Task::none();
            }
            let Some((scope, items)) = hermes
                .review_list
                .as_mut()
                .and_then(ReviewList::confirm_replan)
            else {
                return Task::none();
            };
            super::refile::confirm_replan(hermes, scope, items, ctx, actions);
            Task::none()
        }
        ReviewListMessage::ReplanCancel => {
            if let Some(view) = hermes.review_list.as_mut() {
                view.cancel_replan();
            }
            Task::none()
        }
        ReviewListMessage::PreviewLoaded {
            generation,
            serial,
            content,
        } => {
            if generation == hermes.review_list_generation {
                if let Some(view) = hermes.review_list.as_mut() {
                    view.preview_loaded(serial, content);
                }
            }
            Task::none()
        }
    }
}

/// Makes the preview of the document under the cursor, on a blocking thread,
/// when the cursor has moved to another one.
pub fn request_review_preview(hermes: &mut Hermes) -> Task<Message> {
    let generation = hermes.review_list_generation;
    let Some(request) = hermes
        .review_list
        .as_mut()
        .and_then(ReviewList::preview_request)
    else {
        return Task::none();
    };
    let serial = request.serial;
    Task::perform(
        document_preview::load_off_thread(request.path, request.passages),
        move |content| {
            Message::ReviewList(ReviewListMessage::PreviewLoaded {
                generation,
                serial,
                content,
            })
        },
    )
}

/// Opens the review page if it is not up and plans the documents of `scope`
/// again there, on a blocking thread. It only reads; the user sees the result
/// before anything is filed.
pub fn start_replan(
    hermes: &mut Hermes,
    scope: ReplanScope,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(access) = ctx.filing.and_then(FilingModule::replan_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    let opened = open(hermes, ctx, actions);
    let generation = hermes.review_list_generation;
    let Some(serial) = hermes
        .review_list
        .as_mut()
        .and_then(|view| view.begin_replan(scope.clone()))
    else {
        return opened;
    };
    let preview = Task::perform(
        tokio::task::spawn_blocking(move || access.preview(&scope)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::ReviewList(ReviewListMessage::Previewed {
                generation,
                serial,
                result,
            })
        },
    );
    Task::batch([opened, preview])
}

fn review_access(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Option<ReviewAccess> {
    let access = ctx.filing.and_then(FilingModule::review_access);
    if access.is_none() {
        if let Some(view) = hermes.review_list.as_mut() {
            view.failed(INDEX_NOT_OPEN.to_owned());
        }
    }
    access
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.review_list.is_some() || blocking_overlay_is_open(hermes, ctx) {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.review_list_generation = hermes.review_list_generation.wrapping_add(1);
    hermes.review_list = Some(
        ReviewList::new()
            .with_default_pattern(ctx.default_pattern.map(str::to_owned))
            .with_kinds(ctx.kinds.clone()),
    );
    if ctx.hermes_counts_missing {
        actions.push(Action::RequestHermesCounts);
    }
    run_review_load(hermes, ctx)
}

/// Hands every open review to Hermes again, now that the AI can read it. Only
/// on the click: nothing is re-read behind the user's back.
fn read_again_with_ai(hermes: &Hermes, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(paths) = hermes.review_list.as_ref().map(|view| {
        view.rows()
            .iter()
            .map(|row| row.path.clone())
            .collect::<Vec<_>>()
    }) else {
        return Task::none();
    };
    actions.push(Action::ReadPathsAgain(paths));
    Task::none()
}

/// Escape leaves the form first, the list second.
fn close(hermes: &mut Hermes) -> Task<Message> {
    match hermes.review_list.as_mut() {
        Some(view) if view.is_editing() => {
            view.cancel_edit();
            release_focus()
        }
        Some(view) if view.replan().is_some() => {
            view.cancel_replan();
            Task::none()
        }
        Some(_) => {
            hermes.review_list = None;
            release_focus()
        }
        None => Task::none(),
    }
}

/// Loads on a blocking thread: the load plans every open entry again, and the
/// index is shared with the intake.
pub fn run_review_load(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let generation = hermes.review_list_generation;
    let Some(access) = review_access(hermes, ctx) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.load()),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::ReviewList(ReviewListMessage::Loaded { generation, result })
        },
    )
}
