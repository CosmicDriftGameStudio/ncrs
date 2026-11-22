//! The Git tab in a narrow window, rendered headless.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::path::PathBuf;

    use iced::Size;
    use iced_test::Simulator;
    use ncrs_git::{
        BranchInfo, ChangeKind, ChangedFile, CommitDetail, GraphRow, Log, LogCommit, RefsOverview,
        WorktreeInfo,
    };
    use ncrs_git_view::{update, view, Ctx, Message, State, ViewCtx};
    use ncrs_i18n::Language;
    use ncrs_ui_kit::format::DateFormat;
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    const CTX: Ctx = Ctx {
        lang: Language::English,
    };
    const SUMMARIES: [&str; 8] = [
        "Add the command palette",
        "Fix the crash when a folder is renamed while it is shown",
        "Read archives without unpacking them first",
        "Show the date a photo was taken",
        "Warn before deleting a folder with many files",
        "Remember the sort order of every folder",
        "Open a file with the program of its type",
        "Stop the copy when the target disk is full",
    ];
    const AUTHORS: [&str; 3] = ["Anna Becker", "Jonas Weber", "Marc Winter"];

    fn commit(number: usize) -> LogCommit {
        let id = format!("{:040x}", number * 7_919 + 0xba99bff);
        LogCommit {
            short: id.chars().take(7).collect(),
            id,
            parents: Vec::new(),
            author: AUTHORS
                .get(number % AUTHORS.len())
                .expect("an author")
                .to_string(),
            time: 1_790_000_000 - number as i64 * 5_400,
            summary: SUMMARIES
                .get(number % SUMMARIES.len())
                .expect("a summary")
                .to_string(),
            refs: Vec::new(),
            is_merge: false,
            graph: GraphRow::default(),
        }
    }

    fn state() -> State {
        let mut state = State {
            root: Some(PathBuf::from("/repo")),
            name: "ncrs".to_owned(),
            ..State::default()
        };
        let (read_log, _) = state.loads.log.begin();
        let log = Log {
            commits: (0..30).map(commit).collect(),
            has_more: false,
            lane_count: 1,
            head: None,
        };
        drop(update(
            &mut state,
            Message::LogLoaded(read_log, Box::new(Ok(log))),
            &CTX,
        ));
        let (read_refs, _) = state.loads.refs.begin();
        let overview = RefsOverview {
            main_branch: Some("main".to_owned()),
            local: vec![BranchInfo {
                name: "main".to_owned(),
                is_current: true,
                has_upstream: true,
                ahead: 0,
                behind: 0,
                merged: false,
            }],
            tags: vec!["v1.0".to_owned()],
            worktrees: vec![WorktreeInfo {
                path: PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                head_short: None,
                wip: 0,
                ahead: 0,
                behind: 0,
                merged: false,
                is_main: true,
                is_current: true,
            }],
            ..RefsOverview::default()
        };
        drop(update(
            &mut state,
            Message::RefsLoaded(read_refs, Box::new(Ok(overview))),
            &CTX,
        ));
        let (read_detail, _) = state.loads.detail.begin();
        let first = commit(0);
        let info = CommitDetail {
            id: first.id,
            short: first.short,
            parents: Vec::new(),
            author: first.author,
            time: first.time,
            message: SUMMARIES.first().expect("a summary").to_string(),
            files: vec![ChangedFile {
                path: "src/engine.rs".to_owned(),
                old_path: None,
                kind: ChangeKind::Modified,
                added: 2,
                removed: 2,
                binary: false,
            }],
        };
        drop(update(
            &mut state,
            Message::DetailLoaded(read_detail, Box::new(Ok(info))),
            &CTX,
        ));
        state
    }

    #[test]
    fn git_tab_narrow_picture() {
        let size = Size::new(1280.0, 800.0);
        let palette = ThemePreset::Cosmic.palette();
        let ctx = ViewCtx {
            lang: Language::English,
            palette: &palette,
            date_format: DateFormat::Iso,
            window: size,
        };
        let state = state();
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            size,
            view(&state, ctx).map(|message: Message| message),
        );
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image("tests/snapshots/git-narrow-cosmic")
            .unwrap();
        assert!(matches, "the Git tab picture differs from its image");
    }
}
