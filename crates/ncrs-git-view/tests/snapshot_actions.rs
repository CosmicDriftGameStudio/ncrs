//! The screens of the Git actions (#338) in German, with a fixture repository.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::path::{Path, PathBuf};

    use iced::widget::text_editor;
    use iced::{Point, Size};
    use iced_test::Simulator;
    use ncrs_git::{
        BranchInfo, ChangeKind, ChangedFile, CommitDetail, DiffHunk, DiffRow, FileDiff, GraphRow,
        Log, LogCommit, RefKind, RefLabel, RefsOverview, RowKind, WorktreeInfo,
    };
    use ncrs_git::askpass::{AskRequest, PromptKind, Secret};
    use ncrs_git::ForcePushPlan;
    use ncrs_git_view::credentials::CredentialAsk;
    use ncrs_git_view::sync_menu::{SyncKind, SyncMenu};
    use ncrs_git_view::actions::{
        ForceBlock, ChangeSide, Conflict, ConflictLine, Dialog, LocalChanges, MenuTarget, MergeInProgress,
        NewBranch, Notice, OpenMenu, Question, RenameBranch, StashEntry, SyncBar, Unavailable,
    };
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
    mod file_views;

    #[test]
    fn local_changes_picture() {
        let mut state = repository();
        state.commit_cursor = 0;
        picture(&state, "tests/snapshots/git-local-changes-cosmic");
    }

    fn staged_diff() -> FileDiff {
        use RowKind::{Added, Context};
        FileDiff {
            path: "src/kassenbuch/abschluss.rs".to_owned(),
            hunks: vec![
                DiffHunk {
                    old_start: 10,
                    new_start: 10,
                    context: "mod abschluss {".to_owned(),
                    rows: vec![
                        row(
                            Context,
                            Some(10),
                            Some(10),
                            "pub fn monatsabschluss(monat: Monat) -> Abschluss {",
                        ),
                        row(
                            Added,
                            None,
                            Some(11),
                            "    let buchungen = buchungen_im(monat);",
                        ),
                        row(
                            Added,
                            None,
                            Some(12),
                            "    let summe = buchungen.iter().map(|b| b.betrag).sum();",
                        ),
                        row(
                            Context,
                            Some(11),
                            Some(13),
                            "    Abschluss::neu(monat, summe)",
                        ),
                    ],
                },
                DiffHunk {
                    old_start: 52,
                    new_start: 54,
                    context: String::new(),
                    rows: vec![
                        row(Context, Some(52), Some(54), "impl Abschluss {"),
                        row(
                            Added,
                            None,
                            Some(55),
                            "    pub fn ist_offen(&self) -> bool {",
                        ),
                        row(
                            Added,
                            None,
                            Some(56),
                            "        self.festgeschrieben.is_none()",
                        ),
                        row(Added, None, Some(57), "    }"),
                        row(Context, Some(53), Some(58), "}"),
                    ],
                },
            ],
            added: 6,
            removed: 0,
            binary: false,
            too_large: false,
            old_total: Some(120),
            new_total: Some(124),
        }
    }

    #[test]
    fn local_changes_staged_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        let mut local = local_changes();
        local.selected = Some((ChangeSide::Staged, 0));
        local.diff = Some(staged_diff());
        state.actions.local = Some(local);
        state.commit_cursor = 0;
        picture(&state, "tests/snapshots/git-local-changes-staged-cosmic");
    }

    #[test]
    fn toolbar_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        picture_at(
            &state,
            "tests/snapshots/git-toolbar-cosmic",
            Some(Point::new(TOOLBAR_FETCH_X, 16.0)),
        );
    }

    fn with_menu(target: MenuTarget, cursor: usize) -> State {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.area = Area::Sidebar;
        state.sidebar_cursor = cursor;
        state.actions.stash_ids = (0..state.actions.stashes.len())
            .map(|index| format!("stash-{index}"))
            .collect();
        state.actions.menu = Some(OpenMenu::new(target));
        state
    }

    #[test]
    fn local_branch_menu_picture() {
        let state = with_menu(MenuTarget::LocalBranch(1), 3);
        picture(&state, "tests/snapshots/git-branch-menu-local-cosmic");
    }

    #[test]
    fn remote_branch_menu_picture() {
        let state = with_menu(MenuTarget::RemoteBranch(1), 15);
        picture(&state, "tests/snapshots/git-branch-menu-remote-cosmic");
    }

    #[test]
    fn stash_menu_picture() {
        let state = with_menu(MenuTarget::Stash("stash-0".to_owned()), 24);
        picture(&state, "tests/snapshots/git-stash-menu-cosmic");
    }

    #[test]
    fn new_branch_dialog_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.new_branch = Some(NewBranch {
            name: "feature/neue suche".to_owned(),
            from: "feature/rechnungs-export".to_owned(),
            checkout: true,
            problem: Some("Der Name darf keine Leerzeichen enthalten.".to_owned()),
        });
        picture(&state, "tests/snapshots/git-new-branch-dialog-cosmic");
    }

    fn with_dialog(dialog: Dialog) -> State {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.dialog = Some(dialog);
        state
    }

    #[test]
    fn discard_file_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DiscardFile {
            path: "src/rechnung/runden.rs".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-discard-file-cosmic");
    }

    #[test]
    fn discard_all_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DiscardAll { count: 7 }));
        picture(&state, "tests/snapshots/git-ask-discard-all-cosmic");
    }

    #[test]
    fn delete_branch_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DeleteBranch {
            name: "feature/mehrwertsteuer".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-delete-branch-cosmic");
    }

    #[test]
    fn delete_on_remote_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DeleteOnRemote {
            remote: "origin".to_owned(),
            branch: "fix/rundung".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-delete-remote-cosmic");
    }

    #[test]
    fn drop_stash_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DropStash {
            sha: "0123456789abcdef".to_owned(),
            message: "Skonto-Experiment".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-drop-stash-cosmic");
    }

    #[test]
    fn rename_dialog_picture() {
        let state = with_dialog(Dialog::Rename(RenameBranch {
            from: "feature/mehrwertsteuer".to_owned(),
            name: "feature/mehrwertsteuer-satz".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-rename-dialog-cosmic");
    }

    fn with_sync_menu(kind: SyncKind, force_blocked: Option<ForceBlock>) -> State {
        let mut state = repository();
        on_commit(&mut state);
        let mut bar = sync();
        bar.force_blocked = force_blocked;
        state.actions.sync = Some(bar);
        state.actions.sync_menu = Some(SyncMenu::new(kind));
        state
    }

    #[test]
    fn pull_menu_picture() {
        let state = with_sync_menu(SyncKind::Pull, None);
        picture(&state, "tests/snapshots/git-pull-menu-cosmic");
    }

    #[test]
    fn push_menu_picture() {
        let state = with_sync_menu(SyncKind::Push, Some(ForceBlock::NoUpstream));
        picture(&state, "tests/snapshots/git-push-menu-cosmic");
    }

    #[test]
    fn force_push_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::ForcePush {
            plan: ForcePushPlan {
                remote: "origin".to_owned(),
                branch: "feature/rechnungs-export".to_owned(),
                target: "feature/rechnungs-export".to_owned(),
                expected: "3f9c2d1".to_owned(),
                lost: 3,
                lost_titles: vec![
                    "Kassenbuch: Abschluss pro Monat".to_owned(),
                    "Fix Rundung der Mehrwertsteuer".to_owned(),
                    "Merge branch 'main' into feature".to_owned(),
                ],
            },
        }));
        picture(&state, "tests/snapshots/git-ask-force-push-cosmic");
    }

    #[test]
    fn credentials_password_picture() {
        let mut ask = CredentialAsk::of(&AskRequest {
            id: 1,
            job: 1,
            prompt: "Password for 'https://marc@git.example.test': ".to_owned(),
            kind: PromptKind::Secret,
        });
        ask.typed = Secret::new("hunter2hunter2");
        let state = with_dialog(Dialog::Credential(ask));
        picture(&state, "tests/snapshots/git-credentials-password-cosmic");
    }

    #[test]
    fn merge_conflicts_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        state.actions.merge = Some(merge());
        picture(&state, "tests/snapshots/git-merge-conflicts-cosmic");
    }

    #[test]
    fn rebase_conflicts_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        state.actions.merge = Some(merge());
        state.actions.rebasing = true;
        picture(&state, "tests/snapshots/git-rebase-conflicts-cosmic");
    }

    #[test]
    fn conflict_menu_picture() {
        let mut state = repository();
        state.actions.merge = Some(merge());
        let conflict = merge().conflicts.swap_remove(2).path;
        state.actions.menu = Some(OpenMenu::new(MenuTarget::Conflict(conflict)));
        picture(&state, "tests/snapshots/git-conflict-menu-cosmic");
    }

    #[test]
    fn push_rejected_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.notice = Some(Notice::PushRejected {
            branch: "main".to_owned(),
            behind: 2,
        });
        picture(&state, "tests/snapshots/git-push-rejected-cosmic");
    }

    #[test]
    fn auth_failed_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.notice = Some(Notice::AuthFailed {
            remote: "origin".to_owned(),
            output: "git@git.kassenbuch.example: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.".to_owned(),
            details_open: true,
        });
        picture(&state, "tests/snapshots/git-auth-failed-cosmic");
    }

    #[test]
    fn stash_list_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.area = Area::Sidebar;
        state.sidebar_cursor = 23;
        picture(&state, "tests/snapshots/git-stash-list-cosmic");
    }

    /// A branch list and a repository list with enough rows to scroll.
    fn open_picker(kind: PickerKind, query: &str, cursor: usize) -> State {
        let mut state = repository();
        on_commit(&mut state);
        state.picker = Some(Picker {
            kind,
            query: query.to_owned(),
            cursor,
            repositories: vec![
                RepoEntry {
                    name: "kassenbuch".to_owned(),
                    path: PathBuf::from("/repo"),
                },
                RepoEntry {
                    name: "kassenbuch-rechnungs-export".to_owned(),
                    path: PathBuf::from("/work/kassenbuch-rechnungs-export"),
                },
                RepoEntry {
                    name: "buchhaltung-web".to_owned(),
                    path: PathBuf::from("/work/buchhaltung-web"),
                },
                RepoEntry {
                    name: "steuer-tabellen".to_owned(),
                    path: PathBuf::from("/work/steuer-tabellen"),
                },
            ],
        });
        state
    }

    #[test]
    fn header_behind_picture() {
        let mut state = repository();
        on_commit(&mut state);
        picture(&state, "tests/snapshots/git-header-behind-cosmic");
    }

    #[test]
    fn header_up_to_date_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.local = None;
        state.actions.sync = Some(SyncBar {
            ahead: 0,
            behind: 0,
            undo: None,
            force_blocked: None,
        });
        if let Some(overview) = state.overview.as_mut() {
            for branch in &mut overview.local {
                branch.ahead = 0;
                branch.behind = 0;
            }
            for worktree in &mut overview.worktrees {
                worktree.wip = 0;
            }
        }
        picture(&state, "tests/snapshots/git-header-up-to-date-cosmic");
    }

    #[test]
    fn header_sandbox_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = None;
        state.actions.local = None;
        state.actions.unavailable = Some(Unavailable::Sandbox);
        picture_at(
            &state,
            "tests/snapshots/git-header-sandbox-cosmic",
            Some(Point::new(700.0, 20.0)),
        );
    }

    #[test]
    fn branch_picker_picture() {
        let state = open_picker(PickerKind::Branch, "", 1);
        picture(&state, "tests/snapshots/git-branch-picker-cosmic");
    }

    #[test]
    fn branch_picker_filtered_picture() {
        let state = open_picker(PickerKind::Branch, "rund", 0);
        picture(&state, "tests/snapshots/git-branch-picker-filtered-cosmic");
    }

    #[test]
    fn repo_picker_picture() {
        let state = open_picker(PickerKind::Repository, "", 2);
        picture(&state, "tests/snapshots/git-repo-picker-cosmic");
    }

    #[test]
    fn splitter_resized_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.area = Area::SidebarSplit;
        picture_in(
            &state,
            "tests/snapshots/git-splitter-resized-cosmic",
            None,
            PaneWidths {
                sidebar: 380,
                detail: 560,
            },
            ThemePreset::Cosmic,
        );
    }

    #[test]
    fn splitter_dragged_picture() {
        let mut state = repository();
        on_commit(&mut state);
        let widths = PaneWidths {
            sidebar: 372,
            detail: 450,
        };
        state.pane_drag = Some(PaneDrag {
            split: Split::Sidebar,
            start: PaneWidths::STANDARD,
            origin: Some(300.0),
            current: widths,
        });
        picture(&state, "tests/snapshots/git-splitter-dragged-cosmic");
    }

    #[test]
    fn head_row_picture() {
        let mut state = repository();
        on_commit(&mut state);
        if let Some(log) = state.log.as_mut() {
            log.head = log.commits.get(3).map(|head| head.id.clone());
        }
        state.commit_cursor = 5;
        picture(&state, "tests/snapshots/git-head-row-cosmic");
    }

    #[test]
    fn detail_full_picture() {
        let mut state = repository();
        on_commit(&mut state);
        let second = commit(1);
        let third = commit(2);
        if let Some(info) = state
            .detail
            .as_mut()
            .and_then(|detail| detail.info.as_mut())
        {
            info.parents = vec![second.id.clone(), third.id];
            info.committer = "Tobias Engel".to_owned();
            info.committer_time += 3_600;
        }
        let shown = commit(0).id;
        state.containing = Some((
            shown,
            vec![
                label("main", RefKind::Branch),
                label("origin/main", RefKind::Remote),
                label("feature/rechnungs-export", RefKind::Branch),
                label("release/1.4", RefKind::Branch),
            ],
        ));
        state.area = Area::Detail;
        picture(&state, "tests/snapshots/git-detail-full-cosmic");
    }

    #[test]
    fn sidebar_sections_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.area = Area::Sidebar;
        state.sidebar_cursor = 7;
        state.collapsed.insert(Fold::Section(Section::Tags));
        picture(&state, "tests/snapshots/git-sidebar-sections-cosmic");
    }

    #[test]
    fn dracula_picture() {
        let mut state = repository();
        on_commit(&mut state);
        picture_in(
            &state,
            "tests/snapshots/git-overview-dracula",
            None,
            PaneWidths::STANDARD,
            ThemePreset::Dracula,
        );
    }

    #[test]
    fn wip_row_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        state.actions.local = Some(local_changes());
        state.commit_cursor = 0;
        state.area = Area::Detail;
        picture(&state, "tests/snapshots/git-wip-row-cosmic");
    }

    #[test]
    fn wip_diff_picture() {
        let mut state = repository();
        let mut local = local_changes();
        local.selected = Some((ChangeSide::Unstaged, 0));
        local.diff = Some(staged_diff());
        local.diff_open = true;
        state.actions.local = Some(local);
        state.commit_cursor = 0;
        state.area = Area::Detail;
        picture(&state, "tests/snapshots/git-wip-diff-cosmic");
    }

    #[test]
    fn commit_box_disabled_picture() {
        let mut state = repository();
        let mut local = local_changes();
        local.staged.clear();
        local.focus = ncrs_git_view::actions::WipFocus::Title;
        local.title =
            "Rundung der Mehrwertsteuer auf Cent korrigiert und den Monatsabschluss ergänzt"
                .to_owned();
        state.actions.local = Some(local);
        state.commit_cursor = 0;
        state.area = Area::Detail;
        picture(&state, "tests/snapshots/git-commit-box-disabled-cosmic");
    }

    #[test]
    fn ignore_dialog_picture() {
        let state = with_dialog(Dialog::Ask(Question::Ignore {
            path: "target/debug/kassenbuch.log".to_owned(),
            folder: Some("target/debug".to_owned()),
            extension: Some("log".to_owned()),
            chosen: ncrs_git_view::actions::IgnoreChoice::File,
        }));
        picture(&state, "tests/snapshots/git-ignore-dialog-cosmic");
    }

    #[test]
    fn sandbox_changes_picture() {
        let mut state = repository();
        state.actions.sync = None;
        state.actions.local = None;
        state.actions.unavailable = Some(Unavailable::Sandbox);
        state.commit_cursor = 0;
        state.detail = Some(Detail {
            subject: Subject::Working,
            info: None,
            files: vec![
                file("src/steuer/mwst.rs", ChangeKind::Modified, 3, 1),
                file("README.md", ChangeKind::Modified, 2, 0),
            ],
        });
        picture(&state, "tests/snapshots/git-sandbox-changes-cosmic");
    }

    fn with_commit_menu(cursor: usize, submenu: Option<usize>) -> State {
        let mut state = repository();
        state.actions.sync = Some(sync());
        state.area = Area::Commits;
        state.commit_cursor = 3;
        state.marked_commit = Some(commit(6).id);
        let opened_on = state.commit_at(3).unwrap().id.clone();
        state.actions.menu = Some(OpenMenu {
            target: MenuTarget::Commit(opened_on),
            cursor,
            submenu,
        });
        state
    }

    #[test]
    fn commit_menu_picture() {
        let state = with_commit_menu(2, None);
        picture(&state, "tests/snapshots/git-commit-menu-cosmic");
    }

    #[test]
    fn commit_menu_reset_picture() {
        let state = with_commit_menu(6, Some(2));
        picture(&state, "tests/snapshots/git-commit-menu-reset-cosmic");
    }

    #[test]
    fn reset_hard_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::ResetHard {
            branch: "main".to_owned(),
            sha: commit(3).id,
            commits: 3,
            changed_files: 2,
        }));
        picture(&state, "tests/snapshots/git-ask-reset-hard-cosmic");
    }

    #[test]
    fn tag_dialog_picture() {
        let state = with_dialog(Dialog::Tag(ncrs_git_view::actions::TagDraft {
            name: "v1.8.0".to_owned(),
            sha: commit(3).id,
            problem: None,
        }));
        picture(&state, "tests/snapshots/git-tag-dialog-cosmic");
    }

    #[test]
    fn compare_working_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        state.area = Area::Detail;
        state.diff = Some(ncrs_git_view::state::DiffState {
            subject: Subject::Compare {
                from: commit(5).id,
                to: None,
            },
            files: vec![
                file("src/kassenbuch/abschluss.rs", ChangeKind::Modified, 4, 1),
                file("src/steuer/mwst.rs", ChangeKind::Modified, 3, 1),
            ],
            file: 0,
            diff: Some(staged_diff()),
            unified: false,
            ignore_whitespace: false,
            hunk: 0,
        });
        picture(&state, "tests/snapshots/git-compare-working-cosmic");
    }
}
