use std::path::Path;

use crate::facts::{DocumentsFacts, FavouritesFacts};
use crate::message::{SettingChange, SettingField};
use ncrs_config::config::{
    AiEndpointProblem, AiProviderChoice, AiProviderName, Filing, OwnDetail, ProgramLine,
    ProgramLineProblem,
};
use ncrs_config::SettingsFeature;
use ncrs_fs::OpenKind;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::{ColorRole, Palette, ThemePreset};

use super::*;

fn state_of(source: &str) -> SettingsState {
    SettingsState::new(
        false,
        Ok(Box::new(ncrs_config::config::from_text(source).unwrap())),
        false,
    )
}

fn typed(state: &mut SettingsState, field: SettingField, text: &str) -> Option<SettingChange> {
    state.edit(field, text.to_owned());
    state.submit(field)
}

#[test]
fn searching_the_page_name_lists_the_page_and_shows_all_its_rows() {
    let filing = Filing::default();
    let palette = Palette::default();
    for typed_query in ["hermes", "HERMES"] {
        let mut state = state_of("");
        state.query = typed_query.to_owned();
        let view = SettingsView {
            state: &state,
            preset: ThemePreset::Cosmic,
            vscode_theme: None,
            filing: &filing,
            servers: &[],
            protection: None,
            home: Path::new("/home/test"),
            keys_changed: 0,
            updates_listed: true,
            facts: DocumentsFacts::default(),
            favourites: FavouritesFacts::default(),
            lang: Language::EnglishFuturama,
            palette: &palette,
        };

        assert!(listed_features(&view).contains(&SettingsFeature::Hermes));
        let query = typed_query.to_lowercase();
        assert!(feature_name_matches(
            SettingsFeature::Hermes,
            &query,
            Language::EnglishFuturama
        ));
        assert!(groups_of(SettingsFeature::Hermes, &view)
            .into_iter()
            .flat_map(|group| group.rows)
            .any(|setting| !row_matches(&setting, &query)));
    }
}

#[test]
fn the_command_list_gets_every_page_and_every_row_the_tab_search_filters() {
    let state = state_of("");
    let filing = Filing::default();
    let palette = Palette::default();
    let view = SettingsView {
        state: &state,
        preset: ThemePreset::Cosmic,
        vscode_theme: None,
        filing: &filing,
        servers: &[],
        protection: None,
        home: Path::new("/home/test"),
        keys_changed: 0,
        updates_listed: true,
        facts: DocumentsFacts::default(),
        favourites: FavouritesFacts::default(),
        lang: Language::English,
        palette: &palette,
    };

    let targets = search_targets(&view);

    for feature in SettingsFeature::ALL {
        let page = targets
            .iter()
            .find(|target| target.feature == feature && target.row.is_none())
            .unwrap();
        assert!(page.label.starts_with("Settings: "), "{}", page.label);
        let rows: Vec<String> = groups_of(feature, &view)
            .into_iter()
            .flat_map(|group| group.rows)
            .filter(|setting| setting.in_command_list)
            .map(|setting| setting.label)
            .collect();
        let listed: Vec<String> = targets
            .iter()
            .filter(|target| target.feature == feature)
            .filter_map(|target| target.row.clone())
            .collect();
        assert_eq!(listed, rows, "{feature:?}");
    }
    assert!(targets
        .iter()
        .any(|target| target.row.as_deref() == Some("Viewer (F3)")));
}

#[test]
fn showing_a_row_filters_to_it_and_marks_it_and_showing_a_page_clears_both() {
    let mut state = state_of("");
    state.show(SettingsFeature::Programs, Some("Viewer (F3)".to_owned()));
    assert_eq!(state.feature, SettingsFeature::Programs);
    assert_eq!(state.query, "Viewer (F3)");
    assert_eq!(state.marked_row.as_deref(), Some("Viewer (F3)"));

    state.show(SettingsFeature::Keys, None);
    assert_eq!(state.query, "");
    assert_eq!(state.marked_row, None);
}

#[test]
fn a_field_shows_the_file_until_something_is_typed() {
    let mut state = state_of("[open]\nview = [\"less\", \"-R\"]\n[theme]\naccent = \"#abc\"\n");
    let viewer = SettingField::Program(OpenKind::View);
    assert_eq!(state.field_text(viewer), "less -R");
    assert_eq!(
        state.field_text(SettingField::Color(ColorRole::Accent)),
        "#abc"
    );
    state.edit(viewer, "bat".to_owned());
    assert_eq!(state.field_text(viewer), "bat");
}

#[test]
fn leaving_writes_what_was_typed_without_enter_and_skips_what_did_not_change() {
    let mut state = state_of("[open]\nview = [\"less\", \"-R\"]\n");
    state.edit(
        SettingField::Program(OpenKind::View),
        " less -R ".to_owned(),
    );
    state.edit(SettingField::Color(ColorRole::Accent), "#ff0".to_owned());
    state.edit(SettingField::Program(OpenKind::Edit), "vim".to_owned());
    assert_eq!(
        state.leave(),
        Some(vec![
            SettingChange::Program {
                kind: OpenKind::Edit,
                line: Some(ProgramLine::new("vim", &[])),
            },
            SettingChange::Color {
                role: ColorRole::Accent,
                color: Some("#ff0".to_owned()),
            },
        ])
    );
}

#[test]
fn leaving_with_a_wrong_value_writes_nothing_and_shows_it() {
    let mut state = state_of("");
    let accent = SettingField::Color(ColorRole::Accent);
    state.edit(accent, "red".to_owned());
    state.edit(SettingField::Program(OpenKind::View), "bat".to_owned());
    assert_eq!(state.leave(), None);
    assert_eq!(state.problems.get(&accent), Some(&FieldProblem::Color));
}

#[test]
fn leaving_writes_an_own_server_once() {
    let mut state = state_of("");
    assert_eq!(state.choose_provider(AiProviderName::Http), None);
    state.edit(
        SettingField::AiEndpoint,
        "http://localhost:11434".to_owned(),
    );
    state.edit(SettingField::AiModel, "qwen".to_owned());
    assert_eq!(
        state.leave(),
        Some(vec![SettingChange::AiProvider(AiProviderChoice::Http {
            endpoint: "http://localhost:11434".to_owned(),
            model: "qwen".to_owned(),
        })])
    );
}

#[test]
fn enter_without_typing_writes_nothing() {
    let mut state = state_of("");
    assert_eq!(state.submit(SettingField::AiModel), None);
    assert!(state.problems.is_empty());
}

#[test]
fn a_wrong_value_stays_at_its_row_and_is_not_written() {
    let mut state = state_of("");
    let accent = SettingField::Color(ColorRole::Accent);
    assert_eq!(typed(&mut state, accent, "red"), None);
    assert_eq!(state.problems.get(&accent), Some(&FieldProblem::Color));
    let editor = SettingField::Program(OpenKind::Edit);
    assert_eq!(typed(&mut state, editor, "bin/edit"), None);
    assert_eq!(
        state.problems.get(&editor),
        Some(&FieldProblem::Program(ProgramLineProblem::RelativePath))
    );
    state.edit(accent, "#ff0".to_owned());
    assert!(!state.problems.contains_key(&accent));
}

#[test]
fn an_own_detail_joins_the_list_of_the_file_and_a_wrong_iban_is_refused() {
    let mut state = state_of("[modules.filing]\nown_names = [\"Max Muster\"]\n");
    let names = SettingField::Own(OwnDetail::Names);

    assert_eq!(
        typed(&mut state, names, " Muster GmbH "),
        Some(SettingChange::OwnDetails {
            detail: OwnDetail::Names,
            entries: vec!["Max Muster".to_owned(), "Muster GmbH".to_owned()],
        })
    );
    assert_eq!(
        typed(&mut state, names, "Max Muster"),
        Some(SettingChange::OwnDetails {
            detail: OwnDetail::Names,
            entries: vec!["Max Muster".to_owned()],
        })
    );
    assert_eq!(typed(&mut state, names, "  "), None);
    assert!(!state.drafts.contains_key(&names));

    let ibans = SettingField::Own(OwnDetail::Ibans);
    assert_eq!(typed(&mut state, ibans, "DE00 1234"), None);
    assert_eq!(state.problems.get(&ibans), Some(&FieldProblem::NotAnIban));
    assert_eq!(
        typed(&mut state, ibans, "DE89 3704 0044 0532 0130 00"),
        Some(SettingChange::OwnDetails {
            detail: OwnDetail::Ibans,
            entries: vec!["DE89 3704 0044 0532 0130 00".to_owned()],
        })
    );
}

#[test]
fn a_right_value_becomes_its_change_and_an_empty_one_drops_the_line() {
    let mut state = state_of("");
    assert_eq!(
        typed(&mut state, SettingField::Color(ColorRole::Text), " #FFF "),
        Some(SettingChange::Color {
            role: ColorRole::Text,
            color: Some("#FFF".to_owned()),
        })
    );
    assert_eq!(
        typed(&mut state, SettingField::Program(OpenKind::View), "  "),
        Some(SettingChange::Program {
            kind: OpenKind::View,
            line: None,
        })
    );
}

#[test]
fn an_own_server_waits_for_its_address_and_model() {
    let mut state = state_of("");
    assert_eq!(state.choose_provider(AiProviderName::Http), None);
    assert_eq!(state.provider(), AiProviderName::Http);
    let endpoint = SettingField::AiEndpoint;
    assert_eq!(typed(&mut state, endpoint, "http://localhost:11434"), None);
    assert_eq!(
        state.problems.get(&SettingField::AiModel),
        Some(&FieldProblem::Needed)
    );
    assert_eq!(typed(&mut state, endpoint, "http://example.com"), None);
    assert_eq!(
        state.problems.get(&endpoint),
        Some(&FieldProblem::Endpoint(AiEndpointProblem::PlainHttpFar))
    );
    state.edit(endpoint, "http://127.0.0.1:8080".to_owned());
    assert_eq!(
        typed(&mut state, SettingField::AiModel, "qwen"),
        Some(SettingChange::AiProvider(AiProviderChoice::Http {
            endpoint: "http://127.0.0.1:8080".to_owned(),
            model: "qwen".to_owned(),
        }))
    );
}

#[test]
fn going_back_to_this_machine_is_written_at_once_and_only_when_it_changes() {
    let mut local = state_of("");
    assert_eq!(local.choose_provider(AiProviderName::Local), None);
    let mut http = state_of(
        "[modules.filing.ai]\nprovider = \"http\"\nendpoint = \"http://localhost:1\"\nmodel = \"m\"\n",
    );
    assert_eq!(
        http.choose_provider(AiProviderName::Local),
        Some(SettingChange::AiProvider(AiProviderChoice::Local {
            threads: None
        }))
    );
}

#[test]
fn a_written_change_settles_its_fields_and_a_refused_one_shows_at_them() {
    let mut state = state_of("");
    let field = SettingField::Color(ColorRole::Accent);
    let change = typed(&mut state, field, "#123").unwrap();
    assert!(state.refused(&change, "disk full".to_owned()));
    assert_eq!(
        state.problems.get(&field),
        Some(&FieldProblem::Write("disk full".to_owned()))
    );
    let config = ncrs_config::config::from_text("[theme]\naccent = \"#123\"\n").unwrap();
    state.written(&change, &config);
    assert!(state.drafts.is_empty() && state.problems.is_empty());
    assert_eq!(state.saved_color(ColorRole::Accent), Some("#123"));
    assert_eq!(state.changed, 1);
    assert!(!state.refused(&SettingChange::VscodeTheme(None), String::new()));
}

#[test]
fn a_picked_app_is_opened_with_open_and_the_app_store_build_takes_nothing_else() {
    let mut state = state_of("");
    let tool = state.picked_program(OpenKind::Edit, Path::new("/usr/local/bin/hx"));
    assert_eq!(
        tool,
        Some(SettingChange::Program {
            kind: OpenKind::Edit,
            line: Some(ProgramLine::new("/usr/local/bin/hx", &[])),
        })
    );
    state.sandboxed = true;
    assert_eq!(
        state.picked_program(OpenKind::Edit, Path::new("/usr/local/bin/hx")),
        None
    );
    assert_eq!(
        state.problems.get(&SettingField::Program(OpenKind::Edit)),
        Some(&FieldProblem::OnlyApps)
    );
    if cfg!(target_os = "macos") {
        assert_eq!(
            state.picked_program(OpenKind::View, Path::new("/Applications/Preview.app")),
            Some(SettingChange::Program {
                kind: OpenKind::View,
                line: Some(ProgramLine::opening_app("/Applications/Preview.app")),
            })
        );
    }
}

/// Applies `change` the way the page does: written to the file, then read
/// back into the state.
fn applied(state: &mut SettingsState, path: &Path, change: SettingChange) {
    let config = crate::write::write_setting_change(path, change.clone()).unwrap();
    state.written(&change, &config);
}

#[test]
fn a_chosen_program_can_be_reset_to_the_system_default_and_the_file_loses_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    let mut state = state_of("");
    let viewer = SettingField::Program(OpenKind::View);
    assert!(!state.can_reset_program(OpenKind::View));

    let chosen = state
        .picked_program(OpenKind::View, Path::new("lookat"))
        .unwrap();
    applied(&mut state, &path, chosen);
    assert!(state.can_reset_program(OpenKind::View));
    assert_eq!(state.field_text(viewer), "lookat");
    assert!(std::fs::read_to_string(&path).unwrap().contains("lookat"));

    applied(
        &mut state,
        &path,
        SettingChange::Program {
            kind: OpenKind::View,
            line: None,
        },
    );
    assert!(!state.can_reset_program(OpenKind::View));
    assert_eq!(state.field_text(viewer), "");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("lookat"), "{saved}");
    assert!(ncrs_config::config::load(&path)
        .unwrap()
        .open
        .view
        .is_none());
}

#[test]
fn a_typed_program_offers_the_reset_before_it_is_saved_and_clearing_it_drops_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    let mut state = state_of("");
    let editor = SettingField::Program(OpenKind::Edit);
    state.edit(editor, "vim".to_owned());
    assert!(state.can_reset_program(OpenKind::Edit));

    let typed_change = state.submit(editor).unwrap();
    applied(&mut state, &path, typed_change);
    assert_eq!(state.field_text(editor), "vim");

    state.edit(editor, "  ".to_owned());
    let cleared = state.leave().unwrap();
    assert_eq!(
        cleared,
        vec![SettingChange::Program {
            kind: OpenKind::Edit,
            line: None,
        }]
    );
    for change in cleared {
        applied(&mut state, &path, change);
    }
    assert_eq!(state.field_text(editor), "");
    assert!(ncrs_config::config::load(&path)
        .unwrap()
        .open
        .edit
        .is_none());
}

fn found(name: &str, program: &str, args: &[&str]) -> ncrs_fs::KnownTerminal {
    ncrs_fs::KnownTerminal {
        name: name.to_owned(),
        line: ProgramLine::new(program, args),
    }
}

#[test]
fn the_terminal_choice_round_trips_through_the_open_section() {
    use crate::message::TerminalChoice;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    let mut state = state_of("");
    state.terminals = vec![
        found("Konsole", "konsole", &["--workdir", "{path}"]),
        found("foot", "foot", &["--working-directory={path}"]),
    ];
    let saved = |config: &Path| ncrs_config::config::load(config).unwrap().open.terminal;
    assert_eq!(state.terminal_choice(), TerminalChoice::Automatic);

    let change = state.choose_terminal(TerminalChoice::Detected(1)).unwrap();
    applied(&mut state, &path, change);
    assert_eq!(state.terminal_choice(), TerminalChoice::Detected(1));
    assert_eq!(
        saved(&path),
        Some(ProgramLine::new("foot", &["--working-directory={path}"]))
    );
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("terminal = [\"foot\", \"--working-directory={path}\"]"));

    let own = SettingField::Program(OpenKind::Terminal);
    assert_eq!(state.choose_terminal(TerminalChoice::Own), None);
    assert_eq!(state.terminal_choice(), TerminalChoice::Own);
    let typed_change = typed(&mut state, own, "myterm --cd {path}").unwrap();
    applied(&mut state, &path, typed_change);
    assert_eq!(state.terminal_choice(), TerminalChoice::Own);
    assert_eq!(
        saved(&path),
        Some(ProgramLine::new("myterm", &["--cd", "{path}"]))
    );

    let back_to_automatic = state.choose_terminal(TerminalChoice::Automatic).unwrap();
    applied(&mut state, &path, back_to_automatic);
    assert_eq!(state.terminal_choice(), TerminalChoice::Automatic);
    assert_eq!(saved(&path), None);
}

#[test]
fn own_command_shows_its_field_before_anything_is_saved_and_reset_drops_it() {
    use crate::message::TerminalChoice;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    let mut state = state_of("");
    assert_eq!(state.choose_terminal(TerminalChoice::Own), None);
    assert_eq!(state.terminal_choice(), TerminalChoice::Own);
    assert!(!state.can_reset_program(OpenKind::Terminal));

    applied(
        &mut state,
        &path,
        SettingChange::Program {
            kind: OpenKind::Terminal,
            line: None,
        },
    );
    assert_eq!(state.terminal_choice(), TerminalChoice::Automatic);
}

#[test]
fn an_off_ai_with_the_model_on_disk_offers_the_switch_not_the_download() {
    use super::index_page::{ai_button, AiButton};
    use crate::facts::AiState;

    assert_eq!(ai_button(&AiState::Off, true), Some(AiButton::TurnOn));
    assert_eq!(ai_button(&AiState::Off, false), Some(AiButton::Load));
    assert_eq!(ai_button(&AiState::Missing, true), Some(AiButton::Load));
    assert_eq!(ai_button(&AiState::Failed, true), Some(AiButton::Load));
    assert_eq!(ai_button(&AiState::Ready, true), Some(AiButton::Remove));
    assert_eq!(ai_button(&AiState::Downloading, false), None);
}

#[test]
fn a_saved_unknown_terminal_is_the_own_entry_in_the_sandbox() {
    use crate::message::TerminalChoice;

    let mut state = state_of("[open]\nterminal = [\"myterm\", \"--cd\", \"{path}\"]\n");
    state.sandboxed = true;
    state.terminals = vec![found("foot", "foot", &["--working-directory={path}"])];

    assert_eq!(state.terminal_choice(), TerminalChoice::Own);
}

fn rows_of(feature: SettingsFeature, state: &SettingsState, lang: Language) -> Vec<String> {
    let filing = Filing::default();
    let palette = Palette::default();
    let view = SettingsView {
        state,
        preset: ThemePreset::Cosmic,
        vscode_theme: None,
        filing: &filing,
        servers: &[],
        protection: None,
        home: Path::new("/home/test"),
        keys_changed: 0,
        updates_listed: true,
        facts: DocumentsFacts::default(),
        favourites: FavouritesFacts::default(),
        lang,
        palette: &palette,
    };
    groups_of(feature, &view)
        .into_iter()
        .flat_map(|group| group.rows)
        .map(|setting| setting.label)
        .collect()
}

#[test]
fn the_columns_page_lists_every_column_of_this_system_in_both_languages() {
    let state = state_of("");
    for lang in [Language::English, Language::German] {
        let labels = rows_of(SettingsFeature::Columns, &state, lang);
        let columns = ncrs_core::Column::ALL
            .into_iter()
            .filter(|column| column.on_this_system())
            .count();
        // Panels, the columns and the restore button, date and size.
        assert_eq!(labels.len(), 1 + columns + 1 + 2, "{lang:?}");
        assert_eq!(
            labels.first().map(String::as_str),
            Some(lang.text(Msg::SettingsRowColumnsPanels))
        );
    }
}

#[test]
fn the_look_page_has_the_font_size_and_preview_rows_one_after_the_other() {
    let state = state_of("");
    for lang in [Language::English, Language::German] {
        let labels = rows_of(SettingsFeature::Look, &state, lang);
        let expected = [
            Msg::SettingsRowFont,
            Msg::SettingsRowFontSize,
            Msg::SettingsRowFontPreview,
        ]
        .map(|msg| lang.text(msg));
        let at = labels
            .iter()
            .position(|label| label == lang.text(Msg::SettingsRowFont))
            .unwrap();
        let found: Vec<&str> = labels.iter().skip(at).take(3).map(String::as_str).collect();
        assert_eq!(found, expected, "{lang:?}");
    }
}

#[test]
fn the_keyboard_starts_the_hermes_page_on_its_first_row() {
    let state = state_of("");
    let filing = Filing::default();
    let palette = Palette::default();
    let view = SettingsView {
        state: &state,
        preset: ThemePreset::Cosmic,
        vscode_theme: None,
        filing: &filing,
        servers: &[],
        protection: None,
        home: Path::new("/home/test"),
        keys_changed: 0,
        updates_listed: true,
        facts: DocumentsFacts::default(),
        favourites: FavouritesFacts::default(),
        lang: Language::English,
        palette: &palette,
    };
    let hermes_keys =
        ncrs_ui_kit::setting_rows::keyed_rows(groups_of(SettingsFeature::Hermes, &view), |_| true);
    assert!(matches!(
        hermes_keys.first(),
        Some(ncrs_ui_kit::setting_rows::RowKey::Press(_))
    ));
}
