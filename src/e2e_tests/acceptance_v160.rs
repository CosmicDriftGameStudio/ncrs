//! Acceptance shots for 1.6.0: the real app, headless, each new flow driven
//! through its messages and keys. The asserts always run; the pictures in
//! `target/acceptance/v160/` are only taken when `NCRS_ACCEPTANCE_SHOTS` is set.
//!
//! Pictures: `NCRS_ACCEPTANCE_SHOTS=1 cargo test -p ncrs --bin ncrs acceptance_v160 -- --test-threads=1`.
//!
//! The Git flows (#336, #337) are in `acceptance_v160_git`, the notes (#296)
//! and the chat list (#297) in `acceptance_v160_notes_chats`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced_test::Simulator;
use ncrs_handoff::Request;
use ncrs_settings::message::SettingsMessage;

use super::Session;
use crate::app::App;
use crate::hermes_protocol::real_start::{boot_with, empty_store, pump_for};
use crate::messages::{FavouritesBarMessage, Message, PanelMessage, PanelSide, SettingsFeature};
use crate::session_state::{SessionState, WindowSize};

pub(in crate::e2e_tests) const SHOT_DIR: &str = "target/acceptance/v160";
pub(in crate::e2e_tests) const SHOT_SIZE: iced::Size = iced::Size::new(1740.0, 1130.0);

pub(in crate::e2e_tests) trait Live:
    iced::Program<State = App, Message = Message> + 'static
{
}
impl<P: iced::Program<State = App, Message = Message> + 'static> Live for P {}

pub(in crate::e2e_tests) fn shooting() -> bool {
    std::env::var_os("NCRS_ACCEPTANCE_SHOTS").is_some()
}

/// Takes the picture `name` of `app`, then lets `touch` act on the drawn view
/// first when the picture needs a focused or selected widget.
pub(in crate::e2e_tests) fn shoot_with(
    app: &App,
    name: &str,
    touch: impl FnOnce(&mut Simulator<'_, Message>),
) {
    shoot_into(SHOT_DIR, app, name, touch);
}

/// [`shoot_with`] into the folder `dir`.
pub(in crate::e2e_tests) fn shoot_into(
    dir: &str,
    app: &App,
    name: &str,
    touch: impl FnOnce(&mut Simulator<'_, Message>),
) {
    if !shooting() {
        return;
    }
    std::fs::create_dir_all(dir).unwrap();
    let shot = PathBuf::from(dir);
    drop(std::fs::remove_file(
        shot.join(format!("{name}-tiny-skia.png")),
    ));
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        SHOT_SIZE,
        app.view(),
    );
    touch(&mut simulator);
    // A missing file is written instead of compared.
    simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(shot.join(format!("{name}.png")))
        .unwrap();
    assert!(shot.join(format!("{name}-tiny-skia.png")).is_file());
}

pub(in crate::e2e_tests) fn shoot(app: &App, name: &str) {
    shoot_with(app, name, |_| {});
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

const PDFS: [&str; 20] = [
    "Rechnung-Telekom-2025-10.pdf",
    "Rechnung-Stadtwerke-2025-09.pdf",
    "Rechnung-Amazon-Bestellung.pdf",
    "Mietvertrag-Vermieter.pdf",
    "Kaufvertrag-Auto.pdf",
    "Steuerbescheid-2024.pdf",
    "Lohnabrechnung-Oktober.pdf",
    "Lohnabrechnung-September.pdf",
    "Krankenkasse-Beitragsbescheinigung.pdf",
    "Versicherungspolice-Haftpflicht.pdf",
    "Versicherungspolice-Hausrat.pdf",
    "Kontoauszug-2025-10.pdf",
    "Kontoauszug-2025-09.pdf",
    "Angebot-Heizung-Vaillant.pdf",
    "Handbuch-Waschmaschine.pdf",
    "Garantie-Fernseher.pdf",
    "Flugticket-Rom.pdf",
    "Hotelbuchung-Rom.pdf",
    "Zeugnis-2019.pdf",
    "Vollmacht.pdf",
];

const OTHER_FILES: [&str; 30] = [
    "Steuererklärung-2025-Entwurf.docx",
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
    "Passfoto.jpg",
    "IMG_2041.jpg",
    "IMG_2042.jpg",
    "IMG_2043.jpg",
    "IMG_2044.jpg",
    "IMG_2045.jpg",
    "Screenshot 2025-11-02 um 09.14.22.png",
    "Logo-Entwurf.png",
    "podcast-folge-12.mp3",
    "Urlaubsvideo.mov",
    "Kochrezepte.md",
    "Ideen-Garten.md",
    "Todo.md",
    "Vorlage-Brief.odt",
    "Fahrplan-Sommer.xlsx",
    "Reisebericht.txt",
];

const NINE_FAVOURITES: [&str; 9] = [
    "Steuerunterlagen und Belege 2025",
    "Versicherungen und Verträge Familie",
    "Fotos Urlaub Italien Sommer 2025",
    "Handbücher und Garantiescheine",
    "Rechnungen Stadtwerke und Telekom",
    "Bewerbungen und Zeugnisse",
    "Kontoauszüge aller Konten",
    "Projekte Garten und Haus",
    "Scans zum Einsortieren",
];

/// The folder of one subfolder tree for the folder size flow: the bytes each
/// file holds, so a total can be checked.
const ITALY: [(&str, usize); 4] = [
    ("Rom/IMG_0001.jpg", 40_000),
    ("Rom/IMG_0002.jpg", 25_000),
    ("Florenz/IMG_0003.jpg", 60_000),
    ("Venedig/Gondel/IMG_0004.jpg", 10_000),
];

pub(in crate::e2e_tests) struct World {
    pub data: tempfile::TempDir,
    /// The left panel's folder.
    pub downloads: PathBuf,
    /// The right panel's folder.
    pub other: PathBuf,
    pub notes: PathBuf,
    pub root: PathBuf,
}

impl World {
    pub(super) fn new(label: &str, config_tail: &str) -> Self {
        let data = tempfile::Builder::new()
            .prefix(&format!("a{label}-"))
            // A short root keeps the pasted path in the shots readable.
            .tempdir_in(if cfg!(unix) {
                PathBuf::from("/tmp")
            } else {
                std::env::temp_dir()
            })
            .unwrap();
        let root = std::fs::canonicalize(data.path()).unwrap();
        let downloads = root.join("Downloads");
        let other = root.join("Dokumente");
        let notes = root.join("Notizen");
        for folder in FOLDERS {
            std::fs::create_dir_all(downloads.join(folder)).unwrap();
        }
        for file in PDFS.iter().chain(OTHER_FILES.iter()) {
            super::write(&downloads.join(file), "x");
        }
        for (path, bytes) in ITALY {
            super::write(
                &downloads.join("Fotos Urlaub Italien").join(path),
                &"x".repeat(bytes),
            );
        }
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
            root,
        }
    }

    pub(super) fn saved(&self) -> SessionState {
        let mut saved = SessionState::default();
        saved.left.path = Some(self.downloads.to_string_lossy().into_owned());
        saved.right.path = Some(self.other.to_string_lossy().into_owned());
        saved.window = Some(WindowSize {
            width: SHOT_SIZE.width,
            height: SHOT_SIZE.height,
        });
        saved
    }

    pub(super) fn boot(&self) -> Session<impl Live> {
        boot_with(self.data.path(), empty_store(), self.saved())
    }

    pub(super) fn boot_in(&self, left: &Path) -> Session<impl Live> {
        let mut saved = self.saved();
        saved.left.path = Some(left.to_string_lossy().into_owned());
        boot_with(self.data.path(), empty_store(), saved)
    }
}

pub(in crate::e2e_tests) fn row_of(app: &App, name: &str) -> usize {
    app.panel(PanelSide::Left)
        .entries
        .iter()
        .position(|entry| entry.name.to_string_lossy() == name)
        .unwrap_or_else(|| panic!("no row {name:?}"))
}

/// Puts the cursor of the left panel on `name` with the keys a user presses.
pub(in crate::e2e_tests) fn go_to<P: Live>(session: &mut Session<P>, name: &str) {
    session.press(Key::Named(Named::Home));
    for _ in 0..row_of(&session.app, name) {
        session.press(Key::Named(Named::ArrowDown));
    }
    assert_eq!(
        super::selected(&session.app, PanelSide::Left).as_deref(),
        Some(name)
    );
}

pub(in crate::e2e_tests) fn pump_until<P: Live>(
    session: &mut Session<P>,
    seconds: u64,
    until: fn(&App) -> bool,
) {
    pump_for(session, Duration::from_secs(seconds), until);
    assert!(
        until(&session.app),
        "not reached within {seconds} s, notice {:?}",
        session.app.notice_for_test()
    );
}

fn command() -> Modifiers {
    Modifiers::COMMAND
}

#[test]
fn favourites_bar_overflow_and_cmd_7() {
    let world = World::new("favourites", "");
    let favourites: Vec<PathBuf> = NINE_FAVOURITES
        .iter()
        .map(|name| {
            let folder = world.root.join("Favoriten").join(name);
            super::write(&folder.join("Inhalt.txt"), "x");
            folder
        })
        .collect();
    let mut saved = world.saved();
    saved.favourites = favourites
        .iter()
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect();
    let mut session = boot_with(world.data.path(), empty_store(), saved);
    assert_eq!(session.app.favourites_for_test(), favourites.as_slice());

    let labels: Vec<String> = NINE_FAVOURITES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let fit = ncrs_ui_kit::favourites_bar::slots_that_fit(&labels, SHOT_SIZE.width);
    assert!(
        (3..9).contains(&fit),
        "at this width some favourites fit and some do not: {fit} of 9"
    );
    shoot(&session.app, "01-favoriten-leiste");

    session.send(Message::FavouritesBar(FavouritesBarMessage::Overflow));
    assert!(session.app.favourite_menu_is_open_for_test());
    shoot(&session.app, "02-favoriten-ueberlauf-menue");
    session.press(Key::Named(Named::Escape));
    assert!(!session.app.favourite_menu_is_open_for_test());

    session.press_with(Key::Character("7".into()), command());
    assert_eq!(session.app.panel(PanelSide::Left).path, favourites[6]);
    shoot(&session.app, "03-favorit-7-geoeffnet");
}

#[test]
fn folder_sizes_show_progress_and_keep_the_subfolders() {
    let world = World::new("sizes", "");
    let mut session = world.boot();
    go_to(&mut session, "Fotos Urlaub Italien");
    for name in [
        "Fotos Urlaub Italien",
        "Rechnungen 2025",
        "Verträge",
        "Steuer 2025",
    ] {
        let path = world.downloads.join(name);
        session
            .app
            .panel_mut_for_test(PanelSide::Left)
            .selection
            .toggle(&path);
    }
    let held = session.send_held(Message::ComputeFolderSizes);
    assert!(
        session.app.folder_size_progress_for_test().is_some(),
        "the status bar has the progress while the walk runs"
    );
    let (run, walked) = session.app.folder_size_walk_for_test().unwrap();
    session.send(Message::Panel(PanelMessage::FolderSizeRunning {
        run,
        folder: walked.expect("one folder is being walked"),
        bytes: 1_200_000_000,
    }));
    shoot(&session.app, "04-ordnergroessen-laeuft");
    session.drive(held);
    session.settle();
    assert!(session.app.folder_size_progress_for_test().is_none());

    let total: u64 = ITALY.iter().map(|(_, bytes)| *bytes as u64).sum();
    let italy = session
        .app
        .panel(PanelSide::Left)
        .entries
        .iter()
        .find(|entry| entry.name == "Fotos Urlaub Italien")
        .and_then(|entry| entry.folder_size)
        .expect("the folder has a size");
    assert_eq!(italy.bytes, total);
    shoot(&session.app, "05-ordnergroessen-fertig");

    session.press(Key::Named(Named::Enter));
    assert!(session
        .app
        .panel(PanelSide::Left)
        .path
        .ends_with("Fotos Urlaub Italien"));
    let sized = |name: &str| {
        session
            .app
            .panel(PanelSide::Left)
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| entry.folder_size)
            .map(|size| size.bytes)
    };
    assert_eq!(sized("Rom"), Some(65_000));
    assert_eq!(sized("Florenz"), Some(60_000));
    assert_eq!(sized("Venedig"), Some(10_000));
    shoot(&session.app, "06-unterordner-mit-groessen");
}

fn pdf_count(app: &App) -> usize {
    app.panel(PanelSide::Left)
        .entries
        .iter()
        .filter(|entry| !entry.is_dir && entry.name.to_string_lossy().ends_with(".pdf"))
        .count()
}

fn tagged(app: &App) -> usize {
    app.panel(PanelSide::Left).selection.len()
}

#[test]
fn tag_all_and_tag_or_untag_by_pattern() {
    let world = World::new("tags", "");
    let mut session = world.boot();
    session.press_with(Key::Character("a".into()), command());
    let rows = session
        .app
        .panel(PanelSide::Left)
        .entries
        .iter()
        .filter(|entry| !entry.is_parent)
        .count();
    assert!(rows >= 50, "a dense folder: {rows} rows");
    assert_eq!(tagged(&session.app), rows);
    shoot(&session.app, "07-alles-markiert");

    session.press_with(Key::Character("*".into()), Modifiers::CTRL);
    assert_eq!(tagged(&session.app), 0);

    session.press(Key::Character("+".into()));
    assert!(session.app.prompt_is_open_for_test());
    shoot(&session.app, "08-muster-dialog");
    session.send(Message::Ops(ncrs_ops::OpsMessage::PromptInput(
        "*.pdf".to_owned(),
    )));
    session.send(Message::Ops(ncrs_ops::OpsMessage::PromptSubmit));
    assert_eq!(pdf_count(&session.app), 20);
    assert_eq!(tagged(&session.app), 20, "only the PDFs are tagged");
    shoot(&session.app, "09-pdfs-markiert");

    session.press(Key::Character("-".into()));
    assert!(session.app.prompt_is_open_for_test());
    session.send(Message::Ops(ncrs_ops::OpsMessage::PromptInput(
        "*.pdf".to_owned(),
    )));
    session.send(Message::Ops(ncrs_ops::OpsMessage::PromptSubmit));
    assert_eq!(tagged(&session.app), 0, "the PDFs are untagged again");
}

#[test]
fn cmd_g_opens_a_dialog_and_a_pasted_file_path_goes_there() {
    let world = World::new("goto", "");
    let mut session = world.boot();
    session.press_with(Key::Character("g".into()), command());
    assert!(session.app.prompt_is_open_for_test());
    assert!(
        session.app.path_field_for_test().is_none(),
        "no field in the status bar"
    );
    let target = world.other.join("Beleg-17.pdf");
    session.send(Message::Ops(ncrs_ops::OpsMessage::PromptInput(
        target.display().to_string(),
    )));
    shoot(&session.app, "10-gehe-zu-pfad-dialog");

    session.send(Message::Ops(ncrs_ops::OpsMessage::PromptSubmit));
    pump_until(&mut session, 5, |app| {
        app.panel(PanelSide::Left)
            .selected_entry()
            .is_some_and(|entry| entry.name == "Beleg-17.pdf")
    });
    assert_eq!(session.app.panel(PanelSide::Left).path, world.other);
    assert_eq!(
        super::selected(&session.app, PanelSide::Left).as_deref(),
        Some("Beleg-17.pdf")
    );
    assert!(!session.app.prompt_is_open_for_test());
    shoot(&session.app, "11-gehe-zu-pfad-cursor-auf-datei");
}

#[test]
fn the_column_header_row_is_centred() {
    let world = World::new("header", "");
    let session = world.boot();
    shoot(&session.app, "12-spaltenkopf");
}

#[test]
fn a_fresh_start_has_the_programs_folder_among_the_favourites() {
    let world = World::new("fresh", "");
    let mut saved = App::with_first_start_favourites(SessionState::default());
    saved.left = world.saved().left;
    saved.right = world.saved().right;
    saved.window = world.saved().window;
    let session = boot_with(world.data.path(), empty_store(), saved);
    let programs = ncrs_files::special_folders::first_start_favourites();
    let favourites = session.app.favourites_for_test();
    if cfg!(target_os = "macos") {
        assert_eq!(favourites, [PathBuf::from("/Applications")]);
    }
    assert_eq!(favourites, programs.as_slice());
    shoot(&session.app, "13-programme-bei-erstem-start");
}

#[test]
fn another_start_hands_a_folder_and_a_file_to_the_running_ncrs() {
    let world = World::new("handoff", "");
    let mut session = world.boot();
    let data = world.data.path();

    // The second start leaves the folder in the inbox; the running ncrs takes it.
    let folder = world.downloads.join("Verträge");
    ncrs_handoff::send(data, &[Request::Open(folder.clone())]).unwrap();
    let handed = ncrs_handoff::take(data);
    assert_eq!(handed, [Request::Open(folder.clone())]);
    session.send(Message::HandedOver(handed));
    assert_eq!(session.app.panel(PanelSide::Left).path, folder);
    shoot(&session.app, "14-uebergabe-ordner");

    let file = world.other.join("Beleg-23.pdf");
    ncrs_handoff::send(data, &[Request::Open(file.clone())]).unwrap();
    session.send(Message::HandedOver(ncrs_handoff::take(data)));
    assert_eq!(session.app.panel(PanelSide::Left).path, world.other);
    assert_eq!(
        super::selected(&session.app, PanelSide::Left).as_deref(),
        Some("Beleg-23.pdf")
    );
    assert!(
        ncrs_handoff::take(data).is_empty(),
        "the inbox is empty after the look"
    );
    shoot(&session.app, "15-uebergabe-datei-cursor");

    session.send(Message::Settings(SettingsMessage::Toggle));
    session.send(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Programs,
    )));
    assert!(session.app.settings_for_test().is_some());
    shoot(&session.app, "16-programme-integration");
}
