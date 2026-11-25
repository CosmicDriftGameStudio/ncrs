//! A bulk action over the favourites or the marked repos: the queue, the
//! result, the retry and what is locked while it runs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::{fixture, read, repo, summary, Fixture};
use ncrs_repos::{BulkAction, Message, Outcome, Scope};
use ncrs_ui_kit::git_state::RunOutcome;

const NAMES: [&str; 6] = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];

fn behind_fixture(favourites: usize) -> Fixture {
    let mut fixture = fixture(&NAMES, favourites);
    for name in NAMES {
        fixture.reading(name, read(summary(2, 0, 0)));
    }
    fixture
}

fn finish(fixture: &mut Fixture, name: &str, outcome: Outcome) {
    fixture.send(Message::StepDone(
        repo(name),
        Box::new(ncrs_repos::Executed {
            outcome,
            ran: Vec::new(),
            after: None,
        }),
    ));
}

#[test]
fn the_action_runs_on_the_favourites_when_nothing_is_marked() {
    let mut fixture = behind_fixture(3);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    let run = fixture.state.run().unwrap();
    assert_eq!(run.scope(), Scope::Favourites(3));
    assert_eq!(run.targets().len(), 3);
}

#[test]
fn marked_repos_win_over_the_favourites() {
    let mut fixture = behind_fixture(3);
    fixture.send(Message::RowToggled(repo("echo")));
    fixture.send(Message::Bulk(BulkAction::Fetch));
    let run = fixture.state.run().unwrap();
    assert_eq!(run.scope(), Scope::Marked(1));
    assert_eq!(run.targets(), [repo("echo")]);
}

#[test]
fn no_more_than_the_concurrency_run_at_once_and_the_queue_moves_up() {
    let mut fixture = behind_fixture(6);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    let run = fixture.state.run().unwrap();
    assert_eq!((run.running(), run.waiting()), (2, 4));
    finish(&mut fixture, "alpha", Outcome::Updated { behind: 2 });
    let second = fixture.state.run().unwrap();
    assert_eq!((second.running(), second.waiting()), (2, 3));
    assert!(!second.is_finished());
}

#[test]
fn the_banner_counts_what_came_of_each_repo() {
    let mut fixture = behind_fixture(4);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    finish(&mut fixture, "alpha", Outcome::Updated { behind: 2 });
    finish(&mut fixture, "bravo", Outcome::AlreadyCurrent);
    finish(
        &mut fixture,
        "charlie",
        Outcome::Failed {
            last_line: "fatal".to_owned(),
        },
    );
    finish(&mut fixture, "delta", Outcome::Updated { behind: 1 });
    let run = fixture.state.run().unwrap();
    assert!(run.is_finished());
    let summary = run.summary();
    assert_eq!(summary.total(), 4);
    assert_eq!(summary.count(RunOutcome::Updated), 2);
    assert_eq!(summary.count(RunOutcome::Failed), 1);
}

#[test]
fn a_second_action_is_ignored_while_the_first_runs() {
    let mut fixture = behind_fixture(3);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    fixture.send(Message::Bulk(BulkAction::Both));
    assert_eq!(fixture.state.run().unwrap().action(), BulkAction::Fetch);
}

#[test]
fn retry_runs_only_the_repos_that_failed() {
    let mut fixture = behind_fixture(3);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    finish(&mut fixture, "alpha", Outcome::Updated { behind: 2 });
    finish(
        &mut fixture,
        "bravo",
        Outcome::Failed {
            last_line: "fatal".to_owned(),
        },
    );
    finish(&mut fixture, "charlie", Outcome::AlreadyCurrent);
    fixture.send(Message::RetryFailed);
    let run = fixture.state.run().unwrap();
    assert_eq!(run.running(), 1);
    assert!(run
        .result(&repo("bravo"))
        .is_some_and(|result| matches!(result, Outcome::Running)));
    assert_eq!(run.targets().len(), 3);
}

#[test]
fn escape_closes_a_finished_run_but_not_a_running_one() {
    let mut fixture = behind_fixture(1);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    fixture.send(Message::Escape);
    assert!(fixture.state.run().is_some());
    finish(&mut fixture, "alpha", Outcome::AlreadyCurrent);
    fixture.send(Message::Escape);
    assert!(fixture.state.run().is_none());
}

#[test]
fn only_problems_keeps_the_repos_that_went_wrong() {
    let mut fixture = behind_fixture(3);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    finish(&mut fixture, "alpha", Outcome::Updated { behind: 2 });
    finish(
        &mut fixture,
        "bravo",
        Outcome::Failed {
            last_line: "fatal".to_owned(),
        },
    );
    finish(&mut fixture, "charlie", Outcome::AlreadyCurrent);
    fixture.send(Message::OnlyProblemsToggled);
    let shown: Vec<_> = fixture.state.visible();
    assert_eq!(shown, [repo("bravo")]);
}

#[test]
fn a_run_writes_one_log_line_per_finished_repo() {
    let mut fixture = behind_fixture(2);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    finish(
        &mut fixture,
        "alpha",
        Outcome::Failed {
            last_line: "fatal: no route".to_owned(),
        },
    );
    let log = fixture.state.run().unwrap().log();
    let last = log.lines(None).last().unwrap();
    assert_eq!(last.repo, "alpha");
    assert_eq!(last.result, "fatal: no route");
}

#[test]
fn nothing_runs_in_the_sandbox() {
    let mut fixture = behind_fixture(3);
    fixture.ctx.actions_available = false;
    fixture.send(Message::Bulk(BulkAction::Fetch));
    assert!(fixture.state.run().is_none());
}

#[test]
fn a_repo_whose_folder_is_gone_fails_at_once_instead_of_hanging() {
    let mut fixture = fixture(&["alpha"], 1);
    fixture.reading("alpha", ncrs_repos::Reading::Gone);
    fixture.send(Message::Bulk(BulkAction::Fetch));
    let run = fixture.state.run().unwrap();
    assert!(run.is_finished());
    assert!(run.result(&repo("alpha")).is_some_and(Outcome::is_failure));
}
