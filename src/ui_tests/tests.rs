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

/// The prompt is drawn over the panels: the view differs from the plain
/// two-panel view, whose reference image is pinned by its own test.
#[cfg(target_os = "macos")]
#[test]
fn the_prompt_is_drawn_over_the_panels() {
    let same = simulator(&app_with_prompt())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/two_panels.png")
        .expect("compare");

    assert!(
        !same,
        "the prompt changed no pixel — the overlay is not being drawn"
    );
}

/// A long name has to fit the text field, not stretch the dialog.
#[test]
fn a_long_name_does_not_change_the_layout() {
    let mut app = app_with_prompt();
    app.set_prompt("kurz", None);
    let short = frame_bounds(&app);

    app.set_prompt(&"a".repeat(300), None);

    assert_eq!(
        frame_bounds(&app),
        short,
        "a 300-character name changed the dialog; the field is not clipping"
    );
}

/// A validation message that never reaches the view is caught here.
#[test]
fn the_error_line_is_shown_in_the_prompt() {
    const ERROR: &str = "Bitte einen Namen eingeben";
    let mut app = app_with_prompt();
    app.set_prompt("kurz", None);
    assert!(simulator(&app).find(ERROR).is_err());

    app.set_prompt("kurz", Some(ERROR));
    assert!(
        simulator(&app).find(ERROR).is_ok(),
        "the error text is not being shown"
    );
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
    let messages: Vec<Message> = sim.into_messages().filter(is_action).collect();
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Panel(_))),
        "the click reached the panels behind the prompt: {messages:?}"
    );
}

/// A click on the dialog's own area reaches the prompt, not the panels.
#[test]
fn a_click_on_the_prompt_does_not_reach_the_panels() {
    let mut app = app_with_prompt();
    app.set_prompt("neuer ordner", None);
    let mut sim = simulator(&app);
    sim.click(Point::new(600.0, 380.0))
        .expect("the dialog should be clickable");
    let messages: Vec<Message> = sim.into_messages().filter(is_action).collect();
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Panel(_))),
        "the click reached the panels: {messages:?}"
    );
}

/// The confirm button of the prompt submits it.
#[test]
fn clicking_the_confirm_button_of_the_prompt_submits_it() {
    let mut app = app_with_prompt();
    app.set_prompt("neuer ordner", None);
    let mut sim = simulator(&app);
    sim.click("Create").expect("the confirm button");
    let messages: Vec<Message> = sim.into_messages().filter(is_action).collect();
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::PromptSubmit)),
        "the click did not submit: {messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Panel(_))),
        "the click reached the panels: {messages:?}"
    );
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
        .matches_reference_image("tests/snapshots/two_panels.png")
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
        .matches_reference_image(reference)
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
#[cfg(target_os = "macos")]
#[test]
fn a_configured_color_changes_the_picture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[theme]\npanel = \"#ff0000\"\n").unwrap();
    let app = App::with_fixed_panels().with_config(crate::config::load(&path));
    let same = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/two_panels.png")
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
        .matches_reference_image("tests/snapshots/prompt.png")
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
        .matches_reference_image("tests/snapshots/prompt_de.png")
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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the pack prompt does not match {name}.png");
    }
}

#[test]
fn the_pack_prompt_shows_the_chosen_format() {
    let mut app = App::with_fixed_pack_prompt().in_language(Language::English);
    drop(app.update(Message::PromptFormat(
        crate::fs::archive::ArchiveFormat::TarGz,
    )));
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/pack_prompt_tar_gz_en.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the pack prompt does not match pack_prompt_tar_gz_en.png"
    );
}

#[test]
fn the_new_file_prompt_matches_its_snapshot() {
    let app = App::with_fixed_new_file_prompt();
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/new_file_prompt.png")
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
        .matches_reference_image("tests/snapshots/new_file_prompt_de.png")
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
        .matches_reference_image("tests/snapshots/search.png")
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
        .matches_reference_image("tests/snapshots/trash.png")
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
        .matches_reference_image("tests/snapshots/quick_open.png")
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
        .matches_reference_image("tests/snapshots/quick_open_de.png")
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
        .matches_reference_image("tests/snapshots/quick_open_documents_en.png")
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
        .matches_reference_image("tests/snapshots/quick_open_documents_de.png")
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
