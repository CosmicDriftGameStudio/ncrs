//! The start dialog, the sync dialog with its delete list and the compare bar.

// reason: a failing assertion is the signal in a test
#![allow(clippy::unwrap_used)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Size;
use iced_test::Simulator;
use ncrs_core::{CompareMark, CompareMarks, FileEntry};
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::{Palette, ThemePreset};
use ncrs_ui_kit::APP_FONT;

use crate::core::{Compared, Direction};
use crate::message::Message;
use crate::state::{Compare, Folders, Run, SyncDialog};
use crate::view;

const WINDOW: Size = Size::new(1200.0, 800.0);
const LEFT: &str = "/Users/marc/Documents/Scans";
const RIGHT: &str = "/Volumes/Archiv/Scans";

fn entry(side: &str, name: &str, size: u64, is_dir: bool) -> FileEntry {
    let mut entry = FileEntry::parent(Path::new("/"));
    entry.name = OsString::from(name);
    entry.path = PathBuf::from(side).join(name);
    entry.is_parent = false;
    entry.is_dir = is_dir;
    entry.size = size;
    entry
}

fn compared() -> Compared {
    let left_rows = [
        ("2026-Eingang", 0, true, CompareMark::OnlyHere),
        ("2023", 0, true, CompareMark::Different),
        ("2025", 0, true, CompareMark::Different),
        ("Rechnungen", 0, true, CompareMark::Different),
        (
            "Brother-2026-09-12-001.pdf",
            412_672,
            false,
            CompareMark::NewerHere,
        ),
        (
            "Brother-2026-10-01-001.pdf",
            124_928,
            false,
            CompareMark::OnlyHere,
        ),
        (
            "Brother-2026-10-01-002.pdf",
            86_016,
            false,
            CompareMark::OnlyHere,
        ),
        (
            "Brother-2026-10-09-001.pdf",
            96_256,
            false,
            CompareMark::Different,
        ),
        (
            "Brother-2026-10-12-001.pdf",
            184_320,
            false,
            CompareMark::OnlyHere,
        ),
        (
            "Brother-2026-10-19-001.pdf",
            2_202_009,
            false,
            CompareMark::NewerHere,
        ),
        (
            "Brother-2026-10-28-001.pdf",
            12_288,
            false,
            CompareMark::OnlyHere,
        ),
        (
            "Brother-2026-11-01-001.pdf",
            124_928,
            false,
            CompareMark::NewerHere,
        ),
        (
            "Brother-2026-08-01-001.pdf",
            70_000,
            false,
            CompareMark::Same,
        ),
    ];
    let right_rows = [
        ("Alt-Bank", 0, true, CompareMark::OnlyHere),
        (
            "Brother-2026-10-15-001.pdf",
            1024,
            false,
            CompareMark::OnlyHere,
        ),
        (
            "Brother-2026-11-03-001.pdf",
            1_009_664,
            false,
            CompareMark::OnlyHere,
        ),
        (
            "Brother-2026-11-08-002.pdf",
            18_739,
            false,
            CompareMark::OnlyHere,
        ),
    ];
    let marks = |rows: &[(&str, u64, bool, CompareMark)]| -> CompareMarks {
        rows.iter()
            .map(|(name, _, _, mark)| (OsString::from(*name), *mark))
            .collect()
    };
    let rows = |side: &str, rows: &[(&str, u64, bool, CompareMark)]| -> Vec<FileEntry> {
        rows.iter()
            .map(|(name, size, is_dir, _)| entry(side, name, *size, *is_dir))
            .collect()
    };
    Compared {
        left: marks(&left_rows),
        right: marks(&right_rows),
        left_rows: rows(LEFT, &left_rows),
        right_rows: rows(RIGHT, &right_rows),
    }
}

fn with_run(sync: Option<SyncDialog>) -> Compare {
    Compare {
        run: Some(Run {
            folders: Folders {
                left: PathBuf::from(LEFT),
                right: PathBuf::from(RIGHT),
            },
            generation: 1,
            stop: Arc::new(AtomicBool::new(false)),
            result: Some(compared()),
            only_differences: false,
        }),
        sync,
        ..Compare::default()
    }
}

fn palette() -> Palette {
    ThemePreset::Cosmic.palette()
}

fn simulator<'a>(content: iced::Element<'a, Message>, size: Size) -> Simulator<'a, Message> {
    Simulator::with_size(
        iced::Settings {
            default_font: APP_FONT,
            ..iced::Settings::default()
        },
        size,
        content,
    )
}

/// Named for CI, which compares pixels only on macOS.
mod ui_tests {
    use super::*;

    fn matches_image(content: iced::Element<'_, Message>, size: Size, image: &str) {
        let mut simulator = simulator(content, size);
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(image)
            .unwrap();
        assert!(matches, "{image} differs");
    }

    #[test]
    fn the_start_dialog_matches_its_image() {
        let compare = Compare {
            start: Some(Folders {
                left: PathBuf::from(LEFT),
                right: PathBuf::from(RIGHT),
            }),
            ..Compare::default()
        };
        let dialog = view::dialog(&compare, WINDOW, Language::English, &palette()).unwrap();
        matches_image(dialog, WINDOW, "tests/snapshots/compare-start.png");
    }

    #[test]
    fn the_sync_dialog_with_the_delete_list_matches_its_image() {
        let compare = with_run(Some(SyncDialog {
            direction: Direction::ToRight,
            delete: true,
            copy_window: ncrs_ui_kit::list_window::ListWindow::default(),
            delete_window: ncrs_ui_kit::list_window::ListWindow::default(),
        }));
        let (_, plan) = compare.plan().unwrap();
        assert_eq!(plan.copies.len(), 12);
        assert_eq!(plan.deletions.len(), 4);
        let dialog = view::dialog(&compare, WINDOW, Language::English, &palette()).unwrap();
        matches_image(dialog, WINDOW, "tests/snapshots/compare-sync-delete.png");
    }

    #[test]
    fn the_compare_bar_matches_its_image() {
        let compare = with_run(None);
        let bar = view::bar(&compare, Language::English, &palette()).unwrap();
        matches_image(
            bar,
            Size::new(1200.0, 32.0),
            "tests/snapshots/compare-bar.png",
        );
    }
}
