use super::*;

fn app_with_delete(count: usize, permanent: bool) -> App {
    let mut app = App::with_fixed_panels();
    app.open_delete_dialog_for_test(count, permanent);
    app
}

/// The trash dialog names the entry. The paths are fixed fixtures, never
/// the real filesystem, so the image does not depend on the machine.
#[test]
fn the_trash_dialog_names_the_entry() {
    let matches = simulator(&app_with_delete(1, false))
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/delete_trash.png")
        .expect("snapshot comparison");
    assert!(matches, "the trash dialog does not match delete_trash.png");
}

/// The permanent dialog counts several entries and carries the warning.
#[test]
fn the_permanent_dialog_counts_the_entries_and_warns() {
    let matches = simulator(&app_with_delete(7, true))
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/delete_permanent.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the permanent dialog does not match delete_permanent.png"
    );
}

#[test]
fn the_german_dialogs_match_their_snapshots() {
    for (count, permanent, name) in [
        (1, false, "delete_trash_de"),
        (7, true, "delete_permanent_de"),
    ] {
        let app = app_with_delete(count, permanent).in_language(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the dialog does not match {name}.png");
    }
}

/// An overlay that is not drawn would leave the plain panels. Compared with
/// the two-panel reference, which is committed, rather than with a dialog
/// reference that this very test could be the one to write.
#[cfg(target_os = "macos")]
#[test]
fn the_dialog_is_drawn_over_the_panels() {
    let same_as_panels = simulator(&app_with_delete(1, false))
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/two_panels.png")
        .expect("compare with the panels");
    assert!(
        !same_as_panels,
        "the delete dialog is not drawn over the panels"
    );
}
