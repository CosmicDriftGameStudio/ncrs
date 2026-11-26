//! The history and the blame of a file on real repositories: a message goes
//! through `update`, the task it returns runs, and the state shows what came.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Once;

use iced::keyboard::{Key, Modifiers};
use iced::Task;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_git::{Blame, BlameLimit, ChangeKind, FileHistory};
use ncrs_git_view::actions::{MenuTarget, Unavailable};
use ncrs_git_view::file_views::{menu_entries, FileAction};
use ncrs_git_view::state::{Area, Subject};
use ncrs_git_view::{git_view_key, open, update, Ctx, GitKeys, Message, State};
use ncrs_i18n::Language;
use tempfile::TempDir;

const CTX: Ctx = Ctx::new(Language::English);

static ISOLATE: Once = Once::new();

fn sh(dir: &Path, args: &[&str]) -> String {
    ISOLATE.call_once(|| {
        let empty = std::env::temp_dir().join("ncrs-git-view-file-tests-empty-config");
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

fn commit_file(dir: &Path, relative: &str, content: &str, message: &str) {
    fs::write(dir.join(relative), content).unwrap();
    sh(dir, &["add", "--", relative]);
    sh(dir, &["commit", "-q", "-m", message]);
}

fn numbered(count: usize) -> String {
    (1..=count).map(|n| format!("line {n}\n")).collect()
}

/// `old.txt` is made, moved to `new.txt` and changed there.
fn repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let at = dir.path();
    sh(at, &["init", "-q", "-b", "main"]);
    sh(at, &["config", "user.name", "Test"]);
    sh(at, &["config", "user.email", "test@example.com"]);
    sh(at, &["config", "commit.gpgsign", "false"]);
    sh(at, &["config", "core.autocrlf", "false"]);
    commit_file(at, "old.txt", &numbered(20), "made");
    sh(at, &["mv", "old.txt", "new.txt"]);
    sh(at, &["commit", "-q", "-m", "moved"]);
    let changed = numbered(20).replace("line 3\n", "line three\n");
    commit_file(at, "new.txt", &changed, "changed");
    dir
}

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

async fn press(state: &mut State, letter: &str) {
    let keys = GitKeys::default();
    let key = Key::Character(letter.into());
    let message = git_view_key(&keys, &key, Modifiers::COMMAND, state.key_context(), false)
        .expect("the key is bound here");
    send(state, message).await;
}

async fn opened(dir: &Path) -> State {
    let mut state = State::default();
    let task = open(&mut state, dir.to_path_buf(), "repo".to_owned(), &CTX);
    settle(&mut state, task).await;
    state
}

/// The diff of the newest commit, as Enter on its file opens it.
async fn in_the_diff(dir: &Path) -> State {
    let mut state = opened(dir).await;
    send(&mut state, Message::CommitPressed(0)).await;
    send(&mut state, Message::FilePressed(0)).await;
    assert!(state.diff.is_some(), "the diff is open");
    state
}

fn summaries(state: &State) -> Vec<String> {
    let log = state.history.as_ref().and_then(|it| it.log.as_ref());
    log.expect("the history is read")
        .commits
        .iter()
        .map(|entry| entry.commit.summary.clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_history_key_in_the_diff_shows_the_commits_of_the_file() {
    let dir = repo();
    let mut state = in_the_diff(dir.path()).await;
    press(&mut state, "y").await;
    assert!(state.diff.is_none());
    assert_eq!(summaries(&state), ["changed", "moved", "made"]);
    let history = state.history.as_ref().unwrap();
    assert!(history.diff.is_some(), "the first commit's diff is read");
}

#[tokio::test(flavor = "multi_thread")]
async fn enter_in_the_history_opens_the_diff_at_the_path_of_that_commit() {
    let dir = repo();
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::OpenHistory("new.txt".to_owned())).await;
    send(&mut state, Message::Down).await;
    send(&mut state, Message::Down).await;
    send(&mut state, Message::Enter).await;
    let shown = state.diff.as_ref().expect("the diff is open");
    let file = shown.files.first().unwrap();
    assert_eq!(file.path, "old.txt");
    assert_eq!(file.kind, ChangeKind::Added);
    assert_eq!(
        shown.subject,
        Subject::Commit(sh(dir.path(), &["rev-parse", "HEAD~2"]))
    );
    assert_eq!(shown.diff.as_ref().map(|diff| diff.hunks.len()), Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_blame_key_shows_who_changed_each_line() {
    let dir = repo();
    let mut state = in_the_diff(dir.path()).await;
    press(&mut state, "i").await;
    let shown = state.blame.as_ref().expect("the blame is open");
    assert_eq!(shown.at, Some(sh(dir.path(), &["rev-parse", "HEAD"])));
    let found = shown.blame.as_ref().expect("the blame is read");
    let groups: Vec<(u32, &str)> = found
        .groups
        .iter()
        .map(|group| (group.first_line, group.summary.as_str()))
        .collect();
    assert_eq!(groups, [(1, "made"), (3, "changed"), (4, "made")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn enter_in_the_blame_goes_to_the_commit_of_the_line() {
    let dir = repo();
    let mut state = in_the_diff(dir.path()).await;
    press(&mut state, "i").await;
    send(&mut state, Message::Down).await;
    send(&mut state, Message::Down).await;
    send(&mut state, Message::Down).await;
    send(&mut state, Message::Enter).await;
    assert!(state.blame.is_none() && state.diff.is_none() && state.history.is_none());
    let chosen = state.commit_at(state.commit_cursor).unwrap();
    assert_eq!(chosen.summary, "made");
}

#[tokio::test(flavor = "multi_thread")]
async fn enter_in_the_blame_reads_on_for_a_commit_further_down() {
    let dir = repo();
    let at = dir.path();
    let wanted = sh(at, &["rev-parse", "HEAD~2"]);
    let tree = sh(at, &["rev-parse", "HEAD^{tree}"]);
    let mut tip = sh(at, &["rev-parse", "HEAD"]);
    for number in 0..ncrs_git_view::state::PAGE + 5 {
        let message = format!("filler {number}");
        tip = sh(at, &["commit-tree", &tree, "-p", &tip, "-m", &message]);
    }
    sh(at, &["reset", "-q", "--soft", &tip]);
    let mut state = opened(at).await;
    assert!(
        state.log.as_ref().unwrap().has_more,
        "the commit is not listed yet"
    );
    send(&mut state, Message::OpenBlame("new.txt".to_owned(), None)).await;
    send(&mut state, Message::Enter).await;
    let chosen = state.commit_at(state.commit_cursor).unwrap();
    assert_eq!(chosen.id, wanted);
}

#[tokio::test(flavor = "multi_thread")]
async fn escape_leaves_the_blame_then_the_diff_then_the_history() {
    let dir = repo();
    let mut state = opened(dir.path()).await;
    send(&mut state, Message::OpenHistory("new.txt".to_owned())).await;
    send(&mut state, Message::Enter).await;
    press(&mut state, "i").await;
    assert!(state.blame.is_some() && state.diff.is_some() && state.history.is_some());
    send(&mut state, Message::Escape).await;
    assert!(state.blame.is_none() && state.diff.is_some());
    send(&mut state, Message::Escape).await;
    assert!(state.diff.is_none() && state.history.is_some());
    send(&mut state, Message::Escape).await;
    assert!(state.history.is_none());
    assert!(state.is_open(), "the tab stays");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_that_comes_after_escape_is_dropped() {
    let dir = repo();
    let mut state = opened(dir.path()).await;
    let history = update(&mut state, Message::OpenHistory("new.txt".to_owned()), &CTX).0;
    drop(update(&mut state, Message::Escape, &CTX));
    settle(&mut state, history).await;
    assert!(state.history.is_none());

    let (late, _) = state.loads.blame.begin();
    send(&mut state, Message::OpenBlame("new.txt".to_owned(), None)).await;
    let before = state.blame.clone();
    let empty = Blame {
        lines: Vec::new(),
        groups: Vec::new(),
        refused: BlameLimit::Binary,
    };
    send(&mut state, Message::BlameLoaded(late, Box::new(Ok(empty)))).await;
    assert_eq!(state.blame, before);

    let (late_history, _) = state.loads.history.begin();
    send(&mut state, Message::OpenHistory("new.txt".to_owned())).await;
    let answer = Message::HistoryLoaded(late_history, Box::new(Ok(FileHistory::default())));
    send(&mut state, answer).await;
    assert_eq!(summaries(&state).len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sandbox_shows_history_and_blame() {
    let dir = repo();
    let mut state = opened(dir.path()).await;
    state.actions.unavailable = Some(Unavailable::Sandbox);
    send(&mut state, Message::CommitPressed(0)).await;
    send(&mut state, Message::FilePressed(0)).await;
    press(&mut state, "y").await;
    assert_eq!(summaries(&state).len(), 3);
    press(&mut state, "i").await;
    assert!(state
        .blame
        .as_ref()
        .is_some_and(|shown| shown.blame.is_some()));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_menu_of_a_file_opens_its_history_with_the_keys() {
    let dir = repo();
    let mut state = opened(dir.path()).await;
    state.actions.unavailable = Some(Unavailable::Sandbox);
    send(&mut state, Message::CommitPressed(0)).await;
    let target = MenuTarget::File {
        path: "new.txt".to_owned(),
        at: None,
    };
    send(&mut state, Message::OpenMenu(target)).await;
    assert_eq!(state.area, Area::Detail);
    assert!(state.actions.menu.is_some());
    send(&mut state, Message::Enter).await;
    assert!(state.actions.menu.is_none());
    assert_eq!(summaries(&state).len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_menu_of_an_untracked_file_starts_on_an_entry_that_works() {
    let dir = repo();
    fs::write(dir.path().join("junk.log"), "x").unwrap();
    let mut state = opened(dir.path()).await;
    let path = "junk.log".to_owned();
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::File { path, at: None }),
    )
    .await;
    let cursor = state.actions.menu.as_ref().unwrap().cursor;
    let entries = menu_entries(&state, "junk.log");
    assert!(entries[cursor].1, "{:?} is greyed out", entries[cursor].0);
    assert_eq!(entries[cursor].0, FileAction::OpenEditor);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_menu_closes_when_its_file_is_gone() {
    let dir = repo();
    fs::write(dir.path().join("junk.log"), "x").unwrap();
    let mut state = opened(dir.path()).await;
    let path = "junk.log".to_owned();
    send(
        &mut state,
        Message::OpenMenu(MenuTarget::File { path, at: None }),
    )
    .await;
    assert!(state.actions.menu.is_some(), "the new file has a menu");
    let entries = menu_entries(&state, "junk.log");
    assert!(entries.contains(&(FileAction::History, false)));
    assert!(entries.contains(&(FileAction::Ignore, true)));
    fs::remove_file(dir.path().join("junk.log")).unwrap();
    send(&mut state, Message::Refresh).await;
    assert!(state.actions.menu.is_none());
}
