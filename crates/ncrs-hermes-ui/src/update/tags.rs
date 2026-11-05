use iced::widget::operation;
use iced::Task;
use ncrs_config::file_list_tags::TagGroup;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::tags_work::{TagsAccess, TagsFailure, TagsLoaded};
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, TagsMessage};
use crate::state::Hermes;
use crate::tags_page::TagsPage;
use crate::ui::tags as tags_ui;

pub fn update(
    hermes: &mut Hermes,
    message: TagsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        TagsMessage::CancelStep => close_dialog(hermes),
        TagsMessage::Done { generation, result } => {
            if generation != hermes.tags_generation {
                return Task::none();
            }
            match result {
                Ok(loaded) => {
                    actions.push(Action::SetTagExceptions(loaded.exceptions.clone()));
                    if let Some(page) = hermes.tags.as_mut() {
                        page.loaded(loaded);
                    }
                }
                Err(failure) => {
                    if let Some(page) = hermes.tags.as_mut() {
                        page.failed(failure);
                    }
                }
            }
            Task::none()
        }
        TagsMessage::ExceptionsRead(result) => {
            if let Ok(exceptions) = result {
                actions.push(Action::SetTagExceptions(exceptions));
            }
            Task::none()
        }
        TagsMessage::ToggleOpen(group) => {
            if let Some(page) = hermes.tags.as_mut() {
                page.toggle_open(group);
            }
            Task::none()
        }
        TagsMessage::Unfold(group) => {
            if let Some(page) = hermes.tags.as_mut() {
                page.unfold(group);
            }
            Task::none()
        }
        TagsMessage::ToggleTag { group, tag } => {
            let Some(page) = hermes.tags.as_mut().filter(|page| !page.is_busy()) else {
                return Task::none();
            };
            let shown = page.exception_after_toggle(ctx.file_list_tags, group, &tag);
            page.begin_change();
            run_tags_work(hermes, move |access| access.set_shown(&tag, shown))
        }
        TagsMessage::ToggleGroup(group) => toggle_tag_group(hermes, ctx, group, actions),
        TagsMessage::SearchChanged(text) => {
            if let Some(page) = hermes.tags.as_mut() {
                page.set_search(text);
            }
            Task::none()
        }
        TagsMessage::StartRename(tag) => {
            let Some(page) = hermes.tags.as_mut() else {
                return Task::none();
            };
            page.start_rename(tag);
            if page.is_editing() {
                operation::focus(tags_ui::RENAME_ID)
            } else {
                Task::none()
            }
        }
        TagsMessage::RenameChanged(text) => {
            if let Some(page) = hermes.tags.as_mut() {
                page.set_rename_text(text);
            }
            Task::none()
        }
        TagsMessage::ConfirmRename => {
            let Some((old_name, new_name)) = hermes.tags.as_mut().and_then(TagsPage::begin_rename)
            else {
                return Task::none();
            };
            run_tags_work(hermes, move |access| access.rename(&old_name, &new_name))
        }
        TagsMessage::AskDelete(tag) => {
            if let Some(page) = hermes.tags.as_mut() {
                page.ask_delete(tag);
            }
            Task::none()
        }
        TagsMessage::ConfirmDelete => {
            let Some(page) = hermes.tags.as_mut() else {
                return Task::none();
            };
            let Some(name) = page.begin_delete() else {
                return Task::none();
            };
            run_tags_work(hermes, move |access| access.delete(&name))
        }
    }
}

/// The default of the group follows the tick; the tags of the group that
/// differed from it follow the group again.
fn toggle_tag_group(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    group: TagGroup,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(page) = hermes.tags.as_mut().filter(|page| !page.is_busy()) else {
        return Task::none();
    };
    let on = page.group_will_be_on(ctx.file_list_tags, group);
    let exceptional = page.exceptional_tags(group);
    let switched = ctx.file_list_tags.with_group(group, on);
    actions.push(Action::ChangeFileListTags(switched));
    if exceptional.is_empty() {
        return Task::none();
    }
    if let Some(open) = hermes.tags.as_mut() {
        open.begin_change();
    }
    run_tags_work(hermes, move |access| access.clear_exceptions(&exceptional))
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.tags.is_some() {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.tags_generation = hermes.tags_generation.wrapping_add(1);
    hermes.tags = Some(TagsPage::new(ctx.kinds.clone()));
    run_tags_work(hermes, TagsAccess::load)
}

/// Closes the dialog that is up: the rename or the delete question.
pub fn close_dialog(hermes: &mut Hermes) -> Task<Message> {
    if let Some(page) = hermes.tags.as_mut() {
        page.cancel_step();
    }
    release_focus()
}

fn run_tags_work(
    hermes: &mut Hermes,
    work: impl FnOnce(&TagsAccess) -> Result<TagsLoaded, TagsFailure> + Send + 'static,
) -> Task<Message> {
    let generation = hermes.tags_generation;
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::tags_access) else {
        if let Some(page) = hermes.tags.as_mut() {
            page.failed(TagsFailure::Other(INDEX_NOT_OPEN.to_owned()));
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || work(&access)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(TagsFailure::Other(join.to_string())));
            Message::Tags(TagsMessage::Done { generation, result })
        },
    )
}
