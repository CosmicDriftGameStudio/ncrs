//! The screens of the clean-up (#374) in German, with the fixture repository.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    dead_code,
    unused_imports,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file; the fixtures are shared"
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use iced::widget::text_editor;
    use iced::{Point, Size};
    use iced_test::Simulator;
    use ncrs_git::{
        ActionError, BranchInfo, ChangeKind, ChangedFile, CleanupBranch, CleanupList,
        CleanupReason, CleanupReport, CleanupRow, CleanupTarget, CleanupWorktree, CommitDetail,
        DiffHunk, DiffRow, FileDiff, GraphRow, KeepReason, Kept, Log, LogCommit, RefKind,
        RefLabel, RefsOverview, RowKind, WorktreeInfo,
    };
    use ncrs_git_view::actions::{
        ChangeSide, Conflict, ConflictLine, Dialog, ForceBlock, LocalChanges, MenuTarget,
        MergeInProgress, NewBranch, Notice, OpenMenu, Question, RenameBranch, StashEntry, SyncBar,
        Unavailable,
    };
    use ncrs_git_view::cleanup::{CleanupDialog, Phase, Pick};
    use ncrs_git_view::panes::PaneDrag;
    use ncrs_git_view::picker::Picker;
    use ncrs_git_view::state::{Area, Detail, Fold, Section, Subject};
    use ncrs_git_view::{
        update, view, Ctx, Message, PaneWidths, PickerKind, RepoEntry, Split, State, ViewCtx,
    };
    use ncrs_i18n::Language;
    use ncrs_ui_kit::format::DateFormat;
    use ncrs_ui_kit::git_rows::CommitColumns;
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    mod fixtures;
    use fixtures::*;

    fn branch_to_go(name: &str, reason: CleanupReason, remote: bool) -> CleanupBranch {
        CleanupBranch {
            name: name.to_owned(),
            sha: "0123456789abcdef".to_owned(),
            reason,
            upstream: Some(format!("origin/{name}")),
            tip_time: 0,
            remote_tip: remote.then(|| "fedcba9876543210".to_owned()),
        }
    }

    fn list() -> CleanupList {
        CleanupList {
            branches: vec![
                branch_to_go("fix/rundung", CleanupReason::InDefault, true),
                branch_to_go("feature/skonto", CleanupReason::InDefault, true),
                branch_to_go("fix/umlaute", CleanupReason::UpstreamGone, false),
            ],
            worktrees: vec![CleanupWorktree {
                path: PathBuf::from("/Users/marc/code/kassenbuch-wt/mahnwesen"),
                branch: branch_to_go("feature/mahnwesen", CleanupReason::InDefault, false),
            }],
            kept: vec![
                Kept {
                    name: "feature/rechnungs-export".to_owned(),
                    is_worktree: false,
                    why: KeepReason::Unpushed { commits: 3 },
                },
                Kept {
                    name: "main".to_owned(),
                    is_worktree: false,
                    why: KeepReason::DefaultBranch,
                },
                Kept {
                    name: "/Users/marc/code/kassenbuch-wt/hotfix".to_owned(),
                    is_worktree: true,
                    why: KeepReason::WorktreeChanges { count: 2 },
                },
            ],
        }
    }

    fn dialog(phase: Phase) -> CleanupDialog {
        let filled = phase != Phase::Fetching;
        let mut shown = CleanupDialog {
            phase,
            list: if filled { list() } else { CleanupList::default() },
            ticked: HashSet::new(),
            cursor: 1,
            delete_remote: false,
            keep_open: true,
            fetch_error: None,
            report: None,
            main: "main".to_owned(),
        };
        shown.ticked = shown.picks().into_iter().collect::<HashSet<Pick>>();
        shown
    }

    fn with_dialog(shown: CleanupDialog) -> State {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.dialog = Some(Dialog::Cleanup(Box::new(shown)));
        state
    }

    #[test]
    fn cleanup_dialog_picture() {
        let state = with_dialog(dialog(Phase::Ready));
        picture(&state, "tests/snapshots/git-cleanup-dialog-cosmic");
    }

    #[test]
    fn cleanup_dialog_while_fetching_picture() {
        let state = with_dialog(dialog(Phase::Fetching));
        picture(&state, "tests/snapshots/git-cleanup-fetching-cosmic");
    }

    #[test]
    fn cleanup_dialog_with_a_failed_row_picture() {
        let mut shown = dialog(Phase::Done);
        shown.report = Some(CleanupReport {
            rows: vec![
                CleanupRow {
                    target: CleanupTarget::Branch("fix/rundung".to_owned()),
                    result: Ok(()),
                },
                CleanupRow {
                    target: CleanupTarget::Branch("feature/skonto".to_owned()),
                    result: Err(ActionError::Other {
                        git_output: "the branch moved since the scan".to_owned(),
                    }),
                },
            ],
            undo: None,
        });
        let state = with_dialog(shown);
        picture(&state, "tests/snapshots/git-cleanup-row-failed-cosmic");
    }

    #[test]
    fn toolbar_in_the_sandbox_shows_the_locked_cleanup_button_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.unavailable = Some(Unavailable::Sandbox);
        state.actions.cleanable = vec!["fix/rundung".to_owned(), "feature/skonto".to_owned()];
        picture(&state, "tests/snapshots/git-cleanup-sandbox-cosmic");
    }

    #[test]
    fn sidebar_marks_merged_branches_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.cleanable = vec!["feature/mehrwertsteuer".to_owned()];
        state.area = Area::Sidebar;
        picture(&state, "tests/snapshots/git-cleanup-sidebar-merged-cosmic");
    }
}
