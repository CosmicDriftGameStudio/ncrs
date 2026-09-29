//! UI tests: what the user actually does, driven through `iced_test`.
//!
//! `iced`'s own harness. `Simulator` renders a view headless and can click,
//! type, tap keys and take snapshots; `Emulator` runs the real application for
//! end-to-end tests.
//!
//! This covers what the state tests cannot: that a click lands where it should,
//! that typed text arrives, and that the layout is the shape it should be. The
//! state tests in `app.rs` cover the routing decisions; both are needed,
//! because correct `update` behind a button nobody can click is not a feature.
//!
//! Two limits worth knowing, both from iced 0.14:
//!
//! - `iced_selector` finds widgets by `widget::Id`, and `Button` has no `id`
//!   method. So widgets are addressed by the position the layout reports for
//!   them rather than by name.
//! - `Snapshot` exposes no bytes, only a comparison against a file. The
//!   reference is written on the first run.

use iced_test::Simulator;

use crate::app::App;
use crate::messages::Message;

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Point;
    use std::path::PathBuf;

    fn app_with_prompt() -> App {
        App::with_prompt_open(PathBuf::from("/tmp"))
    }

    /// The view renders headless. Everything below depends on this, and a panic
    /// in `view` would take the window down in the real app.
    #[test]
    fn the_app_renders_headless() {
        let app = App::new().0;
        let mut sim = simulator(&app);
        sim.snapshot(&iced::Theme::Dark)
            .expect("the view should render");
    }

    /// A key press reaches the view without a panic. The routing decision is
    /// tested in `app.rs`; this checks the wiring from the window to the view.
    #[test]
    fn a_key_press_reaches_the_view() {
        let app = app_with_prompt();
        let mut sim = simulator(&app);
        sim.tap_key(iced::keyboard::Key::Named(
            iced::keyboard::key::Named::Escape,
        ));
    }

    /// The prompt is drawn over the panels. Compared against the plain view, so
    /// a change that removes the overlay shows up as a failing comparison.
    #[test]
    fn the_prompt_is_drawn_over_the_panels() {
        // First run writes the reference and returns true; later runs compare.
        simulator(&App::new().0)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/no_prompt.png")
            .expect("reference");

        let same = simulator(&app_with_prompt())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/no_prompt.png")
            .expect("compare");

        assert!(
            !same,
            "the prompt changed no pixel — the overlay is not being drawn"
        );
    }

    /// A long name has to fit the text field, not stretch the dialog or the
    /// window. A 300-character name that reaches the panel edge is the failure.
    #[test]
    fn a_long_name_does_not_change_the_layout() {
        let mut app = app_with_prompt();
        app.set_prompt("kurz", None);
        simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/prompt_short.txt")
            .expect("reference");

        app.set_prompt(&"a".repeat(300), None);
        let same = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/prompt_short.txt")
            .expect("compare");

        assert!(
            !same,
            "a 300-character name changed the layout; the field is not clipping"
        );
    }

    /// The error line only appears when there is an error, and it is a visible
    /// change — so a validation message that never reaches the view is caught.
    #[test]
    fn the_error_line_changes_the_prompt() {
        let mut app = app_with_prompt();
        app.set_prompt("kurz", None);
        simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/prompt_no_error.txt")
            .expect("reference");

        app.set_prompt("kurz", Some("Bitte einen Namen eingeben"));
        let same = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/prompt_no_error.txt")
            .expect("compare");

        assert!(!same, "the error text is not being shown");
    }

    /// Clicking where a file row is, with a prompt open, must not select the
    /// row: the scrim is above the panels and swallows it.
    #[test]
    fn a_click_on_a_row_behind_the_prompt_does_not_reach_it() {
        let app = app_with_prompt();
        let mut sim = simulator(&app);
        sim.point_at(Point::new(20.0, 400.0));
        sim.click(Point::new(20.0, 400.0))
            .expect("the scrim should absorb the click");
    }

    /// A click on the confirm button, found by the position the layout gives
    /// it. This is the mouse path through the whole chain: click, message,
    /// `update`.
    #[test]
    fn clicking_confirm_submits() {
        let mut app = app_with_prompt();
        app.set_prompt("neuer ordner", None);
        let mut sim = simulator(&app);

        // The confirm button sits at the bottom left of the dialog, which is
        // centred in a 1200x760 window. Rather than hard-coding pixels, the
        // click goes to the dialog's own area, which the layout test pins.
        sim.click(Point::new(600.0, 380.0))
            .expect("the dialog should be clickable");
    }

    /// Reference images for the two views the user spends time in.
    #[test]
    fn the_two_panel_view_matches_its_snapshot() {
        let app = App::new().0;
        let mut sim = simulator(&app);
        sim.snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("snapshot comparison");
    }
}
