//! Flow 5: the activity page lists the runs and the answers, newest first.

use std::time::Duration;

use super::drive;
use super::probe::{wait_until, World};
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;
use crate::messages::{FilingStatusMessage, Message};

pub fn run<P: Live>(world: &World, session: &mut Session<P>) {
    session.send(Message::FilingStatus(FilingStatusMessage::Open));
    assert!(
        wait_until(session, Duration::from_secs(30), |live| {
            live.app
                .activity_page_for_test()
                .is_some_and(|page| !page.is_loading())
        }),
        "the activity page never loaded"
    );
    let page = session.app.activity_page_for_test().unwrap();
    assert!(page.problem().is_none());
    let batches = page.batches();
    assert!(!batches.is_empty());
    assert!(
        batches.windows(2).all(|pair| pair
            .first()
            .zip(pair.get(1))
            .is_some_and(|(newer, older)| newer.at >= older.at)),
        "the runs are not newest first"
    );
    // The last answer that moved a file: a folder picked for a document
    // without a rule.
    let last = &world.inbox.no_rule[0];
    let position = batches
        .iter()
        .position(|batch| batch.moves.iter().any(|(from, _)| from == last));
    assert!(
        position.is_some(),
        "the folder pick is not listed among {} runs",
        batches.len()
    );
    let sender_run = world.inbox.unsure_sender_marked.last().and_then(|file| {
        batches
            .iter()
            .position(|batch| batch.moves.iter().any(|(from, _)| from == file))
    });
    assert!(
        sender_run.is_some_and(|older| older >= position.unwrap()),
        "the answer to the senders is not listed after the folder pick: {sender_run:?} / {position:?}"
    );
    println!(
        "activity lists {} runs, newest with {} moves",
        batches.len(),
        batches[0].moves.len()
    );
    drive::shot(session, "activity");
}
