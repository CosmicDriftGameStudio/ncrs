use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_core::notes_sync::NotesSettings;
use ncrs_i18n::Language;

use crate::crypt::{decrypt, encrypt, NoteKey};
use crate::state::Seal;
use crate::store::{scan, scan_with};
use crate::tree::RowKey;
use crate::vault::{restore_key, KeyStore as _, MemoryKeyStore, Vault};
use crate::{apply_settings, flush_now, update, Ctx, Message, State};

const CTX: Ctx = Ctx {
    lang: Language::English,
};

fn state_with_note(root: &Path, text: &str) -> State {
    fs::write(root.join("a.md"), text).unwrap();
    let mut state = State::new(root.to_owned());
    state.vault = Vault::with_store(Arc::new(MemoryKeyStore::default()));
    let _ = update(&mut state, Message::Loaded(Box::new(scan(root))), &CTX);
    state
}

fn unlock(state: &mut State, at: Instant) -> NoteKey {
    let key = NoteKey::generate().unwrap();
    let _ = update(state, Message::Unlocked(Ok(Some(key.clone())), at), &CTX);
    key
}

fn seal_note(state: &mut State, path: &Path) {
    let seal = Seal::Row(RowKey::Note(path.to_owned()));
    let _ = update(state, Message::Seal(seal, true), &CTX);
    assert_eq!(flush_now(state, &CTX), Vec::<String>::new());
}

#[test]
fn a_note_comes_back_with_the_same_key_and_not_with_another() {
    let key = NoteKey::generate().unwrap();
    let bytes = encrypt(&key, "plan: öl & äpfel").unwrap();
    assert_eq!(decrypt(&key, &bytes).unwrap(), "plan: öl & äpfel");
    let other = NoteKey::generate().unwrap();
    decrypt(&other, &bytes).unwrap_err();
    decrypt(&key, b"not a note").unwrap_err();
    let mut damaged = bytes;
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    decrypt(&key, &damaged).unwrap_err();
}

#[test]
fn the_recovery_key_opens_the_notes_on_a_computer_without_a_key() {
    let key = NoteKey::generate().unwrap();
    let bytes = encrypt(&key, "kept").unwrap();
    let elsewhere = MemoryKeyStore::default();
    let restored = restore_key(&elsewhere, &key.recovery_text()).unwrap();
    assert_eq!(decrypt(&restored, &bytes).unwrap(), "kept");
    assert!(elsewhere.load().unwrap().is_some());
    restore_key(&elsewhere, "not a key").unwrap_err();
    let another = NoteKey::generate().unwrap();
    restore_key(&elsewhere, &another.recovery_text()).unwrap_err();
}

#[tokio::test]
async fn encrypting_a_note_leaves_no_plain_text_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "secret plan");
    unlock(&mut state, Instant::now());
    let plain = dir.path().join("a.md");
    seal_note(&mut state, &plain);
    let sealed = dir.path().join("a.md.enc");
    assert!(!plain.exists());
    let bytes = fs::read(&sealed).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("secret plan"));
    assert_eq!(
        state.notes.first().map(|note| note.path.clone()),
        Some(sealed)
    );
}

#[tokio::test]
async fn decrypting_a_note_brings_the_plain_file_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "secret plan");
    unlock(&mut state, Instant::now());
    let plain = dir.path().join("a.md");
    seal_note(&mut state, &plain);
    let sealed = dir.path().join("a.md.enc");
    let seal = Seal::Row(RowKey::Note(sealed.clone()));
    let _ = update(&mut state, Message::Seal(seal, false), &CTX);
    assert_eq!(flush_now(&mut state, &CTX), Vec::<String>::new());
    assert!(!sealed.exists());
    assert_eq!(fs::read_to_string(plain).unwrap(), "secret plan");
}

#[tokio::test]
async fn locked_notes_stay_out_of_the_search_until_the_key_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "secret plan");
    let key = unlock(&mut state, Instant::now());
    seal_note(&mut state, &dir.path().join("a.md"));
    let _ = update(&mut state, Message::Lock, &CTX);
    assert!(!state.vault.is_unlocked());

    state.apply_scan(scan(dir.path()).unwrap());
    // Even a note that holds its text in memory is not searched while locked.
    if let Some(note) = state.notes.first_mut() {
        note.text = "secret plan".to_owned();
    }
    let _ = update(
        &mut state,
        Message::SearchChanged("secret".to_owned()),
        &CTX,
    );
    assert!(state.hits.is_empty());

    let _ = update(
        &mut state,
        Message::Unlocked(Ok(Some(key.clone())), Instant::now()),
        &CTX,
    );
    state.apply_scan(scan_with(dir.path(), Some(&key)).unwrap());
    let _ = update(
        &mut state,
        Message::SearchChanged("secret".to_owned()),
        &CTX,
    );
    assert_eq!(state.hits.len(), 1);
}

#[test]
fn the_key_leaves_memory_after_the_pause_without_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "x");
    let start = Instant::now();
    unlock(&mut state, start);
    let _ = update(
        &mut state,
        Message::Tick(start + Duration::from_secs(14 * 60)),
        &CTX,
    );
    assert!(state.vault.is_unlocked());
    let _ = update(
        &mut state,
        Message::Tick(start + Duration::from_secs(16 * 60)),
        &CTX,
    );
    assert!(!state.vault.is_unlocked());
}

#[test]
fn input_keeps_the_key_and_the_setting_changes_the_pause() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "x");
    let start = Instant::now();
    unlock(&mut state, start);
    let _ = update(&mut state, Message::CloseMenu, &CTX);
    let _ = update(
        &mut state,
        Message::Tick(start + Duration::from_secs(10 * 60)),
        &CTX,
    );
    let _ = update(
        &mut state,
        Message::Tick(start + Duration::from_secs(20 * 60)),
        &CTX,
    );
    assert!(
        state.vault.is_unlocked(),
        "input at minute 10 restarts the pause"
    );

    apply_settings(
        &mut state,
        &NotesSettings {
            lock_minutes: 5,
            ..NotesSettings::default()
        },
    );
    let _ = update(
        &mut state,
        Message::Tick(start + Duration::from_secs(26 * 60)),
        &CTX,
    );
    assert!(!state.vault.is_unlocked());
}

#[test]
fn a_wrong_key_leaves_the_note_out_instead_of_breaking() {
    let dir = tempfile::tempdir().unwrap();
    let key = NoteKey::generate().unwrap();
    fs::write(dir.path().join("a.md.enc"), encrypt(&key, "mine").unwrap()).unwrap();
    let other = NoteKey::generate().unwrap();
    let unreadable = scan_with(dir.path(), Some(&other)).unwrap();
    assert!(unreadable.notes.is_empty());
    let read = scan_with(dir.path(), Some(&key)).unwrap();
    assert_eq!(
        read.notes.first().map(|note| note.text.as_str()),
        Some("mine")
    );
}
