use super::app_simulator as simulator;
use std::path::PathBuf;

use ncrs_config::config::{SourceSetting, TargetSetting};
use ncrs_index::TargetId;
use ncrs_intake::{LocationMode, LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget};

use super::*;
use crate::config::{AiSetting, Filing, FilingLocation};
use crate::fs::OpenKind;
use crate::messages::{HermesPage, SettingChange, SettingField, SettingsFeature, SettingsMessage};
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
                continuous: false,
            },
            FilingLocation {
                path: PathBuf::from("/home/test/Documents/Filed"),
                mode: LocationMode::Archive,
                images: false,
                is_target: true,
                transfer: Default::default(),
                continuous: false,
            },
        ],
        load: Default::default(),
        ai: AiSetting::Local { threads: None },
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
        first_run: false,
        targets: vec![TargetSetting {
            target: SortTarget {
                id: documents(),
                name: "Documents".to_owned(),
                path: PathBuf::from("/home/test/Documents/Filed"),
                own_volume: Some(false),
            },
            paused: None,
        }],
        sources: vec![SourceSetting {
            source: SortSource {
                path: PathBuf::from("/home/test/Downloads/Scans"),
                include: "*.pdf".to_owned(),
                exclude: String::new(),
                targets: vec![documents()],
                watch: true,
                transfer: LocationTransfer::Move,
                no_text: OnNoText::Ask,
                several: OnSeveralTargets::Ask,
                own_volume: Some(false),
            },
            paused: None,
        }],
        dropped_entries: Vec::new(),
    }
}

fn documents() -> TargetId {
    TargetId::new("documents").expect("a valid id")
}

/// The filing page with a source and a target, as it is before the disk
/// answered.
pub fn filing_page_app() -> App {
    let mut app = App::with_settings_open(SettingsFeature::Hermes, hermes_config());
    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Hermes,
    ))));
    app
}

fn matches_snapshot(app: &App, name: &str) {
    let matches = simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
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
        let app =
            App::with_settings_open(SettingsFeature::Look, Filing::default()).in_language(language);
        let mut sim = simulator(&app);
        sim.point_at(iced::Point::new(700.0, 400.0));
        let _statuses = sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -80.0 },
        })]);
        let matches = sim
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the tab does not match tests/snapshots/{name}.png");
    };
    scrolled_snapshot(Language::German, "settings_appearance_tags_de");
    scrolled_snapshot(Language::English, "settings_appearance_tags_en");
}

#[test]
fn the_filing_page_is_marked_beta_in_both_languages() {
    for language in [Language::English, Language::German] {
        let app =
            App::with_settings_open(SettingsFeature::Hermes, hermes_config()).in_language(language);
        assert!(simulator(&app).find("Beta").is_ok(), "{language:?}");
    }
}

#[test]
fn the_filing_page_matches_its_snapshots() {
    both_languages(filing_page_app, "settings_hermes");
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
    assert!(messages_after_click(&app, "Dracula").iter().any(|message| {
        matches!(
            message,
            Message::Settings(SettingsMessage::FocusThen(_, Some(inner)))
                if **inner == SettingsMessage::Change(SettingChange::Preset(ThemePreset::Dracula))
        )
    }));
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
    let mut app = App::with_settings_open(SettingsFeature::Ai, hermes_config());
    simulator(&app).find("On").expect("the switch");
    drop(app.update(Message::Settings(SettingsMessage::SearchChanged(
        "rules".to_owned(),
    ))));
    let switch_hidden = simulator(&app).find("On").is_err();
    assert!(switch_hidden);
    simulator(&app).find("Rules").expect("the rules row");
}

#[test]
fn the_pages_of_hermes_are_always_in_the_list() {
    let app = App::with_settings_open(SettingsFeature::Hermes, hermes_config());
    let shows = |label: &str| simulator(&app).find(label).is_ok();
    for label in ["Index", "Sorting", "Rules", "Tags", "Reports", "Read errors"] {
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
