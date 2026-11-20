//! 1.6.0 acceptance, the notes (#296: sync, encryption, lock, recovery key)
//! and the Hermes chat list (#297). The notes never touch the system
//! keychain: their key lives in a store of the test. Hermes answers from the
//! fixed sentences of the chat, no model is called.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_hermes_chat::{ListMessage, Message as ChatMessage};
use ncrs_notes::tree::RowKey;
use ncrs_notes::vault::MemoryKeyStore;
use ncrs_notes::Seal;
use ncrs_settings::message::{NotesEdit, SettingChange, SettingsMessage};

use super::acceptance_v160::{pump_until, shoot, Live, World};
use super::Session;
use crate::app::App;
use crate::hermes_protocol::real_start::{boot_with, empty_store, pump_for};
use crate::hermes_protocol::thread::{busy, is_ready};
use crate::messages::{Message, SettingsFeature};

const SECRET_TEXT: &str = "Krause Haustechnik, Telefon 089 1234567";

fn write_notes(root: &Path) {
    let note = |path: &str, text: &str| super::write(&root.join(path), text);
    note(
        "Haus/Heizung Wartung.md",
        &format!(
            "# Heizung Wartung\n\nWartung jeden Oktober, {SECRET_TEXT}.\n\n## Offen\n\n- Termin 2026 buchen\n- Rechnung 2025 abgelegt\n"
        ),
    );
    for (folder, count) in [
        ("Haus", 8),
        ("Steuern", 9),
        ("Reisen", 9),
        ("Arbeit", 9),
        ("Garten", 9),
    ] {
        for n in 1..=count {
            note(
                &format!("{folder}/{folder} Notiz {n:02}.md"),
                &format!("# {folder} Notiz {n:02}\n\nGedanken zu {folder}, Punkt {n}.\n"),
            );
        }
    }
    for name in ["Ideen", "Reisen", "Telefonnummern", "Todo", "Wochenplan"] {
        note(
            &format!("{name}.md"),
            &format!("# {name}\n\nKurz notiert.\n"),
        );
    }
}

fn open_notes_page<P: Live>(session: &mut Session<P>) {
    session.send(Message::Settings(SettingsMessage::Toggle));
    session.send(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Notes,
    )));
}

fn files_below(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return found;
    };
    for entry in entries.map(|entry| entry.unwrap().path()) {
        if entry.is_dir() {
            found.extend(files_below(&entry));
        } else {
            found.push(entry);
        }
    }
    found.sort();
    found
}

fn press_note<P: Live>(session: &mut Session<P>, key: RowKey) {
    session.send(Message::Notes(ncrs_notes::Message::Pressed(key)));
}

#[test]
fn notes_sync_encryption_lock_and_recovery_key() {
    let world = World::new("notes160", "");
    write_notes(&world.notes);
    let sync = world.root.join("Sync-Ordner");
    let mut session = world.boot();
    let store = Arc::new(MemoryKeyStore::default());
    session.app.use_notes_key_store_for_test(
        Arc::clone(&store) as Arc<dyn ncrs_notes::vault::KeyStore>,
        15,
    );

    // Settings > Notizen, a sync folder, one way, and "Jetzt synchronisieren".
    open_notes_page(&mut session);
    shoot(&session.app, "27-einstellungen-notizen");
    session.send(Message::Settings(SettingsMessage::Notes(
        NotesEdit::SyncFolderText(sync.to_string_lossy().into_owned()),
    )));
    session.send(Message::Settings(SettingsMessage::Notes(
        NotesEdit::SyncFolderSubmit,
    )));
    let config = std::fs::read_to_string(world.data.path().join("config.toml")).unwrap();
    assert!(config.contains("sync_folder"), "{config}");
    assert!(config.contains(sync.to_string_lossy().as_ref()), "{config}");
    let options = session.app.notes_for_test().options.clone();
    assert_eq!(options.sync_folder.as_deref(), Some(sync.as_path()));
    assert_eq!(
        options.direction,
        ncrs_core::notes_sync::NotesSyncDirection::OneWay
    );

    session.send(Message::Settings(SettingsMessage::Notes(
        NotesEdit::SyncNow,
    )));
    session.settle();
    let notes_on_disk: Vec<PathBuf> = files_below(&world.notes)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert!(notes_on_disk.len() >= 50, "{} notes", notes_on_disk.len());
    for note in &notes_on_disk {
        let copy = sync.join(note.strip_prefix(&world.notes).unwrap());
        assert_eq!(
            std::fs::read(&copy).unwrap_or_default(),
            std::fs::read(note).unwrap(),
            "{} was copied",
            copy.display()
        );
    }
    assert!(session.app.notes_for_test().sync.last.is_some());
    shoot(&session.app, "28-notizen-sync-fertig");
    session.send(Message::Settings(SettingsMessage::Close));

    // Encrypt a note through its context menu.
    let plain = world.notes.join("Haus/Heizung Wartung.md");
    let sealed = world.notes.join("Haus/Heizung Wartung.md.enc");
    session.send(Message::OpenNotes);
    assert!(session.app.notes_are_shown_for_test());
    press_note(&mut session, RowKey::Folder(PathBuf::from("Haus")));
    press_note(&mut session, RowKey::Note(plain.clone()));
    session.send(Message::Notes(ncrs_notes::Message::OpenMenu(RowKey::Note(
        plain.clone(),
    ))));
    assert!(
        session.app.notes_for_test().menu.is_some(),
        "the menu opens for a note that is in the expanded tree"
    );
    shoot(&session.app, "29-notizen-kontextmenue");
    session.send(Message::Notes(ncrs_notes::Message::Seal(
        Seal::Row(RowKey::Note(plain.clone())),
        true,
    )));
    session.settle();
    assert!(sealed.is_file(), "the .md.enc file exists");
    assert!(!plain.exists(), "the plain file is gone");
    let bytes = std::fs::read(&sealed).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains("Krause")
            && !String::from_utf8_lossy(&bytes).contains("Heizung"),
        "no plain text in the encrypted file"
    );
    assert!(session.app.notes_for_test().vault.is_unlocked());
    press_note(&mut session, RowKey::Note(sealed.clone()));
    shoot(&session.app, "30-notizen-verschluesselt-schloss");

    // Lock: the note is a lock row, the page asks for the key.
    session.send(Message::Notes(ncrs_notes::Message::Lock));
    session.settle();
    assert!(!session.app.notes_for_test().vault.is_unlocked());
    press_note(&mut session, RowKey::Note(sealed.clone()));
    assert!(session.app.notes_for_test().shown_is_locked());
    shoot(&session.app, "31-notizen-entsperren-seite");

    // Unlock through the injected store.
    session.send(Message::Notes(ncrs_notes::Message::Unlock));
    session.settle();
    assert!(session.app.notes_for_test().vault.is_unlocked());
    press_note(&mut session, RowKey::Note(sealed.clone()));
    assert!(
        session
            .app
            .notes_for_test()
            .shown()
            .is_some_and(|note| note.text.contains("Krause")),
        "the note reads again with the key"
    );

    // Auto-lock after the timeout, by the clock of the test.
    let now = Instant::now();
    session.send(Message::Notes(ncrs_notes::Message::Tick(
        now + Duration::from_secs(60),
    )));
    assert!(
        session.app.notes_for_test().vault.is_unlocked(),
        "one minute is inside the 15"
    );
    session.send(Message::Notes(ncrs_notes::Message::Tick(
        now + Duration::from_secs(60 * 17),
    )));
    session.settle();
    assert!(
        !session.app.notes_for_test().vault.is_unlocked(),
        "locked after the timeout"
    );
    assert!(!session
        .app
        .notes_for_test()
        .shown()
        .is_some_and(|note| note.text.contains("Krause")));

    // The recovery key from the settings.
    open_notes_page(&mut session);
    session.send(Message::Settings(SettingsMessage::Notes(
        NotesEdit::ShowRecovery,
    )));
    session.settle();
    let recovery = session
        .app
        .settings_for_test()
        .and_then(|page| page.notes.recovery.clone())
        .expect("the recovery key is shown");
    ncrs_notes::crypt::NoteKey::from_recovery_text(&recovery).unwrap();
    shoot(&session.app, "32-notizen-wiederherstellungsschluessel");
}

const STORE_ON: &str = "[modules.filing]\nenabled = true\n";

fn chats_dir(world: &World) -> PathBuf {
    world.data.path().join("hermes").join("chats")
}

fn chat_files(world: &World) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(chats_dir(world)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

fn chat_world(label: &str) -> World {
    let world = World::new(label, STORE_ON);
    drop(ncrs_index::Index::create_plain(&world.data.path().join("index.db")).unwrap());
    world
}

fn boot_ready(world: &World) -> Session<impl Live> {
    let mut session = boot_with(world.data.path(), empty_store(), world.saved());
    pump_until(&mut session, 30, is_ready);
    session
}

fn say<P: Live>(session: &mut Session<P>, sentence: &str) {
    session.send(Message::HermesChat(ChatMessage::InputChanged(
        sentence.to_owned(),
    )));
    session.send(Message::HermesChat(ChatMessage::Submit));
    pump_for(session, Duration::from_secs(60), |app| !busy(app));
    assert!(!busy(&session.app), "Hermes answered");
}

fn listed(app: &App) -> Vec<(String, bool)> {
    let today = chrono::Local::now().date_naive();
    app.hermes_chat_for_test()
        .chats()
        .groups(today)
        .into_iter()
        .flat_map(|group| group.rows)
        .filter(|row| !row.title.is_empty())
        .map(|row| (row.title, row.marked))
        .collect()
}

#[test]
fn two_chats_are_listed_switched_kept_and_deleted() {
    let world = chat_world("chats160");
    let mut session = boot_ready(&world);
    session.send(Message::HermesChat(ChatMessage::Open));
    say(&mut session, "Sortier mir meine Rechnungen");
    session.send(Message::HermesChat(ChatMessage::NewChat));
    say(&mut session, "Wo liegt der Mietvertrag");

    assert_eq!(chat_files(&world).len(), 2, "both chats are on disk");
    let rows = listed(&session.app);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().any(|(title, _)| title.contains("Rechnungen")));
    assert!(rows.iter().any(|(title, _)| title.contains("Mietvertrag")));
    shoot(&session.app, "33-hermes-chatliste-gruppiert");

    // The list runs newest first: Cmd+Down shows the older first chat, read only.
    session.press_with(Key::Named(Named::ArrowDown), Modifiers::COMMAND);
    assert!(session.app.hermes_chat_for_test().chats().is_read_only());
    assert!(session
        .app
        .hermes_chat_for_test()
        .chats()
        .title_of_shown()
        .is_some_and(|title| title.contains("Rechnungen")));
    shoot(&session.app, "34-hermes-chat-nur-lesen");

    // A restart on the same data folder: both are still there.
    drop(session);
    let mut restarted = boot_ready(&world);
    restarted.send(Message::HermesChat(ChatMessage::Open));
    assert_eq!(listed(&restarted.app).len(), 2);
    assert_eq!(chat_files(&world).len(), 2);
    shoot(&restarted.app, "35-hermes-chatliste-nach-neustart");

    // Mark both with Space, delete them. After a start the empty new chat is on top.
    restarted.press_with(Key::Named(Named::ArrowDown), Modifiers::COMMAND);
    restarted.press(Key::Named(Named::Space));
    restarted.press_with(Key::Named(Named::ArrowDown), Modifiers::COMMAND);
    restarted.press(Key::Named(Named::Space));
    assert!(
        listed(&restarted.app).iter().all(|(_, marked)| *marked),
        "{:?}",
        listed(&restarted.app)
    );
    shoot(&restarted.app, "36-hermes-chats-markiert");
    restarted.send(Message::HermesChat(ChatMessage::List(
        ListMessage::DeleteMarked,
    )));
    assert!(chat_files(&world).is_empty(), "the files are gone");
    assert!(listed(&restarted.app).is_empty());
}

fn chat_json(id: &str, title: &str, updated: i64) -> String {
    format!(
        "{{\n  \"id\": \"{id}\",\n  \"title\": \"{title}\",\n  \"created\": {updated},\n  \"updated\": {updated},\n  \"status\": \"done\",\n  \"lines\": [\n    {{ \"from\": \"user\", \"text\": \"{title}\" }},\n    {{ \"from\": \"hermes\", \"text\": \"Erledigt.\" }}\n  ]\n}}\n"
    )
}

#[test]
fn chats_older_than_the_chosen_days_are_removed_at_start() {
    let world = chat_world("chats-old160");
    let mut session = boot_ready(&world);
    session.send(Message::Settings(SettingsMessage::Toggle));
    session.send(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Hermes,
    )));
    session.send(Message::Settings(SettingsMessage::Change(
        SettingChange::HermesChatKeepDays(Some(30)),
    )));
    let config = std::fs::read_to_string(world.data.path().join("config.toml")).unwrap();
    assert!(config.contains("chat_keep_days = 30"), "{config}");
    assert_eq!(
        session.app.hermes_chat_for_test().chat_keep_days(),
        Some(30)
    );
    shoot(&session.app, "38-einstellungen-hermes-chats-loeschen-nach");
    drop(session);

    let now = chrono::Local::now().timestamp();
    let day = 86_400;
    let dir = chats_dir(&world);
    std::fs::create_dir_all(&dir).unwrap();
    let old = dir.join("000000000001-000.json");
    let recent = dir.join("000000000002-000.json");
    std::fs::write(
        &old,
        chat_json("000000000001-000", "Alte Rechnungen", now - 45 * day),
    )
    .unwrap();
    std::fs::write(
        &recent,
        chat_json("000000000002-000", "Neue Belege", now - 2 * day),
    )
    .unwrap();

    let mut restarted = boot_ready(&world);
    restarted.send(Message::HermesChat(ChatMessage::Open));
    assert!(!old.exists(), "the 45 days old chat is removed at start");
    assert!(recent.is_file(), "the chat of two days ago stays");
    let rows = listed(&restarted.app);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].0.contains("Neue Belege"));
    shoot(&restarted.app, "39-hermes-chatliste-nach-aufraeumen");
}
