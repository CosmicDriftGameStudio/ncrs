use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_core::notes_sync::NotesSettings;
use ncrs_i18n::Language;

use crate::action::Action;
use crate::crypt::{decrypt, encrypt, NoteKey};
use crate::secure::{denied_text, verified_key, verified_recovery};
use crate::state::Seal;
use crate::store::{scan, scan_with};
use crate::tree::RowKey;
use crate::vault::{restore_key, KeyStore as _, MemoryKeyStore, Vault};
use crate::{apply_settings, flush_now, update, Ctx, Message, State};
use ncrs_user_verify::{Check, Denied, Scripted};

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

fn locked_state_with_sealed_note(root: &Path, key: &NoteKey) -> (State, Vec<u8>) {
    let bytes = encrypt(key, "secret plan").unwrap();
    fs::write(root.join("a.md.enc"), &bytes).unwrap();
    let mut state = State::new(root.to_owned());
    state.vault = Vault::with_store(Arc::new(MemoryKeyStore::default()));
    let _ = update(&mut state, Message::Loaded(Box::new(scan(root))), &CTX);
    (state, bytes)
}

#[tokio::test]
async fn decrypting_before_the_keyed_reload_arrived_leaves_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let key = NoteKey::generate().unwrap();
    let (mut state, bytes) = locked_state_with_sealed_note(dir.path(), &key);
    let _ = update(
        &mut state,
        Message::Unlocked(Ok(Some(key)), Instant::now()),
        &CTX,
    );
    let sealed = dir.path().join("a.md.enc");
    let seal = Seal::Row(RowKey::Note(sealed.clone()));
    let _ = update(&mut state, Message::Seal(seal, false), &CTX);
    assert_eq!(flush_now(&mut state, &CTX), Vec::<String>::new());
    assert_eq!(fs::read(&sealed).unwrap(), bytes);
    assert!(!dir.path().join("a.md").exists());
}

#[tokio::test]
async fn editing_before_the_keyed_reload_arrived_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let key = NoteKey::generate().unwrap();
    let (mut state, bytes) = locked_state_with_sealed_note(dir.path(), &key);
    let _ = update(
        &mut state,
        Message::Unlocked(Ok(Some(key)), Instant::now()),
        &CTX,
    );
    let sealed = dir.path().join("a.md.enc");
    state.current = Some(sealed.clone());
    let _ = update(&mut state, Message::SetMode(crate::Mode::Edit), &CTX);
    assert_ne!(state.mode, crate::Mode::Edit);
    assert_eq!(flush_now(&mut state, &CTX), Vec::<String>::new());
    assert_eq!(fs::read(&sealed).unwrap(), bytes);
}

fn stored_key_state(dir: &Path) -> (State, Arc<MemoryKeyStore>) {
    let store = Arc::new(MemoryKeyStore::default());
    store.store(&NoteKey::generate().unwrap()).unwrap();
    let mut state = state_with_note(dir, "plain");
    state.vault = Vault::with_store(Arc::<MemoryKeyStore>::clone(&store));
    (state, store)
}

#[test]
fn the_key_is_only_fetched_after_the_person_was_verified() {
    let dir = tempfile::tempdir().unwrap();
    let (state, _) = stored_key_state(dir.path());
    let verifier = Scripted::new(Check::TouchId);
    let fetched = verified_key(&verifier, state.vault.store().as_ref(), "unlock", false);
    assert!(matches!(fetched, Ok(Ok(Some(_)))));
    assert_eq!(verifier.asked(), ["unlock"]);
}

#[test]
fn a_cancelled_prompt_fetches_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let (state, store) = stored_key_state(dir.path());
    // A store that would fail shows the key is never asked for.
    store.failing(Some("must not be asked"));
    let verifier = Scripted::new(Check::TouchId).answering(Err(Denied::Cancelled));
    let fetched = verified_key(&verifier, state.vault.store().as_ref(), "unlock", false);
    assert_eq!(fetched, Err(Denied::Cancelled));
}

#[tokio::test]
async fn a_denied_unlock_leaves_the_vault_locked_and_says_why_on_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "plain");
    state.unlocking = true;
    let _ = update(&mut state, Message::UnlockDenied(Denied::Cancelled), &CTX);
    assert!(!state.vault.is_unlocked());
    assert!(!state.unlocking);
    assert_eq!(state.unlock_denied, Some(Denied::Cancelled));
    assert!(denied_text(&Denied::Cancelled, CTX.lang).contains("cancelled"));
    let failed = denied_text(&Denied::Failed("no match".to_owned()), CTX.lang);
    assert!(failed.contains("no match"));
}

#[tokio::test]
async fn a_later_unlock_clears_the_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "plain");
    state.unlock_denied = Some(Denied::Cancelled);
    unlock(&mut state, Instant::now());
    assert_eq!(state.unlock_denied, None);
}

#[test]
fn the_unlock_button_names_what_the_system_asks() {
    let touch = Vault::with_store(Arc::new(MemoryKeyStore::default()))
        .with_verifier(Arc::new(Scripted::new(Check::TouchId)));
    assert_eq!(touch.verifier().check(), Check::TouchId);
    assert_eq!(
        Vault::with_store(Arc::new(MemoryKeyStore::default()))
            .verifier()
            .check(),
        Check::Nothing
    );
}

#[test]
fn the_recovery_key_is_only_read_after_the_person_was_verified() {
    let dir = tempfile::tempdir().unwrap();
    let (state, _) = stored_key_state(dir.path());
    let verifier = Scripted::new(Check::TouchId);
    let shown = verified_recovery(&verifier, state.vault.store().as_ref(), "recovery");
    assert!(matches!(shown, Ok(Ok(Some(_)))));
    assert_eq!(verifier.asked(), ["recovery"]);
}

#[test]
fn a_refused_prompt_reads_no_recovery_key() {
    let dir = tempfile::tempdir().unwrap();
    let (state, store) = stored_key_state(dir.path());
    store.failing(Some("must not be asked"));
    let verifier = Scripted::new(Check::TouchId).answering(Err(Denied::Cancelled));
    let shown = verified_recovery(&verifier, state.vault.store().as_ref(), "recovery");
    assert_eq!(shown, Err(Denied::Cancelled));
}

#[tokio::test]
async fn a_refused_recovery_prompt_shows_the_reason_and_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "plain");
    let (_, action) = update(
        &mut state,
        Message::RecoveryDenied(Denied::Failed("no match".to_owned())),
        &CTX,
    );
    match action {
        Some(Action::Error(text)) => assert!(text.contains("no match"), "{text}"),
        other => panic!("expected the reason, got {other:?}"),
    }
}

#[tokio::test]
async fn the_lock_key_locks_an_open_vault_and_starts_the_unlock_of_a_locked_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "secret plan");
    unlock(&mut state, Instant::now());

    let _ = update(&mut state, Message::ToggleLock, &CTX);
    assert!(!state.vault.is_unlocked());
    assert!(!state.unlocking);

    let _ = update(&mut state, Message::ToggleLock, &CTX);
    assert!(state.unlocking);
}

#[tokio::test]
async fn enter_on_the_lock_page_starts_the_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_with_note(dir.path(), "secret plan");
    unlock(&mut state, Instant::now());
    let plain = dir.path().join("a.md");
    seal_note(&mut state, &plain);
    let sealed = RowKey::Note(dir.path().join("a.md.enc"));
    let _ = update(&mut state, Message::Pressed(sealed), &CTX);
    let _ = update(&mut state, Message::Lock, &CTX);
    assert!(state.shown_is_locked());
    assert!(!state.unlocking);

    let _ = update(&mut state, Message::Enter, &CTX);
    assert!(state.unlocking);
}
