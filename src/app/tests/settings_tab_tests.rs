use crate::app::*;
use crate::messages::{SettingField, SettingsFeature};
use crate::palette::ColorRole;

fn open(app: &mut App) {
    drop(app.update(Message::Settings(SettingsMessage::Toggle)));
}

fn press(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, modifiers)
}

#[test]
fn the_key_opens_the_tab_and_closes_it_again() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    assert!(app.settings.page.is_some());
    open(&mut app);
    assert!(app.settings.page.is_none());
}

#[test]
fn escape_closes_it_and_a_panel_tab_only_shows_the_panels() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    // Escape is `InputState`'s, which sees it also in a captured text field.
    assert_eq!(
        press(&app, Key::Named(Named::Escape), Modifiers::empty()),
        None
    );
    drop(app.update(Message::Panel(PanelMessage::ActivatePanel(
        PanelSide::Right,
    ))));
    assert!(app.settings.page.is_some());
    assert_eq!(app.shown_tab, ShownTab::Panels);
    assert!(!app.settings_shown());
}

fn tab(app: &mut App, modifiers: Modifiers) {
    let routed = press(app, Key::Named(Named::Tab), modifiers).unwrap_or_else(|| {
        panic!(
            "Tab {modifiers:?} not routed at {:?} {:?}",
            app.shown_tab, app.panels.active
        )
    });
    match routed {
        // The page asks iced whether a text field has the keyboard; none has.
        Message::TabInPage { backward } => drop(app.update(Message::TabInPageAnswered {
            backward,
            field_focused: false,
        })),
        other => drop(app.update(other)),
    }
}

#[test]
fn tab_stays_on_the_page_and_ctrl_tab_walks_the_tabs_and_closes_none() {
    let mut app = App::with_fixed_panels();
    app.panels.active = PanelSide::Left;
    open(&mut app);
    drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
    assert!(app.settings.page.is_some() && app.monitor.is_open());
    assert_eq!(app.shown_tab, ShownTab::Monitor);
    // Monitor -> Panels -> Settings -> Monitor, Ctrl+Tab each time.
    for expected in [ShownTab::Panels, ShownTab::Settings, ShownTab::Monitor] {
        tab(&mut app, Modifiers::CTRL);
        assert_eq!(app.shown_tab, expected);
    }
    // Back with Ctrl+Shift+Tab.
    tab(&mut app, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(app.shown_tab, ShownTab::Settings);
    // Tab on the page never changes the tab.
    for modifiers in [Modifiers::empty(), Modifiers::empty(), Modifiers::SHIFT] {
        tab(&mut app, modifiers);
        assert_eq!(app.shown_tab, ShownTab::Settings);
    }
    assert!(app.settings.page.is_some() && app.monitor.is_open());
}

#[test]
fn tab_on_the_page_moves_between_the_page_list_and_the_rows() {
    use ncrs_settings::focus::Area;
    let mut app = App::with_fixed_panels();
    open(&mut app);
    assert_eq!(app.settings.page.as_ref().unwrap().focus.area, Area::Pages);
    tab(&mut app, Modifiers::empty());
    assert_eq!(app.settings.page.as_ref().unwrap().focus.area, Area::Rows);
    tab(&mut app, Modifiers::SHIFT);
    assert_eq!(app.settings.page.as_ref().unwrap().focus.area, Area::Pages);
    assert_eq!(app.shown_tab, ShownTab::Settings);
}

#[test]
fn ctrl_tab_changes_the_tab_while_a_settings_text_field_has_the_keyboard() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    app.settings.page.as_mut().unwrap().focus.field = true;
    assert_eq!(
        press(&app, Key::Named(Named::Tab), Modifiers::CTRL),
        Some(Message::NextTab)
    );
    tab(&mut app, Modifiers::CTRL);
    assert_eq!(app.shown_tab, ShownTab::Panels);
    assert!(app.settings.page.is_some());
}

#[test]
fn a_click_on_a_page_tab_shows_it_and_a_panel_click_shows_the_panels() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
    drop(app.update(Message::ShowTab(ShownTab::Settings)));
    assert_eq!(app.shown_tab, ShownTab::Settings);
    drop(app.update(Message::Panel(PanelMessage::ActivatePanel(PanelSide::Left))));
    assert_eq!(app.shown_tab, ShownTab::Panels);
    assert!(app.settings.page.is_some() && app.monitor.is_open());
    // Closing the shown page leaves the other one open.
    drop(app.update(Message::ShowTab(ShownTab::Monitor)));
    drop(app.update(Message::Monitor(MonitorMessage::Close)));
    assert!(app.settings.page.is_some());
    assert_eq!(app.shown_tab, ShownTab::Panels);
}

#[test]
fn tab_on_the_page_is_routed_to_the_page_and_never_switches_the_tab() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    assert_eq!(
        press(&app, Key::Named(Named::Tab), Modifiers::empty()),
        Some(Message::TabInPage { backward: false })
    );
    for field_focused in [true, false] {
        drop(app.update(Message::TabInPageAnswered {
            backward: false,
            field_focused,
        }));
        assert_eq!(app.shown_tab, ShownTab::Settings);
    }
}

#[test]
fn the_panel_keys_do_nothing_while_it_is_open() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    let cursor = app.panel(PanelSide::Left).selected;
    assert_eq!(
        press(&app, Key::Named(Named::ArrowDown), Modifiers::empty()),
        Some(Message::Settings(SettingsMessage::FocusKey(
            ncrs_settings::focus::FocusKey::Down
        )))
    );
    assert_eq!(app.panel(PanelSide::Left).selected, cursor);
}

#[test]
fn a_change_without_a_config_place_is_reported_and_applies_nothing() {
    let mut app = App::with_fixed_panels();
    app.config_path = None;
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Change(
        SettingChange::Language(Language::German),
    ))));
    assert!(app.settings.failure.is_some());
    assert_eq!(app.settings.page.as_ref().unwrap().changed, 0);
}

#[test]
fn a_written_language_is_applied_and_counted() {
    let mut app = App::with_fixed_panels().in_language(Language::English);
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Language(Language::German),
        result: Ok(Box::default()),
    })));
    assert_eq!(app.lang, Language::German);
    assert_eq!(app.settings.page.as_ref().unwrap().changed, 1);
}

#[test]
fn a_failed_write_is_shown_and_changes_nothing() {
    let mut app = App::with_fixed_panels().in_language(Language::English);
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Language(Language::German),
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert_eq!(app.lang, Language::English);
    assert!(app.settings.failure.as_deref().unwrap().contains("denied"));
}

#[test]
fn a_written_program_takes_effect_for_the_next_f3() {
    let mut app = App::with_settings_file(SettingsFeature::Programs, Ok(Config::default()));
    let line = config::ProgramLine::new("bat", &[]);
    let change = SettingChange::Program {
        kind: crate::fs::OpenKind::View,
        line: Some(line.clone()),
    };
    let written = config::from_text("[open]\nview = [\"bat\"]\n").unwrap();
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change,
        result: Ok(Box::new(written)),
    })));
    assert_eq!(app.open_programs.view, Some(line));
}

#[test]
fn a_written_color_is_painted_at_once() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    let written = config::from_text("[theme]\naccent = \"#ff0000\"\n").unwrap();
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Ok(Box::new(written)),
    })));
    assert_eq!(
        app.palette.color(ColorRole::Accent),
        iced::Color::from_rgb8(0xFF, 0, 0)
    );
}

#[test]
fn a_refused_value_of_a_page_that_was_left_is_shown_in_the_status_bar() {
    let mut app = App::with_settings_file(SettingsFeature::Language, Ok(Config::default()));
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert!(app.settings.failure.as_deref().unwrap().contains("denied"));
}

#[test]
fn a_refused_field_value_is_shown_at_its_row_and_not_in_the_status_bar() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    let field = SettingField::Color(ColorRole::Accent);
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert!(app.settings.failure.is_none());
    let state = app.settings.page.as_ref().unwrap();
    assert!(
        matches!(state.problems.get(&field), Some(settings_ui::FieldProblem::Write(text)) if text.contains("denied"))
    );
}

fn typed_without_enter(app: &mut App, field: SettingField, text: &str) {
    drop(app.update(Message::Settings(SettingsMessage::Edit {
        field,
        text: text.to_owned(),
    })));
}

#[test]
fn a_wrong_value_typed_without_enter_keeps_the_page_and_the_tab() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    let accent = SettingField::Color(ColorRole::Accent);
    typed_without_enter(&mut app, accent, "red");

    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Language,
    ))));
    let state = app.settings.page.as_ref().unwrap();
    assert_eq!(state.feature, SettingsFeature::Look);
    assert_eq!(
        state.problems.get(&accent),
        Some(&settings_ui::FieldProblem::Color)
    );

    drop(app.update(Message::Settings(SettingsMessage::Close)));
    assert!(app.settings.page.is_some());
    assert_eq!(
        app.settings.page.as_ref().unwrap().field_text(accent),
        "red"
    );
}

#[tokio::test]
async fn quitting_writes_a_right_value_typed_without_enter() -> super::settings_pages_tests::TestResult
{
    let dir = tempfile::tempdir()?;
    let mut app = super::settings_pages_tests::app_with_file(
        SettingsFeature::Look,
        "[open]\n",
        &dir,
    )?;
    typed_without_enter(&mut app, SettingField::Color(ColorRole::Accent), "#ff0");
    super::settings_pages_tests::settle(&mut app, Message::Quit).await;

    assert!(app.settings.page.is_none());
    let file = std::fs::read_to_string(dir.path().join("config.toml"))?;
    assert!(file.contains("accent"), "the typed colour was lost: {file}");
    Ok(())
}

#[tokio::test]
async fn quitting_keeps_the_page_open_for_a_wrong_value_typed_without_enter(
) -> super::settings_pages_tests::TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = super::settings_pages_tests::app_with_file(
        SettingsFeature::Look,
        "[open]\n",
        &dir,
    )?;
    let accent = SettingField::Color(ColorRole::Accent);
    typed_without_enter(&mut app, accent, "red");
    super::settings_pages_tests::settle(&mut app, Message::Quit).await;

    let state = app.settings.page.as_ref().ok_or("the page closed")?;
    assert_eq!(
        state.problems.get(&accent),
        Some(&settings_ui::FieldProblem::Color)
    );
    Ok(())
}

#[test]
fn a_right_value_typed_without_enter_lets_the_page_change_and_the_tab_close() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    typed_without_enter(&mut app, SettingField::Color(ColorRole::Accent), "#ff0");
    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Language,
    ))));
    assert_eq!(
        app.settings.page.as_ref().unwrap().feature,
        SettingsFeature::Language
    );
    drop(app.update(Message::Settings(SettingsMessage::Close)));
    assert!(app.settings.page.is_none());
}

#[test]
fn a_write_that_fails_after_the_tab_closed_shows_in_the_status_bar() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    drop(app.update(Message::Settings(SettingsMessage::Close)));
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert!(app.notice_for_test().unwrap().contains("denied"));
}

#[test]
fn a_theme_file_next_to_the_config_is_named_from_its_folder() {
    let config_path = Path::new("/home/me/.config/ncrs/config.toml");
    assert_eq!(
        theme_file_entry(
            Some(config_path),
            Path::new("/home/me/.config/ncrs/themes/night.json")
        ),
        Some("themes/night.json".to_owned())
    );
    assert_eq!(
        theme_file_entry(Some(config_path), Path::new("/opt/themes/day.json")),
        Some("/opt/themes/day.json".to_owned())
    );
}

#[test]
fn the_tab_opens_on_a_config_that_does_not_load_and_names_the_error() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "lang = 3\n").unwrap();
    let mut app = App::with_fixed_panels();
    app.config_path = Some(path);
    open(&mut app);
    let state = app.settings.page.as_ref().expect("the tab is open");
    assert_eq!(
        state.file.as_ref().err().and_then(ConfigError::line),
        Some(1)
    );
}

#[test]
fn a_page_is_selected_and_searched() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Hermes,
    ))));
    assert_eq!(
        app.settings.page.as_ref().unwrap().feature,
        SettingsFeature::Hermes
    );
}

#[test]
fn a_click_on_a_page_the_app_fills_moves_the_keyboard_to_its_row() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    drop(app.update(Message::FocusPageThen(2, None)));
    let page = app.settings.page.as_ref().expect("settings are open");
    assert_eq!(page.focus, ncrs_settings::focus::Focus::in_rows(2));
}
