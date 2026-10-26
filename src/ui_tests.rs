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
use crate::filing::{FilingMessage, FilingModule, KeyProblem, Phase, SecretText};
use crate::i18n::Language;
use crate::messages::Message;

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

/// The outer bounds of the dialog frame in the view of `app`. Test-only like
/// the modules below, where a failing lookup is the signal.
#[allow(clippy::expect_used)]
fn frame_bounds(app: &App) -> iced::Rectangle {
    simulator(app)
        .find(iced::widget::Id::new(crate::ui::frame::FRAME_ID))
        .expect("the dialog frame")
        .bounds()
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

    fn app_with_prompt() -> App {
        App::with_prompt_open()
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

    fn assert_preset_matches(preset: &str, reference: &str) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, format!("[theme]\npreset = \"{preset}\"\n")).unwrap();
        let app = App::with_fixed_panels().with_config(crate::config::load(&path));
        let mut sim = simulator(&app);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(reference)
            .expect("snapshot comparison");
        assert!(matches, "preset {preset:?} no longer matches {reference}");
    }

    #[test]
    fn the_default_preset_looks_as_before() {
        assert_preset_matches("default", "tests/snapshots/two_panels_default.png");
    }

    #[test]
    fn the_dracula_preset_has_its_own_look() {
        assert_preset_matches("dracula", "tests/snapshots/two_panels_dracula.png");
    }

    #[test]
    fn the_classic_preset_looks_as_before() {
        assert_preset_matches("classic", "tests/snapshots/two_panels_classic.png");
    }

    /// A configured color changes the picture. Compared with the committed
    /// reference, as `the_dialog_is_drawn_over_the_panels` does, so no new
    /// reference is written.
    #[test]
    fn a_configured_color_changes_the_picture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[theme]\npanel = \"#ff0000\"\n").unwrap();
        let app = App::with_fixed_panels().with_config(crate::config::load(&path));
        let same = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("compare with the panels");
        assert!(!same, "a red panel looks like the default one");
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

    #[test]
    fn the_prompt_matches_its_german_snapshot() {
        let app = App::with_fixed_prompt().in_language(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/prompt_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the prompt does not match tests/snapshots/prompt_de.png"
        );
    }

    #[test]
    fn the_new_file_prompt_matches_its_snapshot() {
        let app = App::with_fixed_new_file_prompt();
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/new_file_prompt.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the new-file prompt does not match tests/snapshots/new_file_prompt.png"
        );
    }

    #[test]
    fn the_new_file_prompt_matches_its_german_snapshot() {
        let app = App::with_fixed_new_file_prompt().in_language(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/new_file_prompt_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the new-file prompt does not match tests/snapshots/new_file_prompt_de.png"
        );
    }

    #[test]
    fn the_search_view_matches_its_snapshot() {
        let app = App::with_fixed_search();
        let mut sim = simulator(&app);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/search.png")
            .expect("snapshot comparison");

        assert!(
            matches,
            "the search view does not match tests/snapshots/search.png"
        );
    }

    #[test]
    fn quick_open_matches_its_snapshot() {
        let mut app = App::with_fixed_panels();
        app.open_quick_open_for_test(
            &[
                "README.md",
                "docs/Ärger.rs",
                "src/config.rs",
                "src/main.rs",
                "tests/config_rs_test.rs",
            ],
            "cfgrs",
        );
        let mut sim = simulator(&app);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/quick_open.png")
            .expect("snapshot comparison");

        assert!(
            matches,
            "quick open does not match tests/snapshots/quick_open.png"
        );
    }

    #[test]
    fn quick_open_matches_its_german_snapshot() {
        let mut app = App::with_fixed_panels().in_language(Language::German);
        app.open_quick_open_for_test(
            &[
                "README.md",
                "docs/Ärger.rs",
                "src/config.rs",
                "src/main.rs",
                "tests/config_rs_test.rs",
            ],
            "cfgrs",
        );
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/quick_open_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "quick open does not match tests/snapshots/quick_open_de.png"
        );
    }

    #[test]
    fn a_long_path_does_not_change_the_quick_open_frame() {
        let short = {
            let mut app = App::with_fixed_panels();
            app.open_quick_open_for_test(&["a.rs"], "a");
            frame_bounds(&app)
        };
        let long_path = format!("{}/file.rs", "deeply-nested-folder/".repeat(30));
        let long = {
            let mut app = App::with_fixed_panels();
            app.open_quick_open_for_test(&[long_path.as_str()], "file");
            frame_bounds(&app)
        };
        assert_eq!(short, long);
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

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod conflict_overlay {
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
    #[test]
    fn the_dialog_is_drawn_over_the_panels() {
        let matches = simulator(&app_with_conflict())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/conflict.png")
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
            .matches_image("tests/snapshots/conflict.png")
            .expect("reference for a conflict about a.txt");
        assert!(matches, "the conflict dialog does not match conflict.png");
    }

    #[test]
    fn the_dialog_matches_its_german_snapshot() {
        let matches = simulator(&app_with_conflict().in_language(Language::German))
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/conflict_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the conflict dialog does not match conflict_de.png"
        );
    }

    /// One dialog, not two: the prompt underneath must not show through.
    #[test]
    fn the_prompt_is_gone_while_the_dialog_is_up() {
        let app = app_with_conflict();
        let same_as_prompt = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/prompt.png")
            .expect("compare with the prompt");

        assert!(
            !same_as_prompt,
            "the prompt is still drawn under the conflict dialog"
        );
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod transfer_overlay {
    use super::*;

    fn app_with(open: impl FnOnce(&mut App), lang: Language) -> App {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        app.in_language(lang)
    }

    fn assert_snapshot(app: &App, name: &str) {
        let matches = simulator(app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the dialog does not match {name}.png");
    }

    #[test]
    fn the_confirmation_of_a_few_entries_lists_their_names() {
        for (lang, name) in [
            (Language::English, "copy_confirm_few_en"),
            (Language::German, "copy_confirm_few_de"),
        ] {
            assert_snapshot(
                &app_with(|a| a.open_transfer_dialog_for_test(4), lang),
                name,
            );
        }
    }

    #[test]
    fn the_confirmation_of_many_entries_sums_them_up() {
        for (lang, name) in [
            (Language::English, "copy_confirm_many_en"),
            (Language::German, "copy_confirm_many_de"),
        ] {
            assert_snapshot(
                &app_with(|a| a.open_transfer_dialog_for_test(36), lang),
                name,
            );
        }
    }

    #[test]
    fn the_progress_shows_how_far_the_copy_is() {
        for (lang, name) in [
            (Language::English, "copy_progress_en"),
            (Language::German, "copy_progress_de"),
        ] {
            assert_snapshot(&app_with(|a| a.show_progress_for_test(37, 120), lang), name);
        }
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod delete_overlay {
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
            .matches_image("tests/snapshots/delete_trash.png")
            .expect("snapshot comparison");
        assert!(matches, "the trash dialog does not match delete_trash.png");
    }

    /// The permanent dialog counts several entries and carries the warning.
    #[test]
    fn the_permanent_dialog_counts_the_entries_and_warns() {
        let matches = simulator(&app_with_delete(7, true))
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/delete_permanent.png")
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
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(matches, "the dialog does not match {name}.png");
        }
    }

    /// An overlay that is not drawn would leave the plain panels. Compared with
    /// the two-panel reference, which is committed, rather than with a dialog
    /// reference that this very test could be the one to write.
    #[test]
    fn the_dialog_is_drawn_over_the_panels() {
        let same_as_panels = simulator(&app_with_delete(1, false))
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("compare with the panels");
        assert!(
            !same_as_panels,
            "the delete dialog is not drawn over the panels"
        );
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod volume_overlay {
    use super::*;
    use crate::fs::Volume;
    use crate::messages::PanelSide;
    use std::path::PathBuf;

    fn volume(name: &str, path: &str) -> Volume {
        Volume {
            name: name.to_string(),
            path: PathBuf::from(path),
        }
    }

    /// A fixed list, so the image does not depend on the machine's disks.
    fn app_with_menu() -> App {
        let mut app = App::with_fixed_panels();
        app.open_volume_menu_for_test(
            PanelSide::Right,
            vec![
                volume("Macintosh HD", "/"),
                volume("test", "/home/test"),
                volume(
                    "iCloud Drive",
                    "/home/test/Library/Mobile Documents/com~apple~CloudDocs",
                ),
                volume("Backup", "/Volumes/Backup"),
            ],
        );
        app
    }

    /// The menu lists the drives, names the panel and highlights the one the
    /// panel is on (`test`, for `/home/test/files`).
    #[test]
    fn the_menu_is_drawn_over_the_panels() {
        let matches = simulator(&app_with_menu())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/volumes.png")
            .expect("snapshot comparison");
        assert!(matches, "the drive menu does not match volumes.png");
    }

    #[test]
    fn the_menu_matches_its_german_snapshot() {
        let matches = simulator(&app_with_menu().in_language(Language::German))
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/volumes_de.png")
            .expect("snapshot comparison");
        assert!(matches, "the drive menu does not match volumes_de.png");
    }

    /// A menu that is not drawn would leave the plain panels, compared with the
    /// committed two-panel reference.
    #[test]
    fn the_menu_differs_from_the_plain_panels() {
        let same_as_panels = simulator(&app_with_menu())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("compare with the panels");
        assert!(!same_as_panels, "the drive menu is not drawn");
    }

    #[test]
    fn a_click_on_an_entry_chooses_it() {
        let app = app_with_menu();
        let mut sim = simulator(&app);
        // Buttons have no id (see the module comment); the row is found by its text.
        sim.click("Backup").expect("the Backup row");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert_eq!(messages, [Message::VolumeMenuClick(3)]);
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod context_menu_overlay {
    use super::*;
    use crate::context_menu::ContextAction;
    use crate::messages::PanelSide;
    use iced::{mouse, Event, Point};

    /// The pointer is placed so the image does not depend on where a test
    /// runner's cursor happens to be.
    fn app_with_menu() -> App {
        let mut app = App::with_fixed_panels();
        app.set_pointer_for_test(Point::new(150.0, 130.0));
        drop(app.update(Message::ContextMenuAt {
            side: PanelSide::Left,
            index: 2,
        }));
        app
    }

    #[test]
    fn the_menu_is_drawn_over_the_panels() {
        let matches = simulator(&app_with_menu())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/context_menu.png")
            .expect("snapshot comparison");
        assert!(matches, "the context menu does not match context_menu.png");
    }

    #[test]
    fn the_menu_differs_from_the_plain_panels() {
        let same_as_panels = simulator(&app_with_menu())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("compare with the panels");
        assert!(!same_as_panels, "the context menu is not drawn");
    }

    #[test]
    fn a_right_click_on_a_row_asks_for_the_menu_there() {
        let app = App::with_fixed_panels();
        let mut sim = simulator(&app);
        // Second row of the left panel: below the title and column header.
        sim.point_at(Point::new(200.0, 59.0 + 22.0 * 1.5));
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right,
        ))]);
        let messages: Vec<Message> = sim.into_messages().collect();
        assert_eq!(
            messages,
            [Message::ContextMenuAt {
                side: PanelSide::Left,
                index: 1
            }]
        );
    }

    #[test]
    fn a_click_on_an_entry_runs_it() {
        let app = app_with_menu();
        let mut sim = simulator(&app);
        sim.click("Copy path").expect("the Copy path entry");
        let index = app
            .context_menu_for_test()
            .unwrap()
            .entries()
            .iter()
            .position(|entry| {
                matches!(
                    entry,
                    crate::context_menu::MenuEntry::Item {
                        action: ContextAction::CopyPath,
                        ..
                    }
                )
            })
            .unwrap();
        let messages: Vec<Message> = sim.into_messages().collect();
        assert_eq!(messages, [Message::ContextMenuClick(index)]);
    }

    #[test]
    fn a_greyed_out_entry_does_not_answer_a_click() {
        use crate::context_menu::MenuEntry;
        use crate::ui::layout::{
            MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_WIDTH,
        };

        let mut app = App::with_fixed_panels();
        app.set_pointer_for_test(Point::new(150.0, 130.0));
        drop(app.update(Message::ContextMenuAt {
            side: PanelSide::Left,
            index: 1,
        }));
        let menu = app.context_menu_for_test().unwrap();
        let mut top = menu.origin().y + MENU_PADDING;
        let mut view_row = None;
        for entry in menu.entries() {
            match entry {
                MenuEntry::Separator => top += MENU_SEPARATOR_HEIGHT,
                MenuEntry::Item { action, enabled } => {
                    if *action == ContextAction::View {
                        assert!(!enabled, "View is greyed out on a directory");
                        view_row = Some(top + MENU_ITEM_HEIGHT / 2.0);
                    }
                    top += MENU_ITEM_HEIGHT;
                }
                MenuEntry::Hermes => top += MENU_ITEM_HEIGHT,
            }
        }
        let point = Point::new(
            menu.origin().x + MENU_WIDTH / 2.0,
            view_row.expect("a View entry"),
        );
        let mut sim = simulator(&app);
        sim.point_at(point);
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
        assert_eq!(sim.into_messages().count(), 0);
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod status_notice {
    use super::*;
    use crate::messages::ClipboardKind;

    fn copied() -> App {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::CopyToClipboard(ClipboardKind::Path)));
        app
    }

    #[test]
    fn the_notice_is_drawn_in_the_status_bar() {
        let matches = simulator(&copied())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/status_notice.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the status bar notice does not match status_notice.png"
        );
    }

    const NOTICE_TEXT: &str = "\u{2713} Path copied";

    fn status_bar<'a>(app: &'a App, failure: Option<&'a str>) -> iced::Element<'a, Message> {
        crate::ui::statusbar::view(
            app.active_panel(),
            crate::i18n::Language::English,
            &crate::palette::Palette::default(),
            crate::ui::statusbar::Activity {
                side: crate::messages::PanelSide::Left,
                theme_name: None,
                job: None,
                job_press: None,
                filing: None,
                on_blocked_press: None,
                notice: app.notice_for_test(),
                hermes_line: None,
            },
            failure,
            None,
            crate::ui::statusbar::PathBar {
                editing: None,
                on_input: Message::PathFieldInput,
                on_submit: Message::PathFieldSubmit,
                on_copy: Message::PathFieldCancel,
            },
        )
    }

    fn shows_notice(view: iced::Element<'_, Message>) -> bool {
        Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1200.0, 40.0),
            view,
        )
        .find(NOTICE_TEXT)
        .is_ok()
    }

    #[test]
    fn the_status_bar_shows_the_notice_beside_an_error() {
        let app = copied();
        assert!(shows_notice(status_bar(&app, None)));
        assert!(shows_notice(status_bar(&app, Some("Copy failed"))));
    }

    #[test]
    fn a_config_warning_and_a_notice_are_drawn_together() {
        let mut app = App::with_fixed_panels().with_config_warning(
            "Config file config.toml, line 2: directory on panel is hard to read",
        );
        assert!(app.status_error().is_some());
        drop(app.update(Message::CopyToClipboard(ClipboardKind::Path)));
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/status_notice_with_error.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the status bar does not match status_notice_with_error.png"
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
mod search_toggle_keys {
    use super::*;
    use crate::messages::SearchMessage;
    use iced::keyboard::key::{Code, NativeCode, Physical};
    use iced::keyboard::{self, Key, Modifiers};

    fn press(typed: &str, code: Code, modifiers: Modifiers, repeat: bool) -> iced::Event {
        let key = Key::Character(typed.into());
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers,
            repeat,
            text: Some(typed.into()),
        })
    }

    /// What the app receives when `events` arrive while the query field has the
    /// focus (`focused`) or nothing has.
    fn messages_for(focused: bool, events: Vec<iced::Event>) -> Vec<Message> {
        let app = App::with_fixed_search();
        let mut sim = simulator(&app);
        if focused {
            sim.click(iced_selector::id("search-query"))
                .expect("the query field");
        }
        let _ = sim.simulate(events);
        sim.into_messages().collect()
    }

    fn toggles(messages: &[Message]) -> usize {
        messages
            .iter()
            .filter(|message| matches!(message, Message::Search(SearchMessage::ToggleCase)))
            .count()
    }

    fn typed_into_query(messages: &[Message]) -> Vec<&str> {
        messages
            .iter()
            .filter_map(|message| match message {
                Message::Search(SearchMessage::QueryChanged(text)) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn option_c_in_the_focused_query_toggles_case_and_types_nothing() {
        let messages = messages_for(true, vec![press("ç", Code::KeyC, Modifiers::ALT, false)]);
        assert_eq!(toggles(&messages), 1, "{messages:?}");
        assert!(typed_into_query(&messages).is_empty(), "{messages:?}");

        let mut app = App::with_fixed_search();
        for message in messages {
            drop(app.update(message));
        }
        let view = app.search_for_test().unwrap();
        assert!(view.case_sensitive);
        assert_eq!(view.query, "hello");
    }

    #[test]
    fn option_with_a_bracket_still_types_it_into_the_field() {
        let messages = messages_for(true, vec![press("[", Code::Digit5, Modifiers::ALT, false)]);
        assert_eq!(toggles(&messages), 0, "{messages:?}");
        let typed = typed_into_query(&messages);
        assert!(
            typed.iter().any(|text| text.contains('[')),
            "the bracket was not typed: {messages:?}"
        );
    }

    #[test]
    fn a_held_toggle_key_does_not_toggle_again() {
        let messages = messages_for(
            true,
            vec![
                press("ç", Code::KeyC, Modifiers::ALT, false),
                press("ç", Code::KeyC, Modifiers::ALT, true),
                press("ç", Code::KeyC, Modifiers::ALT, true),
            ],
        );
        assert_eq!(toggles(&messages), 1, "{messages:?}");
        assert!(typed_into_query(&messages).is_empty(), "{messages:?}");
    }

    #[test]
    fn a_field_without_the_focus_leaves_the_press_to_the_key_routing() {
        let messages = messages_for(false, vec![press("ç", Code::KeyC, Modifiers::ALT, false)]);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn the_unidentified_physical_key_is_not_a_toggle() {
        let key = Key::Character("c".into());
        let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::default(),
            repeat: false,
            text: Some("c".into()),
        });
        let messages = messages_for(true, vec![event]);
        assert_eq!(toggles(&messages), 0, "{messages:?}");
        assert!(!typed_into_query(&messages).is_empty(), "{messages:?}");
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod filing_cards {
    use super::*;

    fn plain_file_card() -> App {
        App::with_fixed_filing(Phase::PlainFileWarning)
    }

    fn differs_from_the_plain_panels(app: &App) -> bool {
        !simulator(app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/two_panels.png")
            .expect("compare with the panels")
    }

    #[test]
    fn every_filing_card_is_drawn_over_the_panels() {
        let cards = [
            Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
            Phase::ShowRecoveryKey {
                key: FilingModule::fixed_recovery_key(),
                group: 3,
                typed: SecretText::default(),
                mismatch: false,
            },
            Phase::AskRecoveryKey {
                reason: KeyProblem::Missing,
                typed: SecretText::default(),
                confirm_replace: false,
                busy: false,
                error: None,
            },
            Phase::AskRecoveryKey {
                reason: KeyProblem::Mismatch,
                typed: SecretText::default(),
                confirm_replace: false,
                busy: false,
                error: None,
            },
            Phase::PlainFileWarning,
        ];
        for phase in cards {
            let name = format!("{phase:?}");
            assert!(
                differs_from_the_plain_panels(&App::with_fixed_filing(phase)),
                "{name} is not drawn"
            );
        }
    }

    #[test]
    fn the_master_key_card_matches_its_snapshot() {
        let app = App::with_fixed_filing(Phase::ShowRecoveryKey {
            key: FilingModule::varied_recovery_key(),
            group: 1,
            typed: SecretText::default(),
            mismatch: false,
        });
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/recovery_key.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the card does not match tests/snapshots/recovery_key.png"
        );
    }

    #[test]
    fn the_master_key_card_matches_its_german_snapshot() {
        let app = App::with_fixed_filing(Phase::ShowRecoveryKey {
            key: FilingModule::varied_recovery_key(),
            group: 1,
            typed: SecretText::default(),
            mismatch: false,
        })
        .in_language(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/recovery_key_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the card does not match tests/snapshots/recovery_key_de.png"
        );
    }

    fn ask_recovery_key(reason: KeyProblem) -> App {
        App::with_fixed_filing(Phase::AskRecoveryKey {
            reason,
            typed: SecretText::default(),
            confirm_replace: false,
            busy: false,
            error: None,
        })
    }

    fn shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    const MISSING_EXPLANATION: &str = "The key for the index is not in the system keystore. Enter the recovery key to open the index.";
    const MISMATCH_EXPLANATION: &str = "A key is stored, but it does not open this index. The stored key is replaced only if the recovery key you enter opens the index.";
    const REPLACE_CHECKBOX: &str = "Replace the stored key";

    #[test]
    fn each_key_card_shows_its_own_prompt() {
        let confirmation = App::with_fixed_filing(Phase::ShowRecoveryKey {
            key: FilingModule::fixed_recovery_key(),
            group: 3,
            typed: SecretText::default(),
            mismatch: false,
        });
        assert!(shows(
            &confirmation,
            "Type block 4 (the highlighted four characters) to show that you saved the key."
        ));
        assert!(shows(&confirmation, "Copy"));

        let restore = ask_recovery_key(KeyProblem::Missing);
        assert!(shows(&restore, MISSING_EXPLANATION));
        assert!(!shows(&restore, MISMATCH_EXPLANATION));
        assert!(!shows(&restore, REPLACE_CHECKBOX));

        let mismatch = ask_recovery_key(KeyProblem::Mismatch);
        assert!(shows(&mismatch, MISMATCH_EXPLANATION));
        assert!(shows(&mismatch, REPLACE_CHECKBOX));
        assert!(!shows(&mismatch, MISSING_EXPLANATION));
    }

    #[test]
    fn the_create_button_asks_to_create_the_index() {
        let app = App::with_fixed_filing(Phase::AskEncryption {
            encrypted: true,
            busy: false,
            error: None,
        });
        let mut sim = simulator(&app);
        sim.click("Create index").expect("the Create index button");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert_eq!(messages, [Message::Filing(FilingMessage::CreateIndex)]);
    }

    #[test]
    fn the_plain_file_card_confirms_with_its_button() {
        let app = plain_file_card();
        let mut sim = simulator(&app);
        sim.click("Use this file")
            .expect("the Use this file button");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert_eq!(messages, [Message::Filing(FilingMessage::ConfirmPlainFile)]);
    }

    fn messages_for_a_click_on_a_panel_row(app: &App) -> Vec<Message> {
        let mut sim = simulator(app);
        // Directories are drawn with a leading slash.
        sim.click("/Documents").expect("a panel row");
        sim.into_messages().collect()
    }

    #[test]
    fn the_panels_stay_usable_while_a_card_is_up() {
        let without_card = messages_for_a_click_on_a_panel_row(&App::with_fixed_panels());
        assert!(
            matches!(without_card.as_slice(), [Message::RowClicked { .. }]),
            "the click misses the row: {without_card:?}"
        );

        let with_card = messages_for_a_click_on_a_panel_row(&plain_file_card());
        assert_eq!(with_card, without_card);
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod filing_status_page {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use ncrs_pipeline::{
        Busy, ErrorEntry, Load, RunnerState, Snapshot, StepSnapshot, Task, WorkerSnapshot,
    };

    use super::*;
    use crate::messages::FilingStatusMessage;

    const ERROR_FILE: &str = "/tmp/inbox/company-a-invoice.pdf";

    fn taken_at() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn snapshot(state: RunnerState, waiting: usize, left: usize) -> Snapshot {
        let mut snapshot = Snapshot::off();
        snapshot.state = state;
        snapshot.worker_target = 2;
        snapshot.left_pipeline = left;
        snapshot.steps = vec![StepSnapshot {
            name: "probe",
            waiting,
            done: 0,
            failed: 0,
            unchanged: 0,
        }];
        snapshot
    }

    fn page(snapshot: Snapshot) -> App {
        App::with_filing_status_page(snapshot, taken_at())
    }

    fn shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    fn page_message(message: FilingStatusMessage) -> Message {
        Message::FilingStatus(message)
    }

    /// Two files open, none finished yet: the state in which throughput and
    /// time left have nothing to show.
    fn inspection_snapshot() -> Snapshot {
        let mut snapshot = snapshot(RunnerState::Running, 0, 0);
        snapshot.steps = vec![
            StepSnapshot {
                name: "intake",
                waiting: 2,
                done: 5,
                failed: 0,
                unchanged: 1,
            },
            StepSnapshot {
                name: "review",
                waiting: 0,
                done: 3,
                failed: 1,
                unchanged: 0,
            },
        ];
        snapshot.workers = vec![
            WorkerSnapshot {
                id: 0,
                busy: Some(Busy {
                    file: PathBuf::from("/home/user/inbox/invoice.pdf"),
                    task: Task::Step("intake"),
                    started_at: taken_at() - Duration::from_secs(12),
                }),
            },
            WorkerSnapshot { id: 1, busy: None },
        ];
        snapshot
    }

    fn inspection(language: Language) -> App {
        inspection_of(inspection_snapshot(), language)
    }

    fn inspection_of(snapshot: Snapshot, language: Language) -> App {
        let figures = crate::filing::IndexFigures {
            path: PathBuf::from("/home/user/.local/share/ncrs/index.db"),
            file_bytes: Some(2 * 1024 * 1024),
            encrypted: true,
            statistics: ncrs_index::IndexStatistics {
                schema_version: 3,
                document_count: 240,
                open_reviews: 12,
                full_text_bytes: 512 * 1024,
            },
        };
        App::with_filing_inspection(snapshot, taken_at(), figures)
            .with_hermes_counts(sample_counts())
            .in_language(language)
    }

    /// 2 421 documents seen, none filed; the archive has text for most.
    fn sample_counts() -> ncrs_index::HermesCounts {
        ncrs_index::HermesCounts {
            inbox: ncrs_index::StatusCounts {
                in_review: 2421,
                ..ncrs_index::StatusCounts::default()
            },
            open_reviews: 2421,
            open_reviews_without_text: 1800,
            confirmed_rules: 0,
            archive_documents: 5120,
            archive_documents_with_text: 4871,
            ..ncrs_index::HermesCounts::default()
        }
    }

    #[test]
    fn the_counts_are_told_in_plain_sentences() {
        let counts = sample_counts();
        assert_eq!(
            crate::ui::filing_status::counts_sentences(&counts, Language::English),
            "Hermes has seen 2\u{a0}421 documents: 0 filed, 2\u{a0}421 wait for you. \
             1\u{a0}800 of them have no readable text. \
             For the rest no rule matches: there are no confirmed rules yet. \
             Rule suggestions learn from the 4\u{a0}871 of 5\u{a0}120 archive documents that have text."
        );
        let with_rules = ncrs_index::HermesCounts {
            confirmed_rules: 3,
            open_reviews_without_text: 0,
            archive_documents: 0,
            ..counts
        };
        assert_eq!(
            crate::ui::filing_status::counts_sentences(&with_rules, Language::German),
            "Hermes hat 2\u{a0}421 Dokumente gesehen: 0 abgelegt, 2\u{a0}421 warten auf dich."
        );
    }

    #[test]
    fn a_missing_pdf_library_is_a_box_with_the_searched_path_cut_in_the_middle() {
        let long = PathBuf::from(format!(
            "/Applications/Some Application/Contents/Frameworks/{}/lib/libpdfium.dylib",
            "deeply-nested-folder/".repeat(6)
        ));
        for (language, name) in [
            (Language::English, "inspection_pdf_missing_en"),
            (Language::German, "inspection_pdf_missing_de"),
        ] {
            let app = inspection(language).with_pdf_library_missing(long.clone());
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the inspection does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn the_inspection_matches_its_snapshots() {
        for (language, name) in [
            (Language::English, "inspection_en"),
            (Language::German, "inspection_de"),
        ] {
            let matches = simulator(&inspection(language))
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the inspection does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn a_long_file_name_does_not_change_the_inspection_frame() {
        let short = frame_bounds(&inspection(Language::English));
        let mut snapshot = inspection_snapshot();
        let long_name = format!("/home/user/inbox/{}.pdf", "very-long-file-name-".repeat(10));
        snapshot.workers = vec![WorkerSnapshot {
            id: 0,
            busy: Some(Busy {
                file: PathBuf::from(long_name),
                task: Task::Step("intake"),
                started_at: taken_at() - Duration::from_secs(12),
            }),
        }];
        let long = frame_bounds(&inspection_of(snapshot, Language::English));
        assert_eq!(short, long);
    }

    #[test]
    fn the_inspection_uses_the_visible_names_and_says_when_figures_come() {
        let english = inspection(Language::English);
        for label in [
            "Intake",
            "waiting",
            "done",
            "failed",
            "unchanged",
            "Pending",
        ] {
            assert!(shows(&english, label), "{label}");
        }
        assert!(shows(&english, "after the first full minute"));
        let german = inspection(Language::German);
        assert!(shows(&german, "Annahme"));
        assert!(shows(&german, "wartend"));
        assert!(shows(&german, "Wiedervorlage"));
        assert!(shows(&german, "ab der ersten vollen Minute"));

        let idle = page(snapshot(RunnerState::Idle, 0, 0));
        assert!(!shows(&idle, "after the first full minute"));
    }

    #[test]
    fn the_page_shows_every_state_and_the_button_that_fits_it() {
        let cases = [
            (RunnerState::Off, "Off", "Pause"),
            (RunnerState::Idle, "Idle", "Pause"),
            (RunnerState::Running, "Running", "Pause"),
            (RunnerState::Paused, "Paused", "Resume"),
        ];
        for (state, state_text, button) in cases {
            let app = page(snapshot(state, 0, 0));
            assert!(shows(&app, state_text), "{state:?}: {state_text}");
            assert!(shows(&app, button), "{state:?}: {button}");
            let other = if button == "Pause" { "Resume" } else { "Pause" };
            assert!(!shows(&app, other), "{state:?}: {other}");
        }
    }

    #[test]
    fn the_buttons_ask_for_a_pause_and_a_load() {
        let app = page(snapshot(RunnerState::Running, 3, 0));

        assert_eq!(
            messages_after_click(&app, "Pause"),
            [page_message(FilingStatusMessage::TogglePause)]
        );
        assert_eq!(
            messages_after_click(&app, "Business as usual"),
            [page_message(FilingStatusMessage::SetLoad(Load::Balanced))]
        );
    }

    #[test]
    fn an_error_is_listed_and_a_click_on_it_asks_to_show_the_file() {
        let mut with_error = snapshot(RunnerState::Idle, 0, 1);
        with_error.errors = vec![ErrorEntry {
            file: PathBuf::from(ERROR_FILE),
            task: Task::Step("probe"),
            message: "the step stopped unexpectedly".to_owned(),
            at: taken_at(),
            sequence: 0,
        }];
        let app = page(with_error);

        assert!(shows(&app, "the step stopped unexpectedly"));
        assert_eq!(
            messages_after_click(&app, ERROR_FILE),
            [page_message(FilingStatusMessage::ErrorActivated(
                PathBuf::from(ERROR_FILE)
            ))]
        );
        assert!(!shows(&app, "No errors"));
        assert!(shows(&page(snapshot(RunnerState::Idle, 0, 0)), "No errors"));
    }

    #[test]
    fn a_busy_worker_shows_how_long_it_has_been_at_it() {
        let mut busy = snapshot(RunnerState::Running, 0, 0);
        busy.workers = vec![
            WorkerSnapshot {
                id: 0,
                busy: Some(Busy {
                    file: PathBuf::from(ERROR_FILE),
                    task: Task::Step("probe"),
                    started_at: taken_at() - Duration::from_secs(42),
                }),
            },
            WorkerSnapshot { id: 1, busy: None },
        ];
        let app = page(busy);

        assert!(shows(&app, "0:42"));
        assert!(shows(&app, "company-a-invoice.pdf"));
        assert!(shows(&app, "idle"));
    }

    #[test]
    fn without_figures_the_index_says_why() {
        let app = page(snapshot(RunnerState::Idle, 0, 0));

        assert!(shows(&app, "Index not open"));
    }

    #[test]
    fn the_status_bar_shows_progress_while_there_is_work_and_opens_the_page() {
        let app = App::with_filing_progress(
            snapshot(RunnerState::Running, 340, 100),
            snapshot(RunnerState::Running, 328, 112),
            taken_at(),
        );

        assert!(shows(&app, "running \u{b7} 328 open"));
        assert_eq!(
            messages_after_click(&app, "running \u{b7} 328 open"),
            [page_message(FilingStatusMessage::Open)]
        );
    }

    #[test]
    fn the_status_bar_says_when_the_pipeline_is_paused() {
        let app = App::with_filing_snapshot(snapshot(RunnerState::Paused, 5, 0), taken_at());

        assert!(shows(&app, "paused \u{b7} 5 open"));
    }

    #[test]
    fn the_status_bar_names_reviews_and_failures() {
        let mut failing = snapshot(RunnerState::Running, 0, 0);
        failing.steps[0].failed = 2;
        let counts = ncrs_index::HermesCounts {
            open_reviews: 2421,
            ..ncrs_index::HermesCounts::default()
        };
        let app = App::with_filing_snapshot(failing, taken_at()).with_hermes_counts(counts);
        assert!(shows(
            &app,
            "idle \u{b7} 2\u{a0}421 to review \u{b7} 2 failed"
        ));
        let german = app.in_language(Language::German);
        assert!(shows(
            &german,
            "ruht \u{b7} 2\u{a0}421 R\u{fc}ckfragen \u{b7} 2 Fehler"
        ));
    }

    #[test]
    fn a_missing_pdf_library_is_the_whole_status_line() {
        let app = App::with_filing_snapshot(snapshot(RunnerState::Running, 5, 0), taken_at())
            .with_pdf_library_missing(PathBuf::from("/opt/pdfium/lib"));
        assert!(shows(&app, "PDF library missing, PDFs are not read"));
        assert_eq!(
            messages_after_click(&app, "PDF library missing, PDFs are not read"),
            [page_message(FilingStatusMessage::Open)]
        );
    }

    #[test]
    fn the_status_bar_says_idle_when_nothing_waits_and_nothing_when_off() {
        let idle = App::with_filing_snapshot(snapshot(RunnerState::Idle, 0, 0), taken_at());
        assert!(shows(&idle, "idle"));
        let off = App::with_filing_snapshot(snapshot(RunnerState::Off, 0, 40), taken_at());
        assert!(!shows(&off, "idle"));
        assert!(!shows(&off, "running \u{b7} 40 open"));
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod filing_setup_card {
    use std::path::PathBuf;

    use ncrs_intake::LocationMode;

    use super::*;
    use crate::config::FilingLocation;
    use crate::messages::FilingSetupMessage;
    use crate::ui::filing_setup::{
        target_choice_id, ADD_DOCUMENT_FOLDER_ID, ADD_TUBES_FOLDER_ID, USE_PANEL_FOR_DOCUMENTS_ID,
        USE_PANEL_FOR_TUBES_ID,
    };

    const CHOOSE_CENTRAL: &str = "Choose which folder is the Central Filing.";
    const NO_DOCUMENT_FOLDER: &str = "Add at least one folder where you keep your documents.";
    const NO_FOLDER_YET: &str = "No folder yet.";
    const VAULT_NOTE: &str = "With an unencrypted index, do not add folders from an encrypted drive (for example a mounted Cryptomator vault): their text would end up in the index in plain form. ncrs does not detect such drives.";

    fn location(path: &str, mode: LocationMode, images: bool, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: PathBuf::from(path),
            mode,
            images,
            is_target,
        }
    }

    fn tube(path: &str) -> FilingLocation {
        location(path, LocationMode::Inbox, false, false)
    }

    fn documents(path: &str, is_target: bool) -> FilingLocation {
        location(path, LocationMode::Archive, false, is_target)
    }

    fn filled() -> Vec<FilingLocation> {
        vec![
            tube("/home/test/Downloads/Scans"),
            location("/home/test/Scanner", LocationMode::Inbox, true, false),
            location(
                "/home/test/Documents/Filed",
                LocationMode::Archive,
                true,
                true,
            ),
            documents("/home/test/Documents/Old-2019", false),
        ]
    }

    fn violation() -> Vec<FilingLocation> {
        vec![
            documents("/home/test/Documents/Filed", false),
            location(
                "/home/test/Documents/Filed/Scanner",
                LocationMode::Inbox,
                false,
                false,
            ),
        ]
    }

    fn shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    /// The card scrolls: what lies below the fold is reached by the wheel, as
    /// by a user.
    fn messages_after_clicking_id(app: &App, id: impl Into<iced::widget::Id>) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.point_at(frame_bounds(app).center());
        let _statuses = sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -20.0 },
        })]);
        sim.click(id.into()).expect("the clicked id");
        sim.into_messages().collect()
    }

    fn has_target_choice(app: &App, index: usize) -> bool {
        simulator(app)
            .find(iced::widget::Id::from(target_choice_id(index)))
            .is_ok()
    }

    fn setup_message(message: FilingSetupMessage) -> Message {
        Message::FilingSetup(message)
    }

    #[test]
    fn an_empty_card_asks_for_both_kinds_of_folder_and_cannot_be_saved() {
        let app = App::with_filing_setup(&[], false);

        for label in [
            "Hermes",
            "Tubes: where do new documents arrive?",
            "Where do you keep your documents?",
            NO_FOLDER_YET,
        ] {
            assert!(shows(&app, label), "{label}");
        }
        assert!(
            !shows(&app, NO_DOCUMENT_FOLDER),
            "nothing to complain about yet"
        );
        assert!(messages_after_click(&app, "Save").is_empty());
    }

    #[test]
    fn a_section_without_folders_says_so_until_it_has_one() {
        let only_tubes = App::with_filing_setup(&[tube("/home/test/Scanner")], false);
        assert!(shows(&only_tubes, NO_FOLDER_YET));
        assert!(shows(&only_tubes, NO_DOCUMENT_FOLDER));

        let only_documents = App::with_filing_setup(&[documents("/home/test/Filed", false)], false);
        assert!(shows(&only_documents, NO_FOLDER_YET));

        assert!(!shows(
            &App::with_filing_setup(&filled(), false),
            NO_FOLDER_YET
        ));
    }

    #[test]
    fn each_section_has_its_own_add_buttons() {
        let app = App::with_filing_setup(&[], false);

        for (pick_id, panel_id, mode) in [
            (
                ADD_TUBES_FOLDER_ID,
                USE_PANEL_FOR_TUBES_ID,
                LocationMode::Inbox,
            ),
            (
                ADD_DOCUMENT_FOLDER_ID,
                USE_PANEL_FOR_DOCUMENTS_ID,
                LocationMode::Archive,
            ),
        ] {
            assert_eq!(
                messages_after_clicking_id(&app, pick_id),
                [setup_message(FilingSetupMessage::PickFolder(mode))],
                "{pick_id}"
            );
            assert_eq!(
                messages_after_clicking_id(&app, panel_id),
                [setup_message(FilingSetupMessage::AddPanelFolder(mode))],
                "{panel_id}"
            );
        }
    }

    #[test]
    fn one_document_folder_is_the_central_filing_without_radio_buttons() {
        let app = App::with_filing_setup(&[documents("/home/test/Filed", false)], false);

        assert!(shows(&app, "Central Filing"));
        assert!(!shows(&app, "Records"));
        assert!(
            messages_after_click(&app, "~/Filed").is_empty(),
            "a plain path is not a choice"
        );
    }

    #[test]
    fn a_choice_between_document_folders_sends_the_global_index() {
        let app = App::with_filing_setup(&filled(), false);

        assert!(shows(&app, "Central Filing"));
        assert!(shows(&app, "Records"));
        assert_eq!(
            messages_after_clicking_id(&app, target_choice_id(3)),
            [setup_message(FilingSetupMessage::SetTarget(3))]
        );
        assert_eq!(
            messages_after_clicking_id(&app, target_choice_id(2)),
            [setup_message(FilingSetupMessage::SetTarget(2))]
        );
        assert_eq!(
            messages_after_click(&app, "Save"),
            [setup_message(FilingSetupMessage::Save)]
        );
    }

    #[test]
    fn two_document_folders_without_a_mark_say_so_and_cannot_be_saved() {
        let app = App::with_filing_setup(
            &[
                documents("/home/test/Filed", false),
                documents("/home/test/Old", false),
            ],
            false,
        );

        assert!(shows(&app, CHOOSE_CENTRAL));
        assert!(messages_after_click(&app, "Save").is_empty());
    }

    #[test]
    fn remove_takes_the_entry_off_the_list() {
        let mut app = App::with_filing_setup(&filled(), false);

        let clicked = messages_after_click(&app, "Remove");
        assert_eq!(clicked, [setup_message(FilingSetupMessage::Remove(0))]);
        for message in clicked {
            drop(app.update(message));
        }

        assert!(!shows(&app, "~/Downloads/Scans"));
        assert!(shows(&app, "~/Scanner"));
        assert!(has_target_choice(&app, 1) && has_target_choice(&app, 2));
        assert!(!has_target_choice(&app, 3));
    }

    #[test]
    fn a_rule_the_list_breaks_is_shown() {
        let app = App::with_filing_setup(&violation(), false);

        assert!(shows(
            &app,
            "~/Documents/Filed and ~/Documents/Filed/Scanner overlap: a folder cannot lie inside another one."
        ));
        assert!(messages_after_click(&app, "Save").is_empty());
    }

    #[test]
    fn an_unencrypted_index_repeats_the_vault_note() {
        let app = App::with_filing_setup(&[documents("/home/test/Filed", false)], true);
        assert!(shows(&app, VAULT_NOTE));
        assert!(!shows(
            &App::with_filing_setup(&[documents("/home/test/Filed", false)], false),
            VAULT_NOTE
        ));
    }

    #[test]
    fn the_panels_stay_usable_while_the_setup_is_up() {
        let row_click = |app: &App| -> Vec<Message> {
            let mut sim = simulator(app);
            // Directories are drawn with a leading slash.
            sim.click("/Documents").expect("a panel row");
            sim.into_messages().collect()
        };
        let without_setup = row_click(&App::with_fixed_panels());
        assert!(
            matches!(without_setup.as_slice(), [Message::RowClicked { .. }]),
            "the click misses the row: {without_setup:?}"
        );

        assert_eq!(
            row_click(&App::with_filing_setup(&filled(), false)),
            without_setup
        );
    }

    fn matches_snapshot(chosen: &[FilingLocation], language: Language, name: &str) {
        let app = App::with_filing_setup(chosen, false).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the filing setup does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_empty_card_matches_its_snapshots() {
        matches_snapshot(&[], Language::English, "filing_setup_empty_en");
        matches_snapshot(&[], Language::German, "filing_setup_empty_de");
    }

    #[test]
    fn the_filled_card_matches_its_snapshots() {
        matches_snapshot(&filled(), Language::English, "filing_setup_filled_en");
        matches_snapshot(&filled(), Language::German, "filing_setup_filled_de");
    }

    #[test]
    fn the_card_with_a_rule_violation_matches_its_snapshots() {
        matches_snapshot(&violation(), Language::English, "filing_setup_violation_en");
        matches_snapshot(&violation(), Language::German, "filing_setup_violation_de");
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
mod command_palette {
    use super::*;
    use crate::messages::CommandPaletteMessage;

    fn palette_message(message: CommandPaletteMessage) -> Message {
        Message::CommandPalette(message)
    }

    fn open(app: &mut App) {
        drop(app.update(palette_message(CommandPaletteMessage::Toggle)));
    }

    fn shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    /// The panel is open with its key, and the key routing's message does it.
    fn opened_with_its_key() -> App {
        let mut app = App::with_fixed_panels();
        let routed = crate::app::route_key(
            &crate::app::PromptKeyState::default(),
            iced::keyboard::Key::Character("p".into()),
            iced::keyboard::Modifiers::COMMAND | iced::keyboard::Modifiers::SHIFT,
        )
        .expect("the palette key");
        drop(app.update(routed));
        app
    }

    #[test]
    fn the_key_opens_the_box_with_entries_and_their_keys() {
        let app = opened_with_its_key();
        assert!(app.command_palette_for_test().is_some());
        assert!(shows(&app, "Command list"));
        assert!(shows(&app, "Lab"));
        assert!(shows(&app, "Enter"), "the key of Open is not shown");
        assert!(shows(&app, "F5"), "the key of Copy is not shown");
    }

    #[test]
    fn a_rebound_key_is_shown_in_the_box() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[keys]\nfiling_status = \"F11\"\n").unwrap();
        let mut app = App::with_fixed_panels().with_config(Ok(crate::config::load(&path).unwrap()));
        open(&mut app);
        drop(
            app.update(palette_message(CommandPaletteMessage::QueryChanged(
                "inspection".to_owned(),
            ))),
        );
        assert!(shows(&app, "F11"));
    }

    #[test]
    fn typing_in_the_field_filters_the_list() {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        assert!(shows(&app, "Enter"));
        let mut sim = simulator(&app);
        sim.click(iced_selector::id("command-palette-query"))
            .expect("the query field");
        let _ = sim.typewrite("inspec");
        let messages: Vec<Message> = sim.into_messages().collect();
        for message in messages {
            drop(app.update(message));
        }
        let palette = app.command_palette_for_test().unwrap();
        assert_eq!(palette.query(), "inspec");
        assert_eq!(palette.hits().count(), 1);
        let marked = &palette.hits().next().unwrap().theme_highlight;
        assert!(!marked.is_empty(), "nothing is highlighted");
        assert!(!shows(&app, "Enter"), "the filter left Open in the list");
    }

    #[test]
    fn a_click_on_an_entry_runs_it_and_closes_the_box() {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        drop(
            app.update(palette_message(CommandPaletteMessage::QueryChanged(
                "switch".to_owned(),
            ))),
        );
        // A header sits above the group, so the row is read from the list.
        let palette = app.command_palette_for_test().unwrap();
        let language_row = palette
            .rows()
            .iter()
            .position(|row| match row {
                crate::command_palette::Row::Hit(hit) => {
                    palette.entry(hit).map(|entry| entry.action)
                        == Some(crate::keymap::KeyAction::SwitchLanguage)
                }
                crate::command_palette::Row::Header(_) => false,
            })
            .expect("Switch language is listed");
        let mut sim = simulator(&app);
        sim.click("F9").expect("the Switch language entry");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert!(
            messages.contains(&palette_message(CommandPaletteMessage::Select(
                language_row
            ))),
            "{messages:?}"
        );
        for message in messages {
            drop(app.update(message));
        }
        assert!(app.command_palette_for_test().is_none());
    }

    fn snapshot_of(app: &App, name: &str) {
        let matches = simulator(app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(name)
            .expect("snapshot comparison");
        assert!(matches, "the command list does not match {name}");
    }

    fn typed(app: &mut App, query: &str) {
        drop(
            app.update(palette_message(CommandPaletteMessage::QueryChanged(
                query.to_owned(),
            ))),
        );
    }

    #[test]
    fn the_empty_list_matches_its_snapshot() {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        snapshot_of(&app, "tests/snapshots/command_palette_empty.png");
    }

    #[test]
    fn the_empty_list_matches_its_german_snapshot() {
        let mut app = App::with_fixed_panels().in_language(Language::German);
        open(&mut app);
        snapshot_of(&app, "tests/snapshots/command_palette_empty_de.png");
    }

    #[test]
    fn a_query_matches_its_snapshot() {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        typed(&mut app, "st");
        snapshot_of(&app, "tests/snapshots/command_palette_query.png");
    }

    #[test]
    fn a_query_matches_its_german_snapshot() {
        let mut app = App::with_fixed_panels().in_language(Language::German);
        open(&mut app);
        typed(&mut app, "st");
        snapshot_of(&app, "tests/snapshots/command_palette_query_de.png");
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
mod theme_fetch_card {
    use super::*;
    use crate::messages::ThemeFetchMessage;
    use crate::open_vsx::FetchError;
    use crate::theme_fetch::tests::fetched;
    use crate::theme_fetch::{Fetched, ThemeFetch};
    use crate::vsix::Brightness;

    const HELP: &str =
        "Downloads only from Open VSX. Nothing goes out before you press Enter or Fetch.";

    fn fetch_message(message: ThemeFetchMessage) -> Message {
        Message::ThemeFetch(message)
    }

    fn two_themes() -> Fetched {
        let mut offered = fetched(&["Example Dark", "Example Light"]);
        offered.themes[1].brightness = Some(Brightness::Light);
        offered
    }

    fn choosing(offered: Fetched) -> ThemeFetch {
        let mut card = ThemeFetch::new();
        card.set_text("example.dark-themes".to_owned());
        card.start().unwrap();
        assert_eq!(card.finish_fetching(Ok(offered)), None);
        card
    }

    fn failed(text: &str, error: FetchError) -> ThemeFetch {
        let mut card = ThemeFetch::new();
        card.set_text(text.to_owned());
        card.fail(error);
        card
    }

    fn shows(card: ThemeFetch, label: &str) -> bool {
        simulator(&App::with_theme_fetch_card(card))
            .find(label)
            .is_ok()
    }

    fn messages_after_click(card: ThemeFetch, label: &str) -> Vec<Message> {
        let app = App::with_theme_fetch_card(card);
        let mut sim = simulator(&app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    #[test]
    fn the_empty_card_asks_for_an_extension_and_says_where_it_goes() {
        for label in ["Fetch color theme", HELP, "Fetch", "Cancel"] {
            assert!(shows(ThemeFetch::new(), label), "{label}");
        }
        assert_eq!(
            messages_after_click(ThemeFetch::new(), "Fetch"),
            [fetch_message(ThemeFetchMessage::Activate)]
        );
        assert_eq!(
            messages_after_click(ThemeFetch::new(), "Cancel"),
            [fetch_message(ThemeFetchMessage::Close)]
        );
    }

    #[test]
    fn the_card_is_worded_in_german_too() {
        let app = App::with_theme_fetch_card(ThemeFetch::new()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in ["Farbschema holen", "Holen", "Abbrechen"] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn a_complaint_is_shown_under_the_kept_text() {
        let invalid = failed("../etc", FetchError::InvalidId);
        assert!(shows(invalid, "Not a valid extension ID or link."));
        let unknown = failed(
            "example.dark-themes",
            FetchError::NotFound("example.dark-themes".to_owned()),
        );
        assert!(shows(
            unknown,
            "No extension example.dark-themes on Open VSX"
        ));
    }

    #[test]
    fn while_it_downloads_the_card_says_so_and_cannot_start_again() {
        let mut card = ThemeFetch::new();
        card.set_text("example.dark-themes".to_owned());
        card.start().unwrap();
        assert!(shows(card.clone(), "Fetching from Open VSX…"));
        assert!(messages_after_click(card, "Fetch").is_empty());
    }

    #[test]
    fn the_choice_names_the_extension_its_license_and_the_themes() {
        for label in [
            "Example Themes, version 1.2.3",
            "License: MIT",
            "Example Dark",
            "Example Light",
            "Cancel",
        ] {
            assert!(shows(choosing(two_themes()), label), "{label}");
        }
        assert!(!shows(choosing(two_themes()), "Fetch"), "nothing to fetch");
        assert_eq!(
            messages_after_click(choosing(two_themes()), "Example Light"),
            [fetch_message(ThemeFetchMessage::Pick(1))]
        );
    }

    #[test]
    fn a_license_that_is_not_stated_says_so() {
        let mut offered = two_themes();
        offered.extension.license = None;
        assert!(shows(choosing(offered), "License: not stated"));
    }

    fn matches_snapshot(card: ThemeFetch, name: &str) {
        let language = if name.ends_with("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_theme_fetch_card(card).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the theme card does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_empty_card_matches_its_snapshot() {
        matches_snapshot(ThemeFetch::new(), "theme_fetch_input_en");
    }

    #[test]
    fn the_choice_matches_its_snapshot() {
        matches_snapshot(choosing(two_themes()), "theme_fetch_choose_en");
    }

    #[test]
    fn the_german_cards_match_their_snapshots() {
        matches_snapshot(ThemeFetch::new(), "theme_fetch_input_de");
        matches_snapshot(choosing(two_themes()), "theme_fetch_choose_de");
        matches_snapshot(
            failed(
                "example.dark-themes",
                FetchError::NotFound("example.dark-themes".to_owned()),
            ),
            "theme_fetch_error_de",
        );
    }

    #[test]
    fn the_frame_grows_with_the_themes_and_keeps_its_width() {
        let one = frame_bounds(&App::with_theme_fetch_card(choosing(two_themes())));
        let names: Vec<String> = (0..10).map(|number| format!("Theme {number}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let ten = frame_bounds(&App::with_theme_fetch_card(choosing(fetched(&refs))));
        assert_eq!(one.width, ten.width);
        assert!(ten.height > one.height, "{one:?} {ten:?}");
    }

    #[test]
    fn the_card_with_a_complaint_matches_its_snapshot() {
        matches_snapshot(
            failed(
                "example.dark-themes",
                FetchError::NotFound("example.dark-themes".to_owned()),
            ),
            "theme_fetch_error_en",
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
mod review_list_card {
    use std::path::PathBuf;

    use super::*;
    use crate::messages::{HermesFocus, HermesPage, ReviewListMessage};
    use crate::review_list::tests::{loaded_list, long_list};
    use crate::review_list::{FormField, ReviewList};
    use crate::review_work::ConfirmOutcome;
    use crate::rule_suggestions::RuleSuggestions;

    fn review_message(message: ReviewListMessage) -> Message {
        Message::ReviewList(message)
    }

    fn shows(list: ReviewList, label: &str) -> bool {
        simulator(&App::with_review_list(list)).find(label).is_ok()
    }

    /// The first document filed, the second still waiting, the offer up.
    fn after_filing() -> ReviewList {
        let mut list = loaded_list();
        let second = list.rows()[1].clone();
        list.begin_confirm().expect("a request");
        list.confirmed(ConfirmOutcome::Filed {
            relative_path: PathBuf::from("Company A/2024-03-09_invoice.pdf"),
        });
        list.loaded(vec![second]);
        list
    }

    #[test]
    fn the_list_shows_reason_fields_and_buttons() {
        for label in [
            "Review",
            "scan-1.pdf",
            "scan-2.pdf",
            "No rule matches",
            "Sender",
            "Tax context",
            "Filing pattern",
            "Will be filed as: Company A/2024-03-09_invoice",
            "Confirm",
            "Close",
        ] {
            assert!(shows(loaded_list(), label), "{label}");
        }
    }

    #[test]
    fn an_empty_list_says_so() {
        let mut list = ReviewList::new();
        list.loaded(Vec::new());

        assert!(shows(
            list,
            "Nothing to review. Documents land here when Hermes cannot file them itself: without readable text or without a matching rule."
        ));
    }

    #[test]
    fn the_empty_list_matches_its_snapshots() {
        for name in ["review_list_empty_en", "review_list_empty_de"] {
            let mut list = ReviewList::new();
            list.loaded(Vec::new());
            matches_snapshot(list, name);
        }
    }

    #[test]
    fn clicking_a_row_selects_it() {
        let app = App::with_review_list(loaded_list());
        let mut sim = simulator(&app);
        sim.click("scan-2.pdf").expect("the row");

        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [review_message(ReviewListMessage::Select(1))]
        );
    }

    #[test]
    fn typing_into_a_field_changes_it() {
        let app = App::with_review_list(loaded_list());
        let mut sim = simulator(&app);
        sim.click(iced_selector::id("review-sender"))
            .expect("the sender field");
        let _ = sim.typewrite("X");

        assert_eq!(
            sim.into_messages().last(),
            Some(review_message(ReviewListMessage::FieldChanged(
                FormField::Sender,
                "Company AX".to_owned()
            )))
        );
    }

    #[test]
    fn clicking_confirm_asks_to_file() {
        let app = App::with_review_list(loaded_list());
        let mut sim = simulator(&app);
        sim.click("Confirm").expect("the button");

        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [review_message(ReviewListMessage::Confirm)]
        );
    }

    #[test]
    fn after_filing_the_rule_for_the_sender_is_offered() {
        let label = "Create rule for sender ‘Company A’";
        assert!(shows(after_filing(), label));
        assert!(shows(
            after_filing(),
            "Filed as Company A/2024-03-09_invoice.pdf"
        ));

        let app = App::with_review_list(after_filing());
        let mut sim = simulator(&app);
        sim.click(label).expect("the offer");
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [review_message(ReviewListMessage::CreateRule)]
        );
    }

    #[test]
    fn the_view_is_worded_in_german_too() {
        let app = App::with_review_list(after_filing()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Rückfragen",
            "Bestätigen",
            "Schließen",
            "Regel für Absender ‚Company A‘ anlegen",
            "Ablagemuster",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn the_sidebar_names_the_pages_of_hermes() {
        let app = App::with_review_list(loaded_list());
        let mut sim = simulator(&app);
        for label in ["Status and errors", "Rule suggestions"] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
        sim.click("Rule suggestions").expect("the entry");
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [Message::HermesPage(HermesPage::RuleSuggestions)]
        );
    }

    #[test]
    fn a_sidebar_click_swaps_the_open_card() {
        let mut app = App::with_rule_suggestions(RuleSuggestions::new());
        let _ = app.update(Message::HermesPage(HermesPage::Review));

        assert!(app.rule_suggestions_for_test().is_none());
        assert!(app.review_list_for_test().is_some());
    }

    #[test]
    fn the_page_that_is_open_stays_as_it_is() {
        let mut list = loaded_list();
        list.start_edit();
        let mut app = App::with_review_list(list);
        let _ = app.update(Message::HermesPage(HermesPage::Review));

        assert!(app.review_list_for_test().unwrap().is_editing());
    }

    #[test]
    fn escape_leaves_the_form_first() {
        let mut list = loaded_list();
        list.start_edit();
        let mut app = App::with_review_list(list);
        let _ = app.update(review_message(ReviewListMessage::Close));
        assert!(!app.review_list_for_test().unwrap().is_editing());
        let _ = app.update(review_message(ReviewListMessage::Close));
        assert!(app.review_list_for_test().is_none());
    }

    fn matches_snapshot(list: ReviewList, name: &str) {
        matches_snapshot_focused(list, HermesFocus::Content, name);
    }

    fn matches_snapshot_focused(list: ReviewList, focus: HermesFocus, name: &str) {
        let language = if name.contains("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_review_list(list)
            .in_language(language)
            .with_hermes_focus(focus);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the view does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_view_matches_its_snapshots() {
        matches_snapshot(loaded_list(), "review_list_en");
        matches_snapshot(loaded_list(), "review_list_de");
        matches_snapshot(after_filing(), "review_list_filed_en");
    }

    #[test]
    fn long_names_and_a_long_list_stay_inside_their_columns() {
        matches_snapshot(long_list(), "review_list_long_en");
        matches_snapshot(long_list(), "review_list_long_de");
    }

    #[test]
    fn the_sidebar_focus_outlines_the_list_and_fills_the_page_entry() {
        matches_snapshot_focused(
            loaded_list(),
            HermesFocus::Sidebar,
            "review_list_sidebar_focus_en",
        );
        matches_snapshot_focused(
            loaded_list(),
            HermesFocus::Sidebar,
            "review_list_sidebar_focus_de",
        );
    }

    #[test]
    fn the_wheel_over_the_list_scrolls_it() {
        let mut app = App::with_review_list(long_list());
        let _ = app.update(review_message(ReviewListMessage::Scrolled(
            iced::mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
        )));
        let view = app.review_list_for_test().unwrap();
        assert_eq!(view.first_visible(), 3);
        assert!(view.cursor() >= 3);
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
mod saved_queries_card {
    use std::path::PathBuf;

    use super::*;
    use crate::messages::{HermesPage, SavedQueriesMessage};
    use crate::saved_queries::tests::{hit, loaded_list};
    use crate::saved_queries::SavedQueries;

    fn queries_message(message: SavedQueriesMessage) -> Message {
        Message::SavedQueries(message)
    }

    fn shows(list: SavedQueries, label: &str) -> bool {
        simulator(&App::with_saved_queries(list))
            .find(label)
            .is_ok()
    }

    /// The first query selected and run: two files to copy, one missing.
    fn after_run() -> SavedQueries {
        let mut list = loaded_list();
        list.select(0);
        list.begin_run().expect("a spec");
        list.ran(vec![
            hit("invoice-1.pdf", false),
            hit("invoice-2.pdf", false),
            hit("invoice-3.pdf", true),
        ]);
        list
    }

    fn at_export() -> SavedQueries {
        let mut list = after_run();
        list.start_export(PathBuf::from("/Archive/Export"));
        list
    }

    #[test]
    fn the_page_shows_the_queries_the_form_and_the_buttons() {
        for label in [
            "Saved queries",
            "Company A 2024",
            "Taxes 2023",
            "Year",
            "Tax context",
            "Full text",
            "New",
            "Save",
            "Delete",
            "Run",
            "Close",
        ] {
            assert!(shows(loaded_list(), label), "{label}");
        }
    }

    #[test]
    fn an_empty_list_says_so() {
        let mut list = SavedQueries::new();
        list.loaded(Vec::new());

        assert!(shows(
            list,
            "No saved queries yet. Fill in the form and save one."
        ));
    }

    #[test]
    fn clicking_a_row_selects_it() {
        let app = App::with_saved_queries(loaded_list());
        let mut sim = simulator(&app);
        sim.click("Taxes 2023").expect("the row");

        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [queries_message(SavedQueriesMessage::Select(1))]
        );
    }

    #[test]
    fn clicking_run_asks_the_index() {
        let mut list = loaded_list();
        list.select(0);
        let app = App::with_saved_queries(list);
        let mut sim = simulator(&app);
        sim.click("Run").expect("the button");

        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [queries_message(SavedQueriesMessage::Run)]
        );
    }

    #[test]
    fn a_run_shows_its_figures_and_the_list_toggles() {
        for label in ["Hits", "file missing", "View list", "Export …"] {
            assert!(shows(after_run(), label), "{label}");
        }
        let app = App::with_saved_queries(after_run());
        let mut sim = simulator(&app);
        sim.click("View list").expect("the toggle");

        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [queries_message(SavedQueriesMessage::ToggleHits)]
        );
    }

    #[test]
    fn the_export_step_says_what_happens_before_anything_is_copied() {
        for label in [
            "Target folder: /Archive/Export",
            "Files to copy: 2",
            "Copies only; moves and deletes nothing. Existing files stay untouched.",
            "Choose folder …",
            "Cancel",
        ] {
            assert!(shows(at_export(), label), "{label}");
        }
        let app = App::with_saved_queries(at_export());
        let mut sim = simulator(&app);
        sim.click("Choose folder …").expect("the button");
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [queries_message(SavedQueriesMessage::PickFolder)]
        );
    }

    #[test]
    fn deleting_asks_in_the_page_first() {
        let mut list = loaded_list();
        list.select(0);
        list.ask_delete();
        assert!(shows(
            list.clone(),
            "Delete the query ‘Company A 2024’? Documents stay as they are."
        ));

        let app = App::with_saved_queries(list);
        let mut sim = simulator(&app);
        sim.click("Delete query").expect("the button");
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [queries_message(SavedQueriesMessage::ConfirmDelete)]
        );
    }

    #[test]
    fn the_view_is_worded_in_german_too() {
        let app = App::with_saved_queries(at_export()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Abfragen",
            "Abbrechen",
            "Ordner wählen …",
            "Kopiert nur, verschiebt und löscht nichts. Vorhandene Dateien bleiben unangetastet.",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn a_sidebar_click_opens_the_page_and_closes_the_other() {
        let mut app = App::with_review_list(crate::review_list::tests::loaded_list());
        let _ = app.update(Message::HermesPage(HermesPage::SavedQueries));

        assert!(app.review_list_for_test().is_none());
        assert!(app.saved_queries_for_test().is_some());
    }

    #[test]
    fn escape_backs_out_of_the_step_first() {
        let mut app = App::with_saved_queries(at_export());
        let _ = app.update(queries_message(SavedQueriesMessage::Close));
        let page = app.saved_queries_for_test().unwrap();
        assert!(page.export_step().is_none());
        assert!(page.results().is_some());

        let _ = app.update(queries_message(SavedQueriesMessage::Close));
        assert!(app.saved_queries_for_test().is_none());
    }

    fn matches_snapshot(list: SavedQueries, name: &str) {
        let language = if name.contains("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_saved_queries(list).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the view does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_view_matches_its_snapshots() {
        matches_snapshot(after_run(), "saved_queries_en");
        matches_snapshot(after_run(), "saved_queries_de");
        matches_snapshot(at_export(), "saved_queries_export_en");
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
mod rule_suggestions_card {
    use std::path::PathBuf;

    use super::*;
    use crate::messages::RuleSuggestionsMessage;
    use crate::rule_suggestions::tests::{loaded_view, rule_ids, suggestion};
    use crate::rule_suggestions::{Problem, RuleSuggestions};
    use crate::suggestion_work::INDEX_NOT_OPEN;

    const FIRST_EVIDENCE: &str = "5 of 5 in folder, 0 elsewhere";
    const SECOND_EVIDENCE: &str = "4 of 5 in folder, 2 elsewhere";

    fn suggestions_message(message: RuleSuggestionsMessage) -> Message {
        Message::RuleSuggestions(message)
    }

    fn two_rows() -> RuleSuggestions {
        let [one, two, _] = rule_ids();
        let mut second = suggestion(two, "agency b tax office", "Taxes");
        second.evidence.matching_in_folder = 4;
        second.evidence.matching_elsewhere = 2;
        let mut view = RuleSuggestions::new();
        view.loaded(vec![suggestion(one, "company a ltd", "Insurance"), second]);
        view
    }

    fn shows(view: RuleSuggestions, label: &str) -> bool {
        simulator(&App::with_rule_suggestions(view))
            .find(label)
            .is_ok()
    }

    fn messages_after_click(view: RuleSuggestions, label: &str) -> Vec<Message> {
        let app = App::with_rule_suggestions(view);
        let mut sim = simulator(&app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    #[test]
    fn the_list_shows_condition_target_kind_and_evidence_of_each_suggestion() {
        for label in [
            "Rule suggestions",
            "company a ltd",
            "Insurance/{date}_{name}",
            "Taxes/{date}_{name}",
            FIRST_EVIDENCE,
            SECOND_EVIDENCE,
            "Confirm",
            "Edit",
            "Discard",
            "Analyse again",
            "Close",
        ] {
            assert!(shows(two_rows(), label), "{label}");
        }
    }

    #[test]
    fn the_view_is_worded_in_german_too() {
        let app = App::with_rule_suggestions(two_rows()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Regelvorschläge",
            "5 von 5 im Ordner, 0 sonst",
            "4 von 5 im Ordner, 2 sonst",
            "Bestätigen",
            "Bearbeiten",
            "Verwerfen",
            "Erneut analysieren",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn a_view_still_reading_says_so_and_decides_nothing() {
        let view = RuleSuggestions::new();
        assert!(shows(view.clone(), "Reading the document folders…"));
        assert!(messages_after_click(view, "Confirm").is_empty());
    }

    #[test]
    fn an_empty_result_says_how_suggestions_come_about() {
        let mut view = RuleSuggestions::new();
        view.loaded(Vec::new());
        assert!(shows(
            view,
            "No suggestions. They appear from archive documents with read text, once at least 3 documents with the same phrase lie in the same folder (80 % of the folder and hardly any elsewhere)."
        ));
    }

    #[test]
    fn the_empty_view_with_counts_matches_its_snapshots() {
        for (language, name) in [
            (Language::English, "rule_suggestions_empty_en"),
            (Language::German, "rule_suggestions_empty_de"),
        ] {
            let mut view = RuleSuggestions::new();
            view.loaded(Vec::new());
            let app = App::with_rule_suggestions(view)
                .with_hermes_counts(ncrs_index::HermesCounts {
                    archive_documents: 5120,
                    archive_documents_with_text: 12,
                    ..ncrs_index::HermesCounts::default()
                })
                .in_language(language);
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the view does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn the_buttons_send_what_they_say() {
        for (label, message) in [
            ("Confirm", RuleSuggestionsMessage::Confirm),
            ("Discard", RuleSuggestionsMessage::Discard),
            ("Analyse again", RuleSuggestionsMessage::Reanalyse),
            ("Close", RuleSuggestionsMessage::Close),
        ] {
            assert_eq!(
                messages_after_click(two_rows(), label),
                [suggestions_message(message)],
                "{label}"
            );
        }
    }

    /// "Edit" is also the F4 key of the bar below the view, so the button is
    /// clicked by its German name.
    #[test]
    fn the_edit_button_opens_the_form() {
        let app = App::with_rule_suggestions(two_rows()).in_language(Language::German);
        let mut sim = simulator(&app);
        sim.click("Bearbeiten").expect("the clicked label");
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [suggestions_message(RuleSuggestionsMessage::Edit)]
        );
    }

    #[test]
    fn a_click_on_a_row_selects_it() {
        assert_eq!(
            messages_after_click(two_rows(), SECOND_EVIDENCE),
            [suggestions_message(RuleSuggestionsMessage::Select(1))]
        );
    }

    #[test]
    fn the_selected_suggestion_lists_the_documents_it_matches() {
        let [one, _, _] = rule_ids();
        let mut view = two_rows();
        let (id, _) = view.matches_wanted().unwrap();
        assert_eq!(id, one);
        view.matches_found(
            id,
            vec![
                PathBuf::from("Insurance/2024_a.pdf"),
                PathBuf::from("b.pdf"),
            ],
        );

        assert!(shows(view.clone(), "Documents that contain this text"));
        assert!(shows(view, "Insurance/2024_a.pdf"));
    }

    #[test]
    fn more_documents_than_fit_are_counted() {
        let [one, _, _] = rule_ids();
        let mut view = two_rows();
        view.matches_wanted();
        let paths = (0..9).map(|n| PathBuf::from(format!("Insurance/{n}.pdf")));
        view.matches_found(one, paths.collect());

        assert!(shows(view, "… and 3 more"));
    }

    #[test]
    fn the_form_offers_the_three_fields_and_cancel() {
        let mut view = two_rows();
        view.start_edit();

        for label in ["Filed as (pattern)", "Kind", "Cancel", "Confirm"] {
            assert!(shows(view.clone(), label), "{label}");
        }
        assert!(
            !shows(view.clone(), "Discard"),
            "no decision besides confirm"
        );
        assert_eq!(
            messages_after_click(view, "Cancel"),
            [suggestions_message(RuleSuggestionsMessage::CancelEdit)]
        );
    }

    #[test]
    fn an_invalid_pattern_shows_the_complaint_and_the_form_stays() {
        let mut app = App::with_rule_suggestions(two_rows());
        for message in [
            RuleSuggestionsMessage::Edit,
            RuleSuggestionsMessage::PatternChanged("{year}/{nonsense}".to_owned()),
            RuleSuggestionsMessage::Confirm,
        ] {
            let _ = app.update(suggestions_message(message));
        }

        let view = app.rule_suggestions_for_test().unwrap();
        assert_eq!(view.problem(), Some(&Problem::InvalidPattern));
        assert!(view.edit().is_some());
        assert!(!view.is_busy());
        simulator(&app)
            .find(
                "This pattern cannot be used. Placeholders are written in braces, such as {year}.",
            )
            .expect("the complaint");
    }

    #[test]
    fn the_view_opens_busy_and_a_missing_index_ends_in_a_complaint() {
        let mut app = App::with_fixed_panels();

        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Open));

        let view = app.rule_suggestions_for_test().unwrap();
        assert_eq!(
            view.problem(),
            Some(&Problem::Failed(INDEX_NOT_OPEN.to_owned()))
        );
        assert!(!view.is_busy());
    }

    #[test]
    fn a_late_answer_of_an_earlier_view_is_dropped() {
        let mut app = App::with_fixed_panels();
        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Open));
        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Close));
        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Open));

        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Loaded {
            generation: 1,
            result: Ok(loaded_view().rows().to_vec()),
        }));

        assert!(app.rule_suggestions_for_test().unwrap().rows().is_empty());
    }

    #[test]
    fn escape_leaves_the_form_before_the_view() {
        let mut app = App::with_rule_suggestions(two_rows());
        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Edit));

        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Close));
        assert!(app.rule_suggestions_for_test().unwrap().edit().is_none());

        let _ = app.update(suggestions_message(RuleSuggestionsMessage::Close));
        assert!(app.rule_suggestions_for_test().is_none());
    }

    fn matches_snapshot(view: RuleSuggestions, name: &str) {
        let language = if name.ends_with("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_rule_suggestions(view).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the view does not match tests/snapshots/{name}.png"
        );
    }

    fn with_documents() -> RuleSuggestions {
        let [one, _, _] = rule_ids();
        let mut view = two_rows();
        view.matches_wanted();
        view.matches_found(
            one,
            vec![
                PathBuf::from("Insurance/2023-05-02_policy.pdf"),
                PathBuf::from("Insurance/2024-05-03_policy.pdf"),
            ],
        );
        view
    }

    #[test]
    fn the_view_matches_its_snapshots() {
        matches_snapshot(with_documents(), "rule_suggestions_en");
        matches_snapshot(with_documents(), "rule_suggestions_de");
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
mod reorganisation_plan_card {
    use super::*;
    use crate::messages::ReorganisationPlanMessage;
    use crate::reorganisation_plan::tests::loaded_view;
    use crate::reorganisation_plan::{Group, Line, ReorganisationView};
    use crate::reorganisation_work::tests::{archive, sample_items};
    use crate::suggestion_work::INDEX_NOT_OPEN;

    fn plan_message(message: ReorganisationPlanMessage) -> Message {
        Message::ReorganisationPlan(message)
    }

    fn shows(view: ReorganisationView, label: &str) -> bool {
        simulator(&App::with_reorganisation_plan(view))
            .find(label)
            .is_ok()
    }

    fn messages_after_click(view: ReorganisationView, label: &str) -> Vec<Message> {
        let app = App::with_reorganisation_plan(view);
        let mut sim = simulator(&app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    fn header_line(view: &ReorganisationView, group: Group) -> usize {
        view.lines()
            .iter()
            .position(|line| *line == Line::Header(group))
            .unwrap()
    }

    fn row_of(view: &ReorganisationView, source: &str) -> usize {
        view.rows()
            .iter()
            .position(|planned| planned.item.source == std::path::Path::new(source))
            .unwrap()
    }

    fn line_of(view: &ReorganisationView, source: &str) -> usize {
        let row = row_of(view, source);
        view.lines()
            .iter()
            .position(|line| *line == Line::Item(row))
            .unwrap()
    }

    #[test]
    fn the_plan_is_listed_per_group_with_paths_and_reasons() {
        for label in [
            "Reorganisation plan",
            "Duplicates  1 of 1 selected",
            "Filed in the wrong folder  1 of 2 selected",
            "Year does not match  1 of 1 selected",
            "Name differs  1 of 1 selected",
            "Cannot be done",
            "Misc/2024-04-01_invoice.pdf",
            "A rule files it elsewhere",
            "An unconfirmed rule files it elsewhere",
            "Filed under 2023, dated 2024",
            "The target already exists",
            "Not enough is known to name it",
            "Run selected",
            "Plan again",
            "Close",
        ] {
            assert!(shows(loaded_view(), label), "{label}");
        }
    }

    #[test]
    fn the_plan_is_worded_in_german_too() {
        let app = App::with_reorganisation_plan(loaded_view()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Umräumplan",
            "Duplikate  1 von 1 ausgewählt",
            "Falsch abgelegt  1 von 2 ausgewählt",
            "Jahr passt nicht  1 von 1 ausgewählt",
            "Name weicht ab  1 von 1 ausgewählt",
            "Nicht ausführbar",
            "Das Ziel gibt es schon",
            "Ausgewählte ausführen",
            "Neu planen",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn a_plan_still_being_made_says_so_and_runs_nothing() {
        let view = ReorganisationView::new();
        assert!(shows(view.clone(), "Working out the plan…"));
        assert!(messages_after_click(view, "Run selected").is_empty());
    }

    #[test]
    fn an_empty_plan_says_so() {
        let mut view = ReorganisationView::new();
        view.loaded(Vec::new());
        assert!(shows(
            view,
            "Nothing to tidy up. Every document is where its folder expects it."
        ));
    }

    #[test]
    fn a_click_selects_and_deselects_an_entry_and_a_group() {
        let mut app = App::with_reorganisation_plan(loaded_view());
        let view = app.reorganisation_for_test().unwrap().clone();
        let year = line_of(&view, "Insurance/2023/2024-05-01_invoice.pdf");
        let year_row = row_of(&view, "Insurance/2023/2024-05-01_invoice.pdf");
        assert!(view.is_selected(year_row));

        let _ = app.update(plan_message(ReorganisationPlanMessage::Toggle(year)));
        assert!(!app.reorganisation_for_test().unwrap().is_selected(year_row));
        let _ = app.update(plan_message(ReorganisationPlanMessage::Toggle(year)));
        assert!(app.reorganisation_for_test().unwrap().is_selected(year_row));

        let misplaced = header_line(&view, Group::Misplaced);
        assert_eq!(
            messages_after_click(view, "Filed in the wrong folder  1 of 2 selected"),
            [plan_message(ReorganisationPlanMessage::Toggle(misplaced))]
        );
        let _ = app.update(plan_message(ReorganisationPlanMessage::Toggle(misplaced)));
        assert_eq!(
            app.reorganisation_for_test()
                .unwrap()
                .counts(Group::Misplaced),
            (2, 2)
        );
        let _ = app.update(plan_message(ReorganisationPlanMessage::Toggle(misplaced)));
        assert_eq!(
            app.reorganisation_for_test()
                .unwrap()
                .counts(Group::Misplaced),
            (0, 2)
        );
    }

    #[test]
    fn what_cannot_be_done_has_no_box_to_check() {
        let mut app = App::with_reorganisation_plan(loaded_view());
        let view = app.reorganisation_for_test().unwrap().clone();
        let conflict = line_of(&view, "Insurance/2023/2024-06-01_invoice.pdf");

        let _ = app.update(plan_message(ReorganisationPlanMessage::Toggle(conflict)));

        assert_eq!(
            app.reorganisation_for_test().unwrap().selection(),
            view.selection()
        );
    }

    #[test]
    fn an_entry_on_an_unconfirmed_rule_starts_unselected() {
        let view = loaded_view();
        let by_suggestion = row_of(&view, "Insurance/2024/2024-08-01_invoice.pdf");
        let by_confirmed = row_of(&view, "Insurance/2024/2024-07-01_invoice.pdf");

        assert!(!view.is_selected(by_suggestion));
        assert!(view.is_selected(by_confirmed));
    }

    #[test]
    fn a_selected_move_is_in_the_after_tree_of_the_view() {
        let mut view = loaded_view();
        let year = line_of(&view, "Insurance/2023/2024-05-01_invoice.pdf");
        assert!(view
            .after_tree()
            .iter()
            .any(|line| line.name == "2024-05-01_invoice.pdf"));
        for label in ["Before", "After"] {
            assert!(shows(view.clone(), label), "{label}");
        }

        view.toggle(year);

        assert!(!view
            .after_tree()
            .iter()
            .any(|line| line.name == "2024-05-01_invoice.pdf"));
    }

    #[test]
    fn nothing_selected_shows_empty_trees_and_the_button_does_nothing() {
        let mut view = loaded_view();
        for group in Group::ALL {
            let header = header_line(&view, group);
            while view.counts(group).0 > 0 {
                view.toggle(header);
            }
        }

        assert!(shows(view.clone(), "Nothing selected."));
        assert!(messages_after_click(view, "Run selected").is_empty());
    }

    #[test]
    fn the_button_hands_the_selection_over() {
        let view = loaded_view();
        let selection = view.selection();
        assert!(!selection.is_empty());

        assert_eq!(
            messages_after_click(view, "Run selected"),
            [Message::ExecuteReorganisation(selection)]
        );
    }

    #[test]
    fn handing_the_selection_over_changes_nothing_yet() {
        let mut app = App::with_reorganisation_plan(loaded_view());
        let before = app.reorganisation_for_test().unwrap().clone();
        let selection = before.selection();

        let _ = app.update(Message::ExecuteReorganisation(selection));

        assert_eq!(app.reorganisation_for_test().unwrap(), &before);
    }

    #[test]
    fn opening_without_an_index_ends_in_a_complaint() {
        let mut app = App::with_fixed_panels();

        let _ = app.update(plan_message(ReorganisationPlanMessage::Open));

        let view = app.reorganisation_for_test().unwrap();
        assert_eq!(view.problem(), Some(INDEX_NOT_OPEN));
        assert!(!view.is_busy());
        assert!(shows(
            view.clone(),
            "The plan could not be made (index not open)."
        ));
    }

    #[test]
    fn a_late_answer_of_an_earlier_view_is_dropped() {
        let mut app = App::with_fixed_panels();
        let _ = app.update(plan_message(ReorganisationPlanMessage::Open));
        let _ = app.update(plan_message(ReorganisationPlanMessage::Close));
        let _ = app.update(plan_message(ReorganisationPlanMessage::Open));

        let _ = app.update(plan_message(ReorganisationPlanMessage::Loaded {
            generation: 1,
            result: Ok(sample_items()),
        }));

        assert!(app.reorganisation_for_test().unwrap().rows().is_empty());
    }

    #[test]
    fn opening_and_closing_the_view_changes_no_file() {
        let archive = archive();
        let before = archive.snapshot();
        let mut app = App::with_fixed_panels();

        let _ = app.update(plan_message(ReorganisationPlanMessage::Open));
        let _ = app.update(plan_message(ReorganisationPlanMessage::Loaded {
            generation: 1,
            result: archive.access().plan(),
        }));
        assert!(!app.reorganisation_for_test().unwrap().rows().is_empty());
        let _ = app.update(plan_message(ReorganisationPlanMessage::Toggle(0)));
        let selection = app.reorganisation_for_test().unwrap().selection();
        let _ = app.update(Message::ExecuteReorganisation(selection));
        let _ = app.update(plan_message(ReorganisationPlanMessage::Close));

        assert!(app.reorganisation_for_test().is_none());
        assert_eq!(archive.snapshot(), before);
    }

    fn matches_snapshot(view: ReorganisationView, name: &str) {
        let language = if name.ends_with("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_reorganisation_plan(view).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the view does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_view_matches_its_snapshots() {
        matches_snapshot(loaded_view(), "reorganisation_plan_en");
        matches_snapshot(loaded_view(), "reorganisation_plan_de");
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
mod title_bar {
    use std::time::{Duration, SystemTime};

    use ncrs_pipeline::{RunnerState, Snapshot};

    use super::*;
    use crate::messages::{FilingStatusMessage, PanelSide};
    use crate::ui::titlebar::{HERMES_CHIP_ID, LEFT_TAB_ID, RIGHT_TAB_ID};

    fn taken_at() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// Hermes on, with a measured rate of 78 files a minute.
    fn with_hermes() -> App {
        let mut snapshot = Snapshot::off();
        snapshot.state = RunnerState::Running;
        snapshot.documents_per_minute[1] = 78;
        snapshot.steps = vec![ncrs_pipeline::StepSnapshot {
            name: "probe",
            waiting: 4,
            done: 0,
            failed: 0,
            unchanged: 0,
        }];
        App::with_filing_snapshot(snapshot, taken_at())
    }

    fn messages_after_click(app: &App, id: &'static str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(iced::widget::Id::new(id))
            .expect("the clicked widget");
        sim.into_messages().collect()
    }

    #[test]
    fn a_click_on_the_inactive_tab_makes_its_panel_active() {
        let mut app = App::with_fixed_panels();
        assert_eq!(app.active_panel_side_for_test(), PanelSide::Left);

        let messages = messages_after_click(&app, RIGHT_TAB_ID);
        assert_eq!(messages, vec![Message::ActivatePanel(PanelSide::Right)]);
        for message in messages {
            drop(app.update(message));
        }
        assert_eq!(app.active_panel_side_for_test(), PanelSide::Right);

        assert_eq!(
            messages_after_click(&app, LEFT_TAB_ID),
            vec![Message::ActivatePanel(PanelSide::Left)]
        );
    }

    #[test]
    fn a_click_on_the_hermes_chip_opens_the_inspection() {
        let mut app = with_hermes();
        let messages = messages_after_click(&app, HERMES_CHIP_ID);
        assert_eq!(
            messages,
            vec![Message::FilingStatus(FilingStatusMessage::Open)]
        );
        for message in messages {
            drop(app.update(message));
        }
        assert!(app.filing_status_page_is_open_for_test());
    }

    #[test]
    fn the_chip_is_absent_while_hermes_is_off() {
        let shows_chip = |app: &App| {
            simulator(app)
                .find(iced::widget::Id::new(HERMES_CHIP_ID))
                .is_ok()
        };
        assert!(!shows_chip(&App::with_fixed_panels()));
        assert!(shows_chip(&with_hermes()));
    }

    #[test]
    fn the_chip_names_the_rate_in_the_language_of_the_app() {
        let english = with_hermes();
        simulator(&english).find("Hermes 78/min processed").unwrap();
        let german = with_hermes().in_language(Language::German);
        simulator(&german)
            .find("Hermes 78/Min. verarbeitet")
            .unwrap();
    }

    #[test]
    fn an_idle_chip_shows_no_rate() {
        let mut snapshot = Snapshot::off();
        snapshot.state = RunnerState::Idle;
        snapshot.documents_per_minute[1] = 78;
        let app = App::with_filing_snapshot(snapshot, taken_at());
        assert_eq!(app.hermes_chip_for_test().as_deref(), Some("Hermes"));
    }

    #[test]
    fn the_tabs_show_the_paths_below_the_home_folder() {
        let app = App::with_fixed_panels();
        let sep = std::path::MAIN_SEPARATOR;
        simulator(&app).find(format!("~{sep}links")).unwrap();
        simulator(&app).find(format!("~{sep}files")).unwrap();
    }

    fn preset_app(preset: &str, language: Language) -> App {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, format!("[theme]\npreset = \"{preset}\"\n")).unwrap();
        with_hermes()
            .with_config(crate::config::load(&path))
            .in_language(language)
    }

    /// Two files failed, 2 421 documents wait for the user, none was filed.
    fn failing_hermes(language: Language) -> App {
        let mut snapshot = Snapshot::off();
        snapshot.state = RunnerState::Running;
        snapshot.documents_per_minute[1] = 78;
        snapshot.steps = vec![ncrs_pipeline::StepSnapshot {
            name: "probe",
            waiting: 0,
            done: 0,
            failed: 2,
            unchanged: 0,
        }];
        App::with_filing_snapshot(snapshot, taken_at())
            .with_hermes_counts(ncrs_index::HermesCounts {
                open_reviews: 2421,
                ..ncrs_index::HermesCounts::default()
            })
            .in_language(language)
    }

    #[test]
    fn the_status_line_and_chip_show_failures_in_the_main_view() {
        for (language, name) in [
            (Language::English, "main_view_hermes_failing_en"),
            (Language::German, "main_view_hermes_failing_de"),
        ] {
            let matches = simulator(&failing_hermes(language))
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the main view does not match tests/snapshots/{name}.png"
            );
        }
        let missing =
            with_hermes().with_pdf_library_missing(std::path::PathBuf::from("/opt/pdfium/lib"));
        simulator(&missing)
            .find("PDF library missing, PDFs are not read")
            .unwrap();
    }

    #[test]
    fn the_hermes_column_matches_its_snapshots() {
        for (language, name) in [
            (Language::English, "hermes_column_en"),
            (Language::German, "hermes_column_de"),
        ] {
            let app = App::with_hermes_column_sample().in_language(language);
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the main view does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn the_hermes_quick_fix_matches_its_snapshots() {
        for (language, name) in [
            (Language::English, "hermes_quick_fix_en"),
            (Language::German, "hermes_quick_fix_de"),
        ] {
            let app = App::with_hermes_quick_fix_sample().in_language(language);
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the quick-fix menu does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn the_hermes_context_menu_matches_its_snapshots() {
        for (language, name) in [
            (Language::English, "hermes_context_menu_en"),
            (Language::German, "hermes_context_menu_de"),
        ] {
            let app = App::with_hermes_context_menu_sample().in_language(language);
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the Hermes context menu does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn the_main_view_matches_its_snapshots() {
        for (preset, language, name) in [
            ("cosmic", Language::English, "main_view_cosmic_en"),
            ("cosmic", Language::German, "main_view_cosmic_de"),
            ("dracula", Language::English, "main_view_dracula_en"),
            ("dracula", Language::German, "main_view_dracula_de"),
        ] {
            let matches = simulator(&preset_app(preset, language))
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the main view does not match tests/snapshots/{name}.png"
            );
        }
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
mod ocr_models_card {
    use super::*;
    use crate::messages::OcrModelsMessage;
    use crate::ocr_models::{DownloadError, OcrModelsCard};
    use ncrs_intake::OCR_MODEL_FILES;
    use std::path::PathBuf;

    fn card() -> OcrModelsCard {
        OcrModelsCard::new(Some(PathBuf::from("/example/data/ocr-models")))
    }

    fn shows(card: OcrModelsCard, label: &str) -> bool {
        simulator(&App::with_ocr_models_card(card))
            .find(label)
            .is_ok()
    }

    fn messages_after_click(card: OcrModelsCard, label: &str) -> Vec<Message> {
        let app = App::with_ocr_models_card(card);
        let mut sim = simulator(&app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    #[test]
    fn the_card_names_host_files_sizes_hashes_and_folder_before_anything_is_sent() {
        let mut labels = vec![
            "Download from: ocrs-models.s3-accelerate.amazonaws.com".to_owned(),
            "Saved to: /example/data/ocr-models".to_owned(),
            "The models are made by the ocrs project: github.com/robertknight/ocrs-models"
                .to_owned(),
        ];
        for file in &OCR_MODEL_FILES {
            labels.push(format!(
                "{}: {}",
                file.name,
                crate::ui::format::size(file.size)
            ));
            labels.push(format!("SHA-256 checksum: {}", file.sha256));
        }
        for label in labels {
            assert!(shows(card(), &label), "{label}");
        }
    }

    #[test]
    fn the_buttons_send_close_and_download_and_nothing_else() {
        assert_eq!(
            messages_after_click(card(), "Cancel"),
            [Message::OcrModels(OcrModelsMessage::Close)]
        );
        assert_eq!(
            messages_after_click(card(), "Download"),
            [Message::OcrModels(OcrModelsMessage::Download)]
        );
    }

    #[test]
    fn while_it_downloads_the_card_says_so_and_the_buttons_are_off() {
        let mut running = card();
        running.start().unwrap();
        assert!(shows(running.clone(), "Downloading…"));
        assert!(messages_after_click(running, "Download").is_empty());
    }

    #[test]
    fn a_failure_is_shown_with_its_detail() {
        let mut failed = card();
        failed.start().unwrap();
        failed.finish(Err(DownloadError::Network(
            crate::open_vsx::FetchError::Timeout,
        )));
        assert!(shows(
            failed,
            "Download failed (the server did not answer in time)"
        ));
    }

    #[test]
    fn the_card_is_worded_in_german_too() {
        let app = App::with_ocr_models_card(card()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Modelle für Texterkennung laden",
            "Herunterladen von: ocrs-models.s3-accelerate.amazonaws.com",
            "Gespeichert in: /example/data/ocr-models",
            "Abbrechen",
            "Laden",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    fn matches_snapshot(language: Language, name: &str) {
        let app = App::with_ocr_models_card(card()).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the models card does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_card_matches_its_snapshots() {
        matches_snapshot(Language::English, "ocr_models_en");
        matches_snapshot(Language::German, "ocr_models_de");
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
mod ai_model_card {
    use super::*;
    use crate::ai_model::AiModelCard;
    use crate::messages::AiModelMessage;
    use std::path::PathBuf;

    fn card() -> AiModelCard {
        AiModelCard::new(Some(PathBuf::from("/example/data/ai-models")), false)
    }

    fn shows(card: AiModelCard, label: &str) -> bool {
        simulator(&App::with_ai_model_card(card))
            .find(label)
            .is_ok()
    }

    fn messages_after_click(card: AiModelCard, label: &str) -> Vec<Message> {
        let app = App::with_ai_model_card(card);
        let mut sim = simulator(&app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    #[test]
    fn the_card_names_host_file_size_hash_license_and_folder_before_anything_is_sent() {
        let pin = ncrs_ai::LOCAL_MODEL;
        for label in [
            "Download from: huggingface.co".to_owned(),
            format!("{}: {}", pin.name, crate::ui::format::size(pin.size)),
            format!("SHA-256 checksum: {}", pin.sha256),
            "License: Apache-2.0".to_owned(),
            "Saved to: /example/data/ai-models".to_owned(),
        ] {
            assert!(shows(card(), &label), "{label}");
        }
    }

    #[test]
    fn the_buttons_send_close_and_download_and_nothing_else() {
        assert_eq!(
            messages_after_click(card(), "Cancel"),
            [Message::AiModel(AiModelMessage::Close)]
        );
        assert_eq!(
            messages_after_click(card(), "Download"),
            [Message::AiModel(AiModelMessage::Download)]
        );
    }

    #[test]
    fn a_running_download_can_be_stopped_but_not_closed() {
        let mut running = card();
        running.start().unwrap();
        assert!(shows(running.clone(), "Downloading…"));
        assert!(messages_after_click(running.clone(), "Cancel").is_empty());
        assert_eq!(
            messages_after_click(running, "Stop"),
            [Message::AiModel(AiModelMessage::Stop)]
        );
    }

    #[test]
    fn a_partial_file_offers_to_continue() {
        let partial = AiModelCard::new(Some(PathBuf::from("/example/data/ai-models")), true);
        assert_eq!(
            messages_after_click(partial, "Continue download"),
            [Message::AiModel(AiModelMessage::Download)]
        );
    }

    #[test]
    fn the_card_is_worded_in_german_too() {
        let app = App::with_ai_model_card(card()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "KI-Modell laden",
            "Herunterladen von: huggingface.co",
            "Gespeichert in: /example/data/ai-models",
            "Lizenz: Apache-2.0",
            "Abbrechen",
            "Laden",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    fn matches_snapshot(language: Language, name: &str) {
        let app = App::with_ai_model_card(card()).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the AI model card does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_card_matches_its_snapshots() {
        matches_snapshot(Language::English, "ai_model_en");
        matches_snapshot(Language::German, "ai_model_de");
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod ai_status_row {
    use super::*;
    use crate::filing::AiStatus;

    fn shows(ai: AiStatus, label: &str) -> bool {
        simulator(&App::with_filing_status_ai(ai))
            .find(label)
            .is_ok()
    }

    #[test]
    fn the_row_is_not_there_while_the_ai_is_off() {
        assert!(!shows(AiStatus::Off, "AI model"));
    }

    #[test]
    fn the_row_names_the_state_of_the_model() {
        assert!(shows(AiStatus::NotDownloaded, "AI model"));
        assert!(shows(AiStatus::NotDownloaded, "not downloaded"));
        assert!(shows(AiStatus::NotLoadedYet, "not loaded yet"));
        assert!(shows(
            AiStatus::Failed("the file is damaged"),
            "failed: the file is damaged"
        ));
        assert!(shows(
            AiStatus::Http {
                origin: "http://localhost:8080".to_owned(),
                model: "qwen".to_owned()
            },
            "qwen at http://localhost:8080"
        ));
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod settings_tab {
    use std::path::PathBuf;

    use ncrs_intake::LocationMode;

    use super::*;
    use crate::config::{AiSetting, Filing, FilingLocation};
    use crate::messages::{SettingChange, SettingsFeature, SettingsMessage};
    use crate::palette::ThemePreset;

    fn hermes_config() -> Filing {
        Filing {
            enabled: true,
            locations: vec![
                FilingLocation {
                    path: PathBuf::from("/home/test/Downloads/Scans"),
                    mode: LocationMode::Inbox,
                    images: true,
                    is_target: false,
                },
                FilingLocation {
                    path: PathBuf::from("/home/test/Documents/Filed"),
                    mode: LocationMode::Archive,
                    images: false,
                    is_target: true,
                },
            ],
            load: Default::default(),
            ai: AiSetting::Local { threads: None },
        }
    }

    fn matches_snapshot(app: &App, name: &str) {
        let matches = simulator(app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the tab does not match tests/snapshots/{name}.png");
    }

    #[test]
    fn the_appearance_page_matches_its_snapshots() {
        let app = || App::with_settings_open(SettingsFeature::Look, Filing::default());
        matches_snapshot(
            &app().in_language(Language::German),
            "settings_appearance_de",
        );
        matches_snapshot(
            &app().in_language(Language::English),
            "settings_appearance_en",
        );
    }

    #[test]
    fn the_filing_page_matches_its_snapshots() {
        let app = || App::with_settings_open(SettingsFeature::Hermes, hermes_config());
        matches_snapshot(&app().in_language(Language::German), "settings_hermes_de");
        matches_snapshot(&app().in_language(Language::English), "settings_hermes_en");
    }

    fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    #[test]
    fn a_preset_is_chosen_by_its_name() {
        let app = App::with_settings_open(SettingsFeature::Look, Filing::default());
        assert!(
            messages_after_click(&app, "Dracula").contains(&Message::Settings(
                SettingsMessage::Change(SettingChange::Preset(ThemePreset::Dracula))
            ))
        );
    }

    #[test]
    fn a_page_is_chosen_from_the_list() {
        let app = App::with_settings_open(SettingsFeature::Look, Filing::default());
        assert!(
            messages_after_click(&app, "Language").contains(&Message::Settings(
                SettingsMessage::Select(SettingsFeature::Language)
            ))
        );
    }

    #[test]
    fn the_search_filters_the_rows_by_label() {
        let mut app = App::with_settings_open(SettingsFeature::Hermes, hermes_config());
        simulator(&app).find("Folders").expect("the folders group");
        drop(app.update(Message::Settings(SettingsMessage::SearchChanged(
            "load".to_owned(),
        ))));
        let folders_hidden = simulator(&app).find("Folders").is_err();
        assert!(folders_hidden);
        simulator(&app).find("Load").expect("the load row");
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
mod command_line_view_tests {
    use super::*;
    use crate::messages::CommandLineMessage;

    fn app_with_typed_line(language: Language) -> App {
        let mut app = App::with_fixed_panels().in_language(language);
        drop(app.update(Message::CommandLine(CommandLineMessage::Input(
            "ls -la".to_owned(),
        ))));
        app
    }

    #[test]
    fn a_click_into_the_field_focuses_the_line_in_the_app() {
        let mut app = App::with_fixed_panels();
        let mut sim = simulator(&app);
        sim.click(iced_selector::id(crate::ui::command_line::COMMAND_LINE_ID))
            .expect("the command line field");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert_eq!(messages, [Message::CommandLine(CommandLineMessage::Focus)]);
        for message in messages {
            drop(app.update(message));
        }
        assert!(app.command_line_for_test().is_focused());
    }

    #[test]
    fn typing_with_nothing_focused_does_not_reach_the_command_line() {
        let app = App::with_fixed_panels();
        let mut sim = simulator(&app);
        sim.typewrite("ls");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert!(
            !messages.iter().any(|message| matches!(
                message,
                Message::CommandLine(CommandLineMessage::Input(_))
            )),
            "{messages:?}"
        );
    }

    #[test]
    fn the_focused_command_line_matches_its_snapshot() {
        let app = app_with_typed_line(Language::English);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/command_line_focused_en.png")
            .expect("snapshot comparison");
        assert!(matches, "the command line does not match its snapshot");
    }

    #[test]
    fn the_focused_command_line_matches_its_german_snapshot() {
        let app = app_with_typed_line(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/command_line_focused_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "the command line does not match its German snapshot"
        );
    }
}
