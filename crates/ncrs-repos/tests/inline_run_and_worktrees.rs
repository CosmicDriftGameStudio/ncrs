//! A run leaves the list as it is, and the switch that hides the worktrees.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::{fixture, read, repo, summary, with_worktree, Fixture};
use ncrs_repos::{Action, BulkAction, Message, Outcome};

const NAMES: [&str; 4] = ["alpha", "bravo", "charlie", "delta"];

fn with_worktrees() -> Fixture {
    let mut fixture = fixture(&NAMES, 0);
    fixture.reading(
        "bravo",
        read(with_worktree(summary(0, 0, 0), "bravo-wt", "feature")),
    );
    fixture
}

#[test]
fn a_run_on_one_row_leaves_the_list_as_it_is_and_the_result_lies_at_the_row() {
    let mut fixture = with_worktrees();
    fixture.send(Message::RowPressed(repo("charlie")));
    let before = fixture.state.visible();
    let groups_before = fixture.state.groups().len();

    fixture.send(Message::Bulk(BulkAction::Fetch));
    assert_eq!(fixture.state.visible(), before);
    assert_eq!(fixture.state.groups().len(), groups_before);

    fixture.send(Message::StepDone(
        repo("charlie"),
        Box::new(ncrs_repos::Executed {
            outcome: Outcome::AlreadyCurrent,
            ran: Vec::new(),
            after: None,
        }),
    ));
    assert_eq!(fixture.state.visible(), before);
    let run = fixture.state.run().unwrap();
    assert_eq!(run.result(&repo("charlie")), Some(&Outcome::AlreadyCurrent));
    assert_eq!(run.result(&repo("alpha")), None);
}

#[test]
fn switching_the_worktrees_off_drops_their_rows_and_moves_the_cursor_to_the_repo() {
    let mut fixture = with_worktrees();
    let worktree_rows = |shown: &Fixture| {
        shown
            .state
            .groups()
            .iter()
            .flat_map(|group| group.rows.iter())
            .filter(|row| row.path != row.owner)
            .count()
    };
    assert_eq!(worktree_rows(&fixture), 1);
    let worktree = fixture
        .state
        .visible()
        .into_iter()
        .find(|path| *path != repo("bravo") && path.to_string_lossy().contains("bravo-wt"))
        .unwrap();
    fixture.send(Message::RowPressed(worktree));

    let switched_off = fixture.send(Message::ShowWorktreesToggled);

    assert_eq!(switched_off, vec![Action::ShowWorktreesChanged(false)]);
    assert_eq!(worktree_rows(&fixture), 0);
    assert_eq!(fixture.state.visible().len(), NAMES.len());
    assert_eq!(fixture.state.cursor(), Some(repo("bravo")));

    let switched_on = fixture.send(Message::ShowWorktreesToggled);
    assert_eq!(switched_on, vec![Action::ShowWorktreesChanged(true)]);
    assert_eq!(worktree_rows(&fixture), 1);
}

#[test]
fn the_tab_opens_with_the_remembered_switch() {
    let mut fixture = fixture(&NAMES, 0);
    fixture.ctx.settings.show_worktrees = false;
    let (state, _task) = ncrs_repos::open(&fixture.ctx);
    assert!(!state.show_worktrees());
}
