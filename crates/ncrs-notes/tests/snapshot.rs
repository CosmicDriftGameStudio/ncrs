//! The notes tab in its three looks, rendered headless.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module
)]

use std::fs;
use std::path::Path;

use iced::Size;
use iced_test::Simulator;
use ncrs_i18n::Language;
use ncrs_notes::store::scan;
use ncrs_notes::tree::RowKey;
use ncrs_notes::{update, view, Ctx, Message, Mode, State, ViewCtx};
use ncrs_ui_kit::palette::ThemePreset;

const CTX: Ctx = Ctx {
    lang: Language::English,
};

const HEATING: &str = "# Heating service\n\nService every October, Krause Haustechnik, phone 089 1234567.\n\n## Open\n\n- Book the 2026 appointment\n- Invoice 2025 filed, see [invoice-2025-10.pdf](file:///home/anna/Documents/invoice-2025-10.pdf)\n\n## Values\n\n- Pressure should be 1.5 bar, measured 1.3 bar\n- Filter last changed 10/2025\n";

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn notes_state(root: &Path) -> State {
    write(root, "House/Heating service.md", HEATING);
    write(root, "House/Groceries.md", "Bread\nMilk\n");
    write(
        root,
        "Travel.md",
        "Italy in May, check the heating of the hotel.\n",
    );
    write(root, "Ideas.md", "- something small\n");
    write(
        root,
        ".files/Offer-1a2b3c4d.md",
        "---\nfile: /home/anna/Documents/House/Offer-Vaillant.pdf\n---\nOffer valid until 31 December, heating with installation 14,800 EUR.\n",
    );
    let mut state = State::new(root.to_owned());
    let mut found = scan(root).unwrap();
    // The file time of a fresh temp file would change the picture.
    for note in &mut found.notes {
        note.modified = None;
    }
    state.open = true;
    let _ = update(&mut state, Message::Loaded(Box::new(Ok(found))), &CTX);
    state.expanded.insert("House".into());
    let note = root.join("House/Heating service.md");
    let _ = update(&mut state, Message::Pressed(RowKey::Note(note)), &CTX);
    state
}

fn picture(state: &State, home: &Path, image: &str) {
    picture_sized(state, home, image, Size::new(1100.0, 640.0));
}

fn picture_sized(state: &State, home: &Path, image: &str, size: Size) {
    let palette = ThemePreset::Cosmic.palette();
    let ctx = ViewCtx {
        lang: Language::English,
        palette: &palette,
        home,
    };
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        size,
        view(state, ctx).map(|message: Message| message),
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(image)
        .unwrap();
    assert!(matches, "the notes picture differs from {image}");
}

#[tokio::test]
async fn notes_tab_view_picture() {
    let dir = tempfile::tempdir().unwrap();
    let state = notes_state(dir.path());
    picture(
        &state,
        Path::new("/home/anna"),
        "tests/snapshots/notes-view-cosmic",
    );
}

#[tokio::test]
async fn notes_tab_edit_picture() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = notes_state(dir.path());
    let _ = update(&mut state, Message::SetMode(Mode::Edit), &CTX);
    picture(
        &state,
        Path::new("/home/anna"),
        "tests/snapshots/notes-edit-cosmic",
    );
}

#[tokio::test]
async fn notes_tab_search_picture() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = notes_state(dir.path());
    let _ = update(
        &mut state,
        Message::SearchChanged("heating".to_owned()),
        &CTX,
    );
    picture(
        &state,
        Path::new("/home/anna"),
        "tests/snapshots/notes-search-cosmic",
    );
}

#[tokio::test]
async fn notes_tab_edit_picture_wide_shows_editor_and_preview_side_by_side() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = notes_state(dir.path());
    let _ = update(&mut state, Message::SetMode(Mode::Edit), &CTX);
    picture_sized(
        &state,
        Path::new("/home/anna"),
        "tests/snapshots/notes-edit-split-cosmic",
        Size::new(1700.0, 640.0),
    );
}
