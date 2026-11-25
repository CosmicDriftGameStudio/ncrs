//! The clean-up dialog of the Git tab, driven through `update`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

use std::path::PathBuf;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_git::{
    ActionError, CleanupBranch, CleanupList, CleanupReason, CleanupReport, CleanupRow,
    CleanupTarget, CleanupWorktree,
};
use ncrs_git_view::actions::{Dialog, Operation};
use ncrs_git_view::cleanup::{CleanupDialog, CleanupMessage, Phase, Pick};
use ncrs_git_view::job::{Done, Finished, Job};
use ncrs_git_view::{git_view_key, update, Action, Ctx, GitKeys, KeyContext, Message, State};
use ncrs_i18n::Language;

fn branch(name: &str) -> CleanupBranch {
    CleanupBranch {
        name: name.to_owned(),
        sha: "0123456789abcdef".to_owned(),
        reason: CleanupReason::InDefault,
        upstream: Some(format!("origin/{name}")),
        tip_time: 0,
        remote_tip: Some("fedcba9876543210".to_owned()),
    }
}

fn list() -> CleanupList {
    CleanupList {
        branches: vec![branch("fix/a"), branch("fix/b")],
        worktrees: vec![CleanupWorktree {
            path: PathBuf::from("/repo-wt/fix-c"),
            branch: branch("fix/c"),
            ignored_files: Vec::new(),
        }],
        kept: Vec::new(),
    }
}

fn ctx() -> Ctx {
    Ctx::new(Language::English)
}

fn tab() -> State {
    State {
        root: Some(PathBuf::from("/repo")),
        ..State::default()
    }
}

fn finish(state: &mut State, job: Job, result: Result<Done, ActionError>) -> Option<Action> {
    update(
        state,
        Message::JobDone(Box::new(Finished { job, result })),
        &ctx(),
    )
    .1
}

fn open_and_scan(state: &mut State) {
    let _ = update(state, Message::Cleanup(CleanupMessage::Open), &ctx());
    let _ = finish(
        state,
        Job::FetchForCleanup,
        Ok(Done::CleanupScan {
            list: list(),
            fetch_error: None,
        }),
    );
}

fn dialog(state: &State) -> &CleanupDialog {
    match &state.actions.dialog {
        Some(Dialog::Cleanup(dialog)) => dialog,
        other => panic!("no clean-up dialog: {other:?}"),
    }
}

#[test]
fn opening_fetches_first_and_a_second_open_does_nothing() {
    let mut state = tab();
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Open), &ctx());
    assert_eq!(dialog(&state).phase, Phase::Fetching);
    assert_eq!(state.actions.busy, Some(Operation::Fetch));
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Open), &ctx());
    assert_eq!(dialog(&state).phase, Phase::Fetching);
}

#[test]
fn the_list_comes_with_everything_ticked_and_the_remote_off() {
    let mut state = tab();
    open_and_scan(&mut state);
    let shown = dialog(&state);
    assert_eq!(shown.phase, Phase::Ready);
    assert_eq!(shown.ticked.len(), 3);
    assert!(!shown.delete_remote);
    assert_eq!(state.actions.busy, None);
}

#[test]
fn a_worktree_with_ignored_files_is_not_ticked() {
    let mut state = tab();
    let _ = update(
        &mut state,
        Message::Cleanup(CleanupMessage::Open),
        &ctx(),
    );
    let mut offered = list();
    offered.worktrees[0].ignored_files = vec![".env".to_owned()];
    let _ = finish(
        &mut state,
        Job::FetchForCleanup,
        Ok(Done::CleanupScan {
            list: offered,
            fetch_error: None,
        }),
    );
    let plan = dialog(&state).plan();
    assert!(plan.worktrees.is_empty());
    assert_eq!(plan.branches.len(), 2);
}

#[test]
fn space_unticks_and_only_what_is_ticked_goes_into_the_plan() {
    let mut state = tab();
    open_and_scan(&mut state);
    // The worktree comes first, then the branches.
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Down), &ctx());
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Space), &ctx());
    assert!(!dialog(&state).ticked.contains(&Pick::Branch("fix/a".to_owned())));
    let plan = dialog(&state).plan();
    assert_eq!(plan.branches.len(), 1);
    assert_eq!(plan.worktrees.len(), 1);
    assert!(!plan.delete_remote);
}

#[test]
fn confirming_starts_the_deletion_but_never_with_nothing_ticked() {
    let mut state = tab();
    open_and_scan(&mut state);
    for pick in dialog(&state).picks() {
        let _ = update(
            &mut state,
            Message::Cleanup(CleanupMessage::Toggle(pick)),
            &ctx(),
        );
    }
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Confirm), &ctx());
    assert_eq!(dialog(&state).phase, Phase::Ready);
    assert_eq!(state.actions.busy, None);
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Space), &ctx());
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Confirm), &ctx());
    assert_eq!(dialog(&state).phase, Phase::Deleting);
    assert_eq!(state.actions.busy, Some(Operation::Branch));
}

#[test]
fn the_remote_checkbox_is_on_the_last_line_and_tab_reaches_it() {
    let mut state = tab();
    open_and_scan(&mut state);
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Tab(false)), &ctx());
    assert_eq!(dialog(&state).cursor, 3);
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Space), &ctx());
    assert!(dialog(&state).delete_remote);
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Tab(true)), &ctx());
    assert_eq!(dialog(&state).cursor, 0);
}

#[test]
fn a_finished_deletion_reports_each_row_and_hands_the_folders_to_the_app() {
    let mut state = tab();
    open_and_scan(&mut state);
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Confirm), &ctx());
    let plan = dialog(&state).plan();
    let report = CleanupReport {
        rows: vec![
            CleanupRow {
                target: CleanupTarget::Worktree(PathBuf::from("/repo-wt/fix-c")),
                result: Ok(()),
            },
            CleanupRow {
                target: CleanupTarget::Branch("fix/a".to_owned()),
                result: Err(ActionError::Other {
                    git_output: "busy".to_owned(),
                }),
            },
            CleanupRow {
                target: CleanupTarget::Branch("fix/b".to_owned()),
                result: Ok(()),
            },
        ],
        undo: None,
    };
    let action = finish(&mut state, Job::CleanUp(plan), Ok(Done::Cleaned(report)));
    assert_eq!(
        action,
        Some(Action::WorktreesRemoved(vec![PathBuf::from("/repo-wt/fix-c")]))
    );
    let shown = dialog(&state);
    assert_eq!(shown.phase, Phase::Done);
    assert_eq!(shown.deleted_count(), 2);
    assert!(shown
        .result_of(&Pick::Branch("fix/a".to_owned()))
        .is_some_and(Result::is_err));
    // Enter on the finished dialog closes it.
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Confirm), &ctx());
    assert!(state.actions.dialog.is_none());
}

#[test]
fn a_scan_that_fails_closes_the_dialog() {
    let mut state = tab();
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Open), &ctx());
    let _ = finish(
        &mut state,
        Job::FetchForCleanup,
        Err(ActionError::Io("no".to_owned())),
    );
    assert!(state.actions.dialog.is_none());
}

#[test]
fn it_does_not_open_where_the_actions_are_off() {
    let mut state = tab();
    state.actions.unavailable = Some(ncrs_git_view::actions::Unavailable::Sandbox);
    let _ = update(&mut state, Message::Cleanup(CleanupMessage::Open), &ctx());
    assert!(state.actions.dialog.is_none());
}

#[test]
fn while_the_dialog_is_open_its_keys_apply() {
    let keys = GitKeys::default();
    let open = KeyContext {
        cleanup_open: true,
        dialog_open: true,
        ..KeyContext::default()
    };
    let press = |named: Named, modifiers: Modifiers| {
        git_view_key(&keys, &Key::Named(named), modifiers, open)
    };
    let none = Modifiers::empty();
    assert_eq!(
        press(Named::ArrowDown, none),
        Some(Message::Cleanup(CleanupMessage::Down))
    );
    assert_eq!(
        press(Named::Space, none),
        Some(Message::Cleanup(CleanupMessage::Space))
    );
    assert_eq!(
        press(Named::ArrowLeft, none),
        Some(Message::Cleanup(CleanupMessage::Fold(false)))
    );
    assert_eq!(
        press(Named::Tab, Modifiers::SHIFT),
        Some(Message::Cleanup(CleanupMessage::Tab(true)))
    );
    assert_eq!(
        press(Named::Enter, none),
        Some(Message::Cleanup(CleanupMessage::Confirm))
    );
    assert_eq!(press(Named::F3, none), None);
}

#[test]
fn the_clean_up_key_is_alt_command_a_and_can_be_rebound() {
    use ncrs_git_view::shortcuts::Shortcut;
    assert_eq!(Shortcut::CleanUp.name(), "cleanup");
    let chord = Shortcut::CleanUp.default_chord(Modifiers::COMMAND).unwrap();
    assert_eq!(chord.0, Modifiers::COMMAND | Modifiers::ALT);
    assert_eq!(chord.1, Key::Character("a".into()));
}
