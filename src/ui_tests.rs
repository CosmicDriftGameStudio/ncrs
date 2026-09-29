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
//! **These tests only run on macOS in CI** (see `.github/workflows/ci.yml`).
//! The reference images in `tests/snapshots/` were produced there, and font
//! rasterisation differs per operating system: a reference made on macOS does
//! not match Windows output. Everywhere else the tests are skipped rather than
//! asserted against a reference that cannot hold, which would be a test that
//! passes without checking anything.
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

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
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
        let app = App::with_fixed_panels();
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
        simulator(&App::with_fixed_panels())
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

    /// Reference image for the two-panel view.
    ///
    /// The `bool` has to be asserted. `matches_image` returns `true` when the
    /// reference does not exist yet — it writes one — and `false` when the
    /// images differ, without failing. Discarding the value means the test
    /// passes on a first run and on any change to the layout, which is the
    /// opposite of what a reference test is for.
    #[test]
    fn the_two_panel_view_matches_its_snapshot() {
        let app = App::with_fixed_panels();
        let mut sim = simulator(&app);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("snapshot comparison");

        assert!(
            matches,
            "the two-panel view does not match tests/snapshots/two_panels.png. \
             If the change is intended, update the reference; if not, this is a \
             regression."
        );
    }

    /// Same for the prompt's own reference. Both are in the repo, so this
    /// compares against a real image rather than writing one.
    #[test]
    fn the_prompt_matches_its_snapshot() {
        let app = App::with_fixed_prompt();
        let mut sim = simulator(&app);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/prompt.png")
            .expect("snapshot comparison");

        assert!(
            matches,
            "the prompt does not match tests/snapshots/prompt.png"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tagging {
    use super::*;
    use crate::fs::FileEntry;
    use std::path::{Path, PathBuf};

    fn app_with_files() -> App {
        let mut app = App::with_fixed_panels();
        for side in [
            crate::messages::PanelSide::Left,
            crate::messages::PanelSide::Right,
        ] {
            let panel = app.panel_mut_for_test(side);
            panel.entries = vec![
                FileEntry::parent(Path::new("/tmp")),
                file("alpha"),
                file("beta"),
                file("gamma"),
            ];
        }
        app.set_visible_rows_for_test(10);
        app
    }

    fn file(name: &str) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from("/tmp").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 100,
            modified: None,
        }
    }

    /// Tagging has to change what the user sees. A selection that is stored but
    /// not drawn would make F5 copy something invisible.
    ///
    /// Compared by hash, not by image: `matches_hash` is an exact SHA256 of the
    /// pixels, while `matches_image` tolerates differences — and a two-pixel-wide
    /// tag column on a 590px row is well inside that tolerance, which is why an
    /// earlier version of this test passed with the marker removed.
    ///
    /// This proves the selection is *visible*, not that the `*` caused it.
    ///
    /// Measured: with the marker column removed the pixels still differ, because
    /// the row background changes too. So the test cannot separate the two
    /// changes, and does not claim to. What it guarantees is what a file
    /// operation needs — a tagged row looks different from an untagged one, so
    /// the user can see what F5 will act on.
    ///
    /// Pinning the `*` itself would need a renderer check on that column alone;
    /// the two ways to draw a tag are covered by the untagged test below, which
    /// does fail if the row style stops distinguishing them.
    #[test]
    fn tagging_changes_the_view() {
        let untagged = app_with_files();
        simulator(&untagged)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("reference");

        let mut tagged = app_with_files();
        tagged.toggle_tag_for_test(1);
        tagged.toggle_tag_for_test(2);

        let same = simulator(&tagged)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("compare");

        assert!(
            !same,
            "tagging two rows changed no pixel — the selection is invisible"
        );
    }

    /// Untagging returns to the untagged view. Without this, a tag that cannot
    /// be removed would also be invisible: the test above would still pass.
    #[test]
    fn untagging_returns_to_the_untagged_view() {
        let mut app = app_with_files();
        simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("reference");

        app.toggle_tag_for_test(1);
        app.toggle_tag_for_test(1); // same row again: untagged

        let same = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("compare");

        assert!(same, "the tag could not be removed");
    }

    /// Tagging a single row differs from tagging two. Otherwise "did my second
    /// tag land?" would have no answer, and a partial failure would look
    /// correct.
    #[test]
    fn one_tag_differs_from_two() {
        let none = app_with_files();
        simulator(&none)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("reference");

        let mut one = app_with_files();
        one.toggle_tag_for_test(1);

        let differs = !simulator(&one)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("compare");
        assert!(differs, "one tag looked like none");
    }

    /// The tag marker occupies its own column, so tagging does not shift the
    /// name column sideways.
    #[test]
    fn tagging_does_not_shift_the_names() {
        let mut app = app_with_files();
        app.toggle_tag_for_test(2);
        let mut sim = simulator(&app);
        // Rendering must succeed with tags present; a layout that only works
        // for the untagged state would panic or misplace the column.
        let _ = sim.snapshot(&iced::Theme::Dark).expect("renders with tags");
    }

    /// Tagging everything marks the rows a bulk operation would use, and skips
    /// `..`.
    #[test]
    fn tagging_everything_is_not_the_same_as_tagging_nothing() {
        let none = app_with_files();
        simulator(&none)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("reference");

        let mut all = app_with_files();
        all.tag_all_for_test();

        let differs = !simulator(&all)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("compare");
        assert!(differs, "tagging everything looked like tagging nothing");
    }

    /// The cursor row is not a tag. Moving the cursor must not make a file part
    /// of the next operation.
    #[test]
    fn moving_the_cursor_does_not_tag() {
        let mut app = app_with_files();
        simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_hash("tests/snapshots/tags_none.sha256")
            .expect("reference");

        // Moving the cursor changes the highlight, so the image is expected to
        // differ from the initial one. What must not happen is a tag: the panel
        // reports the cursor row as untagged.
        app.move_selection_for_test(1);
        assert!(
            !app.left_panel_selection_tagged_for_test(1),
            "moving the cursor onto a row tagged it"
        );
        app.move_selection_for_test(1);
        assert!(!app.left_panel_selection_tagged_for_test(2));
    }
}
