//! Snapshots of the panel with its extra columns, the header menu, the Columns
//! and font pages and the permissions dialog, in both languages.

use std::ffi::OsString;
use std::path::PathBuf;

use iced_test::Simulator;
use ncrs_core::{Column, EntryDetails, FileEntry};

use crate::app::*;
use crate::config::Filing;
use crate::i18n::Language;
use crate::messages::{PermissionsMessage, SettingsFeature};
use ncrs_config::panel_columns::PanelColumns;

const WINDOW: iced::Size = iced::Size::new(1740.0, 1130.0);
const PAGE_WINDOW: iced::Size = iced::Size::new(1200.0, 1100.0);

fn entry(name: &str, is_dir: bool, size: u64, mode: u32, owner: &str) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: PathBuf::from("/home/test").join(name),
        is_dir,
        is_symlink: false,
        is_parent: false,
        size,
        modified: None,
        folder_size: None,
        details: EntryDetails {
            mode: Some(mode),
            owner: Some(owner.to_owned()),
            group: Some(if owner == "root" { "wheel" } else { "staff" }.to_owned()),
            ..EntryDetails::default()
        },
    }
}

fn link(name: &str, target: &str, is_dir: bool, broken: bool) -> FileEntry {
    let mut linked = entry(name, is_dir, 0, 0o755, "test");
    linked.is_symlink = true;
    linked.details.link_target = Some(PathBuf::from(target));
    linked.details.link_broken = broken;
    linked
}

/// A home folder of 46 entries: the system folders, projects, links, a broken
/// link and files of many types.
fn dense_tree() -> Vec<FileEntry> {
    let mut parent = entry("..", true, 0, 0o755, "root");
    parent.is_parent = true;
    let mut entries = vec![parent];
    for folder in [
        "Desktop",
        "Documents",
        "Downloads",
        "Movies",
        "Music",
        "Pictures",
        "Public",
        "projects",
        "backups",
        "Library",
    ] {
        entries.push(entry(folder, true, 0, 0o755, "test"));
    }
    entries.push(entry("shared", true, 0, 0o775, "test"));
    entries.push(entry("secrets", true, 0, 0o700, "test"));
    entries.push(entry("tmp", true, 0, 0o1777, "root"));
    entries.push(link("current", "projects/ncrs", true, false));
    entries.push(link("latest.log", "/var/log/system.log", false, false));
    entries.push(link("old-notes", "gone/notes.txt", false, true));
    let files: [(&str, u64, u32, &str); 30] = [
        ("README.md", 4_212, 0o644, "test"),
        ("notes.txt", 812, 0o644, "test"),
        ("report-2025.pdf", 2_481_920, 0o644, "test"),
        ("invoice.pdf", 183_000, 0o600, "test"),
        ("photo.jpg", 3_204_112, 0o644, "test"),
        ("scan.png", 1_024_000, 0o644, "test"),
        ("archive.zip", 52_428_800, 0o644, "test"),
        ("backup.tar.gz", 734_003_200, 0o640, "test"),
        ("setup.dmg", 98_000_000, 0o755, "root"),
        ("build.sh", 1_204, 0o755, "test"),
        ("deploy.sh", 2_880, 0o750, "test"),
        ("main.rs", 9_820, 0o644, "test"),
        ("lib.rs", 3_410, 0o644, "test"),
        ("Cargo.toml", 1_920, 0o644, "test"),
        ("config.json", 640, 0o600, "test"),
        ("data.csv", 48_220, 0o644, "test"),
        ("budget.xlsx", 22_100, 0o644, "test"),
        ("letter.docx", 31_400, 0o644, "test"),
        ("slides.pptx", 4_800_000, 0o644, "test"),
        ("song.mp3", 5_900_000, 0o644, "test"),
        ("clip.mov", 120_000_000, 0o644, "test"),
        ("sketch.svg", 7_600, 0o644, "test"),
        ("index.html", 5_200, 0o644, "test"),
        ("style.css", 2_300, 0o644, "test"),
        ("script.js", 4_100, 0o644, "test"),
        (".zshrc", 2_048, 0o644, "test"),
        (".gitconfig", 312, 0o644, "test"),
        ("hosts", 420, 0o644, "root"),
        ("LICENSE", 11_357, 0o444, "test"),
        ("no-extension", 99, 0o000, "test"),
    ];
    for (name, size, mode, owner) in files {
        entries.push(entry(name, false, size, mode, owner));
    }
    entries
}

fn panel_app(language: Language) -> App {
    let mut app = App::with_fixed_panels().with_window_size_for_test(WINDOW);
    app.columns = PanelColumns {
        left: vec![
            Column::Icon,
            Column::Name,
            Column::Type,
            Column::Permissions,
            Column::Owner,
            Column::Size,
        ],
        right: vec![
            Column::Icon,
            Column::Name,
            Column::Type,
            Column::Permissions,
            Column::Owner,
            Column::Size,
        ],
        ..PanelColumns::default()
    };
    for side in [PanelSide::Left, PanelSide::Right] {
        let panel = app.panel_mut(side);
        panel.path = PathBuf::from("/home/test");
        panel.entries = dense_tree();
        panel.selected = 1;
    }
    let folder = |name: &str, mode, is_target| crate::config::FilingLocation {
        path: app.home.join(name),
        mode,
        images: false,
        is_target,
        transfer: Default::default(),
    };
    let hermes = [
        folder("Public", ncrs_intake::LocationMode::Inbox, false),
        folder("Documents", ncrs_intake::LocationMode::Archive, true),
    ];
    app.special_folders = crate::app::columns::special_folders_of(&hermes, &app.home);
    app.in_language(language)
}

fn matches_snapshot(app: &App, size: iced::Size, name: &str) {
    let matches = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        size,
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_image(format!("tests/snapshots/{name}.png"))
    .expect("snapshot comparison");
    assert!(
        matches,
        "the view does not match tests/snapshots/{name}.png"
    );
}

const LANGUAGES: [(Language, &str); 2] = [(Language::German, "de"), (Language::English, "en")];

#[test]
fn the_panel_shows_icons_and_extra_columns_on_a_dense_tree() {
    for (language, suffix) in LANGUAGES {
        matches_snapshot(
            &panel_app(language),
            WINDOW,
            &format!("panel_columns_{suffix}"),
        );
    }
}

#[test]
fn the_header_menu_matches_its_snapshots() {
    for (language, suffix) in LANGUAGES {
        let mut app = panel_app(language);
        drop(app.update(Message::Panel(PanelMessage::HeaderContext(PanelSide::Left))));
        matches_snapshot(&app, WINDOW, &format!("panel_header_menu_{suffix}"));
    }
}

#[test]
fn the_columns_page_matches_its_snapshots() {
    for (language, suffix) in LANGUAGES {
        let app = App::with_settings_open(SettingsFeature::Columns, Filing::default())
            .in_language(language);
        matches_snapshot(&app, PAGE_WINDOW, &format!("settings_columns_{suffix}"));
    }
}

#[test]
fn the_font_rows_and_their_preview_match_their_snapshots() {
    for (language, suffix) in LANGUAGES {
        let app =
            App::with_settings_open(SettingsFeature::Look, Filing::default()).in_language(language);
        matches_snapshot(&app, PAGE_WINDOW, &format!("settings_font_{suffix}"));
    }
}

#[test]
fn the_permissions_dialog_for_one_file_matches_its_snapshots() {
    for (language, suffix) in LANGUAGES {
        let mut app = panel_app(language);
        app.panel_mut(PanelSide::Left).selected = 17;
        drop(app.update(Message::PermissionsPrompt));
        assert!(app.ops.permissions_dialog.is_some());
        matches_snapshot(&app, WINDOW, &format!("permissions_one_{suffix}"));
    }
}

#[test]
fn the_permissions_dialog_for_several_files_shows_the_mixed_state() {
    for (language, suffix) in LANGUAGES {
        let mut app = panel_app(language);
        for index in [16, 17, 18, 20, 21] {
            app.panel_mut(PanelSide::Left).selected = index;
            drop(app.update(Message::Panel(PanelMessage::ToggleTag)));
        }
        drop(app.update(Message::PermissionsPrompt));
        assert!(app.ops.permissions_dialog.is_some());
        matches_snapshot(&app, WINDOW, &format!("permissions_several_{suffix}"));
    }
}

#[test]
fn the_permissions_dialog_for_a_folder_offers_the_contents() {
    for (language, suffix) in LANGUAGES {
        let mut app = panel_app(language);
        app.panel_mut(PanelSide::Left).selected = 8;
        drop(app.update(Message::PermissionsPrompt));
        drop(app.update(Message::Permissions(PermissionsMessage::ToggleContents)));
        matches_snapshot(&app, WINDOW, &format!("permissions_folder_{suffix}"));
    }
}

fn focus_key(app: &mut App, key: ncrs_settings::focus::FocusKey) {
    drop(app.update(Message::Settings(SettingsMessage::FocusKey(key))));
}

#[test]
fn the_keyboard_moves_the_focus_frame_to_the_font_size_row() {
    use ncrs_settings::focus::{Area, FocusKey};
    for (language, suffix) in LANGUAGES {
        let mut app =
            App::with_settings_open(SettingsFeature::Look, Filing::default()).in_language(language);
        for key in [
            FocusKey::Right,
            FocusKey::Down,
            FocusKey::Down,
            FocusKey::Down,
        ] {
            focus_key(&mut app, key);
        }
        let focus = app.settings.page.as_ref().map(|page| page.focus);
        assert_eq!(
            focus.map(|focus| (focus.area, focus.row)),
            Some((Area::Rows, 3))
        );
        matches_snapshot(&app, PAGE_WINDOW, &format!("settings_font_focus_{suffix}"));

        focus_key(&mut app, FocusKey::Escape);
        let area = app.settings.page.as_ref().map(|page| page.focus.area);
        assert_eq!(area, Some(Area::Pages));
        assert!(
            app.settings.page.is_some(),
            "Escape on the rows must not close the tab"
        );
        focus_key(&mut app, FocusKey::Escape);
        assert!(
            app.settings.page.is_none(),
            "Escape on the pages closes the tab"
        );
    }
}
