use std::time::{Duration, SystemTime};

use ncrs_pipeline::{RunnerState, Snapshot};

use super::*;
use crate::messages::{FilingStatusMessage, HermesPage, PanelSide, SettingsMessage};
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
fn a_click_on_the_hermes_chip_opens_the_activity_page() {
    let mut app = with_hermes().with_starting_hermes_for_test();
    let messages = messages_after_click(&app, HERMES_CHIP_ID);
    assert_eq!(
        messages,
        vec![Message::FilingStatus(FilingStatusMessage::Open)]
    );
    for message in messages {
        drop(app.update(message));
    }
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Activity));
}

#[test]
fn the_chip_says_hermes_is_off_while_the_switch_is_off() {
    let shows_chip = |app: &App| {
        simulator(app)
            .find(iced::widget::Id::new(HERMES_CHIP_ID))
            .is_ok()
    };
    let switched_off = || {
        App::with_settings_file(
            crate::messages::SettingsFeature::Look,
            Ok(crate::config::from_text("[hermes]\nenabled = false\n").unwrap()),
        )
    };
    let off = switched_off();
    assert!(shows_chip(&off));
    assert_eq!(off.hermes_chip_for_test().as_deref(), Some("Hermes off"));
    let german = switched_off().in_language(Language::German);
    assert_eq!(german.hermes_chip_for_test().as_deref(), Some("Hermes aus"));
    assert!(shows_chip(&with_hermes()));
}

#[test]
fn a_click_on_the_off_chip_opens_the_ai_settings() {
    let mut app = App::with_settings_file(
        crate::messages::SettingsFeature::Look,
        Ok(crate::config::from_text("[hermes]\nenabled = false\n").unwrap()),
    );
    let messages = messages_after_click(&app, HERMES_CHIP_ID);
    assert_eq!(messages, vec![Message::ShowAiSettings]);
    for message in messages {
        drop(app.update(message));
    }
    simulator(&app)
        .find("One model for everything: sorting, chat (F2), rules, name suggestions.")
        .unwrap();
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
fn a_failed_request_to_the_own_server_shows_in_the_chip() {
    let provider = crate::filing::http_ai_provider(
        "http://127.0.0.1:1/v1",
        "model-x",
        &ncrs_ai::ConsentedOrigins::default(),
        std::path::Path::new("index.db"),
    )
    .unwrap();
    let mut snapshot = Snapshot::off();
    snapshot.state = RunnerState::Running;
    snapshot.documents_per_minute[1] = 78;
    let app = App::with_filing_snapshot(snapshot, taken_at())
        .in_language(Language::English)
        .with_filing_module_for_test(crate::filing::FilingModule::in_phase_with_own_server(
            crate::filing::Phase::Ready,
            std::sync::Arc::clone(&provider),
            "http://127.0.0.1:1",
            "model-x",
        ));
    let chip_before = app.hermes_chip_for_test();

    ncrs_ai::Provider::extract(
        provider.as_ref(),
        &ncrs_ai::ExtractionRequest {
            text: "Invoice 4711".to_owned(),
            open_fields: Vec::new(),
            known: ncrs_index::DocumentFields::default(),
            source: std::path::PathBuf::from("inbox"),
            kinds: std::sync::Arc::default(),
            language: ncrs_index::KindLanguage::default(),
            judge_document: false,
        },
    )
    .unwrap_err();

    let problem = ncrs_i18n::fill_template(
        Language::English.text(ncrs_i18n::Msg::TitleBarHermesServerProblem),
        &[(
            "{problem}",
            &crate::hermes_overview::server_failure_text(
                ncrs_ai::ReplyFailure::NoAnswer,
                Language::English,
            ),
        )],
    );
    assert_eq!(chip_before.as_deref(), Some("Hermes"));
    assert_eq!(app.hermes_chip_for_test(), Some(problem));
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
