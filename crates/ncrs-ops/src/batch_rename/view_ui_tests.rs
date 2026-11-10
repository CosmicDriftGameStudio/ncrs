//! The batch rename dialog with a clash in every row after the first.

// reason: a failing assertion is the signal in a test
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use iced::Size;
use iced_test::Simulator;
use ncrs_core::PanelSide;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::ThemePreset;
use ncrs_ui_kit::APP_FONT;

use super::pattern::{split_name, EntryFacts, NameRules};
use super::state::{BatchRename, Opening};
use super::view::view;

const WINDOW: Size = Size::new(1200.0, 800.0);

fn dialog_with_clashes() -> BatchRename {
    let names: Vec<String> = (1..=18)
        .map(|number| format!("scan {number:02}.pdf"))
        .collect();
    let entries = names
        .iter()
        .map(|name| {
            let (stem, extension) = split_name(name, false);
            (
                PathBuf::from("/Users/marc/Documents/Scans").join(name),
                EntryFacts {
                    stem,
                    extension,
                    ..EntryFacts::default()
                },
            )
        })
        .collect();
    let mut dialog = BatchRename::new(Opening {
        id: 1,
        side: PanelSide::Left,
        folder: PathBuf::from("/Users/marc/Documents/Scans"),
        entries,
        folder_names: names,
        rules: NameRules {
            windows: false,
            fold_case: false,
        },
    });
    dialog.pattern = "invoice{extension}".to_owned();
    dialog.refresh();
    dialog
}

/// Named for CI, which compares pixels only on macOS.
mod ui_tests {
    use super::*;

    #[test]
    fn the_dialog_with_clashes_and_eighteen_rows_matches_its_image() {
        let dialog = dialog_with_clashes();
        assert!(dialog.preview.clashes() > 0);
        let palette = ThemePreset::Cosmic.palette();
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            view(&dialog, WINDOW, Language::English, &palette),
        );

        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/batch-rename-clashes.png")
            .unwrap();

        assert!(matches, "the batch rename dialog differs from its image");
    }
}
