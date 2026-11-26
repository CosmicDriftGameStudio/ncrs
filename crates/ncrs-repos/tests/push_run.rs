//! A push over several repos: one after the other, since each repository's
//! pre-push hook can run a test suite and git may ask for a login.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use common::{fixture, read, repo, summary, Fixture};
use ncrs_git::askpass::{AskRequest, PromptKind};
use ncrs_repos::{BulkAction, Executed, Message, Outcome};

const NAMES: [&str; 4] = ["alpha", "bravo", "charlie", "delta"];

fn ahead_fixture() -> Fixture {
    let mut fixture = fixture(&NAMES, 4);
    for name in NAMES {
        fixture.reading(name, read(summary(0, 2, 0)));
    }
    fixture
}

fn pushed(fixture: &mut Fixture, name: &str) {
    fixture.send(Message::StepDone(
        repo(name),
        Box::new(Executed {
            outcome: Outcome::Pushed { ahead: 2 },
            ran: Vec::new(),
            after: None,
        }),
    ));
}

#[test]
fn pushes_start_one_after_the_other_whatever_the_concurrency() {
    let mut fixture = ahead_fixture();
    assert_eq!(fixture.ctx.settings.concurrency, 2);
    fixture.send(Message::Bulk(BulkAction::Push));
    let run = fixture.state.run().unwrap();
    assert_eq!((run.running(), run.waiting()), (1, 3));

    pushed(&mut fixture, "alpha");
    let second = fixture.state.run().unwrap();
    assert_eq!((second.running(), second.waiting()), (1, 2));
    assert_eq!(second.result(&repo("bravo")), Some(&Outcome::Running));
    assert_eq!(second.result(&repo("charlie")), None);

    for name in ["bravo", "charlie", "delta"] {
        pushed(&mut fixture, name);
    }
    assert!(fixture.state.run().unwrap().is_finished());
}

#[test]
fn a_failed_push_is_retried_one_at_a_time_too() {
    let mut fixture = ahead_fixture();
    fixture.send(Message::Bulk(BulkAction::Push));
    for name in NAMES {
        fixture.send(Message::StepDone(
            repo(name),
            Box::new(Executed {
                outcome: Outcome::Failed {
                    last_line: "! [rejected]".to_owned(),
                },
                ran: Vec::new(),
                after: None,
            }),
        ));
    }
    fixture.send(Message::RetryFailed);
    let run = fixture.state.run().unwrap();
    assert_eq!((run.running(), run.waiting()), (1, 3));
}

#[test]
fn nothing_is_pushed_where_the_write_actions_are_off() {
    let mut fixture = ahead_fixture();
    fixture.ctx.actions_available = false;
    fixture.send(Message::Bulk(BulkAction::Push));
    assert!(fixture.state.run().is_none());
}

#[test]
fn a_login_question_of_the_tab_shows_a_dialog_and_holds_the_list_keys() {
    let mut fixture = ahead_fixture();
    fixture.send(Message::Bulk(BulkAction::Push));
    fixture.send(Message::CredentialAsked(AskRequest {
        id: 1,
        job: fixture.state.job,
        prompt: "Username for 'https://example.invalid': ".to_owned(),
        kind: PromptKind::Text,
    }));
    assert!(fixture.state.credential.is_some());
    let cursor = fixture.state.cursor();
    fixture.send(Message::Down);
    assert_eq!(fixture.state.cursor(), cursor);
    fixture.send(Message::Escape);
    assert!(fixture.state.credential.is_none());
}
