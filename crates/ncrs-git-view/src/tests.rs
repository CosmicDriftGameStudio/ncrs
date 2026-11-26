use std::path::{Path, PathBuf};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_git::{
    ChangeKind, ChangedFile, CommitDetail, FileDiff, GraphRow, Log, LogCommit, RefsOverview,
    WorktreeInfo,
};
use ncrs_i18n::Language;

use ncrs_ui_kit::git_rows::{CommitColumn, CommitColumns, COMMIT_COLUMN_MAX};

use ncrs_ui_kit::splitter::{Direction, Step};

use crate::keys::KeyContext;
use crate::panes::Split;
use crate::state::{remote_groups, Area, Fold, Section, SidebarItem, Subject};
use crate::{git_view_key, open, tab_title, update, Action, Ctx, GitKeys, Message, State};

const CTX: Ctx = Ctx::new(Language::English);

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

pub fn changed(path: &str) -> ChangedFile {
    ChangedFile {
        path: path.to_owned(),
        old_path: None,
        kind: ChangeKind::Modified,
        added: 1,
        removed: 0,
        binary: false,
        untracked: false,
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
        ..WorktreeInfo::default()
    }
}

pub fn opened(commits: &[&str], wip: usize) -> State {
    let mut state = State {
        root: Some(PathBuf::from("/repo")),
        name: "repo".to_owned(),
        ..State::default()
    };
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
    let (refs_generation, _) = state.loads.refs.begin();
    let overview = RefsOverview {
        worktrees: vec![
            worktree("/repo", wip, true),
            worktree("/repo-feature", 0, false),
        ],
        ..RefsOverview::default()
    };
    let _ = update(
        &mut state,
        Message::RefsLoaded(refs_generation, Box::new(Ok(overview))),
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
        author_time: 0,
        committer: String::new(),
        committer_time: 0,
        message: "title\n\nbody".to_owned(),
        files: files.iter().map(|path| changed(path)).collect(),
    };
    let _ = update(
        state,
        Message::DetailLoaded(generation, Box::new(Ok(info))),
        &CTX,
    );
}

pub fn send(state: &mut State, message: Message) -> Option<Action> {
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
    assert_eq!(state.area, Area::DetailSplit);
    send(&mut state, Message::NextArea);
    assert_eq!(state.area, Area::Detail);
    send(&mut state, Message::NextArea);
    assert_eq!(state.area, Area::Sidebar);
    send(&mut state, Message::NextArea);
    assert_eq!(state.area, Area::SidebarSplit);
    send(&mut state, Message::PreviousArea);
    assert_eq!(state.area, Area::Sidebar);
}

#[test]
fn left_and_right_move_between_the_areas_without_wrapping() {
    let mut state = opened(&["aaaaaaa"], 0);
    send(&mut state, Message::Right);
    assert_eq!(state.area, Area::Detail);
    send(&mut state, Message::Right);
    assert_eq!(state.area, Area::Detail);
    send(&mut state, Message::Left);
    assert_eq!(state.area, Area::Commits);
    send(&mut state, Message::Left);
    assert_eq!(state.area, Area::Sidebar);
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
        author_time: 0,
        committer: String::new(),
        committer_time: 0,
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
        context: String::new(),
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
fn enter_on_a_worktree_asks_to_open_it_as_a_git_tab() {
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
        Some(Action::OpenWorktreeTab(PathBuf::from("/repo-feature")))
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

pub fn pressed(key: Key, held: Modifiers, context: KeyContext) -> Option<Message> {
    git_view_key(&GitKeys::default(), &key, held, context, false)
}

pub fn character(letter: &str) -> Key {
    Key::Character(letter.into())
}

#[test]
fn keys_map_to_messages_and_the_filter_keeps_the_letters() {
    let none = Modifiers::empty();
    let plain = KeyContext::default();
    let in_diff = KeyContext {
        in_diff: true,
        ..plain
    };
    let typing = KeyContext {
        filter_focused: true,
        ..plain
    };
    let dialog = KeyContext {
        dialog_open: true,
        ..plain
    };
    assert_eq!(
        pressed(Key::Named(Named::ArrowDown), none, plain),
        Some(Message::Down)
    );
    assert_eq!(
        pressed(character("/"), none, plain),
        Some(Message::FocusFilter)
    );
    assert_eq!(pressed(character("n"), none, plain), None);
    assert_eq!(
        pressed(character("n"), none, in_diff),
        Some(Message::NextHunk)
    );
    assert_eq!(
        pressed(Key::Named(Named::ArrowDown), Modifiers::COMMAND, in_diff),
        Some(Message::NextFile)
    );
    assert_eq!(pressed(Key::Named(Named::ArrowDown), none, typing), None);
    // A dialog over the tab takes every key but Enter.
    assert_eq!(
        pressed(Key::Named(Named::Enter), none, dialog),
        Some(Message::Enter)
    );
    assert_eq!(pressed(Key::Named(Named::ArrowDown), none, dialog), None);
}

#[test]
fn the_jump_keys_do_not_fire_while_typing_and_work_otherwise() {
    let none = Modifiers::empty();
    let plain = KeyContext::default();
    let typing = KeyContext {
        filter_focused: true,
        ..plain
    };
    assert_eq!(
        pressed(character("h"), none, plain),
        Some(Message::JumpToHead)
    );
    assert_eq!(
        pressed(character("w"), none, plain),
        Some(Message::JumpToWip)
    );
    assert_eq!(
        pressed(character("t"), none, plain),
        Some(Message::ToggleTreeKey)
    );
    assert_eq!(
        pressed(Key::Named(Named::Backspace), none, plain),
        Some(Message::GoToParent)
    );
    for letter in ["h", "w", "t"] {
        assert_eq!(pressed(character(letter), none, typing), None);
    }
}

#[test]
fn an_open_picker_keeps_the_arrows_enter_and_its_two_keys() {
    let none = Modifiers::empty();
    let open = KeyContext {
        picker_open: true,
        ..KeyContext::default()
    };
    assert_eq!(
        pressed(Key::Named(Named::ArrowDown), none, open),
        Some(Message::Down)
    );
    assert_eq!(
        pressed(Key::Named(Named::Enter), none, open),
        Some(Message::PickerSubmit)
    );
    // Cmd+Enter commits elsewhere and means "stash and switch" here.
    assert_eq!(
        pressed(Key::Named(Named::Enter), Modifiers::COMMAND, open),
        Some(Message::PickerEnterWithStash)
    );
    // The letters go to the search field.
    assert_eq!(pressed(character("h"), none, open), None);
}

#[test]
fn a_focused_splitter_moves_by_16_and_with_shift_by_64() {
    for (area, split) in [
        (Area::SidebarSplit, Split::Sidebar),
        (Area::DetailSplit, Split::Detail),
    ] {
        let on_bar = KeyContext {
            area,
            ..KeyContext::default()
        };
        let right_means = if split == Split::Sidebar {
            Direction::Wider
        } else {
            Direction::Narrower
        };
        assert_eq!(
            pressed(Key::Named(Named::ArrowRight), Modifiers::empty(), on_bar),
            Some(Message::SplitNudge(split, Step::Fine, right_means))
        );
        assert_eq!(
            pressed(Key::Named(Named::ArrowRight), Modifiers::SHIFT, on_bar),
            Some(Message::SplitNudge(split, Step::Coarse, right_means))
        );
        assert_eq!(
            pressed(Key::Named(Named::Home), Modifiers::empty(), on_bar),
            Some(Message::SplitReset(split))
        );
    }
}

#[test]
fn a_click_on_a_worktree_asks_for_the_same_tab_as_enter() {
    let mut state = opened(&["aaaaaaa"], 0);
    let position = state
        .sidebar_items()
        .iter()
        .position(|item| *item == SidebarItem::Worktree(1))
        .expect("second worktree listed");
    assert_eq!(
        send(&mut state, Message::SidebarPressed(position)),
        Some(Action::OpenWorktreeTab(PathBuf::from("/repo-feature")))
    );
}

fn with_remotes(state: &mut State, remotes: &[&str]) {
    let mut overview = state.overview.clone().expect("refs are loaded");
    overview.remote = remotes.iter().map(|name| (*name).to_owned()).collect();
    let (generation, _) = state.loads.refs.begin();
    let _ = update(
        state,
        Message::RefsLoaded(generation, Box::new(Ok(overview))),
        &CTX,
    );
}

fn press_row(state: &mut State, wanted: SidebarItem) -> Option<Action> {
    let position = state
        .sidebar_items()
        .iter()
        .position(|item| *item == wanted)?;
    send(state, Message::SidebarPressed(position))
}

#[test]
fn remote_branches_are_listed_under_their_remote_without_its_prefix() {
    let mut state = opened(&["aaaaaaa"], 0);
    with_remotes(&mut state, &["origin/main", "origin/dev", "upstream/main"]);
    let remotes_at = |shown: &State| {
        shown
            .sidebar_items()
            .into_iter()
            .skip_while(|item| *item != SidebarItem::Header(Section::Remotes))
            .take_while(|item| *item != SidebarItem::Header(Section::Tags))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        remotes_at(&state),
        [
            SidebarItem::Header(Section::Remotes),
            SidebarItem::RemoteHeader(0),
            SidebarItem::Remote(0),
            SidebarItem::Remote(1),
            SidebarItem::RemoteHeader(1),
            SidebarItem::Remote(2),
        ]
    );
    let overview = state.overview.as_ref().expect("refs");
    let groups = remote_groups(overview);
    assert_eq!(groups[0].name, "origin");
    assert_eq!(groups[0].branches, [(0, "main"), (1, "dev")]);
    assert_eq!(groups[1].name, "upstream");

    press_row(&mut state, SidebarItem::RemoteHeader(0));
    assert_eq!(
        remotes_at(&state),
        [
            SidebarItem::Header(Section::Remotes),
            SidebarItem::RemoteHeader(0),
            SidebarItem::RemoteHeader(1),
            SidebarItem::Remote(2),
        ]
    );
}

#[test]
fn the_arrow_keys_walk_every_visible_sidebar_row_once() {
    let mut state = opened(&["aaaaaaa"], 0);
    with_remotes(&mut state, &["origin/main", "origin/dev", "upstream/main"]);
    press_row(&mut state, SidebarItem::RemoteHeader(1));
    state.area = Area::Sidebar;
    state.sidebar_cursor = 0;
    let items = state.sidebar_items();
    let mut walked = vec![items[state.sidebar_cursor]];
    for _ in 1..items.len() {
        send(&mut state, Message::Down);
        walked.push(state.sidebar_items()[state.sidebar_cursor]);
    }
    assert_eq!(walked, items);
    send(&mut state, Message::Down);
    assert_eq!(state.sidebar_cursor, items.len() - 1, "stops at the end");
}

#[test]
fn left_and_right_fold_a_remote_and_ask_the_app_to_remember_it() {
    let mut state = opened(&["aaaaaaa"], 0);
    with_remotes(&mut state, &["origin/main", "upstream/main"]);
    state.area = Area::Sidebar;
    state.sidebar_cursor = state
        .sidebar_items()
        .iter()
        .position(|item| *item == SidebarItem::RemoteHeader(0))
        .expect("origin");
    let folded = send(&mut state, Message::Left);
    assert_eq!(
        folded,
        Some(Action::FoldsChanged {
            root: PathBuf::from("/repo"),
            folded: vec!["remote:origin".to_owned()],
        })
    );
    assert!(!state.sidebar_items().contains(&SidebarItem::Remote(0)));
    assert!(matches!(
        send(&mut state, Message::Right),
        Some(Action::FoldsChanged { folded: now, .. }) if now.is_empty()
    ));
    assert!(state.sidebar_items().contains(&SidebarItem::Remote(0)));
}

#[test]
fn folded_groups_come_back_when_the_repository_is_opened_again() {
    let mut state = opened(&["aaaaaaa"], 0);
    with_remotes(&mut state, &["origin/main"]);
    press_row(&mut state, SidebarItem::Header(Section::Tags));
    let Some(Action::FoldsChanged { root, folded }) =
        press_row(&mut state, SidebarItem::RemoteHeader(0))
    else {
        panic!("folding asks the app to remember");
    };
    assert_eq!(folded, ["remote:origin", "tags"]);

    let remembered = Ctx {
        folded,
        ..Ctx::new(Language::English)
    };
    let mut reopened = State::default();
    drop(open(&mut reopened, root, "repo".to_owned(), &remembered));
    let mut expected = state.collapsed.clone();
    expected.insert(Fold::Section(Section::Remotes));
    assert_eq!(reopened.collapsed, expected);

    let mut other = State::default();
    drop(open(
        &mut other,
        PathBuf::from("/elsewhere"),
        "elsewhere".to_owned(),
        &Ctx::new(Language::English),
    ));
    assert_eq!(
        other.collapsed,
        [Fold::Section(Section::Remotes)].into_iter().collect()
    );
}

#[test]
fn remotes_start_shut_until_the_user_has_chosen() {
    let mut fresh = State::default();
    drop(open(&mut fresh, PathBuf::from("/a"), "a".to_owned(), &CTX));
    let (generation, _) = fresh.loads.refs.begin();
    drop(update(
        &mut fresh,
        Message::RefsLoaded(generation, Box::new(Ok(RefsOverview::default()))),
        &CTX,
    ));
    assert!(fresh.collapsed.contains(&Fold::Section(Section::Remotes)));

    let chosen = Ctx {
        folded: vec!["seen:remotes".to_owned()],
        ..Ctx::new(Language::English)
    };
    let mut kept_open = State::default();
    drop(open(
        &mut kept_open,
        PathBuf::from("/a"),
        "a".to_owned(),
        &chosen,
    ));
    assert!(kept_open.collapsed.is_empty());

    with_remotes(&mut fresh, &["origin/main"]);
    let Some(Action::FoldsChanged { folded, .. }) =
        press_row(&mut fresh, SidebarItem::Header(Section::Remotes))
    else {
        panic!("opening a group asks the app to remember");
    };
    assert!(folded.contains(&"seen:remotes".to_owned()));
}

#[test]
fn dragging_a_column_header_resizes_it_and_reports_the_width_once() {
    let mut state = opened(&["aaaaaaa"], 0);
    let standard = CommitColumns::STANDARD;
    send(&mut state, Message::ResizePressed(CommitColumn::Author));
    assert_eq!(send(&mut state, Message::ResizeMoved(500.0)), None);
    assert_eq!(send(&mut state, Message::ResizeMoved(450.0)), None);
    assert_eq!(
        state.columns_in_view(standard).author,
        standard.author + 50,
        "dragging left widens the column on the right of the gap"
    );
    assert_eq!(
        send(&mut state, Message::ResizeReleased),
        Some(Action::ColumnsResized(CommitColumns {
            author: standard.author + 50,
            ..standard
        }))
    );
    assert_eq!(send(&mut state, Message::ResizeReleased), None);

    send(&mut state, Message::ResizePressed(CommitColumn::Sha));
    send(&mut state, Message::ResizeMoved(100.0));
    send(&mut state, Message::ResizeMoved(-5_000.0));
    assert_eq!(
        state.columns_in_view(standard).sha,
        COMMIT_COLUMN_MAX,
        "stops at the widest"
    );
    send(&mut state, Message::Escape);
    assert!(state.column_drag.is_none());
    assert_eq!(state.columns_in_view(standard), standard);
}

fn with_branches(state: &mut State, names: &[&str]) {
    let mut overview = state.overview.clone().expect("refs are loaded");
    overview.local = names
        .iter()
        .enumerate()
        .map(|(at, name)| ncrs_git::BranchInfo {
            name: (*name).to_owned(),
            is_current: at == 0,
            ..ncrs_git::BranchInfo::default()
        })
        .collect();
    let (generation, _) = state.loads.refs.begin();
    drop(update(
        state,
        Message::RefsLoaded(generation, Box::new(Ok(overview))),
        &CTX,
    ));
}

#[test]
fn the_branch_picker_filters_and_enter_switches() {
    let mut state = opened(&["aaaaaaa"], 0);
    with_branches(&mut state, &["main", "fix/rundung", "release/1.4"]);
    drop(send(
        &mut state,
        Message::OpenPicker(crate::picker::PickerKind::Branch),
    ));
    send(&mut state, Message::PickerQuery("rund".to_owned()));
    let rows = crate::picker::visible(&state);
    assert_eq!(rows.len(), 2, "the match and the row for a new branch");
    send(&mut state, Message::PickerSubmit);
    assert!(state.picker.is_none());
    assert!(state.actions.busy.is_some());
}

#[test]
fn a_locked_picker_keeps_its_list_open_and_starts_nothing() {
    let mut state = opened(&["aaaaaaa"], 0);
    with_branches(&mut state, &["main", "fix/rundung"]);
    state.actions.unavailable = Some(crate::actions::Unavailable::Sandbox);
    drop(send(
        &mut state,
        Message::OpenPicker(crate::picker::PickerKind::Branch),
    ));
    send(&mut state, Message::PickerQuery("fix".to_owned()));
    send(&mut state, Message::PickerSubmit);
    assert!(state.picker.is_some());
    assert!(state.actions.busy.is_none());
}

#[test]
fn the_repository_picker_opens_another_repository_but_not_the_same() {
    let mut state = opened(&["aaaaaaa"], 0);
    let ctx = Ctx {
        repositories: vec![crate::picker::RepoEntry {
            name: "other".to_owned(),
            path: PathBuf::from("/other"),
        }],
        ..Ctx::new(Language::English)
    };
    drop(update(
        &mut state,
        Message::OpenPicker(crate::picker::PickerKind::Repository),
        &ctx,
    ));
    let rows = crate::picker::visible(&state);
    let other = rows
        .iter()
        .find(|row| row.label == "other")
        .expect("the known repository is listed");
    let (_, action) = update(&mut state, Message::PickerPressed(other.id), &ctx);
    assert_eq!(
        action,
        Some(Action::OpenRepository(PathBuf::from("/other")))
    );

    drop(update(
        &mut state,
        Message::OpenPicker(crate::picker::PickerKind::Repository),
        &ctx,
    ));
    let current = crate::picker::visible(&state)
        .into_iter()
        .find(|row| row.current)
        .expect("the open repository is listed");
    let (_, picked) = update(&mut state, Message::PickerPressed(current.id), &ctx);
    assert_eq!(picked, None);
}

#[test]
fn a_dragged_splitter_stays_in_bounds_and_reports_once_on_release() {
    let mut state = opened(&["aaaaaaa"], 0);
    send(&mut state, Message::SplitPressed(Split::Sidebar));
    send(&mut state, Message::PaneMoved(100.0));
    assert_eq!(
        send(&mut state, Message::PaneMoved(2_000.0)),
        None,
        "moving reports nothing"
    );
    let Some(Action::PanesResized(widths)) = send(&mut state, Message::PaneReleased) else {
        panic!("the release reports the widths");
    };
    assert_eq!(f32::from(widths.sidebar), crate::panes::SIDEBAR_BOUNDS.max);
    assert_eq!(send(&mut state, Message::PaneReleased), None);
}

#[test]
fn a_double_click_resets_a_splitter_and_the_keys_nudge_it() {
    let mut state = opened(&["aaaaaaa"], 0);
    let wide = Ctx {
        panes: crate::PaneWidths {
            sidebar: 400,
            detail: 450,
        },
        ..Ctx::new(Language::English)
    };
    let (_, action) = update(&mut state, Message::SplitReset(Split::Sidebar), &wide);
    assert_eq!(
        action,
        Some(Action::PanesResized(crate::PaneWidths::STANDARD))
    );
    let (_, nudged) = update(
        &mut state,
        Message::SplitNudge(Split::Detail, Step::Fine, Direction::Wider),
        &CTX,
    );
    let Some(Action::PanesResized(widths)) = nudged else {
        panic!("a nudge reports the widths");
    };
    assert_eq!(widths.detail, 466,);
}

#[test]
fn the_jump_keys_find_the_head_and_the_parent() {
    let mut state = opened(&["aaaaaaa", "bbbbbbb", "ccccccc"], 0);
    if let Some(log) = state.log.as_mut() {
        log.head = Some("ccccccc".to_owned());
    }
    send(&mut state, Message::JumpToHead);
    assert_eq!(state.commit_cursor, 2);
    send(&mut state, Message::JumpToCommit("aaaaaaa".to_owned()));
    assert_eq!(state.commit_cursor, 0);
    send(&mut state, Message::JumpToCommit("zzzzzzz".to_owned()));
    assert_eq!(state.commit_cursor, 0, "an unknown commit changes nothing");
    assert_eq!(state.pending_jump, None);
}

#[test]
fn a_commit_not_loaded_yet_is_waited_for() {
    let mut state = opened(&["aaaaaaa"], 0);
    if let Some(log) = state.log.as_mut() {
        log.has_more = true;
    }
    send(&mut state, Message::JumpToCommit("ddddddd".to_owned()));
    assert_eq!(state.pending_jump.as_deref(), Some("ddddddd"));
}

#[test]
fn the_sidebar_lists_the_worktrees_after_the_branches() {
    let state = opened(&["aaaaaaa"], 0);
    let items = state.sidebar_items();
    assert_eq!(items.first(), Some(&SidebarItem::Header(Section::Local)));
    assert!(items.contains(&SidebarItem::Header(Section::Worktrees)));
}
