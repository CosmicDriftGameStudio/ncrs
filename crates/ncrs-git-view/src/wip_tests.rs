//! The pane of the local changes: which key goes where, and the diff it opens.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::keys::KeyContext;
use crate::state::{Area, State};
use crate::tests::{changed, character, opened, pressed, send};
use std::path::PathBuf;

use crate::{Action, GitKeys, Message};

fn with_local_changes(unstaged: &[&str], staged: &[&str]) -> State {
    let mut state = opened(&["aaaaaaa"], unstaged.len() + staged.len());
    state.actions.local = Some(crate::actions::LocalChanges {
        unstaged: unstaged.iter().map(|path| changed(path)).collect(),
        staged: staged.iter().map(|path| changed(path)).collect(),
        ..crate::actions::LocalChanges::default()
    });
    state.commit_cursor = 0;
    state.area = Area::Detail;
    state
}

fn pane_focus(state: &State) -> crate::actions::WipFocus {
    state.actions.local.as_ref().unwrap().focus
}

#[test]
fn the_working_row_stays_in_the_log_while_there_are_actions() {
    let state = with_local_changes(&["a.txt"], &[]);
    assert!(state.has_working_row());
    assert!(state.changes_shown());
    let clean = with_local_changes(&[], &[]);
    assert!(!clean.has_working_row());
}

#[test]
fn the_pane_keys_belong_to_the_detail_area_only() {
    let none = Modifiers::empty();
    let detail = KeyContext {
        area: Area::Detail,
        ..KeyContext::default()
    };
    let commits = KeyContext::default();
    assert_eq!(
        pressed(Key::Named(Named::Space), none, detail),
        Some(Message::ToggleStageSelected)
    );
    assert_eq!(
        pressed(Key::Named(Named::Space), none, commits),
        Some(Message::MarkCommit)
    );
    assert_eq!(
        pressed(Key::Named(Named::ArrowUp), Modifiers::COMMAND, detail),
        Some(Message::PreviousFile)
    );
    assert_eq!(
        pressed(Key::Named(Named::ArrowUp), Modifiers::COMMAND, commits),
        None
    );
}

#[test]
fn the_commit_fields_hand_the_commit_keys_and_tab_to_the_tab() {
    let keys = GitKeys::default();
    let enter = Key::Named(Named::Enter);
    assert_eq!(
        crate::commit_field_key(&keys, &enter, Modifiers::COMMAND),
        Some(Message::Commit)
    );
    assert_eq!(
        crate::commit_field_key(&keys, &enter, Modifiers::COMMAND | Modifiers::ALT),
        Some(Message::CommitAndPush)
    );
    assert_eq!(
        crate::commit_field_key(&keys, &character("l"), Modifiers::COMMAND),
        Some(Message::FocusCommitTitle)
    );
    assert_eq!(
        crate::commit_field_key(&keys, &Key::Named(Named::Tab), Modifiers::SHIFT),
        Some(Message::PreviousArea)
    );
    assert_eq!(
        crate::commit_field_key(&keys, &character("x"), Modifiers::empty()),
        None
    );
}

#[test]
fn the_arrows_walk_the_unstaged_files_then_the_staged_ones() {
    use crate::actions::ChangeSide;
    let mut state = with_local_changes(&["a.txt", "b.txt"], &["c.txt"]);
    send(&mut state, Message::Down);
    send(&mut state, Message::Down);
    let local = state.actions.local.as_ref().unwrap();
    assert_eq!(local.selected, Some((ChangeSide::Staged, 0)));
    send(&mut state, Message::Down);
    assert_eq!(
        state.actions.local.as_ref().unwrap().selected,
        Some((ChangeSide::Staged, 0))
    );
}

#[test]
fn a_click_on_a_changed_file_opens_its_diff() {
    use crate::actions::{ChangeSide, WipFocus};
    let mut state = with_local_changes(&["a.txt"], &["c.txt"]);
    state.area = Area::Commits;
    send(&mut state, Message::SelectChange(ChangeSide::Staged, 0));
    let local = state.actions.local.as_ref().unwrap();
    assert_eq!(local.selected, Some((ChangeSide::Staged, 0)));
    assert!(local.diff_open);
    assert_eq!(local.focus, WipFocus::Staged);
    assert_eq!(state.area, Area::Detail);
    assert_eq!(
        crate::file_views::middle(&state),
        crate::file_views::Middle::LocalDiff
    );
}

#[test]
fn tab_walks_the_stops_of_the_pane_before_it_leaves() {
    use crate::actions::WipFocus;
    let mut state = with_local_changes(&["a.txt"], &["b.txt"]);
    state.actions.local.as_mut().unwrap().focus = WipFocus::Unstaged;
    send(&mut state, Message::NextArea);
    assert_eq!(pane_focus(&state), WipFocus::Staged);
    for stop in [
        WipFocus::Title,
        WipFocus::Body,
        WipFocus::Amend,
        WipFocus::PushAfter,
        WipFocus::Commit,
    ] {
        send(&mut state, Message::NextArea);
        assert_eq!(pane_focus(&state), stop);
        assert_eq!(state.area, Area::Detail);
    }
    send(&mut state, Message::NextArea);
    assert_eq!(state.area, Area::Sidebar);
    send(&mut state, Message::PreviousArea);
    assert_eq!(state.area, Area::Detail);
    assert_eq!(pane_focus(&state), WipFocus::Staged);
}

#[test]
fn escape_leaves_the_field_then_the_diff_then_the_pane_then_the_tab() {
    use crate::actions::WipFocus;
    let mut state = with_local_changes(&["a.txt"], &[]);
    let local = state.actions.local.as_mut().unwrap();
    local.focus = WipFocus::Title;
    local.diff_open = true;
    send(&mut state, Message::Escape);
    assert_eq!(pane_focus(&state), WipFocus::Unstaged);
    assert!(state.actions.local.as_ref().unwrap().diff_open);
    send(&mut state, Message::Escape);
    assert!(!state.actions.local.as_ref().unwrap().diff_open);
    send(&mut state, Message::Escape);
    assert_eq!(state.area, Area::Commits);
    assert_eq!(send(&mut state, Message::Escape), Some(Action::Close));
}

#[test]
fn enter_opens_the_diff_of_the_chosen_change_and_f4_asks_for_the_editor() {
    let mut state = with_local_changes(&["a.txt"], &[]);
    state.root = Some(PathBuf::from("/repo"));
    state.actions.local.as_mut().unwrap().selected =
        Some((crate::actions::ChangeSide::Unstaged, 0));
    send(&mut state, Message::Enter);
    assert!(state.actions.local.as_ref().unwrap().diff_open);
    assert_eq!(
        send(
            &mut state,
            Message::ConflictShortcut(crate::actions::ConflictAction::OpenEditor)
        ),
        Some(Action::OpenInEditor(PathBuf::from("/repo/a.txt")))
    );
}

#[test]
fn only_a_file_git_does_not_know_can_be_ignored() {
    use crate::actions::{Dialog, Question};
    let mut state = with_local_changes(&["src/new.log", "known.rs"], &[]);
    state.actions.sync = Some(crate::actions::SyncBar::default());
    state.actions.local.as_mut().unwrap().unstaged[0].untracked = true;
    send(&mut state, Message::IgnoreFile("known.rs".to_owned()));
    assert!(state.actions.dialog.is_none());
    send(&mut state, Message::IgnoreFile("src/new.log".to_owned()));
    let Some(Dialog::Ask(Question::Ignore {
        folder, extension, ..
    })) = &state.actions.dialog
    else {
        panic!("the ignore question is open");
    };
    assert_eq!(folder.as_deref(), Some("src"));
    assert_eq!(extension.as_deref(), Some("log"));
}

fn finished_commit_and_push(error: ncrs_git::ActionError) -> Message {
    Message::JobDone(Box::new(crate::job::Finished {
        job: crate::job::Job::CommitAndPush {
            message: ncrs_git::CommitMessage {
                title: "mine".to_owned(),
                body: String::new(),
            },
            amend: false,
        },
        result: Ok(crate::job::Done::CommittedButNotPushed { undo: None, error }),
    }))
}

#[test]
fn a_failed_push_after_commit_and_push_offers_a_retry_of_the_push_only() {
    use ncrs_git::ActionError;
    for error in [
        ActionError::NetworkFailed {
            git_output: "unreachable".to_owned(),
        },
        ActionError::AuthFailed {
            remote: "origin".to_owned(),
            git_output: "denied".to_owned(),
        },
    ] {
        let mut state = with_local_changes(&["a.txt"], &[]);
        send(&mut state, finished_commit_and_push(error));
        assert_eq!(state.actions.retry, Some(crate::job::Job::Push));
        assert!(state.actions.notice.is_some());
    }
}

#[test]
fn hunks_are_not_applied_while_whitespace_is_ignored() {
    let mut state = with_local_changes(&["a.txt"], &[]);
    let local = state.actions.local.as_mut().unwrap();
    local.selected = Some((crate::actions::ChangeSide::Unstaged, 0));
    local.diff = Some(ncrs_git::FileDiff {
        path: "a.txt".to_owned(),
        hunks: vec![ncrs_git::DiffHunk {
            old_start: 1,
            new_start: 1,
            rows: Vec::new(),
            context: String::new(),
        }],
        ..ncrs_git::FileDiff::default()
    });
    local.ignore_whitespace = true;
    for message in [Message::StageHunk(0), Message::DiscardHunk(0)] {
        send(&mut state, message);
        assert!(state.actions.busy.is_none());
    }
}
