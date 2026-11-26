//! The clean-up pane: open, read, tick, confirm, results, and what is stale.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::{fixture, repo, Fixture};
use ncrs_git::{
    ActionError, CleanupBranch, CleanupList, CleanupReason, CleanupReport, CleanupRow,
    CleanupTarget, KeepReason, Kept,
};
use ncrs_repos::{CleanupMessage, Line, Message, Offer, Phase, Pick, RepoState, Scan};

const NAMES: [&str; 2] = ["alpha", "bravo"];

fn branch(name: &str) -> CleanupBranch {
    CleanupBranch {
        name: name.to_owned(),
        sha: "abc".to_owned(),
        reason: CleanupReason::InDefault,
        upstream: Some(format!("origin/{name}")),
        tip_time: 0,
        remote_tip: Some("def".to_owned()),
    }
}

fn scan(names: &[&str]) -> Scan {
    Scan {
        list: CleanupList {
            branches: names.iter().map(|name| branch(name)).collect(),
            worktrees: Vec::new(),
            kept: vec![Kept {
                name: "wip".to_owned(),
                is_worktree: false,
                why: KeepReason::NotMerged,
            }],
        },
        default_branch: Some("main".to_owned()),
        fetch_error: None,
    }
}

fn pane(fixture: &Fixture) -> &ncrs_repos::Cleanup {
    fixture.state.cleanup.as_ref().unwrap()
}

fn scanned(fixture: &mut Fixture, name: &str, result: Result<Scan, String>) {
    let generation = fixture.state.cleanup_generation;
    fixture.send(Message::Cleanup(CleanupMessage::Scanned(
        generation,
        repo(name),
        Box::new(result),
    )));
}

/// Opens the pane and answers both reads, so it is `Ready`.
fn ready() -> Fixture {
    let mut fixture = fixture(&NAMES, 2);
    fixture.send(Message::Cleanup(CleanupMessage::Open));
    scanned(&mut fixture, "alpha", Ok(scan(&["one", "two"])));
    scanned(&mut fixture, "bravo", Ok(scan(&["three"])));
    fixture
}

fn done_report(names: &[&str]) -> CleanupReport {
    CleanupReport {
        rows: names
            .iter()
            .map(|name| CleanupRow {
                target: CleanupTarget::Branch((*name).to_owned()),
                result: Ok(()),
            })
            .collect(),
        undo: None,
    }
}

#[test]
fn open_reads_the_repositories_then_lists_what_they_offer() {
    let mut fixture = fixture(&NAMES, 2);
    fixture.send(Message::Cleanup(CleanupMessage::Open));
    assert_eq!(pane(&fixture).phase, Phase::Reading);
    scanned(&mut fixture, "alpha", Ok(scan(&["one"])));
    assert_eq!(pane(&fixture).phase, Phase::Reading);
    scanned(&mut fixture, "bravo", Ok(scan(&["two"])));
    assert_eq!(pane(&fixture).phase, Phase::Ready);
    assert_eq!(pane(&fixture).lines().len(), 4);
}

#[test]
fn space_ticks_a_row_and_a_group() {
    let mut fixture = ready();
    fixture.send(Message::Down);
    fixture.send(Message::Mark);
    let first = Pick {
        repo: repo("alpha"),
        offer: Offer::Branch("one".to_owned()),
    };
    assert!(!pane(&fixture).is_ticked(&first));
    assert_eq!(pane(&fixture).ticked().0, 2);

    fixture.send(Message::Up);
    fixture.send(Message::Mark);
    assert!(pane(&fixture).is_ticked(&first));
    fixture.send(Message::Mark);
    assert_eq!(
        pane(&fixture).cursor_line(),
        Some(Line::Group(repo("alpha")))
    );
    assert_eq!(pane(&fixture).ticked().0, 1);
}

#[test]
fn enter_confirms_the_plans_and_escape_closes_the_pane() {
    let mut fixture = ready();
    fixture.send(Message::Enter);
    assert_eq!(pane(&fixture).phase, Phase::Deleting);
    assert_eq!(pane(&fixture).plans.len(), 2);
    fixture.send(Message::Escape);
    assert!(fixture.state.cleanup.is_some(), "no close while deleting");

    let mut unconfirmed = ready();
    unconfirmed.send(Message::Escape);
    assert!(unconfirmed.state.cleanup.is_none());
}

#[test]
fn a_failed_row_stays_visible_next_to_the_deleted_ones() {
    let mut fixture = ready();
    fixture.send(Message::Enter);
    let generation = fixture.state.cleanup_generation;
    let mut report = done_report(&["one"]);
    report.rows.push(CleanupRow {
        target: CleanupTarget::Branch("two".to_owned()),
        result: Err(ActionError::Other {
            git_output: "two changed since the scan".to_owned(),
        }),
    });
    fixture.send(Message::Cleanup(CleanupMessage::Deleted(
        generation,
        repo("alpha"),
        Box::new(Ok(report)),
    )));
    fixture.send(Message::Cleanup(CleanupMessage::Deleted(
        generation,
        repo("bravo"),
        Box::new(Ok(done_report(&["three"]))),
    )));
    let pane = pane(&fixture);
    assert_eq!(pane.phase, Phase::Done);
    let items = pane.items(&repo("alpha"));
    assert_eq!(pane.result_of(&items[0]), Some(Ok(())));
    assert!(matches!(pane.result_of(&items[1]), Some(Err(_))));
    assert_eq!(pane.deleted_count(), 2);
    assert_eq!(pane.failed_count(), 1);
}

#[test]
fn a_locked_sandbox_opens_nothing() {
    let mut fixture = fixture(&NAMES, 2);
    fixture.ctx.actions_available = false;
    fixture.send(Message::Cleanup(CleanupMessage::Open));
    assert!(fixture.state.cleanup.is_none());
}

#[test]
fn a_failed_read_shows_as_failed_and_does_not_block_the_rest() {
    let mut fixture = fixture(&NAMES, 2);
    fixture.send(Message::Cleanup(CleanupMessage::Open));
    scanned(&mut fixture, "alpha", Err("no network".to_owned()));
    scanned(&mut fixture, "bravo", Ok(scan(&["two"])));
    assert_eq!(pane(&fixture).phase, Phase::Ready);
    assert!(matches!(
        pane(&fixture).repos.get(&repo("alpha")),
        Some(RepoState::Failed(_))
    ));
}

#[test]
fn a_read_that_arrives_after_enter_changes_nothing() {
    let mut fixture = fixture(&NAMES, 2);
    fixture.send(Message::Cleanup(CleanupMessage::Open));
    scanned(&mut fixture, "alpha", Ok(scan(&["one"])));
    scanned(&mut fixture, "bravo", Ok(scan(&["two"])));
    fixture.send(Message::Enter);
    let plans = pane(&fixture).plans.clone();
    scanned(&mut fixture, "alpha", Ok(scan(&["one", "surprise"])));
    assert_eq!(pane(&fixture).plans, plans);
    assert_eq!(pane(&fixture).items(&repo("alpha")).len(), 1);
}

#[test]
fn a_result_of_an_earlier_opening_is_dropped() {
    let mut fixture = ready();
    fixture.send(Message::Enter);
    let old = fixture.state.cleanup_generation;
    let _ = old;
    // Close is refused while deleting, so finish, close and open again.
    fixture.send(Message::Cleanup(CleanupMessage::Deleted(
        old,
        repo("alpha"),
        Box::new(Ok(done_report(&["one", "two"]))),
    )));
    fixture.send(Message::Cleanup(CleanupMessage::Deleted(
        old,
        repo("bravo"),
        Box::new(Ok(done_report(&["three"]))),
    )));
    fixture.send(Message::Escape);
    fixture.send(Message::Cleanup(CleanupMessage::Open));
    assert_eq!(fixture.state.cleanup_generation, old + 1);

    fixture.send(Message::Cleanup(CleanupMessage::Scanned(
        old,
        repo("alpha"),
        Box::new(Ok(scan(&["stale"]))),
    )));
    fixture.send(Message::Cleanup(CleanupMessage::Deleted(
        old,
        repo("alpha"),
        Box::new(Ok(done_report(&["stale"]))),
    )));
    fixture.send(Message::Cleanup(CleanupMessage::Undone(
        old,
        vec![(repo("alpha"), Ok(()))],
    )));
    let pane = pane(&fixture);
    assert_eq!(pane.phase, Phase::Reading);
    assert!(pane.reports.is_empty());
    assert!(pane.scan(&repo("alpha")).is_none());
}

#[test]
fn the_menu_opens_the_pane_for_its_row_without_marks_or_favourites() {
    let mut fixture = fixture(&["alpha", "bravo"], 0);
    fixture.send(Message::OpenMenuAt(repo("bravo")));
    fixture.send(Message::MenuPicked(ncrs_repos::MenuEntry::Cleanup));
    let opened = pane(&fixture);
    assert_eq!(opened.scope, ncrs_repos::Scope::Row);
    assert_eq!(opened.order, [repo("bravo")]);
}
