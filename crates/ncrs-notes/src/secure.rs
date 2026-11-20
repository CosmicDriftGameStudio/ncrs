//! Locking and unlocking the encrypted notes, encrypting and decrypting them,
//! and the recovery key. The key itself comes from the credential store on a
//! blocking thread; nothing here touches the disk but through the write queue.

use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::Task;
use ncrs_core::notes_sync::NotesSettings;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_user_verify::{Denied, UserVerifier};

use crate::action::Action;
use crate::crypt::{is_encrypted_path, NoteKey};
use crate::ctx::Ctx;
use crate::message::{Message, Mode};
use crate::state::{Options, Seal, State, Write};
use crate::store::{self, Expect, Kind, Replaced};
use crate::syncing;
use crate::tree::RowKey;
use crate::update::{flush, reload, written};
use crate::vault::{obtain_key, restore_key, KeyStore};

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
pub fn unlock(state: &mut State, create: bool, ctx: &Ctx) -> Task<Message> {
    if state.unlocking {
        return Task::none();
    }
    state.unlocking = true;
    state.unlock_denied = None;
    let store = state.vault.store();
    let verifier = state.vault.verifier();
    let reason = ctx.lang.text(Msg::NotesVerifyReason).to_owned();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                verified_key(verifier.as_ref(), store.as_ref(), &reason, create)
            })
            .await
        },
        |joined| match joined {
            Ok(Ok(result)) => Message::Unlocked(result, Instant::now()),
            Ok(Err(denied)) => Message::UnlockDenied(denied),
            Err(err) => Message::Unlocked(Err(err.to_string()), Instant::now()),
        },
    )
}

/// The key, fetched only after the person was verified. Blocking.
pub fn verified_key(
    verifier: &dyn UserVerifier,
    store: &dyn KeyStore,
    reason: &str,
    create: bool,
) -> Result<Result<Option<NoteKey>, String>, Denied> {
    verifier.verify(reason)?;
    Ok(obtain_key(store, create))
}

/// A new note in an encrypted folder needs the key first; it is fetched and
/// the person asks again.
pub fn unlock_for_new_note(state: &mut State, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let text = ctx.lang.text(Msg::NotesLockedNewNote).to_owned();
    (unlock(state, true, ctx), Some(Action::Notice(text)))
}

pub fn handle(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    match message {
        Message::Tick(now) => tick(state, now, ctx),
        Message::Unlock => (unlock(state, false, ctx), None),
        Message::UnlockDenied(denied) => unlock_denied(state, denied, ctx),
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
        Message::RecoveryRequested => (recovery(state, ctx), None),
        Message::RecoveryDenied(denied) => (
            Task::none(),
            Some(Action::Error(denied_text(&denied, ctx.lang))),
        ),
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
            state.unlock_denied = None;
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

/// The person was not verified: the vault stays locked, the locked page says
/// why, and anything that was waiting for the key is dropped. When no locked
/// page is on screen, the notice carries the same words.
fn unlock_denied(state: &mut State, denied: Denied, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    state.unlocking = false;
    state.pending_seal = None;
    let text = denied_text(&denied, ctx.lang);
    state.unlock_denied = Some(denied);
    let notice = (!state.shown_is_locked()).then_some(Action::Notice(text));
    (Task::none(), notice)
}

/// What the person reads when the verification did not pass.
pub fn denied_text(denied: &Denied, lang: Language) -> String {
    match denied {
        Denied::Cancelled => lang.text(Msg::NotesUnlockCancelled).to_owned(),
        Denied::Failed(reason) => {
            fill_template(lang.text(Msg::NotesUnlockFailed), &[("{reason}", reason)])
        }
    }
}

fn key_failed(ctx: &Ctx, reason: &str) -> Action {
    Action::Error(fill_template(
        ctx.lang.text(Msg::NotesKeyFailed),
        &[("{reason}", reason)],
    ))
}

/// The recovery key, read from the credential store only after the person was
/// verified. Blocking.
pub fn verified_recovery(
    verifier: &dyn UserVerifier,
    store: &dyn KeyStore,
    reason: &str,
) -> Result<Result<Option<String>, String>, Denied> {
    verifier.verify(reason)?;
    Ok(store
        .load()
        .map(|key| key.map(|found| found.recovery_text())))
}

fn recovery(state: &State, ctx: &Ctx) -> Task<Message> {
    let store = state.vault.store();
    let verifier = state.vault.verifier();
    let reason = ctx.lang.text(Msg::NotesVerifyRecoveryReason).to_owned();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                verified_recovery(verifier.as_ref(), store.as_ref(), &reason)
            })
            .await
        },
        |joined| match joined {
            Ok(Ok(result)) => Message::RecoveryReady(result),
            Ok(Err(denied)) => Message::RecoveryDenied(denied),
            Err(err) => Message::RecoveryReady(Err(err.to_string())),
        },
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
        .filter(|note| note.kind == Kind::Topic && !note.unread && inside(note))
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
            return (unlock(state, true, ctx), None);
        }
        let text = ctx.lang.text(Msg::NotesUnlockFirst).to_owned();
        return (unlock(state, false, ctx), Some(Action::Notice(text)));
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
