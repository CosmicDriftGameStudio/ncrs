//! The Notes page: the sync folder as typed, the recovery key, and what is
//! asked of the notes tab.

use std::path::PathBuf;

use iced::Task;

use super::page::in_settings;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, NotesEdit, SettingChange};
use crate::state::Settings;

pub fn update(
    settings: &mut Settings,
    edit: NotesEdit,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match edit {
        NotesEdit::SyncFolderText(text) => in_settings(settings, ctx, |state| {
            state.notes.sync_folder_draft = Some(text);
            None
        }),
        NotesEdit::SyncFolderSubmit => in_settings(settings, ctx, |state| {
            let typed = state.notes.sync_folder_draft.take()?;
            let mut next = state.config()?.notes.settings();
            let typed = typed.trim();
            next.sync_folder = (!typed.is_empty()).then(|| PathBuf::from(typed));
            Some(SettingChange::Notes(next))
        }),
        NotesEdit::ShowRecovery => {
            actions.push(Action::NotesRecovery);
            Task::none()
        }
        NotesEdit::HideRecovery => in_settings(settings, ctx, |state| {
            state.notes.recovery = None;
            None
        }),
        NotesEdit::RestoreText(text) => in_settings(settings, ctx, |state| {
            state.notes.restore_draft = text;
            None
        }),
        NotesEdit::RestoreSubmit => {
            if let Some(state) = settings.page.as_mut() {
                let typed = std::mem::take(&mut state.notes.restore_draft);
                if !typed.trim().is_empty() {
                    actions.push(Action::NotesRestore(typed));
                }
            }
            Task::none()
        }
        NotesEdit::SyncNow => {
            actions.push(Action::NotesSyncNow);
            Task::none()
        }
    }
}
