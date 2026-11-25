//! The Repos tab at the window size Marc works in, rendered headless: the
//! list with 36 repositories and the same list after a bulk run.

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
    use std::time::Duration;

    use iced::Size;
    use iced_test::Simulator;
    use ncrs_i18n::Language;
    use ncrs_repos::{
        view, BulkAction, Executed, GroupKey, Message, Outcome, Reading, ReposKeys, SkipReason,
        State, ViewCtx,
    };
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    use crate::common::{fixture, now, read, repo, summary, with_worktree, Fixture, HOME};

    const WINDOW: Size = Size::new(1740.0, 1130.0);
    const NAMES: [&str; 36] = [
        "ncrs",
        "ncrs-web",
        "kumiko",
        "kumiko-studio",
        "solon",
        "infra",
        "infra-docs",
        "space-conquest",
        "cosmic-site",
        "agent-config",
        "dotfiles",
        "homebrew-tap",
        "photo-sorter",
        "invoice-parser",
        "iced-playground",
        "terraform-modules",
        "k8s-manifests",
        "runbooks",
        "blog",
        "fonts",
        "scan-archive",
        "tax-2025",
        "recipes",
        "garden-planner",
        "weather-bot",
        "mail-rules",
        "home-assistant",
        "backup-scripts",
        "notes-sync",
        "pdf-tools",
        "keyboard-layouts",
        "game-jam-2026",
        "shader-lab",
        "ledger",
        "old-website",
        "thesis",
    ];

    fn crowded() -> Fixture {
        let mut fixture = fixture(&NAMES, 5);
        for (index, name) in NAMES.iter().enumerate() {
            let behind = if index % 4 == 1 {
                (index % 7) as u32 + 1
            } else {
                0
            };
            let ahead = if index % 9 == 3 { 2 } else { 0 };
            let changed = if index % 5 == 2 { index % 11 + 1 } else { 0 };
            let mut base = summary(behind, ahead, changed);
            base.fetched_at = Some(now() - Duration::from_secs(60 * (index as u64 * 37 + 5)));
            if index % 6 == 0 {
                base.branch = Some("feature/rebindable-keys".to_owned());
            }
            if *name == "ncrs" {
                base = with_worktree(base, "ncrs-1-8-0", "feat/1-8-0");
            }
            fixture.reading(name, read(base));
        }
        fixture.reading("old-website", Reading::Gone);
        fixture.reading("thesis", Reading::Gone);
        fixture
    }

    fn matches_picture(state: &State, reference: &str) -> bool {
        let palette = ThemePreset::Cosmic.palette();
        let keys = ReposKeys::defaults_for(true);
        let ctx = ViewCtx {
            lang: Language::English,
            palette: &palette,
            window: WINDOW,
            keys: &keys,
            home: Some(Path::new(HOME)),
            now: now(),
            actions_available: true,
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            view(state, ctx).map(|message: Message| message),
        );
        simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(reference)
            .unwrap()
    }

    #[test]
    fn repos_list_full() {
        let fixture = crowded();
        assert!(matches_picture(
            &fixture.state,
            "tests/snapshots/repos-list-full"
        ));
    }

    #[test]
    fn repos_list_marked_with_a_folded_group() {
        let mut fixture = crowded();
        for name in ["kumiko", "solon", "infra", "dotfiles"] {
            fixture.send(Message::RowToggled(repo(name)));
        }
        fixture.send(Message::GroupToggled(GroupKey::Favourites));
        assert!(matches_picture(
            &fixture.state,
            "tests/snapshots/repos-list-marked"
        ));
    }

    #[test]
    fn repos_run_result() {
        let mut fixture = crowded();
        for name in NAMES.iter().take(22) {
            fixture.send(Message::RowToggled(repo(name)));
        }
        fixture.send(Message::Bulk(BulkAction::Both));
        let targets = fixture.state.run().unwrap().targets().to_vec();
        for (index, path) in targets.into_iter().enumerate() {
            let outcome = match index % 6 {
                0 => Outcome::Updated { behind: 3 },
                1 => Outcome::AlreadyCurrent,
                2 => Outcome::Skipped(SkipReason::LocalChanges(4)),
                3 => Outcome::NoAccess {
                    remote: "origin".to_owned(),
                },
                4 => Outcome::Switched {
                    from: "feature/rebindable-keys".to_owned(),
                    to: "main".to_owned(),
                    behind: 2,
                },
                _ => Outcome::Failed {
                    last_line: "fatal: unable to access the remote".to_owned(),
                },
            };
            fixture.send(Message::StepDone(
                path,
                Box::new(Executed {
                    outcome,
                    ran: Vec::new(),
                    after: None,
                }),
            ));
        }
        assert!(matches_picture(
            &fixture.state,
            "tests/snapshots/repos-run-result"
        ));
    }

    #[test]
    fn repos_run_in_progress() {
        let mut fixture = crowded();
        fixture.ctx.settings.concurrency = 4;
        for name in NAMES.iter().take(12) {
            fixture.send(Message::RowToggled(repo(name)));
        }
        fixture.send(Message::Bulk(BulkAction::Fetch));
        let first = fixture
            .state
            .run()
            .unwrap()
            .targets()
            .first()
            .cloned()
            .unwrap();
        fixture.send(Message::StepDone(
            first,
            Box::new(Executed {
                outcome: Outcome::Updated { behind: 2 },
                ran: Vec::new(),
                after: None,
            }),
        ));
        assert!(matches_picture(
            &fixture.state,
            "tests/snapshots/repos-run-progress"
        ));
    }
}
