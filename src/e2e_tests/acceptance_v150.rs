//! Acceptance shots for 1.5.0: the real app, headless, each new flow driven
//! through its messages and keys. The asserts are the check, the pictures in
//! `target/acceptance/v150/` are the evidence for the acceptance.
//!
//! Run: `cargo test -p ncrs --bin ncrs acceptance_v150 -- --ignored --test-threads=1`.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::text_editor::{Action, Edit};
use iced_test::Simulator;
use ncrs_settings::message::SettingsMessage;
use ncrs_settings::settings::TypeRuleEdit;

use super::Session;
use crate::app::App;
use crate::filing::{FilingMessage, Phase};
use crate::fs::archive::fixtures::{write_archive, Item};
use crate::fs::archive::ArchiveFormat;
use crate::hermes_protocol::real_start::{boot_with, empty_store, pump_for};
use crate::hermes_protocol::thread::is_ready;
use crate::messages::{HermesFileMessage, Message, PanelSide, SettingsFeature};
use crate::session_state::SessionState;

const SHOT_DIR: &str = "target/acceptance/v150";
const SHOT_SIZE: iced::Size = iced::Size::new(1740.0, 1130.0);

trait Live: iced::Program<State = App, Message = Message> + 'static {}
impl<P: iced::Program<State = App, Message = Message> + 'static> Live for P {}

fn shoot(app: &App, name: &str) {
    std::fs::create_dir_all(SHOT_DIR).unwrap();
    let shot = PathBuf::from(SHOT_DIR);
    drop(std::fs::remove_file(
        shot.join(format!("{name}-tiny-skia.png")),
    ));
    // A missing file is written instead of compared.
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        SHOT_SIZE,
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_image(shot.join(format!("{name}.png")))
    .unwrap();
    assert!(shot.join(format!("{name}-tiny-skia.png")).is_file());
}

const FOLDERS: [&str; 14] = [
    "Rechnungen 2025",
    "Verträge",
    "Steuer 2025",
    "Fotos Urlaub Italien",
    "Versicherung",
    "Bewerbung",
    "Kontoauszüge",
    "Handbücher",
    "Scans",
    "Schule",
    "Reisen",
    "Garten",
    "Auto",
    "Musik",
];

const FILES: [&str; 62] = [
    "Rechnung-Telekom-2025-10.pdf",
    "Rechnung-Stadtwerke-2025-09.pdf",
    "Rechnung-Amazon-Bestellung.pdf",
    "Mietvertrag-Vermieter.pdf",
    "Kaufvertrag-Auto.pdf",
    "Steuerbescheid-2024.pdf",
    "Steuererklärung-2025-Entwurf.docx",
    "Lohnabrechnung-Oktober.pdf",
    "Lohnabrechnung-September.pdf",
    "Krankenkasse-Beitragsbescheinigung.pdf",
    "Versicherungspolice-Haftpflicht.pdf",
    "Versicherungspolice-Hausrat.pdf",
    "Kontoauszug-2025-10.pdf",
    "Kontoauszug-2025-09.pdf",
    "Angebot-Heizung-Vaillant.pdf",
    "Angebot-Fenster.pdf",
    "Heizung-Wartungsprotokoll.pdf",
    "IMG_2041.jpg",
    "IMG_2042.jpg",
    "IMG_2043.jpg",
    "IMG_2044.jpg",
    "IMG_2045.jpg",
    "IMG_2046.jpg",
    "IMG_2047.jpg",
    "Passfoto.jpg",
    "Screenshot 2025-11-02 um 09.14.22.png",
    "Screenshot 2025-11-07 um 17.40.05.png",
    "Logo-Entwurf.png",
    "Lebenslauf.docx",
    "Anschreiben-Bewerbung.docx",
    "Protokoll-Elternabend.docx",
    "Einkaufsliste.txt",
    "Passwörter-Ideen.txt",
    "notizen-telefonat.txt",
    "server.log",
    "install.log",
    "Haushaltsbuch-2025.xlsx",
    "Budget.xlsx",
    "Präsentation-Quartal.pptx",
    "Backup-Dokumente-2024.zip",
    "Archiv-alt.tar.gz",
    "Fotos-Urlaub.zip",
    "Projekt-Alpha.zip",
    "Handbuch-Waschmaschine.pdf",
    "Handbuch-Drucker.pdf",
    "Garantie-Fernseher.pdf",
    "Fahrplan-Sommer.pdf",
    "Reiseversicherung.pdf",
    "Flugticket-Rom.pdf",
    "Hotelbuchung-Rom.pdf",
    "Zeugnis-2019.pdf",
    "Impfpass-Scan.pdf",
    "Vollmacht.pdf",
    "Kündigung-Fitnessstudio.pdf",
    "Mahnung-Zahnarzt.pdf",
    "Spendenquittung-2025.pdf",
    "podcast-folge-12.mp3",
    "Urlaubsvideo.mov",
    "Kochrezepte.md",
    "Ideen-Garten.md",
    "Todo.md",
    "Vorlage-Brief.odt",
];

const README: &str = "# ncrs\n\nA file manager with a **second brain**.\n\n## Install\n\n- Download the app\n- Open it\n- Press `F1`\n\n## Build\n\n```sh\ncargo build --release\n```\n\nSee the [website](https://example.com) for more.\n";

/// A Downloads folder of realistic density below `root`: 14 folders and the
/// files above, 77 rows.
fn fill_downloads(root: &Path) {
    for folder in FOLDERS {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    for file in FILES {
        super::write(&root.join(file), "x");
    }
    super::write(&root.join("README.md"), README);
    super::write(
        &root.join("Einkaufsliste.txt"),
        "Brot\nMilch\nEier\nKaffee\n",
    );
    write_archive(
        &root.join("Fotos-Urlaub.zip"),
        &[
            Item::File("strand.jpg", b"jpg-1"),
            Item::File("pizza.jpg", b"jpg-2"),
            Item::File("Reisebericht.txt", b"Es war schoen."),
        ],
    );
    write_archive(
        &root.join("Projekt-Alpha.zip"),
        &[
            Item::Dir("Projekt-Alpha"),
            Item::File("Projekt-Alpha/plan.md", b"# Plan"),
            Item::File("Projekt-Alpha/budget.txt", b"1000"),
        ],
    );
}

struct World {
    data: tempfile::TempDir,
    /// The left panel's folder.
    downloads: PathBuf,
    /// The right panel's folder.
    other: PathBuf,
    notes: PathBuf,
}

impl World {
    fn new(label: &str, config_tail: &str) -> Self {
        let data = tempfile::Builder::new()
            .prefix(&format!("ncrs-acc-{label}-"))
            .tempdir()
            .unwrap();
        let root = std::fs::canonicalize(data.path()).unwrap();
        let downloads = root.join("Downloads");
        let other = root.join("Dokumente");
        let notes = root.join("Notizen");
        fill_downloads(&downloads);
        for name in ["Mietverträge", "Zeugnisse", "Fotos", "Alt"] {
            std::fs::create_dir_all(other.join(name)).unwrap();
        }
        for n in 0..40 {
            super::write(&other.join(format!("Beleg-{n:02}.pdf")), "x");
        }
        std::fs::write(
            root.join("config.toml"),
            format!("version = 1\nlang = \"de\"\n{config_tail}\n[notes]\nfolder = {notes:?}\n"),
        )
        .unwrap();
        Self {
            data,
            downloads,
            other,
            notes,
        }
    }

    fn saved(&self) -> SessionState {
        let mut saved = SessionState::default();
        saved.left.path = Some(self.downloads.to_string_lossy().into_owned());
        saved.right.path = Some(self.other.to_string_lossy().into_owned());
        saved
    }

    fn boot(&self) -> Session<impl Live> {
        boot_with(self.data.path(), empty_store(), self.saved())
    }
}

/// The row order of the left panel, to find where a name is.
fn row_of(app: &App, name: &str) -> usize {
    app.panel(PanelSide::Left)
        .entries
        .iter()
        .position(|entry| entry.name.to_string_lossy() == name)
        .unwrap_or_else(|| panic!("no row {name:?}"))
}

/// Puts the cursor of the left panel on `name` with the keys a user presses.
fn go_to<P: Live>(session: &mut Session<P>, name: &str) {
    session.press(Key::Named(Named::Home));
    for _ in 0..row_of(&session.app, name) {
        session.press(Key::Named(Named::ArrowDown));
    }
    assert_eq!(
        super::selected(&session.app, PanelSide::Left).as_deref(),
        Some(name)
    );
}

fn pump_until<P: Live>(session: &mut Session<P>, seconds: u64, until: fn(&App) -> bool) {
    pump_for(session, Duration::from_secs(seconds), until);
    assert!(
        until(&session.app),
        "not reached within {seconds} s, notice {:?}",
        session.app.notice_for_test()
    );
}

fn paste<P: Live>(session: &mut Session<P>, text: &str) {
    session.send(Message::Notes(ncrs_notes::Message::Edited(Action::Edit(
        Edit::Paste(text.to_owned().into()),
    ))));
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn viewer_markdown_and_source() {
    let world = World::new("viewer", "");
    let mut session = world.boot();
    go_to(&mut session, "README.md");
    session.press(Key::Named(Named::F3));
    assert!(session.app.viewer_is_shown_for_test());
    assert_eq!(session.app.viewer_for_test().title(), Some("README.md"));
    assert!(session.app.viewer_for_test().shows_markdown());
    shoot(&session.app, "01-viewer-markdown");

    session.press(Key::Named(Named::Tab));
    assert_eq!(
        session.app.viewer_for_test().mode,
        ncrs_viewer::Mode::Source
    );
    shoot(&session.app, "02-viewer-quelltext");
}

const THREE_RULES: &str = "[open]\nby_type = [\n\
    { extensions = [\"md\", \"markdown\"], with = \"internal\" },\n\
    { extensions = [\"pdf\"], with = \"system\" },\n\
    { extensions = [\"log\", \"txt\"], with = \"program\", program = \"code --goto {path}:{line}\" },\n\
    ]\n";

fn open_programs_page<P: Live>(session: &mut Session<P>) {
    session.send(Message::Settings(SettingsMessage::Toggle));
    session.send(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Programs,
    )));
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn programs_page_by_type() {
    let world = World::new("programs", THREE_RULES);
    let mut session = world.boot();
    open_programs_page(&mut session);
    shoot(&session.app, "03-programme-f3-je-typ");
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn a_txt_rule_sends_f3_to_the_viewer() {
    let rules = "[open]\nby_type = [\n\
        { extensions = [\"md\", \"markdown\"], with = \"internal\" },\n\
        { extensions = [\"pdf\"], with = \"system\" },\n\
        ]\n";
    let world = World::new("txt-rule", rules);
    let mut session = world.boot();
    open_programs_page(&mut session);
    let row = 2;
    session.send(Message::Settings(SettingsMessage::TypeRule(
        TypeRuleEdit::Add,
    )));
    session.send(Message::Settings(SettingsMessage::TypeRule(
        TypeRuleEdit::Extensions {
            row,
            text: "txt".to_owned(),
        },
    )));
    session.send(Message::Settings(SettingsMessage::TypeRule(
        TypeRuleEdit::Commit(row),
    )));
    let config = std::fs::read_to_string(world.data.path().join("config.toml")).unwrap();
    assert!(
        config.contains("txt") && config.contains("by_type"),
        "{config}"
    );

    session.send(Message::Settings(SettingsMessage::Close));
    assert!(!session.app.viewer_is_shown_for_test());
    go_to(&mut session, "Einkaufsliste.txt");
    session.press(Key::Named(Named::F3));
    assert!(session.app.viewer_is_shown_for_test());
    assert_eq!(
        session.app.viewer_for_test().title(),
        Some("Einkaufsliste.txt")
    );
    assert!(!session.app.viewer_for_test().shows_markdown());
    shoot(&session.app, "04-f3-txt-intern");
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn process_again_asks_before_many_inbox_files() {
    let world = World::new("again", "");
    let inbox = world.downloads.parent().unwrap().join("Eingang");
    for name in ["scan-1.txt", "scan-2.txt", "scan-3.txt"] {
        super::write(
            &inbox.join("Scans Oktober").join(name),
            &format!("Rechnung Stadtwerke {name}"),
        );
    }
    for n in 0..40 {
        super::write(&inbox.join(format!("Altlast-{n:02}.txt")), "Brief");
    }
    let target = world.other.clone();
    std::fs::write(
        world.data.path().join("config.toml"),
        format!(
            "version = 1\nlang = \"de\"\n[modules.filing]\nenabled = true\n\
             [[modules.filing.locations]]\npath = {inbox:?}\nmode = \"inbox\"\n\
             [[modules.filing.locations]]\npath = {target:?}\nmode = \"archive\"\ntarget = true\n\
             [notes]\nfolder = {:?}\n",
            world.notes
        ),
    )
    .unwrap();
    drop(ncrs_index::Index::create_plain(&world.data.path().join("index.db")).unwrap());

    let mut saved = world.saved();
    saved.left.path = Some(inbox.to_string_lossy().into_owned());
    let mut session = boot_with(world.data.path(), empty_store(), saved);
    let mut confirmed = false;
    for _ in 0..300 {
        pump_for(&mut session, Duration::from_millis(200), is_ready);
        if is_ready(&session.app) {
            break;
        }
        if !confirmed
            && session
                .app
                .filing_module_for_test()
                .is_some_and(|module| matches!(module.phase(), Phase::PlainFileWarning))
        {
            confirmed = true;
            session.send(Message::Filing(FilingMessage::ConfirmPlainFile));
        }
    }
    pump_until(&mut session, 1, is_ready);
    pump_for(&mut session, Duration::from_secs(5), |_| false);

    go_to(&mut session, "Scans Oktober");
    session.press(Key::Named(Named::Insert));
    assert!(session.app.panel(PanelSide::Left).selection.any_tagged());
    let task = session.send_held(Message::HermesFile(HermesFileMessage::ProcessAgain));
    session.drive(task);
    pump_until(&mut session, 20, |app| {
        app.confirm_again_for_test().is_some()
    });
    assert_eq!(session.app.confirm_again_for_test().unwrap().len(), 3);
    assert!(
        session.app.notice_for_test().is_none(),
        "nothing queued yet"
    );
    shoot(&session.app, "05-erneut-verarbeiten-rueckfrage");

    let confirm = session.send_held(Message::HermesFile(HermesFileMessage::ConfirmProcessAgain));
    session.drive(confirm);
    pump_until(&mut session, 20, |app| {
        app.notice_for_test()
            .is_some_and(|notice| notice.contains('3'))
    });
    assert!(session.app.confirm_again_for_test().is_none());
}

fn write_notes(root: &Path, file_note_for: &Path) {
    let note = |path: &str, text: &str| super::write(&root.join(path), text);
    note(
        "Haus/Heizung Wartung.md",
        "# Heizung Wartung\n\nWartung jeden Oktober, Krause Haustechnik, Telefon 089 1234567.\n\n## Offen\n\n- Termin 2026 buchen\n- Rechnung 2025 abgelegt\n\n## Werte\n\n- Druck soll 1,5 bar sein, gemessen 1,3 bar\n- Filter zuletzt 10/2025 gewechselt\n",
    );
    note(
        "Haus/Einkaufen Baumarkt.md",
        "# Einkaufen Baumarkt\n\n- Dichtungen\n- Entlüftungsschlüssel\n",
    );
    note(
        "Steuern/Steuererklärung 2025.md",
        "# Steuererklärung 2025\n\nBelege sammeln: Spenden, Handwerker, Heizung (Wartung absetzbar).\n",
    );
    note(
        "Steuern/Belege.md",
        "# Belege\n\nFahrtkosten, Arbeitszimmer, Fortbildung.\n",
    );
    note(
        "Ideen.md",
        "# Ideen\n\n- Hochbeet bauen\n- Neue Heizkörperventile\n",
    );
    note(
        "Reisen.md",
        "# Reisen\n\nItalien im Mai, die Heizung im Hotel prüfen.\n",
    );
    note(
        "Telefonnummern.md",
        "# Telefonnummern\n\nKrause Haustechnik 089 1234567\n",
    );
    note(
        ".files/Angebot-1a2b3c4d.md",
        &format!(
            "---\nfile: {}\n---\nAngebot gültig bis 31. Dezember, Heizung mit Einbau 14.800 EUR.\n",
            file_note_for.display()
        ),
    );
}

fn notes_world(label: &str) -> World {
    let world = World::new(label, "");
    write_notes(
        &world.notes,
        &world.downloads.join("Angebot-Heizung-Vaillant.pdf"),
    );
    world
}

fn press_note<P: Live>(session: &mut Session<P>, key: ncrs_notes::tree::RowKey) {
    session.send(Message::Notes(ncrs_notes::Message::Pressed(key)));
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn notes_view_edit_and_search() {
    use ncrs_notes::tree::RowKey;
    let world = notes_world("notes");
    let mut session = world.boot();
    session.send(Message::OpenNotes);
    assert!(session.app.notes_are_shown_for_test());
    press_note(&mut session, RowKey::Folder("Haus".into()));
    press_note(&mut session, RowKey::Folder("Steuern".into()));
    press_note(
        &mut session,
        RowKey::Note(world.notes.join("Haus/Heizung Wartung.md")),
    );
    assert!(session.app.notes_for_test().shown().is_some());
    shoot(&session.app, "06-notizen-ansicht");

    session.send(Message::Notes(ncrs_notes::Message::SetMode(
        ncrs_notes::Mode::Edit,
    )));
    paste(&mut session, "Neue Zeile: Ventil getauscht\n");
    shoot(&session.app, "07-notizen-bearbeiten");
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    let on_disk = std::fs::read_to_string(world.notes.join("Haus/Heizung Wartung.md")).unwrap();
    assert!(
        on_disk.contains("Neue Zeile: Ventil getauscht"),
        "{on_disk}"
    );

    session.send(Message::Notes(ncrs_notes::Message::SearchChanged(
        "heizung".to_owned(),
    )));
    assert!(session.app.notes_for_test().hits.len() >= 3);
    shoot(&session.app, "08-notizen-suche");
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn note_on_file_menu_and_marker() {
    let world = notes_world("note-on-file");
    let mut session = world.boot();
    go_to(&mut session, "Rechnung-Telekom-2025-10.pdf");
    session.send(Message::ContextMenuKey);
    assert!(session.app.context_menu_for_test().is_some());
    shoot(&session.app, "09-notiz-zur-datei-menue");
    session.send(Message::ContextMenuClose);

    session.press_with(
        Key::Character("n".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    session.settle();
    assert!(session.app.notes_are_shown_for_test());
    paste(&mut session, "Kundennummer 4711, Vertrag läuft bis März");
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    assert!(!session.app.notes_are_shown_for_test());
    let file_note = std::fs::read_dir(world.notes.join(".files"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .find(|text| text.contains("Kundennummer 4711"))
        .expect("the note on the file is on disk");
    assert!(
        file_note.contains("file: ") && file_note.contains("Rechnung-Telekom-2025-10.pdf"),
        "{file_note}"
    );
    shoot(&session.app, "10-notiz-markierung-status");
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn archive_menu_and_extract_here() {
    let world = World::new("extract", "");
    let mut session = world.boot();
    go_to(&mut session, "Backup-Dokumente-2024.zip");
    session.send(Message::ContextMenuKey);
    assert!(session.app.context_menu_for_test().is_some());
    shoot(&session.app, "11-archiv-menue");
    session.send(Message::ContextMenuClose);

    go_to(&mut session, "Fotos-Urlaub.zip");
    session.send(Message::ExtractHere);
    session.settle();
    let folder = world.downloads.join("Fotos-Urlaub");
    for name in ["strand.jpg", "pizza.jpg", "Reisebericht.txt"] {
        assert!(folder.join(name).is_file(), "{name} missing");
    }
    assert_eq!(
        super::selected(&session.app, PanelSide::Left).as_deref(),
        Some("Fotos-Urlaub")
    );
    shoot(&session.app, "12-hier-entpackt");

    go_to(&mut session, "Projekt-Alpha.zip");
    session.send(Message::ExtractHere);
    session.settle();
    assert!(world.downloads.join("Projekt-Alpha/plan.md").is_file());
    assert!(!world.downloads.join("Projekt-Alpha/Projekt-Alpha").exists());
}

#[test]
#[ignore = "acceptance shots, run by hand"]
fn pack_three_files_as_7z() {
    let world = World::new("pack", "");
    let mut session = world.boot();
    for name in [
        "Rechnung-Telekom-2025-10.pdf",
        "Rechnung-Stadtwerke-2025-09.pdf",
        "Kontoauszug-2025-10.pdf",
    ] {
        go_to(&mut session, name);
        session.press(Key::Named(Named::Insert));
    }
    session.send(Message::PackPrompt);
    session.send(Message::PromptFormat(ArchiveFormat::SevenZip));
    assert!(session.app.prompt_is_open_for_test());
    shoot(&session.app, "13-packen-dialog");

    session.send(Message::PromptSubmit);
    session.settle();
    let packed: Vec<PathBuf> = std::fs::read_dir(&world.other)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "7z"))
        .collect();
    assert_eq!(packed.len(), 1, "{packed:?}");
    let location = crate::fs::archive::locate_in_archive(&packed[0]).expect("an archive");
    let listed = crate::fs::archive::list(&location).unwrap();
    assert_eq!(
        listed
            .entries
            .iter()
            .filter(|entry| !entry.is_parent)
            .count(),
        3
    );
}
