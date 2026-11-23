//! Flow 4: a reviewed document is filed by hand, by moving it in the panels.
//! Its review has to go.

use std::time::Duration;

use iced::keyboard::key::Named;
use iced::keyboard::Key;

use super::drive;
use super::probe::{wait_until, World};
use crate::e2e_tests::acceptance_v160::{go_to, Live};
use crate::e2e_tests::Session;
use crate::messages::ShownTab;

pub fn run<P: Live>(world: &World, session: &mut Session<P>) {
    let file = world.inbox.no_rule[3].clone();
    let name = file.file_name().unwrap().to_owned();
    let document = {
        let index = world.index();
        let document = index
            .document_by_path(&file)
            .unwrap()
            .expect("a known document");
        assert!(index.open_review_of(document.id).unwrap().is_some());
        document.id
    };
    let open_before = drive::open_reviews(world);

    session.press(Key::Named(Named::Escape));
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Panels);
    go_to(session, &name.to_string_lossy());
    session.press(Key::Named(Named::F6));
    session.press(Key::Named(Named::Enter));

    let moved = world.archive_dir.join(&name);
    assert!(
        wait_until(session, Duration::from_secs(30), |_| moved.is_file()),
        "the file was not moved to the archive"
    );
    assert!(!file.exists());
    assert!(
        wait_until(session, Duration::from_secs(60), |_| {
            world.index().open_review_of(document).unwrap().is_none()
        }),
        "the review of a document filed by hand stays open"
    );
    assert_eq!(drive::open_reviews(world), open_before - 1);
    let stored = world.index().document(document).unwrap().unwrap();
    assert_eq!(stored.path, moved, "the index still knows the old place");
    drive::config_intact(world);

    // The page does not list it any more.
    drive::show_reviews(session);
    drive::reload(session);
    assert!(drive::find(session, &file).is_none() && drive::find(session, &moved).is_none());
}
