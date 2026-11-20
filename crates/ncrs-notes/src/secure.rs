//! Locking and unlocking the encrypted notes, encrypting and decrypting them,
//! and the recovery key. The key itself comes from the credential store on a
//! blocking thread; nothing here touches the disk but through the write queue.

use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::Task;
use ncrs_core::notes_sync::NotesSettings;
use ncrs_i18n::{fill_template, Msg};

use crate::action::Action;
use crate::crypt::is_encrypted_path;
use crate::ctx::Ctx;
use crate::message::{Message, Mode};
use crate::state::{Options, Seal, State, Write};
use crate::store::{self, Expect, Kind, Replaced};
use crate::syncing;
use crate::tree::RowKey;
use crate::update::{flush, reload, written};
use crate::vault::{obtain_key, restore_key};

/// Takes over what the settings say; the lock time applies from now on.
pub fn apply_settings(state: &mut State, settings: &NotesSettings) {
    state.options = Options {
        encrypt_all: settings.encrypt_all,
        hermes_access: settings.hermes_access,
        sync_folder: settings.sync_folder.clone(),
        direction: settings.direction,
        when: settings.when,
    };
    state.vault.set_timeout_minutes(settings.lock_minutes);
}

/// Starts fetching the key; the answer is [`Message::Unlocked`]. With `create`
/// a key is made when the credential store has none.
pub fn unlock(state: &mut State, create: bool) -> Task<Message> {
    if state.unlocking {
        return Task::none();
    }
    state.unlocking = true;
    let store = state.vault.store();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || obtain_key(store.as_ref(), create)).await },
        |joined| {
            Message::Unlocked(
                joined.unwrap_or_else(|err| Err(err.to_string())),
                Instant::now(),
            )
        },
    )
}

/// A new note in an encrypted folder needs the key first; it is fetched and
/// the person asks again.
pub fn unlock_for_new_note(state: &mut State, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let text = ctx.lang.text(Msg::NotesLockedNewNote).to_owned();
    (unlock(state, true), Some(Action::Notice(text)))
}

pub fn handle(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    match message {
        Message::Tick(now) => tick(state, now, ctx),
        Message::Unlock => (unlock(state, false), None),
        Message::Unlocked(result, now) => unlocked(state, result, now, ctx),
        Message::Lock => (lock(state), None),
        Message::OpenMenu(row) => {
            state.menu = Some(row);
            (Task::none(), None)
        }
        Message::CloseMenu => {
            state.menu = None;
            (Task::none(), None)
        }
        Message::Seal(seal, on) => seal_rows(state, seal, on, ctx),
        Message::RecoveryRequested => (recovery(state), None),
        Message::RecoveryReady(result) => (Task::none(), Some(recovery_ready(result, ctx))),
        Message::Restore(text) => (restore(state, text), None),
        Message::Restored(result, now) => restored(state, result, now, ctx),
        other => syncing::handle(state, other, ctx),
    }
}

fn tick(state: &mut State, now: Instant, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let synced = syncing::on_tick(state, now, ctx);
    if !state.vault.tick(now) {
        return (synced, None);
    }
    // The vault is locked already; what was typed is written first, and the
    // notes are read again without the key so no plain text stays in memory.
    (Task::batch([forget(state), synced]), None)
}

fn lock(state: &mut State) -> Task<Message> {
    if !state.vault.is_unlocked() {
        return Task::none();
    }
    // Written first: a note typed in is sealed with the key that is still held.
    let saved = flush(state);
    state.vault.lock();
    Task::batch([saved, forget(state)])
}

/// Writes what is pending with the key that is still held by the queued
/// writes, then reads the folder without the key.
fn forget(state: &mut State) -> Task<Message> {
    let saved = flush(state);
    state.menu = None;
    state.pending_seal = None;
    if state.shown_is_locked() {
        state.mode = Mode::View;
    }
    Task::batch([saved, reload(state)])
}

fn unlocked(
    state: &mut State,
    result: Result<Option<crate::crypt::NoteKey>, String>,
    now: Instant,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    state.unlocking = false;
    match result {
        Ok(Some(key)) => {
            state.vault.unlock(key, now);
            let read = reload(state);
            let sealing = match state.pending_seal.take() {
                Some((seal, on)) => seal_rows(state, seal, on, ctx),
                None => (Task::none(), None),
            };
            (Task::batch([read, sealing.0]), sealing.1)
        }
        Ok(None) => {
            state.pending_seal = None;
            let text = ctx.lang.text(Msg::NotesNoKey).to_owned();
            (Task::none(), Some(Action::Error(text)))
        }
        Err(reason) => {
            state.pending_seal = None;
            (Task::none(), Some(key_failed(ctx, &reason)))
        }
    }
}

fn key_failed(ctx: &Ctx, reason: &str) -> Action {
    Action::Error(fill_template(
        ctx.lang.text(Msg::NotesKeyFailed),
        &[("{reason}", reason)],
    ))
}

fn recovery(state: &State) -> Task<Message> {
    let store = state.vault.store();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                store
                    .load()
                    .map(|key| key.map(|found| found.recovery_text()))
            })
            .await
        },
        |joined| Message::RecoveryReady(joined.unwrap_or_else(|err| Err(err.to_string()))),
    )
}

fn recovery_ready(result: Result<Option<String>, String>, ctx: &Ctx) -> Action {
    match result {
        Ok(Some(text)) => Action::RecoveryKey(text),
        Ok(None) => Action::Error(ctx.lang.text(Msg::NotesNoKey).to_owned()),
        Err(reason) => key_failed(ctx, &reason),
    }
}

fn restore(state: &State, text: String) -> Task<Message> {
    let store = state.vault.store();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || restore_key(store.as_ref(), &text)).await },
        |joined| {
            Message::Restored(
                joined.unwrap_or_else(|err| Err(err.to_string())),
                Instant::now(),
            )
        },
    )
}

fn restored(
    state: &mut State,
    result: Result<crate::crypt::NoteKey, String>,
    now: Instant,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    match result {
        Ok(key) => {
            state.vault.unlock(key, now);
            let text = ctx.lang.text(Msg::NotesRestored).to_owned();
            (reload(state), Some(Action::Notice(text)))
        }
        Err(reason) => (Task::none(), Some(key_failed(ctx, &reason))),
    }
}

/// The notes of `seal` that are not in the wanted form yet.
fn to_convert(state: &State, seal: &Seal, on: bool) -> Vec<PathBuf> {
    let inside = |note: &store::Note| match seal {
        Seal::Everything => true,
        Seal::Row(RowKey::Note(path)) => &note.path == path,
        Seal::Row(RowKey::Folder(folder)) => note.folder.starts_with(folder),
        Seal::Row(RowKey::Files) => false,
    };
    state
        .notes
        .iter()
        .filter(|note| note.kind == Kind::Topic && inside(note))
        .filter(|note| is_encrypted_path(&note.path) != on)
        .map(|note| note.path.clone())
        .collect()
}

/// Encrypts (`on`) or decrypts the notes. Each becomes a write of the new file
/// that removes the old one, so it goes through the same queue as an edit.
fn seal_rows(
    state: &mut State,
    seal: Seal,
    on: bool,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    if !state.vault.is_unlocked() {
        if on {
            state.pending_seal = Some((seal, on));
            return (unlock(state, true), None);
        }
        let text = ctx.lang.text(Msg::NotesUnlockFirst).to_owned();
        return (unlock(state, false), Some(Action::Notice(text)));
    }
    let mut tasks = vec![flush(state)];
    for old in to_convert(state, &seal, on) {
        tasks.push(convert(state, &old, on));
    }
    state.menu = None;
    (Task::batch(tasks), None)
}

fn convert(state: &mut State, old: &Path, on: bool) -> Task<Message> {
    let Some(note) = state.note(old) else {
        return Task::none();
    };
    let new = if on {
        store::sealed_path(old)
    } else {
        store::unsealed_path(old)
    };
    if new != old && (store::is_taken(&new) || state.note(&new).is_some()) {
        return Task::none();
    }
    let (text, base) = (note.text.clone(), note.base);
    if let Some(moved) = state.note_mut(old) {
        moved.path = new.clone();
        moved.base = None;
    }
    if state.current.as_deref() == Some(old) {
        state.current = Some(new.clone());
    }
    if state.cursor == Some(RowKey::Note(old.to_owned())) {
        state.cursor = Some(RowKey::Note(new.clone()));
    }
    let write = state.numbered(Write {
        path: new,
        text,
        expect: Expect::Absent,
        replaces: Some(Replaced {
            path: old.to_owned(),
            base,
        }),
        generation: 0,
        root: state.root.clone(),
        background: true,
        key: None,
    });
    written(state, write)
}
