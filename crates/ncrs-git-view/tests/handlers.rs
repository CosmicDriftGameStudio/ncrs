//! The actions of the Git tab on real repositories: a message goes through
//! `update`, the task it returns runs, its answers go back in, and the state
//! shows what git did.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Once;

use iced::Task;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_git::{Integration, PullStrategy, ResetMode};
use ncrs_git_view::actions::{
    ChangeSide, ConflictAction, ConflictLine, Dialog, MenuAction, MenuTarget, Notice, Question,
    Side, Unavailable,
};
use ncrs_git_view::commit_menu::{branch_label, enabled, CommitAction};
use ncrs_git_view::snapshot::view_lines;
use ncrs_git_view::state::{Area, Subject};
use ncrs_git_view::{open, update, Ctx, Message, State};
use ncrs_i18n::Language;
use tempfile::TempDir;

const CTX: Ctx = Ctx::new(Language::English);

static ISOLATE: Once = Once::new();

fn sh(dir: &Path, args: &[&str]) -> String {
    ISOLATE.call_once(|| {
        let empty = std::env::temp_dir().join("ncrs-git-view-tests-empty-config");
        fs::write(&empty, "").unwrap();
        std::env::set_var("GIT_CONFIG_GLOBAL", &empty);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    });
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write(dir: &Path, relative: &str, content: &str) {
    fs::write(dir.join(relative), content).unwrap();
}

fn read(dir: &Path, relative: &str) -> String {
    fs::read_to_string(dir.join(relative)).unwrap()
}

fn commit_file(dir: &Path, relative: &str, content: &str, message: &str) {
    write(dir, relative, content);
    sh(dir, &["add", "--", relative]);
    sh(dir, &["commit", "-q", "-m", message]);
}

fn repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    sh(dir.path(), &["init", "-q", "-b", "main"]);
    sh(dir.path(), &["config", "user.name", "Test"]);
    sh(dir.path(), &["config", "user.email", "test@example.com"]);
    sh(dir.path(), &["config", "commit.gpgsign", "false"]);
    sh(dir.path(), &["config", "merge.conflictStyle", "merge"]);
    commit_file(dir.path(), "a.txt", &numbered(30), "first");
    dir
}

fn numbered(count: usize) -> String {
    (1..=count).map(|n| format!("line {n}\n")).collect()
}

/// Changes the second and the next to last line: two hunks.
fn two_hunks() -> String {
    numbered(30)
        .replace("line 2\n", "line two\n")
        .replace("line 29\n", "line twenty-nine\n")
}

/// Runs the task and every task its answers start, feeding the answers back in.
async fn settle(state: &mut State, task: Task<Message>) {
    let mut queue = vec![task];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(next) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(message) = action {
                let (follow_up, _) = update(state, message, &CTX);
                queue.push(follow_up);
            }
        }
    }
}

async fn send(state: &mut State, message: Message) {
    let (task, _) = update(state, message, &CTX);
    settle(state, task).await;
}

async fn opened(dir: &Path) -> State {
    let mut state = State::default();
    let task = open(&mut state, dir.to_path_buf(), "repo".to_owned(), &CTX);
    settle(&mut state, task).await;
    state
}

fn local_index(state: &State, branch: &str) -> usize {
    state
        .overview
        .as_ref()
        .unwrap()
        .local
        .iter()
        .position(|found| found.name == branch)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn loading_fills_the_actions_from_the_repository() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let state = opened(dir.path()).await;
    let local = state.actions.local.as_ref().unwrap();
    assert_eq!(local.unstaged.len(), 1);
    assert_eq!(local.selected, Some((ChangeSide::Unstaged, 0)));
    assert_eq!(local.diff.as_ref().unwrap().hunks.len(), 2);
    assert_eq!(state.actions.sync.as_ref().unwrap().undo, None);
    assert!(state.actions.unavailable.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn staging_one_hunk_moves_only_that_hunk() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::StageHunk(0)).await;
    let local = state.actions.local.as_ref().unwrap();
    assert_eq!(local.staged.len(), 1);
    assert_eq!(local.unstaged.len(), 1);
    assert_eq!(local.diff.as_ref().unwrap().hunks.len(), 1);
    assert_eq!(
        sh(dir.path(), &["diff", "--cached", "--numstat"]),
        "1\t1\ta.txt"
    );
    assert_eq!(
        state.actions.sync.as_ref().unwrap().undo.as_deref(),
        Some("staged one file")
    );
    assert!(state.actions.busy.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn discarding_a_file_can_be_undone() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::DiscardFile(0)).await;
    assert_eq!(
        state.actions.dialog,
        Some(Dialog::Ask(Question::DiscardFile {
            path: "a.txt".to_owned()
        }))
    );
    assert_eq!(read(dir.path(), "a.txt"), two_hunks());
    send(&mut state, Message::Escape).await;
    assert_eq!(state.actions.dialog, None);
    assert_eq!(read(dir.path(), "a.txt"), two_hunks());
    send(&mut state, Message::DiscardFile(0)).await;
    send(&mut state, Message::Enter).await;
    assert_eq!(state.actions.dialog, None);
    assert_eq!(read(dir.path(), "a.txt"), numbered(30));
    assert!(state.actions.local.as_ref().unwrap().unstaged.is_empty());
    assert_eq!(
        state.actions.sync.as_ref().unwrap().undo.as_deref(),
        Some("discarded one file")
    );
    send(&mut state, Message::Undo).await;
    assert_eq!(read(dir.path(), "a.txt"), two_hunks());
    assert_eq!(state.actions.local.as_ref().unwrap().unstaged.len(), 1);
    assert_eq!(state.actions.sync.as_ref().unwrap().undo, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_takes_the_staged_files_and_clears_the_form() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::Commit).await;
    assert_eq!(state.actions.notice, Some(Notice::CommitTitleMissing));
    send(&mut state, Message::StageAll).await;
    send(&mut state, Message::CommitTitleChanged("second".to_owned())).await;
    send(&mut state, Message::Commit).await;
    assert_eq!(sh(dir.path(), &["log", "-1", "--format=%s"]), "second");
    let local = state.actions.local.as_ref().unwrap();
    assert!(local.staged.is_empty() && local.unstaged.is_empty());
    assert!(local.title.is_empty());
    assert_eq!(
        state.actions.sync.as_ref().unwrap().undo.as_deref(),
        Some("committed “second”")
    );
    assert_eq!(state.log.as_ref().unwrap().commits.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unmerged_branch_is_deleted_after_the_second_question() {
    let dir = repo();
    sh(dir.path(), &["checkout", "-q", "-b", "topic"]);
    commit_file(dir.path(), "t.txt", "t\n", "only on topic");
    sh(dir.path(), &["checkout", "-q", "main"]);
    let mut state = opened(dir.path()).await;
    let topic = local_index(&state, "topic");
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::LocalBranch(topic)),
    )
    .await;
    send(&mut state, Message::MenuPressed(MenuAction::Delete)).await;
    assert!(matches!(
        state.actions.dialog,
        Some(Dialog::Ask(Question::DeleteBranch { .. }))
    ));
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(
        state.actions.notice,
        Some(Notice::BranchNotMerged {
            branch: "topic".to_owned()
        })
    );
    assert!(sh(dir.path(), &["branch", "--list", "topic"]).contains("topic"));
    send(&mut state, Message::DeleteBranchAnyway).await;
    assert_eq!(state.actions.notice, None);
    assert_eq!(sh(dir.path(), &["branch", "--list", "topic"]), "");
    assert_eq!(
        state.actions.sync.as_ref().unwrap().undo.as_deref(),
        Some("deleted branch topic")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pull_on_diverged_branches_asks_and_rebases() {
    let origin = TempDir::new().unwrap();
    sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let url = origin.path().to_str().unwrap().to_owned();
    sh(dir.path(), &["remote", "add", "origin", &url]);
    sh(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    let other = TempDir::new().unwrap();
    sh(other.path(), &["clone", "-q", &url, "."]);
    sh(other.path(), &["config", "user.name", "Other"]);
    sh(other.path(), &["config", "user.email", "other@example.com"]);
    commit_file(other.path(), "o.txt", "o\n", "other");
    sh(other.path(), &["push", "-q"]);
    commit_file(dir.path(), "m.txt", "m\n", "mine");

    let mut state = opened(dir.path()).await;
    send(&mut state, Message::Pull).await;
    assert_eq!(state.actions.notice, Some(Notice::NeedsPullStrategy));
    send(&mut state, Message::PullWith(PullStrategy::Rebase)).await;
    assert_eq!(state.actions.notice, None);
    assert_eq!(
        sh(dir.path(), &["rev-list", "--merges", "--count", "HEAD"]),
        "0"
    );
    assert_eq!(sh(dir.path(), &["rev-list", "--count", "HEAD"]), "3");
}

fn lines(first: &str, last: &str) -> String {
    let middle: String = (2..=9).map(|n| format!("mid {n}\n")).collect();
    format!("{first}\n{middle}{last}\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_with_conflicts_is_resolved_block_by_block_and_continued() {
    let dir = repo();
    commit_file(dir.path(), "c.txt", &lines("start", "end"), "base");
    sh(dir.path(), &["checkout", "-q", "-b", "feature"]);
    commit_file(dir.path(), "c.txt", &lines("F1", "F10"), "feature side");
    sh(dir.path(), &["checkout", "-q", "main"]);
    commit_file(dir.path(), "c.txt", &lines("M1", "M10"), "main side");

    let mut state = opened(dir.path()).await;
    let feature = local_index(&state, "feature");
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::LocalBranch(feature)),
    )
    .await;
    send(&mut state, Message::MenuPressed(MenuAction::MergeInto)).await;
    let merge = state.actions.merge.as_ref().unwrap();
    assert_eq!(
        (merge.from.as_str(), merge.into.as_str()),
        ("feature", "main")
    );
    assert_eq!(merge.conflicts.len(), 1);
    assert_eq!(merge.conflicts[0].places, 2);
    assert!(merge
        .preview
        .iter()
        .any(|line| *line == ConflictLine::Mine("M1".to_owned())));

    send(&mut state, Message::MergeContinue).await;
    assert_eq!(
        state.actions.notice,
        Some(Notice::UnresolvedConflicts { count: 1 })
    );

    send(&mut state, Message::TakeBlock(0, Side::Mine)).await;
    assert_eq!(state.actions.merge.as_ref().unwrap().conflicts[0].places, 1);
    send(&mut state, Message::TakeBlock(0, Side::Theirs)).await;
    let resolved = state.actions.merge.as_ref().unwrap();
    assert!(resolved.conflicts[0].resolved);
    assert_eq!(resolved.open_count(), 0);

    send(&mut state, Message::MergeContinue).await;
    assert!(state.actions.merge.is_none());
    assert_eq!(read(dir.path(), "c.txt"), lines("M1", "F10"));
}

#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_branch_asks_for_the_name_and_the_backend_checks_it() {
    let dir = repo();
    sh(dir.path(), &["branch", "topic"]);
    let mut state = opened(dir.path()).await;
    let topic = local_index(&state, "topic");
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::LocalBranch(topic)),
    )
    .await;
    send(&mut state, Message::MenuPressed(MenuAction::Rename)).await;
    let Some(Dialog::Rename(rename)) = &state.actions.dialog else {
        panic!("no rename dialog: {:?}", state.actions.dialog);
    };
    assert_eq!(rename.name, "topic");

    send(
        &mut state,
        Message::RenameNameChanged("bad..name".to_owned()),
    )
    .await;
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(
        state.actions.notice,
        Some(Notice::InvalidName {
            name: "bad..name".to_owned()
        })
    );
    assert!(sh(dir.path(), &["branch", "--list", "topic"]).contains("topic"));

    send(
        &mut state,
        Message::OpenMenu(MenuTarget::LocalBranch(topic)),
    )
    .await;
    send(&mut state, Message::MenuPressed(MenuAction::Rename)).await;
    send(&mut state, Message::RenameNameChanged("better".to_owned())).await;
    send(&mut state, Message::Enter).await;
    assert_eq!(state.actions.dialog, None);
    assert_eq!(sh(dir.path(), &["branch", "--list", "topic"]), "");
    assert!(sh(dir.path(), &["branch", "--list", "better"]).contains("better"));
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_a_stash_and_deleting_on_the_remote_ask_and_cannot_be_undone() {
    let origin = TempDir::new().unwrap();
    sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let url = origin.path().to_str().unwrap().to_owned();
    sh(dir.path(), &["remote", "add", "origin", &url]);
    sh(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    sh(dir.path(), &["push", "-q", "origin", "main:topic"]);
    sh(dir.path(), &["fetch", "-q", "origin"]);
    write(dir.path(), "a.txt", &two_hunks());
    sh(dir.path(), &["stash", "push", "-q", "-m", "work"]);
    let mut state = opened(dir.path()).await;

    let stash = state.actions.stash_ids[0].clone();
    send(&mut state, Message::OpenMenu(MenuTarget::Stash(stash))).await;
    send(&mut state, Message::MenuPressed(MenuAction::StashDrop)).await;
    let Some(Dialog::Ask(question)) = state.actions.dialog.clone() else {
        panic!("no question: {:?}", state.actions.dialog);
    };
    assert!(matches!(question, Question::DropStash { .. }));
    assert!(!question.can_be_undone());
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(sh(dir.path(), &["stash", "list"]), "");
    assert!(state.actions.sync.as_ref().unwrap().undo.is_none());

    let remote = state
        .overview
        .as_ref()
        .unwrap()
        .remote
        .iter()
        .position(|name| name == "origin/topic")
        .unwrap();
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::RemoteBranch(remote)),
    )
    .await;
    send(&mut state, Message::MenuPressed(MenuAction::DeleteOnRemote)).await;
    assert_eq!(
        state.actions.dialog,
        Some(Dialog::Ask(Question::DeleteOnRemote {
            remote: "origin".to_owned(),
            branch: "topic".to_owned()
        }))
    );
    assert!(sh(origin.path(), &["branch", "--list", "topic"]).contains("topic"));
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(sh(origin.path(), &["branch", "--list", "topic"]), "");
}

#[tokio::test(flavor = "multi_thread")]
async fn in_a_rebase_both_puts_mine_first() {
    let dir = repo();
    commit_file(dir.path(), "c.txt", &lines("start", "end"), "base");
    sh(dir.path(), &["checkout", "-q", "-b", "feature"]);
    commit_file(dir.path(), "c.txt", &lines("F1", "F10"), "feature side");
    sh(dir.path(), &["checkout", "-q", "main"]);
    commit_file(dir.path(), "c.txt", &lines("M1", "M10"), "main side");
    sh(dir.path(), &["checkout", "-q", "feature"]);

    let mut state = opened(dir.path()).await;
    let main = local_index(&state, "main");
    send(&mut state, Message::OpenMenu(MenuTarget::LocalBranch(main))).await;
    send(&mut state, Message::MenuPressed(MenuAction::RebaseOnto)).await;
    assert!(state.actions.rebasing);
    send(
        &mut state,
        Message::ConflictShortcut(ConflictAction::Take(Side::Both)),
    )
    .await;
    let middle: String = (2..=9).map(|n| format!("mid {n}\n")).collect();
    assert_eq!(
        read(dir.path(), "c.txt"),
        format!("F1\nM1\n{middle}F10\nM10\n")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_a_conflict_in_the_editor_asks_the_app() {
    let dir = repo();
    commit_file(dir.path(), "c.txt", &lines("start", "end"), "base");
    sh(dir.path(), &["checkout", "-q", "-b", "feature"]);
    commit_file(dir.path(), "c.txt", &lines("F1", "F10"), "feature side");
    sh(dir.path(), &["checkout", "-q", "main"]);
    commit_file(dir.path(), "c.txt", &lines("M1", "M10"), "main side");
    let mut state = opened(dir.path()).await;
    let feature = local_index(&state, "feature");
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::LocalBranch(feature)),
    )
    .await;
    send(&mut state, Message::MenuPressed(MenuAction::MergeInto)).await;
    let (_, asked) = update(
        &mut state,
        Message::ConflictShortcut(ConflictAction::OpenEditor),
        &CTX,
    );
    assert_eq!(
        asked,
        Some(ncrs_git_view::Action::OpenInEditor(
            dir.path().join("c.txt")
        ))
    );
}

#[test]
fn in_a_rebase_the_sides_swap_so_mine_stays_first() {
    use ncrs_git::PreviewLine;
    let lines = vec![
        PreviewLine::Shared(1, "same".to_owned()),
        PreviewLine::Ours("onto".to_owned()),
        PreviewLine::Theirs("replayed".to_owned()),
        PreviewLine::Shared(5, "end".to_owned()),
    ];
    assert_eq!(
        view_lines(lines.clone(), true),
        vec![
            ConflictLine::Shared(1, "same".to_owned()),
            ConflictLine::Mine("replayed".to_owned()),
            ConflictLine::Theirs("onto".to_owned()),
            ConflictLine::Shared(5, "end".to_owned()),
        ]
    );
    assert_eq!(
        view_lines(lines, false),
        vec![
            ConflictLine::Shared(1, "same".to_owned()),
            ConflictLine::Mine("onto".to_owned()),
            ConflictLine::Theirs("replayed".to_owned()),
            ConflictLine::Shared(5, "end".to_owned()),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn space_stages_the_chosen_file_and_takes_it_back() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::ToggleStageSelected).await;
    let local = state.actions.local.as_ref().unwrap();
    assert_eq!((local.unstaged.len(), local.staged.len()), (0, 1));
    assert_eq!(local.selected, Some((ChangeSide::Staged, 0)));
    send(&mut state, Message::ToggleStageSelected).await;
    let after = state.actions.local.as_ref().unwrap();
    assert_eq!((after.unstaged.len(), after.staged.len()), (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn ignoring_an_untracked_file_hides_it_and_can_be_undone() {
    let dir = repo();
    write(dir.path(), "junk.log", "noise\n");
    let mut state = opened(dir.path()).await;
    let place = |shown: &State| {
        shown
            .actions
            .local
            .as_ref()
            .unwrap()
            .unstaged
            .iter()
            .position(|file| file.path == "junk.log")
    };
    let index = place(&state).expect("the new file is listed");
    send(&mut state, Message::IgnoreFile(index)).await;
    send(
        &mut state,
        Message::IgnoreChoiceChanged(ncrs_git_view::actions::IgnoreChoice::Extension),
    )
    .await;
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(read(dir.path(), ".gitignore"), "*.log\n");
    assert_eq!(place(&state), None);
    send(&mut state, Message::Undo).await;
    assert!(!dir.path().join(".gitignore").exists());
    assert!(place(&state).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tracked_file_is_not_offered_for_ignoring() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::IgnoreFile(0)).await;
    assert!(state.actions.dialog.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn commit_and_push_pushes_after_the_commit() {
    let origin = TempDir::new().unwrap();
    sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let url = origin.path().to_str().unwrap().to_owned();
    sh(dir.path(), &["remote", "add", "origin", &url]);
    sh(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::StageAll).await;
    send(&mut state, Message::CommitTitleChanged("second".to_owned())).await;
    send(&mut state, Message::CommitAndPush).await;
    assert_eq!(sh(origin.path(), &["log", "-1", "--format=%s"]), "second");
    assert!(state
        .actions
        .local
        .as_ref()
        .is_some_and(|local| local.title.is_empty()));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_push_afterwards_box_turns_the_commit_into_commit_and_push() {
    let origin = TempDir::new().unwrap();
    sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let url = origin.path().to_str().unwrap().to_owned();
    sh(dir.path(), &["remote", "add", "origin", &url]);
    sh(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::StageAll).await;
    send(&mut state, Message::TogglePushAfter).await;
    send(&mut state, Message::CommitTitleChanged("third".to_owned())).await;
    send(&mut state, Message::Commit).await;
    assert_eq!(sh(origin.path(), &["log", "-1", "--format=%s"]), "third");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_title_over_the_counter_limit_still_commits() {
    let dir = repo();
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::StageAll).await;
    let long = "x".repeat(100);
    send(&mut state, Message::CommitTitleChanged(long.clone())).await;
    send(&mut state, Message::Commit).await;
    assert_eq!(sh(dir.path(), &["log", "-1", "--format=%s"]), long);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_push_after_commit_and_push_keeps_the_commit_and_its_undo() {
    let origin = TempDir::new().unwrap();
    sh(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repo();
    let url = origin.path().to_str().unwrap().to_owned();
    sh(dir.path(), &["remote", "add", "origin", &url]);
    sh(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    let other = TempDir::new().unwrap();
    sh(other.path(), &["clone", "-q", &url, "."]);
    sh(other.path(), &["config", "user.name", "Other"]);
    sh(other.path(), &["config", "user.email", "other@example.com"]);
    commit_file(other.path(), "o.txt", "o\n", "other");
    sh(other.path(), &["push", "-q"]);
    let before = sh(dir.path(), &["rev-parse", "HEAD"]);
    write(dir.path(), "a.txt", &two_hunks());
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::StageAll).await;
    send(&mut state, Message::CommitTitleChanged("mine".to_owned())).await;
    send(&mut state, Message::CommitAndPush).await;
    assert!(matches!(
        state.actions.notice,
        Some(Notice::PushRejected { .. })
    ));
    assert_eq!(sh(dir.path(), &["log", "-1", "--format=%s"]), "mine");
    assert!(state.actions.undo.is_some());
    assert!(state
        .actions
        .local
        .as_ref()
        .is_some_and(|local| local.title.is_empty()));
    send(&mut state, Message::Undo).await;
    assert_eq!(sh(dir.path(), &["rev-parse", "HEAD"]), before);
}

// The menu of a commit (#369).

/// main: base, second, third. side (from base): "on side", "clash" (which
/// changes a.txt as "third" does).
fn menu_repo() -> TempDir {
    let dir = repo();
    let path = dir.path();
    sh(path, &["checkout", "-q", "-b", "side"]);
    commit_file(path, "other.txt", "other\n", "on side");
    commit_file(path, "a.txt", "side\n", "clash");
    sh(path, &["checkout", "-q", "main"]);
    commit_file(path, "b.txt", "two\n", "second");
    commit_file(path, "a.txt", "main\n", "third");
    dir
}

fn row_of(state: &State, summary: &str) -> usize {
    (0..state.row_count())
        .find(|row| state.commit_at(*row).is_some_and(|c| c.summary == summary))
        .unwrap()
}

/// Opens the menu on the commit and runs one entry, as the menu does.
async fn run(state: &mut State, summary: &str, action: CommitAction) {
    let row = row_of(state, summary);
    let sha = state.commit_at(row).unwrap().id.clone();
    send(state, Message::OpenMenu(MenuTarget::Commit(sha))).await;
    send(state, Message::CommitAction(action)).await;
}

fn head(dir: &Path) -> String {
    sh(dir, &["rev-parse", "HEAD"])
}

#[tokio::test(flavor = "multi_thread")]
async fn checking_out_a_commit_detaches_head_and_names_it() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    run(&mut state, "second", CommitAction::Checkout).await;
    assert_eq!(head(dir.path()), sh(dir.path(), &["rev-parse", "main~1"]));
    assert_eq!(sh(dir.path(), &["branch", "--show-current"]), "");
    assert_eq!(branch_label(&state, Language::English), "detached HEAD");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tag_is_asked_for_made_and_undone() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    run(&mut state, "second", CommitAction::Tag).await;
    assert!(matches!(state.actions.dialog, Some(Dialog::Tag(_))));
    send(&mut state, Message::TagNameChanged("v 1".to_owned())).await;
    send(&mut state, Message::Enter).await;
    assert!(matches!(state.actions.dialog, Some(Dialog::Tag(_))));
    assert_eq!(sh(dir.path(), &["tag", "--list"]), "");
    send(&mut state, Message::TagNameChanged("v1".to_owned())).await;
    send(&mut state, Message::Enter).await;
    assert_eq!(state.actions.dialog, None);
    assert_eq!(sh(dir.path(), &["tag", "--list"]), "v1");
    assert_eq!(
        sh(dir.path(), &["rev-parse", "v1^{commit}"]),
        sh(dir.path(), &["rev-parse", "main~1"])
    );
    send(&mut state, Message::Undo).await;
    assert_eq!(sh(dir.path(), &["tag", "--list"]), "");
}

#[tokio::test(flavor = "multi_thread")]
async fn cherry_pick_copies_the_commit_and_can_be_undone() {
    let dir = menu_repo();
    let before = head(dir.path());
    let mut state = opened(dir.path()).await;
    run(&mut state, "on side", CommitAction::CherryPick).await;
    assert!(dir.path().join("other.txt").exists());
    assert_ne!(head(dir.path()), before);
    assert!(state.actions.undo.is_some());
    send(&mut state, Message::Undo).await;
    assert_eq!(head(dir.path()), before);
    assert!(!dir.path().join("other.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cherry_pick_with_conflicts_shows_the_banner_and_aborts() {
    let dir = menu_repo();
    let before = head(dir.path());
    let mut state = opened(dir.path()).await;
    run(&mut state, "clash", CommitAction::CherryPick).await;
    let merge = state.actions.merge.as_ref().unwrap();
    assert_eq!(merge.kind, Integration::CherryPick);
    assert_eq!(merge.conflicts.len(), 1);
    send(&mut state, Message::MergeAbort).await;
    assert!(state.actions.merge.is_none());
    assert_eq!(head(dir.path()), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn revert_makes_a_commit_that_takes_the_change_back() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    run(&mut state, "second", CommitAction::Revert).await;
    assert!(!dir.path().join("b.txt").exists());
    assert!(sh(dir.path(), &["log", "-1", "--format=%s"]).starts_with("Revert"));
    send(&mut state, Message::Undo).await;
    assert!(dir.path().join("b.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_commit_cannot_be_picked_or_reverted() {
    let dir = menu_repo();
    sh(
        dir.path(),
        &["merge", "-q", "--no-ff", "-m", "joined", "side~1"],
    );
    let state = opened(dir.path()).await;
    let row = row_of(&state, "joined");
    assert!(!enabled(&state, row, CommitAction::CherryPick));
    assert!(!enabled(&state, row, CommitAction::Revert));
    assert!(enabled(&state, row, CommitAction::Checkout));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sandbox_leaves_copying_and_comparing() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    state.actions.unavailable = Some(Unavailable::Sandbox);
    let row = row_of(&state, "second");
    assert!(enabled(&state, row, CommitAction::CopySha));
    assert!(enabled(&state, row, CommitAction::CompareWorking));
    for blocked in [
        CommitAction::Checkout,
        CommitAction::NewBranch,
        CommitAction::Tag,
        CommitAction::CherryPick,
        CommitAction::Revert,
        CommitAction::Reset(ResetMode::Soft),
        CommitAction::Reset(ResetMode::Hard),
    ] {
        assert!(!enabled(&state, row, blocked), "{blocked:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_soft_reset_keeps_the_changes_staged_a_mixed_one_unstaged() {
    let dir = menu_repo();
    let second = sh(dir.path(), &["rev-parse", "main~1"]);
    let mut state = opened(dir.path()).await;
    run(&mut state, "second", CommitAction::Reset(ResetMode::Soft)).await;
    assert_eq!(head(dir.path()), second);
    assert_eq!(sh(dir.path(), &["diff", "--cached", "--name-only"]), "a.txt");
    send(&mut state, Message::Undo).await;
    run(&mut state, "second", CommitAction::Reset(ResetMode::Mixed)).await;
    assert_eq!(sh(dir.path(), &["diff", "--cached", "--name-only"]), "");
    assert_eq!(sh(dir.path(), &["diff", "--name-only"]), "a.txt");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hard_reset_asks_with_the_counts_and_enter_cancels() {
    let dir = menu_repo();
    let second = sh(dir.path(), &["rev-parse", "main~1"]);
    write(dir.path(), "a.txt", "dirty\n");
    let before = head(dir.path());
    let mut state = opened(dir.path()).await;
    run(&mut state, "second", CommitAction::Reset(ResetMode::Hard)).await;
    let Some(Dialog::Ask(Question::ResetHard {
        branch,
        commits,
        changed_files,
        ..
    })) = state.actions.dialog.clone()
    else {
        panic!("no question: {:?}", state.actions.dialog);
    };
    assert_eq!(branch, "main");
    assert_eq!((commits, changed_files), (1, 1));
    send(&mut state, Message::Enter).await;
    assert_eq!(state.actions.dialog, None);
    assert_eq!(head(dir.path()), before);
    run(&mut state, "second", CommitAction::Reset(ResetMode::Hard)).await;
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(head(dir.path()), second);
    send(&mut state, Message::Undo).await;
    assert_eq!(head(dir.path()), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_keys_walk_the_menu_and_its_submenu() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    state.area = Area::Commits;
    state.commit_cursor = row_of(&state, "second");
    send(&mut state, Message::OpenMenuKey).await;
    let open = state.actions.menu.clone().unwrap();
    let cursor_sha = state.commit_at(state.commit_cursor).unwrap().id.clone();
    assert_eq!(open.target, MenuTarget::Commit(cursor_sha));
    for _ in 0..6 {
        send(&mut state, Message::Down).await;
    }
    assert_eq!(state.actions.menu.clone().unwrap().cursor, 6);
    send(&mut state, Message::Right).await;
    assert_eq!(state.actions.menu.clone().unwrap().submenu, Some(0));
    send(&mut state, Message::Down).await;
    assert_eq!(state.actions.menu.clone().unwrap().submenu, Some(1));
    send(&mut state, Message::Left).await;
    assert_eq!(state.actions.menu.clone().unwrap().submenu, None);
    send(&mut state, Message::Escape).await;
    assert_eq!(state.actions.menu, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn space_marks_a_commit_and_the_comparison_runs() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    state.area = Area::Commits;
    state.commit_cursor = row_of(&state, "second");
    let second = state.commit_at(state.commit_cursor).unwrap().id.clone();
    let third_row = row_of(&state, "third");
    let third = state.commit_at(third_row).unwrap().id.clone();
    assert!(!enabled(&state, third_row, CommitAction::CompareMarked));
    send(&mut state, Message::MarkCommit).await;
    assert_eq!(state.marked_commit.as_deref(), Some(second.as_str()));
    assert!(enabled(&state, third_row, CommitAction::CompareMarked));
    run(&mut state, "third", CommitAction::CompareMarked).await;
    let shown = state.diff.as_ref().unwrap();
    assert_eq!(
        shown.subject,
        Subject::Compare {
            from: second,
            to: Some(third)
        }
    );
    assert_eq!(shown.files.len(), 1);
    assert!(shown.diff.is_some());
    state.commit_cursor = row_of(&state, "second");
    send(&mut state, Message::MarkCommit).await;
    assert_eq!(state.marked_commit, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn comparing_with_the_working_copy_lists_the_differences() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    run(&mut state, "first", CommitAction::CompareWorking).await;
    let shown = state.diff.as_ref().unwrap();
    assert!(matches!(shown.subject, Subject::Compare { to: None, .. }));
    let paths: Vec<_> = shown.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["a.txt", "b.txt"]);
    send(&mut state, Message::FilePressed(1)).await;
    let shown = state.diff.as_ref().unwrap();
    assert_eq!(shown.file, 1);
    assert!(shown.diff.is_some());
}

#[tokio::test]
async fn a_right_click_on_a_commit_moves_the_cursor_to_it() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    state.area = Area::Sidebar;
    let sha = state.commit_at(row_of(&state, "second")).unwrap().id.clone();
    send(&mut state, Message::OpenMenu(MenuTarget::Commit(sha))).await;
    assert_eq!(state.commit_cursor, row_of(&state, "second"));
    assert_eq!(state.area, Area::Commits);
}

#[tokio::test]
async fn a_commit_menu_closes_when_its_commit_is_gone_after_a_reload() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    let sha = state.commit_at(row_of(&state, "second")).unwrap().id.clone();
    send(&mut state, Message::OpenMenu(MenuTarget::Commit(sha.clone()))).await;
    state.log.as_mut().unwrap().commits.retain(|commit| commit.id != sha);
    send(&mut state, Message::CommitAction(CommitAction::CopySha)).await;
    assert_eq!(state.actions.menu, None);
    assert_eq!(state.copied_sha, None, "nothing is copied for a commit that is gone");
}

#[tokio::test]
async fn the_mark_stays_on_its_commit_when_the_list_grows_at_the_top() {
    let dir = menu_repo();
    let mut state = opened(dir.path()).await;
    state.area = Area::Commits;
    state.commit_cursor = row_of(&state, "second");
    commit_file(dir.path(), "c.txt", "c\n", "newest");
    send(&mut state, Message::Refresh).await;
    send(&mut state, Message::MarkCommit).await;
    let marked = state.marked_commit.clone().unwrap();
    assert_eq!(
        state.commit_at(row_of(&state, "second")).unwrap().id,
        marked,
        "the cursor followed its commit down one row"
    );
}

#[tokio::test]
async fn a_stash_menu_acts_on_nothing_once_its_stash_is_gone() {
    let dir = menu_repo();
    write(dir.path(), "a.txt", "dirty\n");
    sh(dir.path(), &["stash", "push", "-q", "-m", "work"]);
    let mut state = opened(dir.path()).await;
    let stash = state.actions.stash_ids[0].clone();
    send(&mut state, Message::OpenMenu(MenuTarget::Stash(stash))).await;
    sh(dir.path(), &["stash", "drop", "-q"]);
    send(&mut state, Message::Refresh).await;
    send(&mut state, Message::MenuPressed(MenuAction::StashDrop)).await;
    assert_eq!(state.actions.dialog, None);
}
