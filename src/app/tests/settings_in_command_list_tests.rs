use crate::app::*;
use crate::messages::SettingsFeature;

fn run_query(query: &str) -> App {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    drop(app.update(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged(query.to_owned()),
    )));
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));
    app
}

#[test]
fn running_a_page_opens_the_settings_tab_on_it() {
    let app = run_query("settings programs");

    assert!(app.command_palette_for_test().is_none());
    let state = app.settings_for_test().unwrap();
    assert_eq!(state.feature, SettingsFeature::Programs);
    assert_eq!(state.query, "");
    assert_eq!(state.marked_row, None);
}

#[test]
fn running_a_single_setting_brings_its_row_into_view_marked() {
    let app = run_query("viewer");

    let state = app.settings_for_test().unwrap();
    assert_eq!(state.feature, SettingsFeature::Programs);
    let label = app.lang.text(Msg::SettingsRowViewer);
    assert_eq!(state.query, label);
    assert_eq!(state.marked_row.as_deref(), Some(label));
}

#[test]
fn searching_or_choosing_a_page_unmarks_the_row() {
    let mut searched = run_query("viewer");
    drop(
        searched.update(Message::Settings(SettingsMessage::SearchChanged(
            "edit".to_owned(),
        ))),
    );
    assert_eq!(searched.settings_for_test().unwrap().marked_row, None);

    let mut chosen = run_query("viewer");
    drop(chosen.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Look,
    ))));
    assert_eq!(chosen.settings_for_test().unwrap().marked_row, None);
}

#[test]
fn a_setting_runs_from_the_list_while_the_tab_is_already_open() {
    let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    drop(app.update(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged("viewer".to_owned()),
    )));
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

    assert_eq!(
        app.settings_for_test().unwrap().feature,
        SettingsFeature::Programs
    );
}
