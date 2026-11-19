use super::*;
use std::path::PathBuf;

/// An app with a conflict waiting, so the dialog is part of the view.
fn app_with_conflict() -> App {
    let mut app = App::with_fixed_panels();
    app.open_conflict_for_test(PathBuf::from("/right"), "a.txt");
    app
}

/// The dialog is drawn over the panels. Without the overlay a copy running
/// in the background would show a list changing under a status bar and
/// nothing explaining why it stopped.
///
/// Compared by hash rather than image, and against the *same* reference
/// both times: two runs of the same test would otherwise write the
/// reference first and compare it against itself second, which is how this
/// passed once with the dialog switched off.
/// The overlay is over the panels, and the dialog names the file in the way.
///
/// One reference, committed, compared on every run after the first. That is
/// the only shape that actually catches a missing overlay: comparing two
/// references cannot, because a run with the dialog off rewrites the second
/// one instead of failing. This test went through three wrong versions
/// before it did.
#[cfg(target_os = "macos")]
#[test]
fn the_dialog_is_drawn_over_the_panels() {
    let matches = simulator(&app_with_conflict())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/conflict.png")
        .expect("snapshot comparison");

    assert!(
        matches,
        "the conflict dialog does not match tests/snapshots/conflict.png. \
         If the dialog is meant to look like this, nothing changed; if not, \
         the change is not drawn."
    );
}

/// And it shows the name that is in the way. A dialog that says "a file
/// already exists" without saying which is a question the user cannot
/// answer.
#[test]
fn the_dialog_names_the_file() {
    let app = app_with_conflict();
    let dialog = simulator(&app).snapshot(&iced::Theme::Dark).unwrap();
    let matches = dialog
        .matches_reference_image("tests/snapshots/conflict.png")
        .expect("reference for a conflict about a.txt");
    assert!(matches, "the conflict dialog does not match conflict.png");
}

#[test]
fn the_dialog_matches_its_german_snapshot() {
    let matches = simulator(&app_with_conflict().in_language(Language::German))
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/conflict_de.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the conflict dialog does not match conflict_de.png"
    );
}

/// One dialog, not two: the prompt underneath must not show through.
#[cfg(target_os = "macos")]
#[test]
fn the_prompt_is_gone_while_the_dialog_is_up() {
    let app = app_with_conflict();
    let same_as_prompt = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/prompt.png")
        .expect("compare with the prompt");

    assert!(
        !same_as_prompt,
        "the prompt is still drawn under the conflict dialog"
    );
}
