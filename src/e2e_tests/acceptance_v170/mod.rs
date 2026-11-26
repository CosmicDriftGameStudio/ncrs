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
mod flow_continuous;
mod flow_plan;
mod flow_status;
mod probe;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ncrs_ai::LocalModelStatus;
use ncrs_ai::ScriptedChatModel;

use probe::{describe, wait_quiet_and, World};

/// The window of a laptop; the acceptance pictures of the pages use the
/// larger one of the other releases.
pub const WINDOW: iced::Size = iced::Size::new(1280.0, 800.0);

pub const LIMIT: Duration = Duration::from_secs(600);

#[test]
fn acceptance_v170() {
    let started = Instant::now();
    let step = |label: &str| println!("[{:>4} s] {label}", started.elapsed().as_secs());
    let world = World::new("v170");
    let limit = if archive::full_scale() {
        LIMIT
    } else {
        Duration::from_secs(90)
    };
    let scripted_model = || Arc::new(ScriptedChatModel::new(LocalModelStatus::NotDownloaded, []));

    // Phase A: the archive is read to the end first, so that every inbox file
    // meets an indexed archive. Then the pipeline reads the inbox and the
    // watched source takes what lies there as its backlog: nothing is moved.
    let mut archive_session = world.boot(WINDOW);
    archive_session
        .app
        .set_chat_model_for_test(scripted_model());
    let archive_read = wait_quiet_and(&mut archive_session, limit, |_| {
        probe::archive_indexed(&world)
    });
    assert!(archive_read, "the archive was not read");
    drop(archive_session);
    world.add_source_to_config();
    let mut session = world.boot(WINDOW);
    session.app.set_chat_model_for_test(scripted_model());
    let settled = wait_quiet_and(&mut session, limit, |_| {
        probe::inbox_read(&world) && probe::backlog_recorded(&world)
    });
    if !settled {
        describe(&world);
    }
    assert!(settled, "the pipeline did not settle");
    step("phase A settled");
    describe(&world);
    probe::check_phase_a(&world);

    let arrivals = flow_continuous::run(&world, &mut session);
    flow_continuous::check(&world, &arrivals);
    step("flow 1 watched source");

    flow_status::run(&world, &mut session);
    step("flow 2 status click");

    flow_activity::run(&mut session, &arrivals);
    step("flow 3 activity");

    flow_continuous::undo_and_check(&world, &mut session, &arrivals);
    step("flow 4 undo of the background filing");

    flow_plan::run(&world, &mut session, &arrivals);
    step("flow 5 plan of the source");
}
