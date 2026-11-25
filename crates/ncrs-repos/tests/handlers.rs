//! The list: groups, filter, marks, favourites, unreachable repos, learning.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::{fixture, read, repo, summary, ROOT};
use ncrs_repos::{open, Action, GroupKey, Message, Origin, Reading, Store, REFRESH_EVERY};
use std::path::PathBuf;

const NAMES: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];

fn names_in(state: &ncrs_repos::State) -> Vec<String> {
    state
        .visible()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn favourites_stand_first_and_the_rest_under_their_source() {
    let fixture = fixture(&NAMES, 2);
    let groups = fixture.state.groups();
    let keys: Vec<&GroupKey> = groups.iter().map(|group| &group.key).collect();
    assert_eq!(
        keys,
        [
            &GroupKey::Favourites,
            &GroupKey::FoundIn(PathBuf::from(ROOT))
        ]
    );
    assert_eq!(groups[0].count, 2);
    assert_eq!(groups[1].count, 3);
}

#[test]
fn the_filter_narrows_the_list_by_name() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::FilterChanged("rav".to_owned()));
    assert_eq!(names_in(&fixture.state), ["bravo"]);
}

#[test]
fn only_repos_that_need_an_action_stay_when_asked() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.reading("charlie", read(summary(3, 0, 0)));
    fixture.send(Message::OnlyNeedingActionToggled);
    assert_eq!(names_in(&fixture.state), ["charlie"]);
}

#[test]
fn mark_moves_with_the_cursor_and_mark_all_takes_every_repo() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::Mark);
    assert_eq!(fixture.state.marked_repos(), [repo("alpha")]);
    fixture.send(Message::Down);
    fixture.send(Message::Mark);
    assert_eq!(fixture.state.marked_repos().len(), 2);
    fixture.send(Message::MarkAll);
    assert_eq!(fixture.state.marked_repos().len(), NAMES.len());
    fixture.send(Message::AllToggled);
    assert!(fixture.state.marked_repos().is_empty());
}

#[test]
fn shift_down_extends_the_marks_over_the_rows_passed() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::ExtendDown);
    fixture.send(Message::ExtendDown);
    assert!(fixture.state.marked_repos().len() >= 2);
}

#[test]
fn a_worktree_row_is_never_marked() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::RowToggled(repo("alpha-feature")));
    assert!(!fixture.state.is_marked(&repo("alpha-feature")));
    assert!(fixture.state.marked_repos().is_empty());
}

#[test]
fn the_star_key_toggles_the_favourite_and_writes_it_down() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::ToggleFavouriteAtCursor);
    let file = fixture.ctx.file.clone().unwrap();
    assert!(Store::read(&file).unwrap().is_favourite(&repo("alpha")));
    fixture.send(Message::ToggleFavouriteAtCursor);
    assert!(!Store::read(&file).unwrap().is_favourite(&repo("alpha")));
}

#[test]
fn a_folder_that_is_gone_moves_to_its_own_group_and_can_be_removed() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.reading("bravo", Reading::Gone);
    let groups = fixture.state.groups();
    assert!(groups
        .iter()
        .any(|group| group.key == GroupKey::Unreachable));
    fixture.send(Message::RowRemoved(repo("bravo")));
    assert!(!fixture.state.store().contains(&repo("bravo")));
    let file = fixture.ctx.file.clone().unwrap();
    assert!(!Store::read(&file).unwrap().contains(&repo("bravo")));
}

#[test]
fn the_remove_key_only_removes_a_repo_that_is_unreachable() {
    let mut fixture = fixture(&["alpha"], 0);
    fixture.send(Message::RemoveAtCursor);
    assert!(fixture.state.store().contains(&repo("alpha")));
    fixture.reading("alpha", Reading::Gone);
    fixture.send(Message::RemoveAtCursor);
    assert!(!fixture.state.store().contains(&repo("alpha")));
}

#[test]
fn a_repo_opened_in_a_panel_is_learned_when_the_setting_is_on() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::Learn(repo("zulu")));
    let entry = fixture.state.store().entry(&repo("zulu")).unwrap().clone();
    assert_eq!(entry.origin, Origin::Learned);
    fixture.ctx.settings.learn_from_panel = false;
    fixture.send(Message::Learn(repo("yankee")));
    assert!(!fixture.state.store().contains(&repo("yankee")));
}

#[test]
fn a_known_repo_is_not_learned_twice() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::Learn(repo("alpha")));
    assert_eq!(fixture.state.store().repos.len(), NAMES.len());
}

#[test]
fn enter_opens_the_repo_under_the_cursor_in_a_git_tab() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::Down);
    assert_eq!(
        fixture.send(Message::Enter),
        [Action::OpenGitTab(repo("bravo"))]
    );
}

#[test]
fn an_unreachable_repo_does_not_open() {
    let mut fixture = fixture(&["alpha"], 0);
    fixture.reading("alpha", Reading::Gone);
    assert!(fixture.send(Message::Enter).is_empty());
}

#[test]
fn open_in_tabs_takes_the_marked_repos() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::RowToggled(repo("bravo")));
    fixture.send(Message::RowToggled(repo("delta")));
    assert_eq!(
        fixture.send(Message::OpenInTabs),
        [Action::OpenGitTabs(vec![repo("bravo"), repo("delta")])]
    );
}

#[test]
fn a_folded_group_hides_its_rows_and_keeps_its_count() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.send(Message::GroupToggled(GroupKey::FoundIn(PathBuf::from(
        ROOT,
    ))));
    let group = fixture.state.groups().remove(0);
    assert_eq!(group.count, NAMES.len());
    assert!(group.rows.is_empty());
}

#[test]
fn a_list_that_cannot_be_read_is_named_once_and_stays_as_it_is() {
    let mut fixture = fixture(&["alpha"], 0);
    let file = fixture.ctx.file.clone().unwrap();
    let text = "version = 99\n";
    std::fs::write(&file, text).unwrap();
    let (state, _task) = open(&fixture.ctx);
    fixture.state = state;
    assert!(fixture.state.failure().is_some());

    fixture.state.failure = None;
    fixture.send(Message::Learn(repo("zulu")));
    assert!(fixture.state.store().contains(&repo("zulu")));
    assert_eq!(fixture.state.failure(), None);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[test]
fn showing_the_tab_reads_again_only_what_is_old() {
    let mut fixture = fixture(&["alpha"], 0);
    let opened = fixture.state.refreshed_at;
    assert_eq!(opened, Some(fixture.ctx.now));
    fixture.ctx.now += REFRESH_EVERY / 2;
    fixture.send(Message::Shown);
    assert_eq!(fixture.state.refreshed_at, opened);
    fixture.ctx.now += REFRESH_EVERY / 2;
    fixture.send(Message::Shown);
    assert_eq!(fixture.state.refreshed_at, Some(fixture.ctx.now));
}
