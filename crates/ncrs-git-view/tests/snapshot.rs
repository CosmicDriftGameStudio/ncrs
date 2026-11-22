//! The Git tab in a narrow and in a wide window, rendered headless.

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
        BranchInfo, ChangeKind, ChangedFile, CommitDetail, GraphRow, Log, LogCommit, RefKind,
        RefLabel, RefsOverview, WorktreeInfo,
    };
    use ncrs_git_view::{update, view, Ctx, Message, State, ViewCtx};
    use ncrs_i18n::Language;
    use ncrs_ui_kit::format::DateFormat;
    use ncrs_ui_kit::git_rows::CommitColumns;
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    const CTX: Ctx = Ctx::new(Language::English);
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
    const LONG_SUMMARIES: [&str; 4] = [
        "Move the certificate renewal of the ingress controllers into the shared cluster module and drop the per-cluster copies that drifted apart",
        "Raise the memory limit of the metrics collector after it was killed twice during the nightly compaction of the long term storage",
        "Pin the container image of the backup job to a digest so a moved tag cannot change what runs in production overnight",
        "Add the missing network policy for the monitoring namespace",
    ];
    const LONG_MESSAGE: &str = "Move the certificate renewal of the ingress controllers into the shared cluster module and drop the per-cluster copies that drifted apart\n\nThe renewal job was copied into every cluster folder and the copies had drifted: two clusters still renewed 30 days before the end, one used a staging issuer that is not trusted by the browsers of our customers.\n\n- one module, one schedule, one issuer\n- the per-cluster values only name the domains\n- a failed renewal pages the on-call person a week before the certificate ends instead of the day before\n\nRefs: INFRA-1042, INFRA-1077";
    const AUTHORS: [&str; 3] = ["Anna Becker", "Jonas Weber", "Marc Winter"];

    fn commit(number: usize, summaries: &[&str]) -> LogCommit {
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
            summary: summaries
                .get(number % summaries.len())
                .expect("a summary")
                .to_string(),
            refs: Vec::new(),
            is_merge: false,
            graph: GraphRow::default(),
        }
    }

    fn branch(name: &str, is_current: bool) -> BranchInfo {
        BranchInfo {
            name: name.to_owned(),
            is_current,
            has_upstream: true,
            ahead: 0,
            behind: 0,
            merged: false,
        }
    }

    /// A repository of `summaries`, the first commit selected with `message`.
    fn state_of(
        summaries: &[&str],
        message: &str,
        local: Vec<BranchInfo>,
        remote: Vec<String>,
    ) -> State {
        let mut state = State {
            root: Some(PathBuf::from("/repo")),
            name: "ncrs".to_owned(),
            ..State::default()
        };
        let (read_log, _) = state.loads.log.begin();
        let mut commits: Vec<LogCommit> = (0..30).map(|number| commit(number, summaries)).collect();
        if let Some(first) = commits.first_mut() {
            first.refs = vec![
                RefLabel {
                    name: "main".to_owned(),
                    kind: RefKind::Branch,
                },
                RefLabel {
                    name: "origin/main".to_owned(),
                    kind: RefKind::Remote,
                },
            ];
        }
        let log = Log {
            commits,
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
            local,
            remote,
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
        let first = commit(0, summaries);
        let info = CommitDetail {
            id: first.id,
            short: first.short,
            parents: Vec::new(),
            author: first.author,
            time: first.time,
            message: message.to_owned(),
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

    fn state() -> State {
        state_of(
            &SUMMARIES,
            SUMMARIES.first().expect("a summary"),
            vec![branch("main", true)],
            Vec::new(),
        )
    }

    fn infra_state() -> State {
        state_of(
            &LONG_SUMMARIES,
            LONG_MESSAGE,
            vec![
                branch("main", true),
                branch(
                    "feature/renew-the-ingress-certificates-in-the-shared-cluster-module",
                    false,
                ),
            ],
            vec![
                "origin/main".to_owned(),
                "origin/feature/renew-the-ingress-certificates-in-the-shared-cluster-module"
                    .to_owned(),
                "upstream/main".to_owned(),
            ],
        )
    }

    fn matches_picture(state: &State, size: Size, reference: &str) -> bool {
        let palette = ThemePreset::Cosmic.palette();
        let ctx = ViewCtx {
            lang: Language::English,
            palette: &palette,
            date_format: DateFormat::Iso,
            window: size,
            columns: CommitColumns::STANDARD,
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            size,
            view(state, ctx).map(|message: Message| message),
        );
        simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(reference)
            .unwrap()
    }

    #[test]
    fn git_tab_narrow_picture() {
        let matches = matches_picture(
            &state(),
            Size::new(1280.0, 800.0),
            "tests/snapshots/git-narrow-cosmic",
        );
        assert!(matches, "the Git tab picture differs from its image");
    }

    #[test]
    fn git_tab_wide_picture_with_long_messages() {
        let matches = matches_picture(
            &infra_state(),
            Size::new(1740.0, 1130.0),
            "tests/snapshots/git-wide-long-messages-cosmic",
        );
        assert!(matches, "the wide Git tab picture differs from its image");
    }
}
