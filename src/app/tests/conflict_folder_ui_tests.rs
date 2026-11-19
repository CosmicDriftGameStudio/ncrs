//! Snapshots of the folder conflict dialog and of the question on quitting
//! with a job running.

use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;
use std::path::PathBuf;

use crate::app::*;

fn matches_snapshot(app: &App, name: &str) -> bool {
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .expect("the view renders")
    .matches_reference_image(format!("tests/snapshots/{name}.png"))
    .expect("snapshot comparison")
}

#[test]
fn the_folder_conflict_matches_its_snapshots() {
    for (language, name) in [
        (Language::English, "conflict_folder"),
        (Language::German, "conflict_folder_de"),
    ] {
        let mut app = App::with_fixed_panels().in_language(language);
        app.open_conflict_for_test(PathBuf::from("/right"), "photos");
        app.ops.conflict_folder = true;
        app.dialog_focus = ncrs_ops::conflict::KEEP_FOCUS;
        assert!(
            matches_snapshot(&app, name),
            "the dialog does not match {name}.png"
        );
    }
}

#[test]
fn the_quit_question_matches_its_snapshots() {
    for (language, name) in [
        (Language::English, "quit_busy"),
        (Language::German, "quit_busy_de"),
    ] {
        let mut app = App::with_fixed_panels().in_language(language);
        app.ops.quit_dialog = true;
        app.dialog_focus = ncrs_ops::quit_dialog::CANCEL_FOCUS;
        assert!(
            matches_snapshot(&app, name),
            "the question does not match {name}.png"
        );
    }
}
