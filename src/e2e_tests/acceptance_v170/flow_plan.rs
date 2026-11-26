//! Flow 5: the plan of the source says for every file where it goes and why,
//! and moves nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ncrs_filing::sorting::{RowState, Taken};
use ncrs_index::Field;

use super::archive::Arrivals;
use super::drive;
use super::probe::{files_in, wait_until, World};
use crate::e2e_tests::acceptance_v160::Live;
use crate::e2e_tests::Session;
use crate::messages::Message;

pub fn run<P: Live>(world: &World, session: &mut Session<P>, arrivals: &Arrivals) {
    let files_before = files_in(&world.inbox_dir);
    let source = session
        .app
        .sources_for_test()
        .iter()
        .map(|setting| setting.source.clone())
        .find(|source| source.path == world.inbox_dir)
        .expect("the inbox is a source");
    let task = session
        .runtime
        .enter(|| session.app.open_filing_plan(source, true));
    session.drive(task);
    session.settle();
    let planned = wait_until(session, Duration::from_secs(120), |live| {
        live.send(Message::FilingView(ncrs_filing_view::Message::Tick));
        live.app
            .filing_session_for_test()
            .is_some_and(|plan| plan.planned && !plan.planning)
    });
    assert!(planned, "the plan was not made");
    drive::shot(session, "plan");

    let plan = session.app.filing_session_for_test().unwrap();
    assert_eq!(plan.error, None);
    let state_of = |file: &Path| -> &RowState {
        plan.rows
            .iter()
            .find(|row| world.inbox_dir.join(&row.relative) == file)
            .map_or_else(|| panic!("{file:?} is not in the plan"), |row| &row.state)
    };
    for (file, target) in world.inbox.sure.iter().chain(&arrivals.by_rule) {
        let state = state_of(file);
        assert!(
            matches!(state, RowState::Place(place) if world.archive_dir.join(&place.relative) == *target),
            "{file:?} should go to {target:?}: {state:?}"
        );
    }
    for (copy, original) in &world.inbox.duplicates {
        let state = state_of(copy);
        assert!(
            matches!(state, RowState::AlreadyThere { path, .. } if path == original),
            "{copy:?} lies at {original:?} already: {state:?}"
        );
    }
    for (file, holder) in &world.inbox.clashes {
        let state = state_of(file);
        assert!(
            matches!(state, RowState::NameTaken { place, by: Taken::OnDisk }
                if world.archive_dir.join(&place.relative) == *holder),
            "the place of {file:?} is taken: {state:?}"
        );
    }
    for file in &world.inbox.no_date {
        let state = state_of(file);
        assert!(
            matches!(
                state,
                RowState::MissingValue {
                    field: Field::Date,
                    ..
                }
            ),
            "{file:?} has no date: {state:?}"
        );
    }
    let without_rule: Vec<&PathBuf> = world
        .inbox
        .no_rule
        .iter()
        .chain(&world.inbox.known_sender)
        .chain(&world.inbox.unconfirmed)
        .chain(&arrivals.known_sender)
        .chain(&arrivals.strangers)
        .collect();
    for file in without_rule {
        let state = state_of(file);
        assert_eq!(*state, RowState::NoRule, "{file:?}");
    }
    assert_eq!(
        files_in(&world.inbox_dir),
        files_before,
        "the plan moved a file"
    );
    drive::config_intact(world);
}
