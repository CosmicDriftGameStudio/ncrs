use std::path::{Path, PathBuf};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_git::{
    ChangeKind, ChangedFile, CommitDetail, FileDiff, GraphRow, Log, LogCommit, RefsOverview,
    WorktreeInfo,
};
use ncrs_i18n::Language;

use crate::state::{Area, SidebarItem, Subject};
use crate::{git_view_key, tab_title, update, Action, Ctx, Message, State};

const CTX: Ctx = Ctx {
    lang: Language::English,
};

fn commit(id: &str) -> LogCommit {
    LogCommit {
        id: id.to_owned(),
        short: id.chars().take(7).collect(),
        parents: Vec::new(),
        author: "Test".to_owned(),
        time: 1_700_000_000,
        summary: format!("commit {id}"),
        refs: Vec::new(),
        is_merge: false,
        graph: GraphRow::default(),
    }
}

fn changed(path: &str) -> ChangedFile {
    ChangedFile {
        path: path.to_owned(),
        old_path: None,
        kind: ChangeKind::Modified,
        added: 1,
        removed: 0,
        binary: false,
    }
}

fn worktree(path: &str, wip: usize, is_current: bool) -> WorktreeInfo {
    WorktreeInfo {
        path: PathBuf::from(path),
        branch: Some("main".to_owned()),
        head_short: None,
        wip,
        ahead: 0,
        behind: 0,
        merged: false,
        is_main: is_current,
        is_current,
    }
}

fn opened(commits: &[&str], wip: usize) -> State {
    let mut state = State::default();
    state.root = Some(PathBuf::from("/repo"));
    state.name = "repo".to_owned();
    let (generation, _) = state.loads.log.begin();
    let log = Log {
        commits: commits.iter().map(|id| commit(id)).collect(),
        has_more: false,
        lane_count: 1,
        head: None,
    };
    let _ = update(
        &mut state,
        Message::LogLoaded(generation, Box::new(Ok(log))),
        &CTX,
    );
    let (generation, _) = state.loads.refs.begin();
    let overview = RefsOverview {
        worktrees: vec![
            worktree("/repo", wip, true),
            worktree("/repo-feature", 0, false),
        ],
        ..RefsOverview::default()
    };
    let _ = update(
        &mut state,
        Message::RefsLoaded(generation, Box::new(Ok(overview))),
        &CTX,
    );
    state
}

fn show_detail(state: &mut State, sha: &str, files: &[&str]) {
    let (generation, _) = state.loads.detail.begin();
    let info = CommitDetail {
        id: sha.to_owned(),
        short: sha.chars().take(7).collect(),
        parents: Vec::new(),
        author: "Test".to_owned(),
        time: 0,
        message: "title\n\nbody".to_owned(),
        files: files.iter().map(|path| changed(path)).collect(),
    };
    let _ = update(
        state,
        Message::DetailLoaded(generation, Box::new(Ok(info))),
        &CTX,
    );
}

fn send(state: &mut State, message: Message) -> Option<Action> {
    update(state, message, &CTX).1
}

#[test]
fn a_dirty_worktree_adds_a_working_copy_row_before_the_commits() {
    let state = opened(&["aaaaaaa", "bbbbbbb"], 2);
    assert!(state.has_working_row());
    assert_eq!(state.row_count(), 3);
    assert_eq!(state.subject_at(0), Some(Subject::Working));
    assert_eq!(
        state.subject_at(1),
        Some(Subject::Commit("aaaaaaa".to_owned()))
    );
}

#[test]
fn a_clean_worktree_has_no_working_copy_row() {
    let state = opened(&["aaaaaaa"], 0);
    assert_eq!(state.row_count(), 1);
    assert_eq!(
        state.subject_at(0),
        Some(Subject::Commit("aaaaaaa".to_owned()))
    );
}

#[test]
fn the_arrows_move_the_cursor_in_the_shown_area_and_stop_at_the_ends() {
    let mut state = opened(&["aaaaaaa", "bbbbbbb"], 0);
    send(&mut state, Message::Down);
    send(&mut state, Message::Down);
    assert_eq!(state.commit_cursor, 1);
    send(&mut state, Message::Up);
    send(&mut state, Message::Up);
    assert_eq!(state.commit_cursor, 0);
}

#[test]
fn tab_walks_through_the_areas_and_back() {
    let mut state = opened(&["aaaaaaa"], 0);
    assert_eq!(state.area, Area::Commits);
    send(&mut state, Message::NextArea);
    assert_eq!(state.area, Area::Detail);
    send(&mut state, Message::NextArea);
    assert_eq!(state.area, Area::Sidebar);
    send(&mut state, Message::PreviousArea);
    assert_eq!(state.area, Area::Detail);
}

#[test]
fn an_answer_of_an_older_read_is_dropped() {
    let mut state = opened(&["aaaaaaa"], 0);
    let (stale, _) = state.loads.detail.begin();
    let _ = state.loads.detail.begin();
    let info = CommitDetail {
        id: "aaaaaaa".to_owned(),
        short: String::new(),
        parents: Vec::new(),
        author: String::new(),
        time: 0,
        message: String::new(),
        files: vec![changed("a.txt")],
    };
    send(&mut state, Message::DetailLoaded(stale, Box::new(Ok(info))));
    assert!(state.detail.is_none());
}

#[test]
fn enter_on_a_file_opens_the_diff_and_escape_leaves_it_before_the_tab() {
    let mut state = opened(&["aaaaaaa"], 0);
    show_detail(&mut state, "aaaaaaa", &["a.txt", "b.txt"]);
    send(&mut state, Message::Enter);
    assert_eq!(state.area, Area::Detail);
    send(&mut state, Message::Down);
    send(&mut state, Message::Enter);
    let shown = state.diff.as_ref().expect("diff opened");
    assert_eq!(shown.file, 1);
    assert_eq!(shown.files.len(), 2);

    assert_eq!(send(&mut state, Message::Escape), None);
    assert!(state.diff.is_none());
    assert_eq!(send(&mut state, Message::Escape), Some(Action::Close));
    assert!(!state.is_open());
}

#[test]
fn the_other_file_keys_stay_inside_the_commit() {
    let mut state = opened(&["aaaaaaa"], 0);
    show_detail(&mut state, "aaaaaaa", &["a.txt", "b.txt"]);
    send(&mut state, Message::FilePressed(0));
    send(&mut state, Message::PreviousFile);
    assert_eq!(state.diff.as_ref().map(|shown| shown.file), Some(0));
    send(&mut state, Message::NextFile);
    send(&mut state, Message::NextFile);
    assert_eq!(state.diff.as_ref().map(|shown| shown.file), Some(1));
}

#[test]
fn f3_asks_for_the_file_of_the_diff_in_the_repository() {
    let mut state = opened(&["aaaaaaa"], 0);
    show_detail(&mut state, "aaaaaaa", &["src/a.txt"]);
    send(&mut state, Message::FilePressed(0));
    assert_eq!(
        send(&mut state, Message::ViewFile),
        Some(Action::ViewFile(Path::new("/repo").join("src/a.txt")))
    );
}

#[test]
fn the_diff_options_toggle_without_leaving_the_diff() {
    let mut state = opened(&["aaaaaaa"], 0);
    show_detail(&mut state, "aaaaaaa", &["a.txt"]);
    send(&mut state, Message::FilePressed(0));
    send(&mut state, Message::SetUnified(true));
    send(&mut state, Message::ToggleIgnoreWhitespace);
    let shown = state.diff.as_ref().expect("diff");
    assert!(shown.unified);
    assert!(shown.ignore_whitespace);
}

#[test]
fn hunks_step_within_the_diff() {
    let mut state = opened(&["aaaaaaa"], 0);
    show_detail(&mut state, "aaaaaaa", &["a.txt"]);
    send(&mut state, Message::FilePressed(0));
    let (generation, _) = state.loads.diff.begin();
    let hunk = |start| ncrs_git::DiffHunk {
        old_start: start,
        new_start: start,
        rows: Vec::new(),
    };
    let diff = FileDiff {
        path: "a.txt".to_owned(),
        hunks: vec![hunk(1), hunk(20)],
        ..FileDiff::default()
    };
    send(
        &mut state,
        Message::DiffLoaded(generation, Box::new(Ok(diff))),
    );
    send(&mut state, Message::NextHunk);
    send(&mut state, Message::NextHunk);
    assert_eq!(state.diff.as_ref().map(|shown| shown.hunk), Some(1));
    send(&mut state, Message::PreviousHunk);
    assert_eq!(state.diff.as_ref().map(|shown| shown.hunk), Some(0));
}

#[test]
fn enter_on_a_worktree_asks_to_show_it_in_the_panel() {
    let mut state = opened(&["aaaaaaa"], 0);
    state.area = Area::Sidebar;
    let items = state.sidebar_items();
    let position = items
        .iter()
        .position(|item| *item == SidebarItem::Worktree(1))
        .expect("second worktree listed");
    state.sidebar_cursor = position;
    assert_eq!(
        send(&mut state, Message::Enter),
        Some(Action::ShowWorktree(PathBuf::from("/repo-feature")))
    );
}

#[test]
fn a_section_header_folds_its_entries() {
    let mut state = opened(&["aaaaaaa"], 0);
    let before = state.sidebar_items().len();
    state.area = Area::Sidebar;
    let worktrees = state
        .sidebar_items()
        .iter()
        .position(|item| matches!(item, SidebarItem::Header(crate::state::Section::Worktrees)))
        .expect("header");
    state.sidebar_cursor = worktrees;
    send(&mut state, Message::Enter);
    assert_eq!(state.sidebar_items().len(), before - 2);
    send(&mut state, Message::Enter);
    assert_eq!(state.sidebar_items().len(), before);
}

#[test]
fn a_new_filter_starts_the_list_over() {
    let mut state = opened(&["aaaaaaa", "bbbbbbb"], 0);
    send(&mut state, Message::Down);
    send(&mut state, Message::FilterChanged("fix".to_owned()));
    assert_eq!(state.commit_cursor, 0);
    assert_eq!(state.filter, "fix");
    assert!(state.detail.is_none());
}

#[test]
fn the_tree_mode_lists_folders_before_their_files() {
    let mut state = opened(&["aaaaaaa"], 0);
    show_detail(&mut state, "aaaaaaa", &["src/a.rs", "src/b.rs", "z.txt"]);
    state.tree_mode = true;
    let rows = state.detail_rows();
    assert_eq!(rows.len(), 4);
    assert!(
        matches!(&rows[0], crate::state::DetailRow::Folder { depth: 0, name } if name == "src")
    );
    assert!(matches!(
        &rows[3],
        crate::state::DetailRow::File {
            depth: 0,
            index: 2,
            ..
        }
    ));
}

#[test]
fn the_tab_is_titled_with_the_repository_and_has_none_while_closed() {
    let mut state = opened(&["aaaaaaa"], 0);
    assert_eq!(
        tab_title(&state, Language::English),
        Some("Git \u{b7} repo".to_owned())
    );
    send(&mut state, Message::Close);
    assert_eq!(tab_title(&state, Language::English), None);
}

#[test]
fn keys_map_to_messages_and_the_filter_keeps_the_letters() {
    let none = Modifiers::empty();
    assert_eq!(
        git_view_key(&Key::Named(Named::ArrowDown), none, false, false),
        Some(Message::Down)
    );
    assert_eq!(
        git_view_key(&Key::Character("/".into()), none, false, false),
        Some(Message::FocusFilter)
    );
    assert_eq!(
        git_view_key(&Key::Character("n".into()), none, false, false),
        None
    );
    assert_eq!(
        git_view_key(&Key::Character("n".into()), none, false, true),
        Some(Message::NextHunk)
    );
    assert_eq!(
        git_view_key(
            &Key::Named(Named::ArrowDown),
            Modifiers::COMMAND,
            false,
            true
        ),
        Some(Message::NextFile)
    );
    assert_eq!(
        git_view_key(&Key::Named(Named::ArrowDown), none, true, false),
        None
    );
}
