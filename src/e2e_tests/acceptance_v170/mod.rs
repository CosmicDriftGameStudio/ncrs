//! Acceptance of the Hermes work of 1.7.0, headless on a generated archive: the
//! real app with its real pipeline, filing and index, a model that never
//! answers, and checks of what lies on disk, in the index and in the config.
//!
//! `cargo test -p ncrs --bin ncrs acceptance_v170 -- --test-threads=1`, with
//! `NCRS_ACCEPTANCE_SHOTS=1` also the pictures in `target/acceptance/v170/`
//! and with `NCRS_ACCEPTANCE_FULL=1` on the archive of 2,000 files.

mod archive;
mod drive;
mod flow_activity;
mod flow_answers_duplicates;
mod flow_answers_values;
mod flow_continuous;
mod flow_hand;
mod flow_status;
mod probe;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_ai::LocalModelStatus;
use ncrs_ai::ScriptedChatModel;

use probe::{describe, files_in, wait_quiet_and, World};

/// The window of a laptop; the acceptance pictures of the pages use the
/// larger one of the other releases.
pub const WINDOW: iced::Size = iced::Size::new(1280.0, 800.0);

pub const LIMIT: Duration = Duration::from_secs(600);

#[test]
fn acceptance_v170() {
    let started = Instant::now();
    let step = |label: &str| println!("[{:>4} s] {label}", started.elapsed().as_secs());
    let world = World::new("v170");
    let limit = if archive::full_scale() { LIMIT } else { Duration::from_secs(90) };
    let scripted_model = || Arc::new(ScriptedChatModel::new(LocalModelStatus::NotDownloaded, []));

    // Phase A: the archive is read to the end first, so that every inbox file
    // meets an indexed archive. Then the pipeline reads the inbox; what it
    // cannot place gets a review, and an exact copy of an archive file is only
    // recorded as a copy.
    let mut archive_session = world.boot(WINDOW);
    archive_session.app.set_chat_model_for_test(scripted_model());
    let archive_read = wait_quiet_and(&mut archive_session, limit, |_| probe::archive_indexed(&world));
    assert!(archive_read, "the archive was not read");
    drop(archive_session);
    world.add_inbox_to_config();
    let mut session = world.boot(WINDOW);
    session.app.set_chat_model_for_test(scripted_model());
    let reviewed = |_: &mut crate::e2e_tests::Session<_>| {
        files_in(&world.inbox_dir) == world.index().open_reviews().unwrap().len() + world.inbox.duplicates.len()
            && probe::copies_recorded(&world) == world.inbox.duplicates.len()
    };
    let settled = wait_quiet_and(&mut session, limit, reviewed);
    if !settled {
        describe(&world);
    }
    assert!(settled, "the pipeline did not settle");
    step("phase A settled");
    describe(&world);
    probe::mark_unsure_senders(&world);
    probe::check_phase_a(&world);

    let arrivals = flow_continuous::run(&world, &mut session);
    flow_continuous::check(&world, &arrivals);
    step("flow 1 continuous filing");

    flow_status::run(&world, &mut session);
    step("flow 2 status click to reviews");

    flow_answers_duplicates::run(&world, &mut session);
    step("flow 3a duplicates and clashes");
    flow_answers_values::run(&world, &mut session);
    step("flow 3b dates, amounts, rules, senders, folders");

    flow_hand::run(&world, &mut session);
    step("flow 4 hand filing");

    flow_activity::run(&world, &mut session);
    step("flow 5 activity");
}
