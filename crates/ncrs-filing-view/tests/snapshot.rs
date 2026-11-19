//! The filing tab mid-reading and done, rendered headless.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    clippy::indexing_slicing
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use iced::Size;
    use iced_test::Simulator;
    use ncrs_filing::chat_work::{LiveFile, LivePlan, LiveRun, LiveState, Override};
    use ncrs_filing_view::{view, Message, Session, State, ViewCtx};
    use ncrs_i18n::Language;
    use ncrs_index::ContentHash;
    use ncrs_ui_kit::palette::ThemePreset;

    const FOLDERS: [&str; 8] = [
        "Rechnungen/Muster GmbH",
        "Rechnungen/Beispiel AG",
        "Verträge",
        "Versicherung",
        "Steuer/2025",
        "Steuer/2026",
        "Gehalt",
        "Garantie",
    ];
    const COMPANIES: [&str; 5] = [
        "Muster GmbH",
        "Beispiel AG",
        "Test KG",
        "Erfinder OHG",
        "Demo GmbH",
    ];
    const FILES: usize = 60;

    fn state_of(index: usize, resolved: usize) -> LiveState {
        if index >= resolved {
            return if index < resolved + 2 {
                LiveState::Looking
            } else {
                LiveState::Waiting
            };
        }
        match index % 10 {
            3 => LiveState::AlreadyThere {
                at: PathBuf::from(FOLDERS[index % FOLDERS.len()])
                    .join(format!("alt-{index:03}.pdf")),
            },
            6 => LiveState::NoFolder,
            9 => LiveState::Unreadable,
            _ => LiveState::Filed {
                folder: PathBuf::from(FOLDERS[index % FOLDERS.len()]),
                name: format!(
                    "{} {} 2026-{:02}.pdf",
                    ["Rechnung", "Vertrag", "Police", "Bescheid"][index % 4],
                    COMPANIES[index % COMPANIES.len()],
                    index % 12 + 1
                ),
                unsure: false,
            },
        }
    }

    fn session(resolved: usize) -> Session {
        let mut session = Session::new(
            PathBuf::from("/home/anna/Scans"),
            PathBuf::from("/home/anna/Dokumente"),
            Arc::new(LiveRun::default()),
        );
        let files = (0..FILES)
            .map(|index| LiveFile {
                from: PathBuf::from(format!("scan-{index:03}.pdf")),
                hash: Some(ContentHash([1; 32])),
                state: state_of(index, resolved),
                by_user: false,
            })
            .collect();
        session.plan = Some(LivePlan {
            source_root: session.source.clone(),
            target_root: session.target.clone(),
            files,
            done: resolved >= FILES,
            folder_choice_unsupported: false,
        });
        for (at, folder) in FOLDERS.iter().enumerate() {
            let there = (0..at % 6 + 1)
                .map(|number| format!("alt-{number:03}.pdf"))
                .collect();
            session.existing.insert(PathBuf::from(folder), there);
        }
        session.selected = Some(PathBuf::from("scan-004.pdf"));
        session
    }

    fn picture(state: &State, image: &str) {
        let palette = ThemePreset::Cosmic.palette();
        let ctx = ViewCtx {
            lang: Language::German,
            palette: &palette,
            window: Size::new(1740.0, 1130.0),
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            Size::new(1740.0, 1130.0),
            view(state, ctx).map(|message: Message| message),
        );
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(image)
            .unwrap();
        assert!(matches, "the filing picture differs from {image}");
    }

    #[test]
    fn filing_tab_mid_reading_picture() {
        let state = State {
            session: Some(session(34)),
        };
        picture(&state, "tests/snapshots/filing-reading-cosmic");
    }

    #[test]
    fn filing_tab_done_picture() {
        let mut session = session(FILES);
        session.overrides.insert(
            PathBuf::from("scan-004.pdf"),
            Override::Name("Rechnung Muster GmbH 2026-05 Heizung.pdf".to_owned()),
        );
        let state = State {
            session: Some(session),
        };
        picture(&state, "tests/snapshots/filing-done-cosmic");
    }
}
