//! Flow 2: the status chip and the activity page behind it, in a laptop window.

use std::time::Duration;

use ncrs_hermes_ui::HermesPage;

use super::drive::{self, SHOT_FOLDER};
use super::probe::{wait_until, World};
use super::WINDOW;
use crate::e2e_tests::acceptance_v160::{shoot_sized, Live};
use crate::e2e_tests::Session;
use crate::messages::{ActivityMessage, FilingStatusMessage, Message};

pub fn run<P: Live>(world: &World, session: &mut Session<P>) {
    shoot_sized(&session.app, (SHOT_FOLDER, "status-chip"), WINDOW, |_| {});

    session.send(Message::FilingStatus(FilingStatusMessage::Open));
    assert_eq!(session.app.open_hermes_page(), Some(HermesPage::Activity));
    assert!(
        wait_until(session, Duration::from_secs(30), |live| {
            live.app
                .activity_page_for_test()
                .is_some_and(|page| !page.is_loading())
        }),
        "the activity page never loaded"
    );
    assert!(
        session
            .app
            .activity_page_for_test()
            .unwrap()
            .problem()
            .is_none(),
        "the activity page failed"
    );

    // No file waits for an answer: the link stays on the page.
    assert_eq!(drive::open_reviews(world), 0);
    session.send(Message::Activity(ActivityMessage::OpenWaiting));
    assert_eq!(session.app.open_hermes_page(), Some(HermesPage::Activity));
    drive::config_intact(world);

    // Whether the sidebar scrolled to the page: the harness runs the task of
    // the click, a widget operation, like the window does.
    let look = drive::look_at_sidebar(session);
    println!(
        "sidebar: scrolled {} px, the chosen row in view: {}",
        look.scrolled(),
        look.row_in_view().unwrap_or(false)
    );
    assert_eq!(
        look.row_in_view(),
        Some(true),
        "the chosen sidebar entry lies outside the visible part of the list"
    );
    sidebar_picture(session, look.scrolled());
}

/// The sidebar at the laptop size, scrolled as far as the app scrolled it.
fn sidebar_picture<P: Live>(session: &Session<P>, scrolled: f32) {
    shoot_sized(
        &session.app,
        (SHOT_FOLDER, "settings-sidebar"),
        WINDOW,
        |simulator| {
            if scrolled > 0.0 {
                simulator.point_at(iced::Point::new(60.0, 300.0));
                simulator.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                    delta: iced::mouse::ScrollDelta::Pixels {
                        x: 0.0,
                        y: -scrolled,
                    },
                })]);
            }
        },
    );
}
