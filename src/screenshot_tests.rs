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
use ncrs_i18n::Language;

use crate::app::{fixed_result_entry, App};
use crate::fs::search::LineMatch;
use crate::fs::{FileEntry, Volume};
use crate::messages::{
    BatchRenameMessage, FavouritesBarMessage, Message, PanelMessage, PanelSide, SettingField,
    SettingsFeature, SettingsMessage,
};
use ncrs_core::CompareMark;
use ncrs_settings::AiCheckResult;

const OUTPUT_DIRECTORY: &str = "docs/screenshots";
const WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 640.0);
const VISIBLE_ROWS: usize = 17;
/// Taller than the panel shots, so the review form fits without scrolling.
const FILING_WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 800.0);
const STORE_DIRECTORY: &str = "docs/store-screenshots";
/// 16:10 as the Mac App Store requires; rendered at 2x it is 2560×1600.
const STORE_WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 800.0);

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
        folder_size: None,
        details: Default::default(),
    };
    let listed = rows.iter().map(|row| FileEntry {
        name: OsString::from(row.name),
        path: PathBuf::from(directory).join(row.name),
        is_dir: row.is_dir,
        is_symlink: false,
        is_parent: false,
        size: row.size,
        modified: Some(local_time(row.modified)),
        folder_size: None,
        details: Default::default(),
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
    let mut app = App::with_fixed_panels().with_version_for_test(env!("CARGO_PKG_VERSION"));
    app.set_visible_rows_for_test(VISIBLE_ROWS);
    app = app.with_favourites_for_test(FAVOURITES);

    let project = app.panel_mut_for_test(PanelSide::Left);
    project.path = PathBuf::from(PROJECT_DIRECTORY);
    project.entries = entries_of(PROJECT_DIRECTORY, PROJECT_ROWS);
    project.selected = position_of(&project.entries, "README.md");
    for name in ["Cargo.toml", "CHANGELOG.md"] {
        project
            .selection
            .toggle(&PathBuf::from(PROJECT_DIRECTORY).join(name));
    }

    let downloads = app.panel_mut_for_test(PanelSide::Right);
    downloads.path = PathBuf::from(DOWNLOADS_DIRECTORY);
    downloads.entries = entries_of(DOWNLOADS_DIRECTORY, DOWNLOADS_ROWS);
    downloads.selected = position_of(&downloads.entries, "project-backup.zip");
    app
}

const BACKUP_DIRECTORY: &str = "/Volumes/Backup/ncrs";

const BACKUP_ROWS: &[Row] = &[
    dir("assets", (2026, 9, 14, 18, 2)),
    dir("docs", (2026, 9, 20, 9, 12)),
    dir("src", (2026, 9, 30, 17, 3)),
    dir(".github", (2026, 8, 30, 21, 5)),
    file(".gitignore", 41, (2026, 7, 3, 12, 30)),
    file("build.rs", 3_212, (2026, 9, 2, 10, 12)),
    file("Cargo.lock", 91_200, (2026, 9, 25, 8, 30)),
    file("Cargo.toml", 1_902, (2026, 10, 2, 7, 10)),
    file("CHANGELOG.md", 7_980, (2026, 10, 2, 7, 12)),
    file("LICENSE", 1_068, (2026, 7, 3, 12, 30)),
    file("notes.txt", 2_204, (2026, 9, 27, 20, 40)),
    file("README.md", 13_100, (2026, 9, 29, 12, 0)),
    file("strings.json", 52_980, (2026, 9, 30, 19, 33)),
    file("todo-old.md", 874, (2026, 6, 14, 10, 5)),
];

const FAVOURITES: &[&str] = &[
    "/Users/alex/Projects/ncrs",
    "/Users/alex/Downloads",
    "/Users/alex/Documents",
    "/Users/alex/Downloads/Invoices",
    "/Users/alex/Pictures",
];

/// Left rows with the mark the left side gets; the right side's marks follow.
const LEFT_MARKS: &[(&str, CompareMark)] = &[
    ("assets", CompareMark::Same),
    ("docs", CompareMark::NewerHere),
    ("src", CompareMark::NewerHere),
    ("target", CompareMark::OnlyHere),
    (".github", CompareMark::Same),
    (".gitignore", CompareMark::Same),
    ("build.rs", CompareMark::Same),
    ("Cargo.lock", CompareMark::NewerHere),
    ("Cargo.toml", CompareMark::OlderHere),
    ("CHANGELOG.md", CompareMark::OlderHere),
    ("deny.toml", CompareMark::OnlyHere),
    ("LICENSE", CompareMark::Same),
    ("README.md", CompareMark::NewerHere),
    ("rust-toolchain.toml", CompareMark::OnlyHere),
    ("strings.json", CompareMark::Same),
];
const RIGHT_ONLY: &[&str] = &["notes.txt", "todo-old.md"];

fn compared_showcase() -> ncrs_compare::Compared {
    let mut left = ncrs_core::CompareMarks::new();
    let mut right = ncrs_core::CompareMarks::new();
    for &(name, mark) in LEFT_MARKS {
        left.insert(OsString::from(name), mark);
        let right_mark = match mark {
            CompareMark::NewerHere => Some(CompareMark::OlderHere),
            CompareMark::OlderHere => Some(CompareMark::NewerHere),
            CompareMark::OnlyHere => None,
            same_or_different => Some(same_or_different),
        };
        if let Some(other) = right_mark {
            right.insert(OsString::from(name), other);
        }
    }
    for name in RIGHT_ONLY {
        right.insert(OsString::from(*name), CompareMark::OnlyHere);
    }
    let rows = |directory: &str, rows: &[Row]| {
        entries_of(directory, rows)
            .into_iter()
            .filter(|entry| !entry.is_parent)
            .collect()
    };
    ncrs_compare::Compared {
        left,
        right,
        left_rows: rows(PROJECT_DIRECTORY, PROJECT_ROWS),
        right_rows: rows(BACKUP_DIRECTORY, BACKUP_ROWS),
    }
}

fn compare_app() -> App {
    let mut app = showcase_app();
    let backup = app.panel_mut_for_test(PanelSide::Right);
    backup.path = PathBuf::from(BACKUP_DIRECTORY);
    backup.entries = entries_of(BACKUP_DIRECTORY, BACKUP_ROWS);
    backup.selected = 0;
    app.with_comparison_for_test(compared_showcase())
}

fn batch_rename_app() -> App {
    let mut app = showcase_app();
    app.activate_panel_for_test(PanelSide::Right);
    for name in ["holiday-001.jpg", "holiday-002.jpg", "holiday-003.jpg"] {
        app.panel_mut_for_test(PanelSide::Right)
            .selection
            .toggle(&PathBuf::from(DOWNLOADS_DIRECTORY).join(name));
    }
    let _opened = app.update(Message::Panel(PanelMessage::RenameStart));
    let _typed = app.update(Message::BatchRename(BatchRenameMessage::PatternInput(
        "Holiday {date modified} {counter}{extension}".to_owned(),
    )));
    app
}

fn own_server_app() -> App {
    let mut app = showcase_app().showing_setting(SettingsFeature::Index);
    drop(app.update(Message::Settings(SettingsMessage::ServerDialog(true))));
    drop(
        app.update(Message::Settings(SettingsMessage::ChooseProvider(
            ncrs_config::config::AiProviderName::Http,
        ))),
    );
    for (field, text) in [
        (SettingField::AiEndpoint, "https://openrouter.ai/api/v1"),
        (SettingField::AiModel, "openai/gpt-5-mini"),
    ] {
        drop(app.update(Message::Settings(SettingsMessage::Edit {
            field,
            text: text.to_owned(),
        })));
    }
    drop(app.update(Message::Settings(SettingsMessage::UnderstandLeaving(true))));
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let _entered = runtime.enter();
    drop(app.update(Message::Settings(SettingsMessage::TestAiServer)));
    let models = [
        "openai/gpt-5-mini",
        "openai/gpt-5",
        "anthropic/claude-sonnet-4.5",
        "google/gemini-2.5-flash",
        "meta-llama/llama-3.3-70b-instruct",
    ];
    drop(app.update(Message::Settings(SettingsMessage::AiChecked {
        endpoint: "https://openrouter.ai/api/v1".to_owned(),
        result: AiCheckResult::Listed(models.map(str::to_owned).to_vec()),
    })));
    app
}

fn showcase_search_results() -> Vec<(FileEntry, Vec<LineMatch>)> {
    let root = PathBuf::from(PROJECT_DIRECTORY);
    let hit = |number: u64, text: &str, from: usize, to: usize| LineMatch {
        number,
        text: text.to_string(),
        ranges: std::iter::once(from..to).collect(),
    };
    let entry = |relative: &str, size: u64, date: (i32, u32, u32, u32, u32)| {
        fixed_result_entry(root.join(relative), size, date)
    };
    vec![
        (
            entry("README.md", 6_144, (2026, 10, 1, 10, 14)),
            vec![hit(12, "Say hello to a two-panel file manager.", 4, 9)],
        ),
        (
            entry("src/main.rs", 18_432, (2026, 10, 2, 11, 5)),
            vec![
                hit(8, "    println!(\"hello, world\");", 14, 19),
                hit(41, "    // hello from the main window", 7, 12),
            ],
        ),
        (
            entry("docs/guide.md", 12_288, (2026, 9, 18, 15, 36)),
            vec![hit(3, "A first hello for new users.", 8, 13)],
        ),
    ]
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
    write_screenshot_to(app, OUTPUT_DIRECTORY, WINDOW_SIZE, file_name);
}

pub fn write_screenshot_to(app: &App, directory: &str, size: iced::Size, file_name: &str) {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let mut simulator = Simulator::with_size(iced::Settings::default(), size, app.view());
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

    std::fs::create_dir_all(directory).expect("output directory");
    let target = PathBuf::from(directory).join(file_name);
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

        let mut context = showcase_app();
        context.set_pointer_for_test(iced::Point::new(260.0, 150.0));
        drop(context.update(Message::ContextMenuAt {
            side: PanelSide::Left,
            index: 5,
        }));
        write_screenshot(&context, "context-menu.png");

        let mut delete = showcase_app();
        drop(delete.update(Message::Delete { permanent: false }));
        write_screenshot(&delete, "delete.png");

        let mut search = showcase_app();
        search.show_search_results_for_test("hello", showcase_search_results());
        write_screenshot(&search, "search.png");

        let search_filters = crate::ui_tests::search_filters_card::app_with_filters();
        write_screenshot(&search_filters, "search-filters.png");

        let filing = App::with_settings_open(
            crate::messages::SettingsFeature::Hermes,
            crate::config::Filing::default(),
        );
        write_screenshot_to(&filing, OUTPUT_DIRECTORY, FILING_WINDOW_SIZE, "filing.png");

        let mut favourites = showcase_app();
        favourites.set_pointer_for_test(iced::Point::new(105.0, 58.0));
        drop(favourites.update(Message::FavouritesBar(FavouritesBarMessage::Menu(1))));
        write_screenshot(&favourites, "favourites.png");

        write_screenshot(&compare_app(), "compare.png");

        write_screenshot_to(
            &batch_rename_app(),
            OUTPUT_DIRECTORY,
            FILING_WINDOW_SIZE,
            "batch-rename.png",
        );

        write_screenshot_to(
            &own_server_app(),
            OUTPUT_DIRECTORY,
            FILING_WINDOW_SIZE,
            "own-server.png",
        );
    }

    #[test]
    #[ignore = "writes docs/store-screenshots; run with `cargo test store_screenshots -- --ignored`"]
    fn store_screenshots() {
        for (file_prefix, language) in [("en", Language::English), ("de", Language::German)] {
            let showcase = || showcase_app().in_language(language);

            let main = showcase();

            let mut search = showcase();
            search.show_search_results_for_test("hello", showcase_search_results());

            let mut context = showcase();
            context.set_pointer_for_test(iced::Point::new(260.0, 150.0));
            drop(context.update(Message::ContextMenuAt {
                side: PanelSide::Left,
                index: 5,
            }));

            let mut menu = showcase();
            menu.open_volume_menu_for_test(PanelSide::Left, drives());

            let mut delete = showcase();
            drop(delete.update(Message::Delete { permanent: false }));

            let scenes = [
                ("1-main", main),
                ("2-search", search),
                ("3-context-menu", context),
                ("4-drive-menu", menu),
                ("5-delete", delete),
            ];
            for (name, app) in scenes {
                write_screenshot_to(
                    &app,
                    STORE_DIRECTORY,
                    STORE_WINDOW_SIZE,
                    &format!("{file_prefix}-{name}.png"),
                );
            }
        }
    }
}
