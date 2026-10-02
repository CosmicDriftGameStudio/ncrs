//! Generator for the README screenshots. Not a test: it asserts nothing about
//! the app and is ignored by default, so the normal suite never touches
//! `docs/screenshots/`. Regenerate with
//! `cargo test readme_screenshots -- --ignored`.
#![allow(clippy::expect_used)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

use chrono::{Local, TimeZone as _};
use iced_test::Simulator;

use crate::app::App;
use crate::fs::{FileEntry, Volume};
use crate::messages::{Message, PanelSide};

const OUTPUT_DIRECTORY: &str = "docs/screenshots";
const WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 640.0);
const VISIBLE_ROWS: usize = 17;

struct Row {
    name: &'static str,
    is_dir: bool,
    size: u64,
    modified: (i32, u32, u32, u32, u32),
}

const fn dir(name: &'static str, modified: (i32, u32, u32, u32, u32)) -> Row {
    Row {
        name,
        is_dir: true,
        size: 0,
        modified,
    }
}

const fn file(name: &'static str, size: u64, modified: (i32, u32, u32, u32, u32)) -> Row {
    Row {
        name,
        is_dir: false,
        size,
        modified,
    }
}

const PROJECT_DIRECTORY: &str = "/Users/alex/Projects/ncrs";
const DOWNLOADS_DIRECTORY: &str = "/Users/alex/Downloads";

const PROJECT_ROWS: &[Row] = &[
    dir("assets", (2026, 9, 14, 18, 2)),
    dir("docs", (2026, 10, 1, 9, 41)),
    dir("src", (2026, 10, 2, 8, 17)),
    dir("target", (2026, 10, 2, 8, 20)),
    dir(".github", (2026, 8, 30, 21, 5)),
    file(".gitignore", 41, (2026, 7, 3, 12, 30)),
    file("build.rs", 3_212, (2026, 9, 2, 10, 12)),
    file("Cargo.lock", 98_304, (2026, 10, 1, 17, 48)),
    file("Cargo.toml", 1_873, (2026, 10, 1, 17, 46)),
    file("CHANGELOG.md", 7_410, (2026, 9, 28, 20, 15)),
    file("deny.toml", 936, (2026, 8, 11, 14, 3)),
    file("LICENSE", 1_068, (2026, 7, 3, 12, 30)),
    file("README.md", 14_562, (2026, 10, 2, 8, 5)),
    file("rust-toolchain.toml", 64, (2026, 7, 21, 9, 9)),
    file("strings.json", 52_980, (2026, 9, 30, 19, 33)),
];

const DOWNLOADS_ROWS: &[Row] = &[
    dir("Invoices", (2026, 9, 3, 16, 20)),
    dir("Wallpapers", (2026, 8, 19, 11, 47)),
    file("boarding-pass.pdf", 184_320, (2026, 9, 29, 7, 55)),
    file(
        "DaVinci_Resolve_19.dmg",
        2_516_582_400,
        (2026, 9, 12, 22, 10),
    ),
    file("holiday-001.jpg", 4_718_592, (2026, 8, 24, 13, 2)),
    file("holiday-002.jpg", 5_033_164, (2026, 8, 24, 13, 3)),
    file("holiday-003.jpg", 3_984_588, (2026, 8, 24, 13, 3)),
    file("invoice-2026-09.pdf", 96_256, (2026, 10, 1, 10, 14)),
    file("logo-final.png", 412_672, (2026, 9, 18, 15, 36)),
    file("project-backup.zip", 156_237_824, (2026, 9, 26, 23, 59)),
    file(
        "screenshot-2026-09-30.png",
        1_153_433,
        (2026, 9, 30, 12, 21),
    ),
    file("tax-return-2025.pdf", 1_468_006, (2026, 5, 6, 18, 44)),
    file("vlc-3.0.21.dmg", 63_963_136, (2026, 7, 9, 9, 28)),
];

fn local_time((year, month, day, hour, minute): (i32, u32, u32, u32, u32)) -> SystemTime {
    // Built from local civil time, because the panel prints local time: the
    // image then shows the same text whatever time zone it is made in.
    Local
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .map(SystemTime::from)
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn entries_of(directory: &str, rows: &[Row]) -> Vec<FileEntry> {
    let parent = FileEntry {
        name: OsString::from(".."),
        path: PathBuf::from(directory).join(".."),
        is_dir: true,
        is_symlink: false,
        is_parent: true,
        size: 0,
        modified: None,
    };
    let listed = rows.iter().map(|row| FileEntry {
        name: OsString::from(row.name),
        path: PathBuf::from(directory).join(row.name),
        is_dir: row.is_dir,
        is_symlink: false,
        is_parent: false,
        size: row.size,
        modified: Some(local_time(row.modified)),
    });
    std::iter::once(parent).chain(listed).collect()
}

fn position_of(entries: &[FileEntry], name: &str) -> usize {
    entries
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_default()
}

/// Two realistic panels. The cursor is on `README.md`, `Cargo.toml` and
/// `CHANGELOG.md` are tagged.
fn showcase_app() -> App {
    let mut app = App::with_fixed_panels();
    app.set_visible_rows_for_test(VISIBLE_ROWS);

    let project = app.panel_mut_for_test(PanelSide::Left);
    project.path = PathBuf::from(PROJECT_DIRECTORY);
    project.entries = entries_of(PROJECT_DIRECTORY, PROJECT_ROWS);
    project.selected = position_of(&project.entries, "README.md");
    for name in ["Cargo.toml", "CHANGELOG.md"] {
        project.selection.toggle(&OsString::from(name));
    }

    let downloads = app.panel_mut_for_test(PanelSide::Right);
    downloads.path = PathBuf::from(DOWNLOADS_DIRECTORY);
    downloads.entries = entries_of(DOWNLOADS_DIRECTORY, DOWNLOADS_ROWS);
    downloads.selected = position_of(&downloads.entries, "project-backup.zip");
    app
}

fn drives() -> Vec<Volume> {
    [
        ("Macintosh HD", "/"),
        ("Alex (Home)", "/Users/alex"),
        ("iCloud Drive", "/Users/alex/iCloud"),
        ("Backup (USB)", "/Volumes/Backup"),
        ("NAS", "/Volumes/NAS"),
    ]
    .into_iter()
    .map(|(name, path)| Volume {
        name: name.to_string(),
        path: PathBuf::from(path),
    })
    .collect()
}

/// `matches_image` writes a missing reference and appends the renderer's name
/// to the file name, so it renders into an empty directory and the single
/// file that appears there is copied to the wanted name.
fn write_screenshot(app: &App, file_name: &str) {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let mut simulator = Simulator::with_size(iced::Settings::default(), WINDOW_SIZE, app.view());
    simulator
        .snapshot(&iced::Theme::Dark)
        .expect("the view should render")
        .matches_image(scratch.path().join("shot.png"))
        .expect("the screenshot should be written");
    let written = std::fs::read_dir(scratch.path())
        .expect("scratch directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "png"))
        .expect("the renderer wrote no png");

    std::fs::create_dir_all(OUTPUT_DIRECTORY).expect("output directory");
    let target = PathBuf::from(OUTPUT_DIRECTORY).join(file_name);
    std::fs::copy(&written, &target).expect("copy the screenshot");
    assert!(target.exists(), "{} was not written", target.display());
}

mod tests {
    use super::*;

    #[test]
    #[ignore = "writes docs/screenshots; run with `cargo test readme_screenshots -- --ignored`"]
    fn readme_screenshots() {
        let mut app = showcase_app();
        write_screenshot(&app, "main.png");

        drop(app.update(Message::SwitchLanguage));
        write_screenshot(&app, "main-de.png");
        drop(app.update(Message::SwitchLanguage));

        let mut menu = showcase_app();
        menu.open_volume_menu_for_test(PanelSide::Left, drives());
        write_screenshot(&menu, "drive-menu.png");

        let mut delete = showcase_app();
        drop(delete.update(Message::Delete { permanent: false }));
        write_screenshot(&delete, "delete.png");
    }
}
