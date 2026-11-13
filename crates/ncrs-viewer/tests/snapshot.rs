//! The viewer tab with a README-like Markdown, rendered headless.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {

    use std::fs;
    use std::path::Path;

    use iced::Size;
    use iced_test::Simulator;
    use ncrs_i18n::Language;
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_viewer::load::{load, Doc, Loaded};
    use ncrs_viewer::{view, Message, State, ViewCtx};

    const README: &str = "# ncrs\n\nA two-panel file manager for macOS, Linux and Windows, with an index of your documents and **Hermes**, which reads, names and files them for you.\n\n## Install\n\n- macOS: `brew install cosmicdrift/ncrs/ncrs`\n- Debian, Ubuntu: `sudo apt install ncrs`\n- Fedora: `sudo dnf install ncrs`\n\n## Keys\n\n1. F3 views a file\n2. F4 edits it\n3. F5 copies, F6 moves\n\nSee the [guide](docs/guide.md) or the [site](https://example.org).\n";

    fn state_with_readme(dir: &Path) -> State {
        let path = dir.join("README.md");
        fs::write(&path, README).unwrap();
        let Loaded::Doc(doc) = load(path) else {
            panic!("not loaded");
        };
        State {
            // The file time of a fresh temp file would change the picture.
            doc: Some(Doc {
                modified: None,
                ..doc
            }),
            ..State::default()
        }
    }

    #[test]
    fn viewer_tab_picture() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_with_readme(dir.path());
        let palette = ThemePreset::Cosmic.palette();
        let ctx = ViewCtx {
            lang: Language::English,
            palette: &palette,
            home: dir.path(),
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            Size::new(1100.0, 640.0),
            view(&state, ctx).map(|message: Message| message),
        );
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/viewer-readme-cosmic.png")
            .unwrap();
        assert!(matches, "the viewer picture differs from its image");
    }
}
