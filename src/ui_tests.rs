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
use crate::messages::PanelMessage;

/// Which panel the pointer is over is bookkeeping, not something the user did.
fn is_action(message: &Message) -> bool {
    !matches!(
        message,
        Message::Panel(PanelMessage::PanelEntered(_))
            | Message::Panel(PanelMessage::PanelExited(_))
    )
}

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

/// Renders like the real window, whose default font is the app's. The Hermes
/// pages set weights on top of it, so their snapshots need the same family.
fn app_simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
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

/// A confirm button that repeats the title of its dialog: plain text would
/// find the title, which comes first.
fn button_repeating_the_title(
    label: &str,
) -> impl FnMut(iced_selector::Candidate<'_>) -> Option<iced_selector::Target> + Send + '_ {
    let mut title_passed = false;
    move |candidate| {
        if !matches!(&candidate, iced_selector::Candidate::Text { content, .. } if *content == label)
        {
            return None;
        }
        if !title_passed {
            title_passed = true;
            return None;
        }
        Some(iced_selector::Target::from(candidate))
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
    fn the_pack_prompt_matches_its_snapshots() {
        for (lang, name) in [
            (Language::English, "pack_prompt_en"),
            (Language::German, "pack_prompt_de"),
        ] {
            let app = App::with_fixed_pack_prompt().in_language(lang);
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(matches, "the pack prompt does not match {name}.png");
        }
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
    fn the_trash_view_matches_its_snapshot() {
        let app = App::with_fixed_trash();
        let mut sim = simulator(&app);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/trash.png")
            .expect("snapshot comparison");

        assert!(
            matches,
            "the trash view does not match tests/snapshots/trash.png"
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

    fn quick_open_with_documents(language: Language) -> App {
        use crate::filing::{FoundDocument, TextSource};
        let mut app = App::with_fixed_panels()
            .in_language(language)
            .with_ready_hermes_for_test();
        app.open_quick_open_for_test(
            &["README.md", "src/config.rs", "tests/config_rs_test.rs"],
            "",
        );
        app.type_into_quick_open_for_test("cfgrs");
        let found = |path: &str, read_by| FoundDocument {
            path: std::path::PathBuf::from(path),
            read_by,
        };
        app.deliver_documents_for_test(Ok(vec![
            found(
                "/Users/me/Documents/Insurance/policy-2024.pdf",
                TextSource::Pdf,
            ),
            found(
                "/Users/me/Documents/Scans/receipt-0412.jpg",
                TextSource::Ocr,
            ),
            found(
                "/Users/me/Documents/Notes/cfgrs-notes.txt",
                TextSource::Text,
            ),
        ]));
        app
    }

    #[test]
    fn quick_open_documents_match_their_snapshot() {
        let app = quick_open_with_documents(Language::English);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/quick_open_documents_en.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "quick open does not match tests/snapshots/quick_open_documents_en.png"
        );
    }

    #[test]
    fn quick_open_documents_match_their_german_snapshot() {
        let app = quick_open_with_documents(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/quick_open_documents_de.png")
            .expect("snapshot comparison");
        assert!(
            matches,
            "quick open does not match tests/snapshots/quick_open_documents_de.png"
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
            folder_size: None,
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
mod archive_panel {
    use super::*;

    #[test]
    fn a_move_inside_an_archive_says_the_archive_is_read_only() {
        for (lang, name) in [
            (Language::English, "archive_read_only_en"),
            (Language::German, "archive_read_only_de"),
        ] {
            let mut app = App::with_fixed_panels().in_language(lang);
            app.show_archive_read_only_for_test();
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(matches, "the panel does not match {name}.png");
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

    fn app_with_favourites() -> App {
        let mut app = App::with_fixed_panels();
        app.open_volume_menu_with_favourites_for_test(
            PanelSide::Right,
            vec![volume("Macintosh HD", "/"), volume("test", "/home/test")],
            &[
                ("/home/test/Documents", true),
                ("/Volumes/Gone/Photos", false),
            ],
            2,
        );
        app
    }

    #[test]
    fn the_menu_lists_favourites_below_the_drives() {
        let matches = simulator(&app_with_favourites())
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/volumes_with_favourites.png")
            .expect("snapshot comparison");
        assert!(matches, "does not match volumes_with_favourites.png");
    }

    #[test]
    fn the_favourites_match_their_german_snapshot() {
        let matches = simulator(&app_with_favourites().in_language(Language::German))
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/volumes_with_favourites_de.png")
            .expect("snapshot comparison");
        assert!(matches, "does not match volumes_with_favourites_de.png");
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
            [Message::Panel(PanelMessage::RowContext {
                side: PanelSide::Left,
                index: 1
            })]
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
                on_failure_press: None,
                blocked_by_sandbox: false,
                notice: app.notice_for_test(),
                hermes_line: None,
                version: None,
                monitor: None,
            },
            failure,
            None,
            crate::ui::statusbar::PathBar {
                editing: None,
                on_input: Message::PathFieldInput,
                on_submit: Message::PathFieldSubmit,
                on_copy: Message::PathFieldCancel,
                hit_line: None,
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
        sim.into_messages().filter(is_action).collect()
    }

    #[test]
    fn a_double_click_on_a_panel_row_activates_it() {
        let app = App::with_fixed_panels();
        let mut sim = simulator(&app);
        sim.click("/Documents").expect("a panel row");
        sim.click("/Documents").expect("a panel row");
        let messages: Vec<Message> = sim.into_messages().filter(is_action).collect();
        assert!(
            matches!(
                messages.as_slice(),
                [
                    Message::Panel(PanelMessage::RowClicked { .. }),
                    Message::Panel(PanelMessage::RowClicked { .. }),
                    Message::Panel(PanelMessage::RowActivated { .. })
                ]
            ),
            "{messages:?}"
        );
    }

    #[test]
    fn the_panels_stay_usable_while_a_card_is_up() {
        let without_card = messages_for_a_click_on_a_panel_row(&App::with_fixed_panels());
        assert!(
            matches!(
                without_card.as_slice(),
                [Message::Panel(PanelMessage::RowClicked { .. })]
            ),
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
    use super::app_simulator as simulator;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use ncrs_pipeline::{ErrorEntry, RunnerState, Snapshot, StepSnapshot, Task};

    use super::*;
    use crate::messages::FilingStatusMessage;

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

    fn two_errors() -> Vec<ErrorEntry> {
        [
            ("scan-0042.pdf", "no text could be read"),
            ("company-a-invoice.pdf", "the step stopped unexpectedly"),
        ]
        .into_iter()
        .enumerate()
        .map(|(sequence, (name, message))| ErrorEntry {
            file: PathBuf::from(format!("/tmp/inbox/{name}")),
            task: Task::Step("probe"),
            message: message.to_owned(),
            at: taken_at() - Duration::from_secs(60 * (2 - sequence as u64)),
            sequence: sequence as u64,
        })
        .collect()
    }

    fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().filter(is_action).collect()
    }

    fn page_message(message: FilingStatusMessage) -> Message {
        Message::FilingStatus(message)
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
            let app = page(snapshot(RunnerState::Idle, 0, 0))
                .in_language(language)
                .with_pdf_library_missing(long.clone());
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the page does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn the_read_errors_match_their_snapshots() {
        for (language, name, errors) in [
            (Language::English, "read_errors_en", two_errors()),
            (Language::German, "read_errors_de", two_errors()),
            (Language::English, "read_errors_none_en", Vec::new()),
            (Language::German, "read_errors_none_de", Vec::new()),
        ] {
            let mut with_errors = snapshot(RunnerState::Idle, 0, 0);
            with_errors.errors = errors;
            let app = page(with_errors).in_language(language);
            let matches = simulator(&app)
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(
                matches,
                "the page does not match tests/snapshots/{name}.png"
            );
        }
    }

    #[test]
    fn an_error_is_a_row_with_its_reason_and_two_buttons_in_the_row() {
        let mut with_error = snapshot(RunnerState::Idle, 0, 1);
        with_error.errors = two_errors();
        let app = page(with_error);

        assert!(shows(&app, "the step stopped unexpectedly"));
        assert!(!shows(&app, "No errors"));
        assert_eq!(
            messages_after_click(&app, "Show file"),
            [page_message(FilingStatusMessage::ErrorActivated(
                PathBuf::from("/tmp/inbox/company-a-invoice.pdf")
            ))]
        );
        assert_eq!(
            messages_after_click(&app, "Process again"),
            [page_message(FilingStatusMessage::ProcessOneAgain(
                PathBuf::from("/tmp/inbox/company-a-invoice.pdf")
            ))]
        );
    }

    #[test]
    fn without_errors_the_page_is_not_missing_but_says_so_in_a_row() {
        let app = page(snapshot(RunnerState::Idle, 0, 0));

        assert!(shows(&app, "No errors"));
        assert!(!shows(&app, "Show file"));
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
        let mut app = App::with_fixed_panels().in_language(Language::EnglishFuturama);
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
                "filing status".to_owned(),
            ))),
        );
        assert!(shows(&app, "F11"));
    }

    #[test]
    fn typing_in_the_field_filters_the_list() {
        let mut app = App::with_fixed_panels().in_language(Language::EnglishFuturama);
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
                ncrs_palette::command_list::Row::Hit(hit) => {
                    palette
                        .entry(hit)
                        .and_then(ncrs_palette::command_list::Entry::command)
                        == Some(crate::keymap::KeyAction::SwitchLanguage)
                }
                ncrs_palette::command_list::Row::Header(_) => false,
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
    fn the_hermes_group_is_marked_beta_in_both_languages() {
        for language in [Language::EnglishFuturama, Language::GermanFuturama] {
            let mut app = App::with_fixed_filing(Phase::Ready).in_language(language);
            open(&mut app);
            typed(&mut app, "hermes");
            assert!(shows(&app, "Beta"), "{language:?}");
        }
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
    fn the_empty_list_matches_its_futurama_snapshots() {
        for (language, name) in [
            (
                Language::EnglishFuturama,
                "command_palette_empty_en_futurama",
            ),
            (
                Language::GermanFuturama,
                "command_palette_empty_de_futurama",
            ),
        ] {
            let mut app = App::with_fixed_panels().in_language(language);
            open(&mut app);
            snapshot_of(&app, &format!("tests/snapshots/{name}.png"));
        }
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

    #[test]
    fn commands_and_settings_match_their_snapshot() {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        typed(&mut app, "language");
        snapshot_of(&app, "tests/snapshots/command_palette_settings.png");
    }

    #[test]
    fn commands_and_settings_match_their_german_snapshot() {
        let mut app = App::with_fixed_panels().in_language(Language::German);
        open(&mut app);
        typed(&mut app, "sprache");
        snapshot_of(&app, "tests/snapshots/command_palette_settings_de.png");
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
    use crate::theme_fetch::fixtures::fetched;
    use crate::theme_fetch::{Fetched, ThemeFetch};
    use crate::vsix::Brightness;

    const HELP: &str =
        "Downloads from Open VSX, or a theme file from the https link you give. Nothing goes out before you press Enter or Fetch.";

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
        sim.into_messages().filter(is_action).collect()
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
pub mod search_filters_card {
    use super::*;
    use crate::keymap::KeyAction;
    use crate::keymap::KeyActionMessage as _;
    use crate::messages::{QuickOpenMessage, SavedQueriesMessage, SearchMessage};
    use crate::query_work::QueryHit;
    use crate::search_filters::tests::found_hit;
    use crate::search_filters::{Facet, FacetValue, SearchFilters};
    use ncrs_index::{Amount, Currency};
    use std::path::Path;

    const MONTHS: [u32; 4] = [1, 2, 3, 4];

    fn invoices_below(root: &Path) -> Vec<QueryHit> {
        MONTHS
            .iter()
            .map(|month| {
                let mut hit = found_hit(
                    &format!("telekom-2026-{month:02}.pdf"),
                    Some((2026, *month, 5)),
                    Some("Telekom"),
                    Some("invoice"),
                    if *month == 4 { &["paid"] } else { &[] },
                );
                hit.path = root.join(format!("telekom-2026-{month:02}.pdf"));
                let file = crate::app::fixed_result_entry(
                    hit.path.clone(),
                    98_304 + u64::from(*month) * 2_048,
                    (2026, *month, 6, 8, 30),
                );
                hit.stamp = file.modified.map(|mtime| ncrs_index::FileStamp {
                    size: file.size,
                    mtime,
                });
                hit.fields.amount = Some(Amount {
                    minor_units: 10_320,
                    currency: Currency::new("EUR").unwrap(),
                });
                hit
            })
            .collect()
    }

    fn filters_for(root: &Path, words: &str) -> SearchFilters {
        let mut filters = SearchFilters::new(Default::default());
        filters.ask(words);
        let mut hits = invoices_below(root);
        let mut elsewhere = found_hit("other.pdf", Some((2025, 5, 1)), None, None, &[]);
        elsewhere.path = root.join("other.pdf");
        hits.push(elsewhere);
        filters.ran(hits);
        filters
    }

    /// The search over four invoices read, the kind and the year chosen.
    pub fn app_with_filters() -> App {
        let mut app = App::with_search_filters("telekom", |root| {
            let mut filters = filters_for(root, "telekom");
            filters.toggle(FacetValue::Kind("invoice".to_owned()));
            filters.toggle(FacetValue::Year(2026));
            filters.add_snippets(
                MONTHS
                    .iter()
                    .map(|month| {
                        (
                            root.join(format!("telekom-2026-{month:02}.pdf")),
                            format!("Mobile contract \u{2}Telekom\u{3} invoice {month}/2026"),
                        )
                    })
                    .collect(),
            );
            filters
        });
        let rows = app.search_rows_for_test();
        if let Some(view) = app.search_for_test_mut() {
            view.mirror_index(rows);
        }
        app
    }

    fn shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    #[test]
    fn the_filters_narrow_the_list_and_the_summary_counts_what_is_left() {
        let app = app_with_filters();

        let view = app.search_for_test().unwrap();
        assert_eq!(view.results.len(), 4);
        assert!(shows(&app, "Files: 4 · Lines: 4"));
        assert!(shows(&app, " · Sum 412.80 EUR"));
        assert!(shows(&app, "Save as report"));
        assert!(shows(
            &app,
            "Filters only apply to documents ncrs has read. Filters apply together (2026 and Invoice)."
        ));
    }

    #[test]
    fn a_search_without_an_index_has_no_filters() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));

        assert!(app.search_for_test().unwrap().filters.is_none());
        assert!(!shows(&app, "Save as report"));
    }

    #[test]
    fn choosing_and_clearing_a_value_switches_between_the_index_and_the_walk() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _inside = runtime.enter();
        let mut app = app_with_filters();

        drop(app.update(Message::Search(SearchMessage::ClearFacet(Facet::Year))));
        assert!(app.search_for_test().unwrap().is_filtered());
        drop(app.update(Message::Search(SearchMessage::ClearFacet(Facet::Kind))));
        assert!(!app.search_for_test().unwrap().is_filtered());

        drop(app.update(Message::Search(SearchMessage::ToggleFacet(
            FacetValue::Year(2025),
        ))));
        let view = app.search_for_test().unwrap();
        assert!(view.is_filtered());
        assert_eq!(view.results.len(), 1);
        assert!(view.results[0].path.ends_with("other.pdf"));
    }

    #[test]
    fn an_answer_of_an_older_question_is_dropped() {
        let mut app = app_with_filters();
        let root = app.search_for_test().unwrap().root.clone();
        let stale = app
            .search_for_test()
            .unwrap()
            .filters
            .as_ref()
            .unwrap()
            .generation()
            .wrapping_sub(1);

        drop(app.update(Message::Search(SearchMessage::IndexRan {
            generation: stale,
            result: Ok(vec![found_hit("x.pdf", None, None, None, &[])]),
        })));

        assert_eq!(app.search_for_test().unwrap().results.len(), 4);
        assert!(app
            .search_for_test()
            .unwrap()
            .results
            .iter()
            .all(|result| result.path.starts_with(&root)));
    }

    #[test]
    fn a_hit_outside_the_folder_or_the_name_filter_is_left_out() {
        let mut app = app_with_filters();
        let rows = app.search_rows_for_test();
        let view = app.search_for_test_mut().unwrap();
        view.include = "*-03.pdf".to_owned();

        view.mirror_index(rows);

        assert_eq!(view.results.len(), 1);
        view.include = String::new();
        view.exclude = "*-0[12].pdf".to_owned();
        view.mirror_index(rows);
        assert_eq!(view.results.len(), 2);
    }

    #[test]
    fn save_as_report_opens_the_reports_page_with_a_new_report_of_the_search() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _inside = runtime.enter();
        let mut app = app_with_filters().with_ready_hermes_for_test();

        drop(app.update(Message::Search(SearchMessage::SaveAsReport)));

        assert!(!app.search_is_open_for_test());
        let generation = app.saved_queries_generation_for_test();
        drop(
            app.update(Message::SavedQueries(SavedQueriesMessage::Loaded {
                generation,
                result: Ok(Vec::new()),
            })),
        );
        let draft = app.saved_queries_for_test().unwrap().unsaved().unwrap();
        assert_eq!(draft.spec.full_text.as_deref(), Some("telekom"));
        assert_eq!(draft.spec.year, Some(2026));
        assert_eq!(draft.name, "");
    }

    #[test]
    fn nothing_is_saved_as_a_report_without_words_or_a_chosen_value() {
        let mut app = App::with_search_filters("", |root| filters_for(root, ""));

        drop(app.update(Message::Search(SearchMessage::SaveAsReport)));

        assert!(app.search_is_open_for_test());
        assert!(app.saved_queries_for_test().is_none());
    }

    #[test]
    fn a_search_opened_from_a_report_offers_to_update_it_and_returns_to_the_reports() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _inside = runtime.enter();
        let mut app = App::with_search_filters("telekom", |root| {
            let mut filters = filters_for(root, "telekom");
            let report = crate::saved_queries::fixtures::stored_queries()[0].id;
            filters.edit_report(report, &ncrs_index::QuerySpec::default());
            filters
        })
        .with_ready_hermes_for_test();
        assert!(shows(&app, "Update report"));
        assert!(!shows(&app, "Save as report"));

        drop(app.update(Message::Search(SearchMessage::ReportUpdated(Ok(())))));

        assert!(!app.search_is_open_for_test());
        assert!(app.saved_queries_for_test().is_some());
    }

    #[test]
    fn a_report_that_could_not_be_updated_stays_on_the_search_with_the_reason() {
        let mut app = App::with_search_filters("telekom", |root| filters_for(root, "telekom"));

        drop(app.update(Message::Search(SearchMessage::ReportUpdated(Err(
            "there is no saved query 3".to_owned(),
        )))));

        assert!(app.search_is_open_for_test());
        assert!(shows(&app, "there is no saved query 3"));
    }

    #[test]
    fn the_old_command_list_action_opens_the_normal_search_and_is_no_longer_offered() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _inside = runtime.enter();
        let mut app = App::with_fixed_panels().with_ready_hermes_for_test();

        drop(app.update(KeyAction::SearchDocuments.message()));

        assert!(app.search_is_open_for_test());
        assert!(!KeyAction::SearchDocuments.is_offered());
    }

    #[test]
    fn the_last_row_of_quick_open_opens_the_search_with_the_text() {
        use crate::filing::{FoundDocument, TextSource};
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _inside = runtime.enter();
        let mut app = App::with_fixed_panels().with_ready_hermes_for_test();
        app.open_quick_open_for_test(&["src/config.rs"], "");
        app.type_into_quick_open_for_test("config");
        app.deliver_documents_for_test(Ok(vec![FoundDocument {
            path: std::path::PathBuf::from("/d/config-notes.txt"),
            read_by: TextSource::Text,
        }]));

        // One name hit, one document, then the row.
        drop(app.update(Message::QuickOpen(QuickOpenMessage::RowActivated(2))));

        assert!(app.quick_open_for_test().is_none());
        assert!(app.search_is_open_for_test());
        assert_eq!(app.search_for_test().unwrap().query, "config");
    }

    fn matches_snapshot(language: Language, name: &str) {
        let app = app_with_filters().in_language(language);
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
    fn the_search_with_filters_matches_its_snapshots() {
        matches_snapshot(Language::English, "search_filters_en");
        matches_snapshot(Language::German, "search_filters_de");
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
    use super::app_simulator as simulator;
    use std::path::PathBuf;

    use super::*;
    use crate::messages::{SavedQueriesMessage, SettingsMessage};
    use crate::saved_queries::fixtures::{hit, loaded_list, THIS_YEAR};
    use crate::saved_queries::{Draft, SavedQueries};

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
        let (serial, _) = list.begin_run().expect("a spec");
        let priced = |name: &str, missing: bool, minor_units: i64| {
            let mut found = hit(name, missing);
            found.fields.sender = Some("Company A".to_owned());
            found.fields.date = chrono::NaiveDate::from_ymd_opt(2024, 3, 9);
            found.fields.amount = Some(ncrs_index::Amount {
                minor_units,
                currency: ncrs_index::Currency::new("EUR").expect("a currency"),
            });
            found
        };
        list.ran(
            serial,
            vec![
                priced("invoice-1.pdf", false, 123_450),
                hit("invoice-2.pdf", false),
                priced("invoice-3.pdf", true, 5_000),
            ],
        );
        list
    }

    fn at_export() -> SavedQueries {
        let mut list = after_run();
        list.start_export(PathBuf::from("/Archive/Export"));
        list
    }

    /// The reports with the hits each one finds today.
    fn listing() -> SavedQueries {
        let mut list = loaded_list();
        let ids: Vec<_> = list.rows().iter().map(|query| query.id).collect();
        list.counted(ids.into_iter().zip([12, 3]).collect());
        list
    }

    fn editing() -> SavedQueries {
        let mut list = after_run();
        list.begin_edit(0);
        list
    }

    fn asking_to_delete() -> SavedQueries {
        let mut list = loaded_list();
        list.select(0);
        list.ask_delete();
        list
    }

    #[test]
    fn the_button_of_the_remove_question_removes_the_report() {
        let app = App::with_saved_queries(asking_to_delete());
        let mut sim = simulator(&app);
        sim.click(button_repeating_the_title("Remove report"))
            .expect("the confirm button");
        assert!(sim.into_messages().any(|message| matches!(
            message,
            Message::SavedQueries(SavedQueriesMessage::ConfirmDelete)
        )));
    }

    fn empty_list() -> SavedQueries {
        let mut list = SavedQueries::new(THIS_YEAR);
        list.loaded(Vec::new());
        list
    }

    fn click_sends(list: SavedQueries, label: &str, message: SavedQueriesMessage) {
        let app = App::with_saved_queries(list);
        let mut sim = simulator(&app);
        sim.click(label).expect(label);
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [queries_message(message)],
            "{label}"
        );
    }

    #[test]
    fn the_page_lists_the_reports_with_their_criteria_and_the_buttons_in_the_row() {
        for label in [
            "Company A 2024",
            "Taxes 2023",
            "12 hits",
            "Change",
            "Export …",
            "Remove",
        ] {
            assert!(shows(listing(), label), "{label}");
        }
    }

    #[test]
    fn an_empty_list_says_so() {
        assert!(shows(
            empty_list(),
            "No reports yet. Choose + Report to make one, or start from a template."
        ));
    }

    #[test]
    fn the_buttons_of_a_row_send_what_they_say_for_that_row() {
        click_sends(listing(), "Change", SavedQueriesMessage::Edit(0));
        click_sends(listing(), "Export …", SavedQueriesMessage::StartExport(0));
        click_sends(listing(), "Remove", SavedQueriesMessage::AskDelete(0));
    }

    #[test]
    fn the_change_dialog_shows_name_criteria_and_result_and_hands_the_report_to_the_search() {
        for label in [
            "Edit in search",
            "RESULT",
            "Hits",
            "View list",
            "Cancel",
            "Save",
        ] {
            assert!(shows(editing(), label), "{label}");
        }
        click_sends(
            editing(),
            "Edit in search",
            SavedQueriesMessage::EditInSearch,
        );
        click_sends(editing(), "View list", SavedQueriesMessage::ToggleMissing);
        click_sends(editing(), "Cancel", SavedQueriesMessage::CancelStep);
    }

    #[test]
    fn the_list_stays_the_same_while_a_dialog_is_up() {
        for list in [editing(), at_export(), asking_to_delete()] {
            assert!(shows(list.clone(), "Taxes 2023"));
            assert!(shows(list, "Change"));
        }
    }

    #[test]
    fn the_export_step_says_what_happens_before_anything_is_copied() {
        for label in [
            "Target folder",
            "/Archive/Export",
            "Files to copy",
            "Copies only; moves and deletes nothing. Existing files stay untouched.",
            "Choose folder …",
            "Cancel",
        ] {
            assert!(shows(at_export(), label), "{label}");
        }
        click_sends(
            at_export(),
            "Choose folder …",
            SavedQueriesMessage::PickFolder,
        );
    }

    #[test]
    fn deleting_asks_in_a_dialog_first() {
        assert!(shows(
            asking_to_delete(),
            "Remove the report ‘Company A 2024’? Documents stay as they are."
        ));
    }

    #[test]
    fn the_view_is_worded_in_german_too() {
        let app = App::with_saved_queries(at_export()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Abbrechen",
            "Zielordner",
            "Ordner wählen …",
            "Kopiert nur, verschiebt und löscht nichts. Vorhandene Dateien bleiben unangetastet.",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn escape_closes_the_dialog_first_and_then_the_settings() {
        let mut app = App::with_saved_queries(at_export());
        let _ = app.update(Message::Settings(SettingsMessage::Close));
        let page = app.saved_queries_for_test().unwrap();
        assert!(page.export_step().is_none());
        assert!(page.results().is_some());
        assert!(app.open_hermes_page().is_some());

        let _ = app.update(Message::Settings(SettingsMessage::Close));
        assert!(app.open_hermes_page().is_none());
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
        for language in ["en", "de"] {
            matches_snapshot(listing(), &format!("saved_queries_{language}"));
            matches_snapshot(empty_list(), &format!("saved_queries_empty_{language}"));
            matches_snapshot(editing(), &format!("saved_queries_edit_{language}"));
            matches_snapshot(at_export(), &format!("saved_queries_export_{language}"));
            matches_snapshot(
                asking_to_delete(),
                &format!("saved_queries_delete_{language}"),
            );
        }
    }

    /// A search handed over to be saved: a dialog for a report not stored yet.
    fn unsaved_list() -> SavedQueries {
        let mut list = SavedQueries::new(THIS_YEAR);
        list.prefill(Draft {
            name: String::new(),
            spec: ncrs_index::QuerySpec {
                year: Some(THIS_YEAR),
                full_text: Some("telekom".to_owned()),
                ..ncrs_index::QuerySpec::default()
            },
        });
        list.loaded(crate::saved_queries::fixtures::stored_queries());
        let (serial, _) = list.begin_run().expect("a spec");
        list.ran(serial, vec![hit("invoice-1.pdf", false)]);
        list
    }

    #[test]
    fn a_search_handed_over_opens_the_dialog_with_a_save_button() {
        click_sends(unsaved_list(), "Save", SavedQueriesMessage::Save);
    }

    #[test]
    fn editing_in_the_search_binds_the_report() {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let _inside = runtime.enter();
        let first = crate::saved_queries::fixtures::stored_queries()[0].id;
        let mut app = App::with_saved_queries(loaded_list()).with_ready_hermes_for_test();

        drop(app.update(queries_message(SavedQueriesMessage::EditInSearch)));

        assert!(app.search_is_open_for_test());
        assert!(app.saved_queries_for_test().is_none());
        let filters = app
            .search_for_test()
            .and_then(|view| view.filters.as_ref())
            .expect("the filters of an indexed search");
        assert_eq!(filters.report(), Some(first));
        assert_eq!(filters.selection().year, Some(2024));
    }

    #[test]
    fn the_unsaved_row_matches_its_snapshot() {
        matches_snapshot(unsaved_list(), "saved_queries_unsaved_de");
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
mod tags_card {
    use super::app_simulator as simulator;
    use std::collections::BTreeSet;

    use ncrs_index::{Condition, ConditionKind, RuleConditions, RuleSpec};

    use super::*;
    use crate::config::Filing;
    use crate::file_list_tags::TagExceptions;
    use crate::messages::{HermesPage, SettingsFeature, SettingsMessage, TagsMessage};
    use crate::tags_page::fixtures::loaded_page;
    use crate::tags_page::TagsPage;
    use crate::tags_work::{TagsFailure, TagsLoaded};

    fn shows(page: TagsPage, label: &str) -> bool {
        simulator(&App::with_tags(page)).find(label).is_ok()
    }

    fn counted(tags: &[(&str, u64)]) -> Vec<(String, u64)> {
        tags.iter()
            .map(|(tag, count)| ((*tag).to_owned(), *count))
            .collect()
    }

    /// The tags and counts of the design board.
    fn board_page() -> TagsPage {
        let mut page = TagsPage::new(ncrs_index::Kinds::built_in(
            ncrs_index::KindLanguage::English,
        ));
        let mut senders = counted(&[("telekom", 48), ("stadtwerke-musterhausen", 24)]);
        senders.extend((1..=38).map(|n| (format!("absender-{n}"), 1)));
        page.loaded(TagsLoaded {
            tally: ncrs_index::TagTally {
                own: counted(&[("steuer", 23), ("offen", 3), ("privat", 8)]),
                kind: counted(&[("invoice", 412), ("contract", 31), ("notice", 12)]),
                sender: senders,
                recipient: counted(&[("firma-a-gmbh", 9), ("max-muster", 4)]),
                year: counted(&[
                    ("2026", 40),
                    ("2025", 61),
                    ("2024", 58),
                    ("2023", 44),
                    ("2022", 30),
                    ("2021", 12),
                ]),
            },
            exceptions: TagExceptions::from([("notice".to_owned(), false)]),
        });
        page
    }

    fn with_sender_group_open() -> TagsPage {
        let mut page = board_page();
        page.toggle_open(crate::file_list_tags::TagGroup::Sender);
        page
    }

    fn renaming() -> TagsPage {
        let mut page = with_sender_group_open();
        page.start_rename("steuer".to_owned());
        page
    }

    fn refused_delete() -> TagsPage {
        let mut page = with_sender_group_open();
        page.ask_delete("steuer".to_owned());
        assert_eq!(page.begin_delete().as_deref(), Some("steuer"));
        page.failed(TagsFailure::InUse(vec![RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Sender,
                "Company A",
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some("Invoice".to_owned()),
            tags: BTreeSet::from(["steuer".to_owned()]),
            filing_pattern: "{year}/{sender}".to_owned(),
            confirmed: true,
            enabled: true,
        }]));
        page
    }

    #[test]
    fn the_page_shows_the_switch_and_the_tags() {
        for label in [
            "Tags",
            "Show tags",
            "At most 3 per file, then “+1”. Applies to both file lists.",
            "Documents",
            "steuer",
            "invoice",
            "… and 38 more",
        ] {
            assert!(shows(with_sender_group_open(), label), "{label}");
        }
    }

    #[test]
    fn the_german_page_uses_the_plain_words() {
        let app = App::with_tags(with_sender_group_open()).in_language(Language::German);
        let mut sim = simulator(&app);
        for label in [
            "Schlagworte",
            "Schlagwort suchen",
            "Dokumente",
            "… und 38 weitere",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn a_search_that_finds_nothing_says_so() {
        let mut page = loaded_page();
        page.set_search("nothing like it".to_owned());
        assert!(shows(page, "No tag matches."));
    }

    #[test]
    fn the_button_of_the_remove_question_removes_the_tag() {
        let mut asking = loaded_page();
        asking.ask_delete("steuer".to_owned());
        let app = App::with_tags(asking);
        let mut sim = simulator(&app);
        sim.click(button_repeating_the_title("Remove tag"))
            .expect("the confirm button");
        assert!(sim
            .into_messages()
            .any(|message| matches!(message, Message::Tags(TagsMessage::ConfirmDelete))));
    }

    #[test]
    fn the_delete_question_has_its_button_and_a_refused_delete_names_the_rules() {
        assert!(renaming().is_editing());
        let mut asking = loaded_page();
        asking.ask_delete("steuer".to_owned());
        assert!(shows(asking, "Remove tag"));
        assert!(shows(
            refused_delete(),
            "‘steuer’ is a condition of these rules and stays: If the sender is “Company A” → Invoice …. Change the rules first."
        ));
    }

    #[test]
    fn a_sidebar_click_opens_the_page_and_closes_the_other() {
        let mut app = App::with_saved_queries(crate::saved_queries::fixtures::loaded_list());
        let _ = app.update(Message::HermesPage(HermesPage::Tags));
        assert!(app.saved_queries_for_test().is_none());
        assert!(app.tags_for_test().is_some());
    }

    #[test]
    fn escape_closes_the_dialog_first_and_then_the_settings() {
        let mut app = App::with_tags(renaming());
        let _ = app.update(Message::Settings(SettingsMessage::Close));
        assert!(app
            .tags_for_test()
            .is_some_and(|page| page.step().is_none()));
        assert!(app.open_hermes_page().is_some());
        let _ = app.update(Message::Settings(SettingsMessage::Close));
        assert!(app.open_hermes_page().is_none());
    }

    #[test]
    fn the_work_done_sets_the_exceptions_the_file_list_follows() {
        let mut app = App::with_tags(loaded_page());
        let generation = app.tags_generation_for_test();
        let loaded = |exceptions: TagExceptions| TagsLoaded {
            exceptions,
            ..TagsLoaded::default()
        };
        let _ = app.update(Message::Tags(TagsMessage::Done {
            generation: generation.wrapping_add(1),
            result: Ok(loaded(TagExceptions::from([("old".to_owned(), true)]))),
        }));
        assert!(app.tag_exceptions_for_test().is_empty(), "a stale answer");

        let _ = app.update(Message::Tags(TagsMessage::Done {
            generation,
            result: Ok(loaded(TagExceptions::from([("steuer".to_owned(), false)]))),
        }));
        assert_eq!(
            app.tag_exceptions_for_test(),
            &TagExceptions::from([("steuer".to_owned(), false)])
        );
    }

    #[test]
    fn the_settings_button_shows_the_tags_page_in_the_same_settings() {
        let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());
        let _ = app.update(Message::Settings(SettingsMessage::ChooseFileListTags));
        assert!(app.tags_for_test().is_some());
        assert_eq!(app.open_hermes_page(), Some(HermesPage::Tags));
        assert!(app.settings_for_test().is_some());
    }

    #[test]
    fn the_look_page_has_the_switch_and_not_the_removed_ones() {
        let app = App::with_settings_open(SettingsFeature::Look, Filing::default());
        let mut sim = simulator(&app);
        assert!(sim.find("Show tags").is_ok(), "the switch");
        assert!(sim.find("Tags in the file list").is_err(), "removed row");
        assert!(sim.find("Which tags?").is_err(), "removed group");
        assert!(sim.find("Only show these").is_err(), "removed row");
    }

    fn matches_snapshot(page: TagsPage, name: &str) {
        let language = if name.ends_with("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_tags(page).in_language(language);
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
    fn the_open_menu_shows_its_entries_in_full() {
        let app = App::with_tags(with_sender_group_open()).in_language(Language::German);
        let mut sim = simulator(&app);
        sim.point_at(iced::Point::new(907.0, 247.0));
        let _ = sim.simulate(iced_test::simulator::click());
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image("tests/snapshots/menu_open_tags_de.png")
            .expect("snapshot comparison");
        assert!(matches, "the view does not match menu_open_tags_de.png");
    }

    #[test]
    fn the_view_matches_its_snapshots() {
        for suffix in ["en", "de"] {
            matches_snapshot(with_sender_group_open(), &format!("hermes_tags_{suffix}"));
            matches_snapshot(renaming(), &format!("hermes_tags_rename_{suffix}"));
            matches_snapshot(refused_delete(), &format!("hermes_tags_in_use_{suffix}"));
        }
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod rules_card {
    use super::app_simulator as simulator;
    use std::collections::BTreeSet;

    use ncrs_index::{Condition, ConditionKind, Rule, RuleConditions, RuleOrigin, RuleSpec};

    use super::*;
    use crate::condition_editor::ConditionEdit;
    use crate::messages::{RuleFormMessage, RulesMessage, SettingsMessage};
    use crate::rules_page::fixtures::rule_id;
    use crate::rules_page::RulesPage;
    use crate::rules_work::{ListedRule, RulesLoad};

    fn app_shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    fn listed(
        id: i64,
        origin: RuleOrigin,
        conditions: RuleConditions,
        spec: (&str, &[&str]),
        enabled: bool,
        hits: u64,
    ) -> ListedRule {
        let (kind, tags) = spec;
        ListedRule {
            rule: Rule {
                id: rule_id(usize::try_from(id).unwrap()),
                position: id,
                origin,
                spec: RuleSpec {
                    name: None,
                    conditions,
                    extract_number: None,
                    extract_date: None,
                    extract_amount: None,
                    kind: Some(kind.to_owned()),
                    tags: tags
                        .iter()
                        .map(|tag| (*tag).to_owned())
                        .collect::<BTreeSet<_>>(),
                    filing_pattern: "{year}/{sender}".to_owned(),
                    confirmed: true,
                    enabled,
                },
            },
            global_index: usize::try_from(id).unwrap() - 1,
            hits,
        }
    }

    fn three_rules() -> RulesPage {
        let named = |name: &str, mut rule: ListedRule| {
            rule.rule.spec.name = Some(name.to_owned());
            rule
        };
        let mut page = RulesPage::new();
        page.loaded(RulesLoad {
            rules: vec![
                named(
                    "Telekom-Rechnungen",
                    listed(
                        1,
                        RuleOrigin::Hand,
                        RuleConditions::all_of(vec![
                            Condition::new(ConditionKind::Sender, "Company A"),
                            Condition::new(ConditionKind::Word, "invoice"),
                        ]),
                        ("Invoice", &["tax"]),
                        true,
                        12,
                    ),
                ),
                named(
                    "Strom",
                    listed(
                        2,
                        RuleOrigin::Suggestion,
                        RuleConditions::any_of(vec![
                            Condition::new(ConditionKind::Substring, "electricity"),
                            Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 00"),
                        ]),
                        ("Contract", &[]),
                        true,
                        1,
                    ),
                ),
                named(
                    "Erinnerungen",
                    listed(
                        3,
                        RuleOrigin::Filing,
                        RuleConditions::all_of(vec![
                            Condition::not(ConditionKind::Word, "reminder"),
                            Condition::new(ConditionKind::Folder, "Inbox/Post"),
                        ]),
                        ("Receipt", &[]),
                        false,
                        0,
                    ),
                ),
            ],
        });
        page
    }

    fn empty() -> RulesPage {
        let mut page = RulesPage::new();
        page.loaded(RulesLoad::default());
        page
    }

    fn shows(page: RulesPage, label: &str) -> bool {
        simulator(&App::with_rules(page)).find(label).is_ok()
    }

    fn click_sends(page: RulesPage, label: &str, message: RulesMessage) {
        let app = App::with_rules(page);
        let mut sim = simulator(&app);
        sim.click(label).expect(label);
        assert_eq!(
            sim.into_messages().filter(is_action).collect::<Vec<_>>(),
            [Message::Rules(message)],
            "{label}"
        );
    }

    fn asking_to_delete() -> RulesPage {
        let mut page = three_rules();
        page.ask_delete(0);
        page
    }

    #[test]
    fn the_button_of_the_remove_question_removes_the_rule() {
        let app = App::with_rules(asking_to_delete());
        let mut sim = simulator(&app);
        sim.click(button_repeating_the_title("Remove rule"))
            .expect("the confirm button");
        assert!(sim
            .into_messages()
            .any(|message| matches!(message, Message::Rules(RulesMessage::ConfirmDelete))));
    }

    #[test]
    fn the_page_lists_the_rules_with_their_buttons_in_the_row() {
        for label in [
            "Telekom-Rechnungen",
            "Strom",
            "Erinnerungen",
            "12 hits",
            "↑",
            "↓",
            "Change",
            "Remove",
            "+ Rule",
        ] {
            assert!(shows(three_rules(), label), "{label}");
        }
    }

    #[test]
    fn an_empty_page_says_where_rules_come_from() {
        assert!(shows(
            empty(),
            "No rules yet. Create one with “+ Rule”, or let rules come from sorting in the chat."
        ));
    }

    #[test]
    fn the_buttons_send_what_they_say_for_their_row() {
        click_sends(three_rules(), "+ Rule", RulesMessage::New);
        click_sends(three_rules(), "Change", RulesMessage::Edit(0));
        click_sends(three_rules(), "Remove", RulesMessage::AskDelete(0));
        click_sends(
            three_rules(),
            "↓",
            RulesMessage::ReorderAt { index: 0, delta: 1 },
        );
    }

    #[test]
    fn the_first_rule_cannot_move_up() {
        let app = App::with_rules(three_rules());
        let mut sim = simulator(&app);
        let _ = sim.click("↑");

        assert!(sim.into_messages().find(is_action).is_none());
    }

    #[test]
    fn save_waits_for_a_change_and_cancel_closes_the_dialog() {
        let app = App::with_rules(with_edit_form());
        let mut sim = simulator(&app);
        let _ = sim.click("Save");
        assert!(
            sim.into_messages().find(is_action).is_none(),
            "Save without a change"
        );

        let mut changed = App::with_rules(with_edit_form());
        let _ = changed.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
            "Contract".to_owned(),
        ))));
        for (label, message) in [
            ("Save", RuleFormMessage::Save),
            ("Cancel", RuleFormMessage::Cancel),
        ] {
            let mut pressed = simulator(&changed);
            pressed.click(label).expect(label);
            assert_eq!(
                pressed
                    .into_messages()
                    .filter(is_action)
                    .collect::<Vec<_>>(),
                [Message::Rules(RulesMessage::Form(message))],
                "{label}"
            );
        }
    }

    #[test]
    fn cancelling_the_dialog_drops_what_was_typed() {
        let mut app = App::with_rules(with_edit_form());
        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
            "Contract".to_owned(),
        ))));

        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Cancel)));

        let page = app.rules_for_test().unwrap();
        assert!(page.form().is_none());
        assert_eq!(page.rows().len(), 3);
        assert_eq!(
            page.rows()[0].rule.spec.name.as_deref(),
            Some("Telekom-Rechnungen")
        );
    }

    #[test]
    fn the_list_stays_the_same_while_a_dialog_is_up() {
        for page in [with_edit_form(), with_new_form(), asking_to_delete()] {
            assert!(shows(page.clone(), "Strom"));
            assert!(shows(page, "Remove"));
        }
    }

    #[test]
    fn the_delete_question_names_the_rule_and_cancel_keeps_it() {
        let mut app = App::with_rules(three_rules());
        let _ = app.update(Message::Rules(RulesMessage::AskDelete(0)));

        assert!(app.rules_for_test().unwrap().is_confirming_delete());
        assert!(app_shows(&app, "Remove rule"));
        assert!(app_shows(&app, "Cancel"));
        let _ = app.update(Message::Rules(RulesMessage::CancelDelete));
        let page = app.rules_for_test().unwrap();
        assert!(!page.is_confirming_delete());
        assert_eq!(page.rows().len(), 3);
    }

    fn matches_snapshot(page: RulesPage, name: &str) {
        let language = if name.ends_with("_de") {
            Language::German
        } else {
            Language::English
        };
        let app = App::with_rules(page).in_language(language);
        let tall = f32::from(name.starts_with("rule_form"));
        let size = iced::Size::new(1200.0, 760.0 + 340.0 * tall);
        let settings = iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        };
        let mut simulator = Simulator::with_size(settings, size, app.view());
        let matches = simulator
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
        matches_snapshot(three_rules(), "rules_en");
        matches_snapshot(three_rules(), "rules_de");
        matches_snapshot(empty(), "rules_empty_en");
        matches_snapshot(empty(), "rules_empty_de");
        matches_snapshot(asking_to_delete(), "rules_delete_en");
        matches_snapshot(asking_to_delete(), "rules_delete_de");
    }

    fn with_new_form() -> RulesPage {
        let mut page = three_rules();
        page.begin_new();
        page
    }

    fn with_edit_form() -> RulesPage {
        let mut page = three_rules();
        page.begin_edit(0);
        page
    }

    fn with_invalid_form() -> RulesPage {
        let mut page = with_new_form();
        let Some(form) = page.form_mut() else {
            panic!("no form");
        };
        form.update(RuleFormMessage::Condition(ConditionEdit::Add));
        form.update(RuleFormMessage::Condition(ConditionEdit::Kind(
            1,
            ConditionKind::Regex,
        )));
        form.update(RuleFormMessage::Condition(ConditionEdit::Value(
            1,
            "(".to_owned(),
        )));
        form.update(RuleFormMessage::Target(
            crate::filing_pieces::TargetEdit::Remove(0),
        ));
        form.update(RuleFormMessage::Target(
            crate::filing_pieces::TargetEdit::Remove(0),
        ));
        assert_eq!(page.begin_save(), None);
        page
    }

    fn tag_rules() -> RulesPage {
        let tagged = |id, origin, conditions, hits| {
            let mut rule = listed(id, origin, conditions, ("", &[]), true, hits);
            rule.rule.spec.kind = None;
            rule.rule.spec.filing_pattern =
                "{recipient}/{year}/{sender}/{date}_{sender}".to_owned();
            rule
        };
        let mut page = RulesPage::new();
        page.loaded(RulesLoad {
            rules: vec![
                tagged(
                    1,
                    RuleOrigin::Hand,
                    RuleConditions::all_of(vec![
                        Condition::new(ConditionKind::Tag, "rechnung"),
                        Condition::new(ConditionKind::Tag, "firma-a"),
                    ]),
                    12,
                ),
                tagged(
                    2,
                    RuleOrigin::Hand,
                    RuleConditions::all_of(vec![Condition::new(ConditionKind::Tag, "steuer")]),
                    4,
                ),
                tagged(
                    3,
                    RuleOrigin::Suggestion,
                    RuleConditions::all_of(vec![
                        Condition::new(ConditionKind::Tag, "vertrag"),
                        Condition::not(ConditionKind::Tag, "firma-a"),
                    ]),
                    3,
                ),
            ],
        });
        page
    }

    #[test]
    fn the_rules_with_tags_match_their_snapshots() {
        matches_snapshot(tag_rules(), "rules_tags_en");
        matches_snapshot(tag_rules(), "rules_tags_de");
    }

    #[test]
    fn the_rule_form_with_tags_matches_its_snapshots() {
        for language in ["en", "de"] {
            let mut page = tag_rules();
            page.begin_edit(0);
            matches_snapshot(page, &format!("rule_form_tags_{language}"));
        }
    }

    #[test]
    fn the_rule_form_matches_its_snapshots() {
        for language in ["en", "de"] {
            matches_snapshot(with_new_form(), &format!("rule_form_new_{language}"));
            matches_snapshot(with_edit_form(), &format!("rule_form_edit_{language}"));
            matches_snapshot(
                with_invalid_form(),
                &format!("rule_form_invalid_{language}"),
            );
        }
    }

    fn with_tried_form() -> RulesPage {
        use crate::rules_work::{TriedDocument, TryOutcome, TryResult, TrySection};
        use ncrs_index::ReviewReason;
        use std::path::PathBuf;

        let mut page = with_edit_form();
        let Some(form) = page.form_mut() else {
            panic!("no form");
        };
        let filed = |path: &str| TryOutcome::Filed(PathBuf::from(path));
        form.live_try.status = crate::rule_form::TryStatus::Ready(TryResult {
            documents: vec![
                TriedDocument {
                    section: TrySection::Inbox,
                    path: PathBuf::from("rent-march.pdf"),
                    now: TryOutcome::Review(ReviewReason::NoMatchingRule),
                    after: filed("Invoice/2024/Telekom"),
                    conflicts: Vec::new(),
                },
                TriedDocument {
                    section: TrySection::Review,
                    path: PathBuf::from("scan-0412.pdf"),
                    now: TryOutcome::Review(ReviewReason::MissingField(ncrs_index::Field::Date)),
                    after: filed("Invoice/2024/Telekom"),
                    conflicts: Vec::new(),
                },
                TriedDocument {
                    section: TrySection::Archive,
                    path: PathBuf::from("2021/telekom-may.pdf"),
                    now: TryOutcome::Lies(PathBuf::from("2021/telekom-may.pdf")),
                    after: filed("Invoice/2021/Telekom"),
                    conflicts: vec![rule_id(2), rule_id(3)],
                },
            ],
            more: 0,
            conflict_names: [(rule_id(2), 2), (rule_id(3), 3)].into_iter().collect(),
        });
        page
    }

    #[test]
    fn the_try_of_the_rule_form_matches_its_snapshots() {
        for language in ["en", "de"] {
            matches_snapshot(with_tried_form(), &format!("rule_form_try_{language}"));
        }
    }

    #[test]
    fn an_edit_of_the_form_starts_a_try_and_a_stale_timer_does_nothing() {
        use crate::rule_form::TryStatus;

        let mut app = App::with_rules(three_rules());
        let _ = app.update(Message::Rules(RulesMessage::Edit(0)));
        let waiting = |shown: &App| {
            shown
                .rules_for_test()
                .unwrap()
                .form()
                .unwrap()
                .live_try
                .clone()
        };
        let opened = waiting(&app);
        assert_eq!(opened.status, TryStatus::Waiting);

        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
            "Contract".to_owned(),
        ))));
        let edited = waiting(&app);
        assert_ne!(edited.generation, opened.generation);

        let _ = app.update(Message::Rules(RulesMessage::TryElapsed(opened.generation)));
        assert_eq!(waiting(&app), edited);
        let _ = app.update(Message::Rules(RulesMessage::Tried {
            generation: opened.generation,
            result: Ok(crate::rules_work::TryResult::default()),
        }));
        assert_eq!(waiting(&app), edited);
    }

    #[test]
    fn the_new_rule_button_opens_the_form_and_the_edit_button_fills_it() {
        let mut app = App::with_rules(three_rules());
        let _ = app.update(Message::Rules(RulesMessage::New));
        let new_form = app.rules_for_test().unwrap().form().unwrap();
        assert_eq!(new_form.name, "");
        assert_eq!(new_form.filing.pattern(), "{year}/{sender}/");

        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Cancel)));
        let _ = app.update(Message::Rules(RulesMessage::Edit(0)));
        let edit_form = app.rules_for_test().unwrap().form().unwrap();
        assert_eq!(edit_form.filing.pattern(), "{year}/{sender}");
        assert!(matches!(
            edit_form.target,
            crate::rule_form::RuleFormTarget::Edit(_)
        ));
    }

    #[test]
    fn escape_closes_the_dialog_first_saves_nothing_and_then_closes_the_settings() {
        let mut app = App::with_rules(with_edit_form());
        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Name(
            "Contract".to_owned(),
        ))));

        let _ = app.update(Message::Settings(SettingsMessage::Close));

        let page = app.rules_for_test().unwrap();
        assert!(page.form().is_none());
        assert_eq!(
            page.rows().first().unwrap().rule.spec.kind.as_deref(),
            Some("Invoice")
        );
        assert!(app.open_hermes_page().is_some());
        let _ = app.update(Message::Settings(SettingsMessage::Close));
        assert!(app.open_hermes_page().is_none());
    }

    #[test]
    fn an_invalid_form_sends_nothing_and_shows_what_is_wrong() {
        let mut app = App::with_rules(with_invalid_form()).in_language(Language::English);
        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Save)));

        let page = app.rules_for_test().unwrap();
        assert!(!page.form().unwrap().saving);
        for text in ["Enter a value.", "Add at least one folder."] {
            assert!(simulator(&app).find(text).is_ok(), "{text}");
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
mod title_bar {
    use std::time::{Duration, SystemTime};

    use ncrs_pipeline::{RunnerState, Snapshot};

    use super::*;
    use crate::messages::{FilingStatusMessage, PanelSide};
    use crate::ui::titlebar::{HERMES_CHIP_ID, LEFT_TAB_ID, RIGHT_TAB_ID};

    fn taken_at() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// Filing on, with a measured rate of 78 files a minute.
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
        assert_eq!(
            messages,
            vec![Message::Panel(PanelMessage::ActivatePanel(
                PanelSide::Right
            ))]
        );
        for message in messages {
            drop(app.update(message));
        }
        assert_eq!(app.active_panel_side_for_test(), PanelSide::Right);

        assert_eq!(
            messages_after_click(&app, LEFT_TAB_ID),
            vec![Message::Panel(PanelMessage::ActivatePanel(PanelSide::Left))]
        );
    }

    #[test]
    fn a_click_on_the_hermes_chip_opens_the_inspection_page() {
        let mut app = with_hermes().with_starting_hermes_for_test();
        let messages = messages_after_click(&app, HERMES_CHIP_ID);
        assert_eq!(
            messages,
            vec![Message::FilingStatus(FilingStatusMessage::Open)]
        );
        for message in messages {
            drop(app.update(message));
        }
        assert!(app.inspection_page_is_open_for_test());
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
        let snapshot_again = snapshot.clone();
        let app = App::with_filing_snapshot(snapshot, taken_at());
        assert_eq!(app.hermes_chip_for_test().as_deref(), Some("Hermes"));
        let themed = App::with_filing_snapshot(snapshot_again, taken_at())
            .in_language(Language::EnglishFuturama);
        assert_eq!(themed.hermes_chip_for_test().as_deref(), Some("Hermes"));
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
    fn the_file_lists_show_the_tags_after_the_names() {
        use crate::file_list_tags::{FileListTags, TagGroup};

        for (language, name) in [
            (Language::English, "main_view_tags_en"),
            (Language::German, "main_view_tags_de"),
        ] {
            let size = iced::Size::new(1600.0, 760.0);
            let app = App::with_hermes_column_sample()
                .with_window_size_for_test(size)
                .with_file_list_tags_for_test(FileListTags {
                    groups: vec![TagGroup::Kind, TagGroup::Sender, TagGroup::Own],
                    ..FileListTags::default()
                })
                .in_language(language);
            let matches = Simulator::with_size(iced::Settings::default(), size, app.view())
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
            let app = App::with_hermes_quick_fix_sample()
                .unwrap()
                .in_language(language);
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
        sim.into_messages().filter(is_action).collect()
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

#[cfg(test)]
mod about_card {
    use super::*;
    use crate::about::AboutCard;
    use crate::messages::AboutMessage;

    fn shows(app: &App, label: &str) -> bool {
        simulator(app).find(label).is_ok()
    }

    fn messages_after_click(label: &str) -> Vec<Message> {
        let app = App::with_about_card(AboutCard::new());
        let mut sim = simulator(&app);
        assert!(sim.click(label).is_ok(), "{label}");
        sim.into_messages().collect()
    }

    #[test]
    fn the_card_shows_title_version_license_and_repository() {
        let app = App::with_about_card(AboutCard::new());
        for label in [
            "About ncrs",
            "Version 1.2.3",
            "MIT License",
            "https://github.com/CosmicDriftGameStudio/ncrs",
        ] {
            assert!(shows(&app, label), "{label}");
        }
    }

    #[test]
    fn the_buttons_send_their_message_and_nothing_else() {
        assert_eq!(
            messages_after_click("Third-party licenses"),
            [Message::About(AboutMessage::OpenLicenses)]
        );
        assert_eq!(
            messages_after_click("Repository"),
            [Message::About(AboutMessage::OpenRepository)]
        );
        assert_eq!(
            messages_after_click("Close"),
            [Message::About(AboutMessage::Close)]
        );
    }

    #[test]
    fn a_missing_licenses_file_names_the_online_copy_of_this_version() {
        let mut card = AboutCard::new();
        card.report_licenses_missing();
        let app = App::with_about_card(card);
        assert!(shows(
            &app,
            "The licence file is not part of this build. Read it online: https://github.com/CosmicDriftGameStudio/ncrs/releases/download/v1.2.3/THIRD_PARTY_LICENSES.html"
        ));
    }

    #[test]
    fn the_card_is_worded_in_german_too() {
        let app = App::with_about_card(AboutCard::new()).in_language(Language::German);
        for label in [
            "Über ncrs",
            "Version 1.2.3",
            "MIT-Lizenz",
            "Lizenzen von Drittanbietern",
            "Schließen",
        ] {
            assert!(shows(&app, label), "{label}");
        }
    }

    fn matches_snapshot(language: Language, name: &str) {
        let app = App::with_about_card(AboutCard::new()).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .and_then(|snapshot| snapshot.matches_image(format!("tests/snapshots/{name}.png")))
            .is_ok_and(|matches| matches);
        assert!(
            matches,
            "the about card does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_card_matches_its_snapshots() {
        matches_snapshot(Language::English, "about_en");
        matches_snapshot(Language::German, "about_de");
        matches_snapshot(Language::EnglishFuturama, "about_en_futurama");
        matches_snapshot(Language::GermanFuturama, "about_de_futurama");
    }

    #[test]
    fn only_the_futurama_variant_shows_the_theme_name_of_the_title() {
        let plain = App::with_about_card(AboutCard::new());
        assert!(!shows(&plain, "Planet Express crew roster"));
        let themed = App::with_about_card(AboutCard::new()).in_language(Language::EnglishFuturama);
        assert!(shows(&themed, "Planet Express crew roster"));
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
            format!("{}: {}", pin.name, crate::ui::format::megabytes(pin.size)),
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
    fn a_running_download_has_only_the_stop_button() {
        let mut running = card();
        running.start().unwrap();
        assert!(shows(running.clone(), "Downloading…"));
        assert!(!shows(running.clone(), "Cancel"));
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

    fn matches_card_snapshot(card: AiModelCard, language: Language, name: &str) {
        let app = App::with_ai_model_card(card).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the card does not match tests/snapshots/{name}.png"
        );
    }

    fn downloading() -> AiModelCard {
        let mut running = card();
        running.start().unwrap();
        running.set_received_for_test(312_000_000);
        running
    }

    fn finished(outcome: crate::ai_model::InstallOutcome) -> AiModelCard {
        let mut done = card();
        done.start().unwrap();
        assert!(done.finish(Ok(())));
        done.set_outcome(outcome);
        done
    }

    #[test]
    fn a_running_download_shows_its_bar_and_the_figures_and_only_stop() {
        assert!(shows(
            downloading(),
            &format!(
                "312 / {} · 12 %",
                crate::ui::format::megabytes(ncrs_ai::LOCAL_MODEL.size)
            )
        ));
        assert!(!shows(downloading(), "Download"));
        matches_card_snapshot(downloading(), Language::English, "ai_model_downloading_en");
        matches_card_snapshot(downloading(), Language::German, "ai_model_downloading_de");
    }

    #[test]
    fn a_finished_download_says_what_became_of_the_ai_switch() {
        use crate::ai_model::InstallOutcome;
        assert!(shows(
            finished(InstallOutcome::TurnedOn),
            "The AI model is installed. AI is now on: ncrs fills in fields for which there is no rule yet."
        ));
        assert_eq!(
            messages_after_click(finished(InstallOutcome::TurnedOn), "Done"),
            [Message::AiModel(AiModelMessage::Close)]
        );
        assert_eq!(
            messages_after_click(finished(InstallOutcome::StillOff), "Turn on AI"),
            [Message::AiModel(AiModelMessage::TurnOn)]
        );
        matches_card_snapshot(
            finished(InstallOutcome::TurnedOn),
            Language::English,
            "ai_model_finished_en",
        );
        matches_card_snapshot(
            finished(InstallOutcome::TurnedOn),
            Language::German,
            "ai_model_finished_de",
        );
    }

    #[test]
    fn a_model_that_is_already_there_shows_installed_and_only_close() {
        let installed = AiModelCard::installed(Some(PathBuf::from("/example/data/ai-models")));
        assert!(shows(installed.clone(), "Installed"));
        assert!(!shows(installed.clone(), "Download"));
        assert!(!shows(installed.clone(), "Download from: huggingface.co"));
        assert_eq!(
            messages_after_click(installed.clone(), "Close"),
            [Message::AiModel(AiModelMessage::Close)]
        );
        matches_card_snapshot(
            installed.clone(),
            Language::English,
            "ai_model_installed_en",
        );
        matches_card_snapshot(installed, Language::German, "ai_model_installed_de");
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
    use super::app_simulator as simulator;
    use std::path::PathBuf;

    use ncrs_intake::LocationMode;

    use super::*;
    use crate::config::{AiSetting, Filing, FilingLocation};
    use crate::fs::OpenKind;
    use crate::messages::{
        HermesPage, SettingChange, SettingField, SettingsFeature, SettingsMessage,
    };
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
                    transfer: Default::default(),
                },
                FilingLocation {
                    path: PathBuf::from("/home/test/Documents/Filed"),
                    mode: LocationMode::Archive,
                    images: false,
                    is_target: true,
                    transfer: Default::default(),
                },
            ],
            load: Default::default(),
            ai: AiSetting::Local { threads: None },
            default_pattern: None,
            own_names: Vec::new(),
            own_ibans: Vec::new(),
            own_tax_ids: Vec::new(),
            first_run: false,
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
    fn the_tags_row_of_the_appearance_page_matches_its_snapshots() {
        let scrolled_snapshot = |language: Language, name: &str| {
            let app = App::with_settings_open(SettingsFeature::Look, Filing::default())
                .in_language(language);
            let mut sim = simulator(&app);
            sim.point_at(iced::Point::new(700.0, 400.0));
            let _statuses = sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -80.0 },
            })]);
            let matches = sim
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}.png"))
                .expect("snapshot comparison");
            assert!(matches, "the tab does not match tests/snapshots/{name}.png");
        };
        scrolled_snapshot(Language::German, "settings_appearance_tags_de");
        scrolled_snapshot(Language::English, "settings_appearance_tags_en");
    }

    #[test]
    fn the_filing_page_is_marked_beta_in_both_languages() {
        for language in [Language::English, Language::German] {
            let app = App::with_settings_open(SettingsFeature::Hermes, hermes_config())
                .in_language(language);
            assert!(simulator(&app).find("Beta").is_ok(), "{language:?}");
        }
    }

    #[test]
    fn the_filing_page_matches_its_snapshots() {
        both_languages(
            || App::with_settings_open(SettingsFeature::Hermes, hermes_config()),
            "settings_hermes",
        );
    }

    fn both_languages(app: impl Fn() -> App, name: &str) {
        matches_snapshot(&app().in_language(Language::German), &format!("{name}_de"));
        matches_snapshot(&app().in_language(Language::English), &format!("{name}_en"));
    }

    fn settings_with(feature: SettingsFeature, source: &str) -> App {
        App::with_settings_file(feature, Ok(crate::config::from_text(source).unwrap()))
    }

    #[test]
    fn the_programs_page_matches_its_snapshots_with_a_refused_editor() {
        both_languages(
            || {
                let mut app = settings_with(
                    SettingsFeature::Programs,
                    "[open]\nview = [\"less\", \"-R\"]\n\n[theme]\npreset = \"cosmic\"\n",
                );
                let editor = SettingField::Program(OpenKind::Edit);
                drop(app.update(Message::Settings(SettingsMessage::Edit {
                    field: editor,
                    text: "bin/edit {path}".to_owned(),
                })));
                drop(app.update(Message::Settings(SettingsMessage::Submit(editor))));
                app
            },
            "settings_programs",
        );
    }

    #[test]
    fn the_keys_page_matches_its_snapshots() {
        both_languages(
            || App::with_settings_open(SettingsFeature::Keys, Filing::default()),
            "settings_keys",
        );
    }

    #[test]
    fn the_colors_match_their_snapshots_with_a_contrast_warning_at_its_row() {
        both_languages(
            || {
                let mut app = settings_with(
                    SettingsFeature::Look,
                    "[theme]\ntext = \"#4a4a4a\"\naccent = \"#e0b050\"\npreset = \"cosmic\"\n",
                );
                drop(app.update(Message::Settings(SettingsMessage::SearchChanged(
                    "text".to_owned(),
                ))));
                app
            },
            "settings_colors",
        );
    }

    #[test]
    fn an_own_ai_server_matches_its_snapshots() {
        both_languages(
            || {
                let mut app = settings_with(
                    SettingsFeature::Hermes,
                    "[modules.filing.ai]\nprovider = \"http\"\n\
                     endpoint = \"http://localhost:11434\"\nmodel = \"qwen2.5:7b\"\n\n\
                     [theme]\npreset = \"cosmic\"\n",
                );
                drop(app.update(Message::Settings(SettingsMessage::ServerDialog(true))));
                app
            },
            "settings_ai_server",
        );
    }

    #[test]
    fn a_config_that_does_not_load_is_named_at_the_top() {
        both_languages(
            || {
                App::with_settings_file(
                    SettingsFeature::Language,
                    crate::config::from_text("lang = \"en\"\n[open]\nview = \"less\"\n"),
                )
            },
            "settings_broken",
        );
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
        simulator(&app).find("Hermes on").expect("the switch");
        drop(app.update(Message::Settings(SettingsMessage::SearchChanged(
            "rules".to_owned(),
        ))));
        let switch_hidden = simulator(&app).find("Hermes on").is_err();
        assert!(switch_hidden);
        simulator(&app).find("Rules").expect("the rules row");
    }

    #[test]
    fn the_pages_of_hermes_are_always_in_the_list() {
        let app = App::with_settings_open(SettingsFeature::Hermes, hermes_config());
        let shows = |label: &str| simulator(&app).find(label).is_ok();
        for label in ["Index", "Hermes", "Rules", "Tags", "Reports", "Read errors"] {
            assert!(shows(label), "{label}");
        }
        assert!(!shows("Advanced"));
    }

    #[test]
    fn a_click_on_a_page_of_hermes_selects_it_and_the_search_finds_it_by_name() {
        let mut app = App::with_settings_open(SettingsFeature::Hermes, hermes_config());
        let mut sim = simulator(&app);
        sim.click("Read errors").expect("the page in the list");
        for message in sim.into_messages() {
            drop(app.update(message));
        }
        assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));

        drop(app.update(Message::Settings(SettingsMessage::SearchChanged(
            "Reports".to_owned(),
        ))));
        simulator(&app)
            .find("Reports")
            .expect("the search finds the page by its name");
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

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod key_bindings_dialog {
    use super::*;
    use crate::keymap::Binding;
    use crate::messages::KeyBindingsMessage;
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};

    fn opened(lang: Language) -> App {
        let mut app = App::with_fixed_panels().in_language(lang);
        drop(app.update(Message::KeyBindings(KeyBindingsMessage::Open)));
        app
    }

    /// Move asks for F5, which Copy holds: the dialog waits for the takeover.
    fn clashing(lang: Language) -> App {
        let mut app = opened(lang);
        let bare = |named| Binding::with_modifiers(Key::Named(named), Modifiers::default());
        for message in [
            KeyBindingsMessage::SearchByKey(bare(Named::F6)),
            KeyBindingsMessage::Record,
            KeyBindingsMessage::Recorded(bare(Named::F5)),
        ] {
            drop(app.update(Message::KeyBindings(message)));
        }
        app
    }

    fn assert_snapshot(app: &App, name: &str) {
        let matches = simulator(app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the shortcuts dialog does not match {name}.png");
    }

    #[test]
    fn the_dialog_lists_the_actions_with_their_keys() {
        for (lang, name) in [
            (Language::English, "key_bindings"),
            (Language::German, "key_bindings_de"),
        ] {
            assert_snapshot(&opened(lang), name);
        }
    }

    #[test]
    fn a_clash_names_the_action_that_holds_the_key() {
        for (lang, name) in [
            (Language::English, "key_bindings_clash"),
            (Language::German, "key_bindings_clash_de"),
        ] {
            assert_snapshot(&clashing(lang), name);
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
mod connect_dialog {
    use super::*;
    use crate::fs::remote::{Secret, ServerId};
    use crate::messages::{ConnectField, RemoteMessage};
    use crate::remote_dialog::{ConnectDialog, Stage};

    fn filled() -> ConnectDialog {
        let mut dialog = ConnectDialog::new(1);
        dialog.set(ConnectField::Host, "example.org".to_owned());
        dialog.set(ConnectField::User, "marc".to_owned());
        dialog.set_password(Secret::new("hunter2".to_owned()));
        dialog
    }

    fn unknown_server() -> ConnectDialog {
        let mut dialog = filled();
        let key = russh::keys::PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
        )
        .unwrap();
        dialog.set_stage(Stage::HostKey {
            fingerprint: "SHA256:abc".to_owned(),
            key: Box::new(key),
        });
        dialog
    }

    fn shows(dialog: ConnectDialog, lang: Language, label: &str) -> bool {
        simulator(&App::with_connect_dialog(dialog).in_language(lang))
            .find(label)
            .is_ok()
    }

    fn messages_after_click(dialog: ConnectDialog, label: &str) -> Vec<Message> {
        let app = App::with_connect_dialog(dialog);
        let mut sim = simulator(&app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().filter(is_action).collect()
    }

    #[test]
    fn the_form_names_its_fields_in_both_languages() {
        for label in ["Connect to server", "Host", "Port", "User", "Password"] {
            assert!(
                shows(ConnectDialog::new(1), Language::English, label),
                "{label}"
            );
        }
        for label in [
            "Mit Server verbinden",
            "Benutzer",
            "Passwort",
            "Startordner (optional)",
        ] {
            assert!(
                shows(ConnectDialog::new(1), Language::German, label),
                "{label}"
            );
        }
    }

    #[test]
    fn the_buttons_cancel_and_connect() {
        assert_eq!(
            messages_after_click(filled(), "Cancel"),
            [Message::Remote(RemoteMessage::Cancel)]
        );
        assert_eq!(
            messages_after_click(filled(), "Connect"),
            [Message::Remote(RemoteMessage::Submit)]
        );
    }

    #[test]
    fn a_locked_form_cannot_connect_again_and_says_what_it_does() {
        let mut dialog = filled();
        dialog.set_stage(Stage::Connecting);
        assert!(shows(dialog.clone(), Language::English, "Connecting…"));
        assert!(messages_after_click(dialog, "Connect").is_empty());
    }

    #[test]
    fn an_error_is_shown_under_the_fields() {
        let mut dialog = filled();
        dialog.set_error("User name or password is wrong".to_owned());
        assert!(shows(
            dialog,
            Language::English,
            "User name or password is wrong"
        ));
    }

    #[test]
    fn an_unknown_server_shows_its_fingerprint_and_trusts_only_on_the_button() {
        let host = ServerId::new("marc", "example.org", 22).host;
        let sentence = format!(
            "{host} is not known yet. Compare the fingerprint with the one the server's administrator gives you: SHA256:abc"
        );
        assert!(shows(unknown_server(), Language::English, "Unknown server"));
        assert!(shows(unknown_server(), Language::English, &sentence));
        assert_eq!(
            messages_after_click(unknown_server(), "Trust and connect"),
            [Message::Remote(RemoteMessage::TrustHostKey)]
        );
        assert_eq!(
            messages_after_click(unknown_server(), "Cancel"),
            [Message::Remote(RemoteMessage::Cancel)]
        );
    }

    #[test]
    fn the_host_key_question_names_the_port_when_it_is_not_22() {
        let mut dialog = unknown_server();
        dialog.set_stage(Stage::Editing);
        dialog.set(ConnectField::Port, "2222".to_owned());
        let key = russh::keys::PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
        )
        .unwrap();
        dialog.set_stage(Stage::HostKey {
            fingerprint: "SHA256:abc".to_owned(),
            key: Box::new(key),
        });

        assert!(shows(
            dialog,
            Language::English,
            "example.org:2222 is not known yet. Compare the fingerprint with the one the server's administrator gives you: SHA256:abc"
        ));
    }

    #[test]
    fn the_host_key_question_is_worded_in_german_too() {
        assert!(shows(
            unknown_server(),
            Language::German,
            "Unbekannter Server"
        ));
        assert!(shows(
            unknown_server(),
            Language::German,
            "Vertrauen und verbinden"
        ));
    }

    fn matches_snapshot(dialog: ConnectDialog, language: Language, name: &str) {
        let app = App::with_connect_dialog(dialog).in_language(language);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the dialog does not match tests/snapshots/{name}.png"
        );
    }

    #[test]
    fn the_form_matches_its_snapshots() {
        let mut dialog = filled();
        dialog.set(ConnectField::StartPath, "/srv/files".to_owned());
        dialog.set_error("User name or password is wrong".to_owned());
        matches_snapshot(dialog.clone(), Language::English, "connect_dialog_en");
        matches_snapshot(dialog, Language::German, "connect_dialog_de");
    }

    #[test]
    fn the_host_key_question_matches_its_snapshots() {
        matches_snapshot(unknown_server(), Language::English, "connect_hostkey_en");
        matches_snapshot(unknown_server(), Language::German, "connect_hostkey_de");
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
mod servers_page {
    use super::*;
    use crate::messages::{SettingChange, SettingsFeature, SettingsMessage};
    use std::path::PathBuf;

    fn page(source: &str) -> App {
        App::with_settings_file(
            SettingsFeature::Servers,
            Ok(crate::config::from_text(source).unwrap()),
        )
    }

    const TWO: &str = "[[servers]]\nname = \"Work\"\nhost = \"files.example.org\"\nuser = \"marc\"\npath = \"/srv\"\n\n[[servers]]\nhost = \"home.example.org\"\nport = 2222\nuser = \"marc\"\n";

    fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(label).expect("the clicked label");
        sim.into_messages().collect()
    }

    #[test]
    fn the_page_lists_each_server_with_its_address() {
        let app = page(TWO);
        let mut sim = simulator(&app);
        for label in [
            "Work",
            "marc@files.example.org /srv",
            "marc@home.example.org:2222",
        ] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
        sim.find("Add server…").unwrap();
    }

    #[test]
    fn connect_opens_the_server_and_remove_writes_the_list_without_it() {
        let app = page(TWO);
        assert_eq!(
            messages_after_click(&app, "Connect"),
            [Message::Settings(SettingsMessage::OpenServer(
                PathBuf::from("sftp://marc@files.example.org/srv")
            ))]
        );
        let sent = messages_after_click(&app, "Remove");
        let [Message::Settings(SettingsMessage::Change(SettingChange::Servers(rest)))] =
            sent.as_slice()
        else {
            panic!("{sent:?}");
        };
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].host, "home.example.org");
    }

    #[test]
    fn an_empty_list_says_so_and_offers_to_add() {
        let app = page("");
        simulator(&app).find("No saved servers").unwrap();
        assert_eq!(
            messages_after_click(&app, "Add…"),
            [Message::Settings(SettingsMessage::OpenServerAdd)]
        );
    }

    #[test]
    fn the_page_is_worded_in_german_too() {
        let app = page(TWO).in_language(Language::German);
        let mut sim = simulator(&app);
        sim.find("Entfernen").unwrap();
        sim.find("Verbinden").unwrap();
    }
}

#[cfg(test)]
mod hit_list_snapshots {
    use super::*;
    use crate::ui_tests::simulator;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn matches_snapshot(app: &App, name: &str) -> TestResult {
        let matches = simulator(app)
            .snapshot(&iced::Theme::Dark)?
            .matches_image(format!("tests/snapshots/{name}.png"))?;
        if matches {
            Ok(())
        } else {
            Err(format!("the view does not match tests/snapshots/{name}.png").into())
        }
    }

    fn both_languages(build: fn() -> App, name: &str) -> TestResult {
        matches_snapshot(
            &build().in_language(Language::English),
            &format!("{name}_en"),
        )?;
        matches_snapshot(
            &build().in_language(Language::German),
            &format!("{name}_de"),
        )
    }

    #[test]
    fn the_search_with_hit_buttons_matches_its_snapshots() -> TestResult {
        both_languages(App::with_fixed_search_hits, "search_hit_buttons")
    }

    #[test]
    fn the_hit_list_matches_its_snapshots() -> TestResult {
        both_languages(App::with_fixed_hit_list, "hit_list")
    }

    #[test]
    fn the_hit_list_context_menu_matches_its_snapshots() -> TestResult {
        both_languages(App::with_fixed_hit_list_menu, "hit_list_context_menu")
    }

    #[test]
    fn the_search_context_menu_matches_its_snapshots() -> TestResult {
        both_languages(App::with_fixed_search_hits_menu, "search_context_menu")
    }

    #[test]
    fn the_pack_prompt_of_a_hit_list_matches_its_snapshots() -> TestResult {
        both_languages(App::with_fixed_hit_list_pack, "hit_list_pack")
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
mod quick_fix_menu_actions {
    use super::*;
    use crate::messages::{QuickFixMessage, RuleFormMessage, RulesMessage};

    fn rules_form_of(app: &App) -> &crate::rule_form::RuleForm {
        app.rules_for_test().unwrap().form().unwrap()
    }

    #[test]
    fn create_rule_in_the_quick_fix_opens_the_rule_form_with_the_document_pattern() {
        let mut item = App::waiting_document_for_test().unwrap();
        item.fields.tags = std::collections::BTreeSet::from(["tax".to_owned()]);
        let mut app = App::with_quick_fix_menu(item);
        let _ = app.update(Message::QuickFix(QuickFixMessage::Hover(2)));

        let _ = app.update(Message::QuickFix(QuickFixMessage::Activate));

        let form = rules_form_of(&app);
        assert_eq!(form.target, crate::rule_form::RuleFormTarget::FromDocument);
        assert_eq!(form.conditions.rows[1].value, "company-a");
        assert_eq!(
            form.tags,
            std::collections::BTreeSet::from(["tax".to_owned()])
        );
        assert_eq!(form.filing.pattern(), "{sender}/{date}_{kind}");
        let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Cancel)));
        assert!(app.rules_for_test().is_some());
        assert!(app.rules_for_test().unwrap().form().is_none());
    }

    #[test]
    fn set_fields_in_the_quick_fix_opens_the_chat() {
        let mut app = App::with_quick_fix_menu(App::waiting_document_for_test().unwrap());
        let _ = app.update(Message::QuickFix(QuickFixMessage::Hover(1)));

        let _ = app.update(Message::QuickFix(QuickFixMessage::Activate));

        assert!(app.hermes_chat_for_test().open);
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
mod settings_index_page {
    use super::app_simulator as simulator;
    use std::path::PathBuf;

    use super::*;
    use crate::config::{AiSetting, Filing, FilingLocation};
    use crate::messages::{HermesOverviewMessage, SettingsMessage};
    use ncrs_intake::LocationMode;

    fn configured() -> Filing {
        let at = |path: &str, mode, is_target| FilingLocation {
            path: PathBuf::from(path),
            mode,
            images: false,
            is_target,
            transfer: Default::default(),
        };
        Filing {
            enabled: true,
            locations: vec![
                at("/home/test/Scans", LocationMode::Inbox, false),
                at("/home/test/Documents", LocationMode::Archive, true),
            ],
            load: Default::default(),
            ai: AiSetting::Local { threads: None },
            default_pattern: None,
            own_names: Vec::new(),
            own_ibans: Vec::new(),
            own_tax_ids: Vec::new(),
            first_run: false,
        }
    }

    fn clicks(app: &App, label: &str) -> Vec<Message> {
        let mut sim = simulator(app);
        sim.click(label).unwrap();
        sim.into_messages().collect()
    }

    fn tall_simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
        Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1200.0, 1100.0),
            app.view(),
        )
    }

    fn overview_message(message: HermesOverviewMessage) -> Message {
        Message::HermesOverview(message)
    }

    #[test]
    fn the_settings_hold_the_protection_and_the_question_before_starting_over() {
        let missing =
            App::with_index_settings(Some(Phase::Ready), configured()).with_missing_key_for_test();
        let mut sim = tall_simulator(&missing);
        sim.click("Start over …").unwrap();
        assert_eq!(
            sim.into_messages().collect::<Vec<_>>(),
            [Message::Settings(SettingsMessage::Protection(2))]
        );
        let asking = App::with_index_settings(Some(Phase::Ready), configured())
            .with_start_over_question_for_test();
        assert_eq!(
            clicks(&asking, "Cancel"),
            [overview_message(HermesOverviewMessage::CancelStartOver)]
        );
        assert_eq!(
            clicks(&asking, "Remove index"),
            [overview_message(HermesOverviewMessage::ConfirmStartOver)]
        );
    }

    fn settings_over_index(encrypted: bool) -> App {
        App::with_index_settings(Some(Phase::Ready), configured()).with_index_for_test(encrypted)
    }

    fn both_languages_tall(app: impl Fn() -> App, name: &str) {
        for (lang, suffix) in [(Language::English, "en"), (Language::German, "de")] {
            let matches = tall_simulator(&app().in_language(lang))
                .snapshot(&iced::Theme::Dark)
                .unwrap()
                .matches_image(format!("tests/snapshots/{name}_{suffix}.png"))
                .expect("snapshot comparison");
            assert!(matches, "the settings do not match {name}_{suffix}.png");
        }
    }

    #[test]
    fn the_settings_with_the_protection_match_their_snapshots() {
        both_languages_tall(|| settings_over_index(true), "settings_hermes_protection");
        both_languages_tall(
            || settings_over_index(false),
            "settings_hermes_protection_plain",
        );
        both_languages_tall(
            || settings_over_index(true).with_missing_key_for_test(),
            "settings_hermes_key_missing",
        );
        both_languages_tall(
            || settings_over_index(true).with_start_over_question_for_test(),
            "settings_hermes_reset_confirm",
        );
    }
}
