use std::time::{Duration, SystemTime};

use ncrs_pipeline::{RunnerState, Snapshot};

use super::*;
use crate::messages::{PanelSide, SettingsMessage};
use crate::ui::titlebar::{LEFT_TAB_ID, RIGHT_TAB_ID};

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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the main view does not match tests/snapshots/{name}.png"
        );
    }
}

/// Names that differ from the first letter on, as real repositories do.
const REPOSITORY_NAMES: [&str; 15] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
    "kilo", "lima", "mike", "november", "oscar",
];

/// Settings, Monitor and `git_tabs` Git tabs in a window as wide as Marc's.
fn crowded_title_bar(git_tabs: usize) -> App {
    let size = iced::Size::new(1740.0, 1130.0);
    let mut app = App::with_fixed_panels().with_window_size_for_test(size);
    drop(app.update(Message::Settings(SettingsMessage::Toggle)));
    drop(app.update(Message::Monitor(ncrs_monitor::MonitorMessage::Toggle)));
    for listed in REPOSITORY_NAMES.iter().take(git_tabs) {
        let name = (*listed).to_owned();
        let mut git = ncrs_git_view::State::default();
        drop(ncrs_git_view::open(
            &mut git,
            std::path::PathBuf::from(format!("/repo/{name}")),
            name,
            &ncrs_git_view::Ctx::new(Language::English),
        ));
        app = app.showing_git_for_test(git);
    }
    app
}

fn matches_title_bar_snapshot(app: &App, name: &str) {
    let matches = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1740.0, 1130.0),
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_reference_image(format!("tests/snapshots/{name}.png"))
    .expect("snapshot comparison");
    assert!(
        matches,
        "the title bar does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn a_few_page_tabs_keep_the_full_tab_width() {
    matches_title_bar_snapshot(&crowded_title_bar(3), "title_bar_tabs_five");
}

#[test]
fn many_page_tabs_shrink_and_keep_the_active_one_in_view() {
    matches_title_bar_snapshot(&crowded_title_bar(8), "title_bar_tabs_ten");
}

#[test]
fn too_many_page_tabs_scroll_to_the_active_one() {
    matches_title_bar_snapshot(&crowded_title_bar(15), "title_bar_tabs_scrolled_end");
}

#[test]
fn a_tab_selected_by_the_keys_scrolls_into_view() {
    let mut app = crowded_title_bar(15);
    for _ in 0..13 {
        drop(app.update(Message::PreviousTab));
    }
    matches_title_bar_snapshot(&app, "title_bar_tabs_scrolled_second");
}
