//! The Platz tab at the window size Marc works in, rendered headless: 70 rows, the
//! dialog, the result with a skipped row, the decide pane and the store build.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::path::Path;

    use iced::Size;
    use iced_test::Simulator;
    use ncrs_i18n::Language;
    use ncrs_space::decide::{DownloadGroups, DownloadItem, Matched};
    use ncrs_space::{LogRow, Skip};
    use ncrs_space_view::{view, Ctx, DecideMessage, Message, SpaceKeys, State, ViewCtx};
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    use crate::common::{crowded_scan, ctx, log_of, now, path, send, tab, GIB, HOME};

    const WINDOW: Size = Size::new(1740.0, 1130.0);

    fn with_simulator<R>(
        state: &State,
        app: &Ctx,
        ask: impl FnOnce(&mut Simulator<'_, Message>) -> R,
    ) -> R {
        let palette = ThemePreset::Cosmic.palette();
        let keys = SpaceKeys::defaults_for(true);
        let ctx = ViewCtx {
            disk_warning: None,
            lang: Language::English,
            palette: &palette,
            window: WINDOW,
            keys: &keys,
            home: Path::new(HOME),
            now: now(),
            store_build: app.store_build,
            processes_known: app.processes_known,
            trash_can_purge: app.trash_can_purge,
            warn_from_percent: 90,
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            view(state, ctx).map(|message: Message| message),
        );
        ask(&mut simulator)
    }

    fn matches_picture(state: &State, app: &Ctx, reference: &str) -> bool {
        with_simulator(state, app, |simulator| {
            simulator
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_reference_image(reference)
                .unwrap()
        })
    }

    #[test]
    fn space_overview() {
        let app = ctx();
        let (state, _) = tab(crowded_scan(true), &app);
        assert!(state.rows().len() >= 60);
        assert!(matches_picture(
            &state,
            &app,
            "tests/snapshots/space-overview"
        ));
    }

    #[test]
    fn space_confirm() {
        let app = ctx();
        let (mut state, _) = tab(crowded_scan(true), &app);
        send(&mut state, Message::DeleteSafe, &app);
        assert!(matches_picture(
            &state,
            &app,
            "tests/snapshots/space-confirm"
        ));
    }

    #[test]
    fn space_confirm_without_process_list_in_the_store_build() {
        let mut app = ctx();
        app.store_build = true;
        app.processes_known = false;
        let (mut state, _) = tab(crowded_scan(false), &app);
        send(&mut state, Message::DeleteSafe, &app);
        assert!(matches_picture(
            &state,
            &app,
            "tests/snapshots/space-confirm-store"
        ));
    }

    fn measured_after_run(free: u64) -> std::sync::Arc<ncrs_space::Scan> {
        let mut scan = (*crowded_scan(true)).clone();
        scan.volume.available = free;
        std::sync::Arc::new(scan)
    }

    #[test]
    fn space_result_with_a_skipped_row() {
        let app = ctx();
        let (mut state, _) = tab(crowded_scan(true), &app);
        send(&mut state, Message::DeleteSafe, &app);
        send(&mut state, Message::ConfirmAccept, &app);
        let run = state.running().unwrap().run;
        let freed: u64 = (0..24)
            .filter(|place| *place != 5)
            .map(|place| u64::div_euclid((place + 2) * GIB, 3))
            .sum();
        let free_after = 78 * GIB + freed;
        let rows = (0..24)
            .map(|place| LogRow {
                path: path(&format!("/Users/marc/.cargo-target/ncrs-{place}")),
                size: u64::div_euclid((place + 2) * GIB, 3),
                outcome: if place == 5 {
                    Err(Skip::UseUnknown)
                } else {
                    Ok(u64::div_euclid((place + 2) * GIB, 3))
                },
                last_used: None,
                use_checked: false,
            })
            .collect();
        send(
            &mut state,
            Message::RunFinished(ncrs_space_view::Finished {
                run,
                log: log_of(rows),
                free_after: Some(free_after),
                log_error: None,
            }),
            &app,
        );
        let again = state.scan_run();
        send(
            &mut state,
            Message::Scanned {
                run: again,
                scan: Some(measured_after_run(free_after)),
            },
            &app,
        );
        assert!(matches_picture(
            &state,
            &app,
            "tests/snapshots/space-result"
        ));
    }

    #[test]
    fn space_decide_pane() {
        let mut app = ctx();
        app.trash_can_purge = true;
        let (mut state, _) = tab(crowded_scan(true), &app);
        send(
            &mut state,
            Message::RowPressed(path(&format!("{HOME}/Downloads"))),
            &app,
        );
        send(&mut state, Message::Enter, &app);
        let item = |name: &str, gib: u64, days: u64| DownloadItem {
            path: path(&format!("{HOME}/Downloads/{name}")),
            size: gib * GIB,
            modified: Some(now() - std::time::Duration::from_secs(days * 86_400)),
            touched: None,
            last_opened: Some(now() - std::time::Duration::from_secs(days * 86_400)),
            open_count: Some(3),
            is_folder: false,
        };
        let groups = DownloadGroups {
            installed: vec![Matched {
                item: item("Setup.dmg", 3, 1),
                with: path("/Applications/Setup.app"),
                installed: None,
            }],
            untouched_year: (0..60)
                .map(|place| item(&format!("file-{place}.pdf"), 1, 400 + place * 9))
                .collect(),
            ..DownloadGroups::default()
        };
        send(
            &mut state,
            Message::Decide(DecideMessage::Loaded {
                folder: path(&format!("{HOME}/Downloads")),
                groups: Box::new(groups),
            }),
            &app,
        );
        send(&mut state, Message::Down, &app);
        assert!(matches_picture(
            &state,
            &app,
            "tests/snapshots/space-decide"
        ));
    }
}
