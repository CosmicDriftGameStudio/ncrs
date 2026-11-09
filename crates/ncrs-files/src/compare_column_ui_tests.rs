// reason: a failing assertion is the signal in a test
#![allow(clippy::unwrap_used)]

//! While folders are compared every row reserves the Compare column, so Size
//! and Modified stay in line on the `..` row and on rows without a mark.

use std::ffi::OsString;
use std::path::PathBuf;

use iced::Size;
use iced_test::Simulator;
use ncrs_core::{CompareMark, CompareMarks, EntryDetails, FileEntry, PanelSide};
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::ThemePreset;

use crate::ctx::{ColumnSettings, ViewCtx};
use crate::panel::PanelState;
use crate::view::view;

fn entry(name: &str, is_parent: bool, size: u64) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: PathBuf::from("/work").join(name),
        is_dir: is_parent,
        is_symlink: false,
        is_parent,
        size,
        modified: None,
        folder_size: None,
        details: EntryDetails::default(),
    }
}

#[test]
fn rows_without_a_mark_keep_the_compare_column_free() {
    let palette = ThemePreset::Dracula.palette();
    let mut state = PanelState::new(PathBuf::from("/work"));
    state.entries = vec![
        entry("..", true, 0),
        entry("marked.txt", false, 2_048),
        entry("unmarked.txt", false, 4_096),
    ];
    let marks = CompareMarks::from([(OsString::from("marked.txt"), CompareMark::NewerHere)]);
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        Size::new(720.0, 200.0),
        view(
            &state,
            ViewCtx {
                side: PanelSide::Left,
                lang: Language::English,
                palette,
                is_active: true,
                visible_rows: 6,
                compare_marks: Some(&marks),
                hermes_cell: None,
                columns: ColumnSettings::STANDARD,
                special_folders: &[],
                header_drag: None,
            },
        ),
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image("tests/snapshots/compare-column-blank-cells.png")
        .unwrap();
    assert!(matches, "the Compare column differs from its image");
}
