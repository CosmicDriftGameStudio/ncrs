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
use ncrs_git::PullStrategy;
use ncrs_git_view::actions::{
    ChangeSide, ConflictAction, ConflictLine, Dialog, MenuAction, MenuTarget, Notice, Question,
    Side,
};
use ncrs_git_view::snapshot::view_lines;
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
    send(&mut state, Message::OpenMenu(MenuTarget::LocalBranch(topic))).await;
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
    assert_eq!(sh(dir.path(), &["rev-list", "--merges", "--count", "HEAD"]), "0");
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
    send(&mut state, Message::OpenMenu(MenuTarget::LocalBranch(feature))).await;
    send(&mut state, Message::MenuPressed(MenuAction::MergeInto)).await;
    let merge = state.actions.merge.as_ref().unwrap();
    assert_eq!((merge.from.as_str(), merge.into.as_str()), ("feature", "main"));
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
    send(&mut state, Message::OpenMenu(MenuTarget::LocalBranch(topic))).await;
    send(&mut state, Message::MenuPressed(MenuAction::Rename)).await;
    let Some(Dialog::Rename(rename)) = &state.actions.dialog else {
        panic!("no rename dialog: {:?}", state.actions.dialog);
    };
    assert_eq!(rename.name, "topic");

    send(&mut state, Message::RenameNameChanged("bad..name".to_owned())).await;
    send(&mut state, Message::ConfirmDialog).await;
    assert_eq!(
        state.actions.notice,
        Some(Notice::InvalidName {
            name: "bad..name".to_owned()
        })
    );
    assert!(sh(dir.path(), &["branch", "--list", "topic"]).contains("topic"));

    send(&mut state, Message::OpenMenu(MenuTarget::LocalBranch(topic))).await;
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

    send(&mut state, Message::OpenMenu(MenuTarget::Stash(0))).await;
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
    send(&mut state, Message::OpenMenu(MenuTarget::RemoteBranch(remote))).await;
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
    send(&mut state, Message::OpenMenu(MenuTarget::LocalBranch(feature))).await;
    send(&mut state, Message::MenuPressed(MenuAction::MergeInto)).await;
    let (_, asked) = update(
        &mut state,
        Message::ConflictShortcut(ConflictAction::OpenEditor),
        &CTX,
    );
    assert_eq!(
        asked,
        Some(ncrs_git_view::Action::OpenInEditor(dir.path().join("c.txt")))
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
