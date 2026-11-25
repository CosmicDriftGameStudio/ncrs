//! Flow 3: the activity page lists the background filing, newest first.

use std::time::Duration;

use super::archive::Arrivals;
use super::drive;
use super::probe::wait_until;
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;
use crate::messages::{FilingStatusMessage, Message};

pub fn run<P: Live>(session: &mut Session<P>, arrivals: &Arrivals) {
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
    for moved in &arrivals.by_rule {
        assert!(
            batches.iter().any(|batch| batch.moves.contains(moved)),
            "{moved:?} is not listed among {} runs",
            batches.len()
        );
    }
    println!(
        "activity lists {} runs, newest with {} moves",
        batches.len(),
        batches[0].moves.len()
    );
    drive::shot(session, "activity");
}
