use crate::app::*;
use crate::messages::SettingsFeature;

fn press(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, modifiers)
}

fn press_and_run(app: &mut App, key: Key, modifiers: Modifiers) {
    let routed = press(app, key, modifiers).expect("the key is routed");
    match routed {
        // The page asks iced whether a text field has the keyboard; none has.
        Message::TabInPage { backward } => drop(app.update(Message::TabInPageAnswered {
            backward,
            field_focused: false,
        })),
        other => drop(app.update(other)),
    }
}

fn f2(app: &mut App) {
    press_and_run(app, Key::Named(Named::F2), Modifiers::empty());
}

fn tab(app: &mut App, modifiers: Modifiers) {
    press_and_run(app, Key::Named(Named::Tab), modifiers);
}

fn stop(app: &App) -> (ShownTab, PanelSide) {
    (app.shown_tab, app.panels.active)
}

fn open_settings(app: &mut App) {
    drop(app.update(Message::Settings(SettingsMessage::Toggle)));
}

#[test]
fn f2_with_the_settings_tab_shown_shows_the_chat_and_the_monitor_does_not_close_it() {
    let mut app = App::with_fixed_panels();
    open_settings(&mut app);
    assert!(app.settings_shown());
    f2(&mut app);
    assert!(app.hermes_chat.open && app.hermes_chat_shown());
    assert!(app.settings.page.is_some());

    drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
    assert!(app.monitor_shown());
    assert!(app.hermes_chat.open, "the chat stays open");
    assert!(app.settings.page.is_some());
}

#[test]
fn tab_walks_all_five_stops_and_back() {
    let mut app = App::with_fixed_panels();
    app.panels.active = PanelSide::Left;
    open_settings(&mut app);
    f2(&mut app);
    drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
    assert_eq!(app.shown_tab, ShownTab::Monitor);

    // Monitor -> Chat -> Left -> Right -> Settings -> Monitor.
    let forward = [
        (ShownTab::Hermes, PanelSide::Left),
        (ShownTab::Panels, PanelSide::Left),
        (ShownTab::Panels, PanelSide::Right),
        (ShownTab::Settings, PanelSide::Right),
        (ShownTab::Monitor, PanelSide::Right),
    ];
    for expected in forward {
        tab(&mut app, Modifiers::empty());
        assert_eq!(stop(&app), expected);
    }
    let backward = [
        (ShownTab::Settings, PanelSide::Right),
        (ShownTab::Panels, PanelSide::Right),
        (ShownTab::Panels, PanelSide::Left),
        (ShownTab::Hermes, PanelSide::Left),
        (ShownTab::Monitor, PanelSide::Left),
    ];
    for expected in backward {
        tab(&mut app, Modifiers::SHIFT);
        assert_eq!(stop(&app), expected);
    }
    assert!(app.hermes_chat.open && app.monitor.is_open() && app.settings.page.is_some());
}

#[test]
fn f2_on_a_hidden_chat_shows_it_and_keeps_the_conversation() {
    let mut app = App::with_fixed_panels();
    f2(&mut app);
    drop(
        app.update(Message::HermesChat(HermesChatMessage::InputChanged(
            "half a sentence".to_owned(),
        ))),
    );
    drop(app.update(Message::ShowTab(ShownTab::Panels)));
    assert!(app.hermes_chat.open && !app.hermes_chat_shown());
    assert!(
        !app.prompt_key_state().hermes_chat,
        "the panels have the keys"
    );

    f2(&mut app);
    assert!(app.hermes_chat_shown());
    assert_eq!(app.hermes_chat.input, "half a sentence");
}

#[test]
fn closing_the_shown_chat_shows_the_panels() {
    let mut app = App::with_fixed_panels();
    f2(&mut app);
    drop(app.update(Message::HermesChat(HermesChatMessage::Close)));
    assert!(!app.hermes_chat.open);
    assert_eq!(app.shown_tab, ShownTab::Panels);
    // F2 and Escape in the shown chat both close it.
    for key in [Named::F2, Named::Escape] {
        let mut again = App::with_fixed_panels();
        f2(&mut again);
        press_and_run(&mut again, Key::Named(key), Modifiers::empty());
        assert!(!again.hermes_chat.open, "{key:?}");
        assert_eq!(again.shown_tab, ShownTab::Panels, "{key:?}");
    }
}

#[test]
fn closing_a_hidden_chat_leaves_the_shown_tab_alone() {
    let mut app = App::with_fixed_panels();
    f2(&mut app);
    open_settings(&mut app);
    assert!(app.settings_shown());
    drop(app.update(Message::HermesChat(HermesChatMessage::Close)));
    assert!(!app.hermes_chat.open);
    assert!(app.settings_shown());
}

#[test]
fn a_setting_from_the_command_list_brings_the_hidden_settings_tab_forward() {
    let mut app = App::with_settings_open(SettingsFeature::Programs, Filing::default());
    drop(app.update(Message::ShowTab(ShownTab::Panels)));
    assert!(!app.settings_shown());
    let label = app.lang.text(Msg::SettingsRowFont).to_owned();
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    drop(app.update(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged(label.clone()),
    )));
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

    assert!(app.command_palette_for_test().is_none());
    assert!(app.settings_shown());
    let state = app.settings_for_test().unwrap();
    assert_eq!(state.feature, SettingsFeature::Look);
    assert_eq!(state.marked_row.as_deref(), Some(label.as_str()));
}

#[test]
fn the_other_ways_to_a_page_bring_a_hidden_tab_forward() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
    f2(&mut app);
    drop(app.update(Message::ShowTab(ShownTab::Panels)));
    drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
    assert!(app.monitor_shown() && app.monitor.is_open());
    drop(app.update(Message::ShowTab(ShownTab::Panels)));
    drop(app.update(Message::HermesChat(HermesChatMessage::Open)));
    assert!(app.hermes_chat_shown());
}

#[test]
fn the_keys_of_the_other_pages_work_from_the_shown_chat() {
    let mut app = App::with_fixed_panels();
    open_settings(&mut app);
    f2(&mut app);
    assert!(app.hermes_chat_shown());
    press_and_run(&mut app, Key::Named(Named::F12), Modifiers::empty());
    assert!(app.monitor_shown());
    assert!(app.hermes_chat.open && app.settings.page.is_some());

    f2(&mut app);
    assert!(app.hermes_chat_shown());
    press_and_run(&mut app, Key::Character(",".into()), Modifiers::COMMAND);
    assert!(app.settings_shown());
    assert!(app.hermes_chat.open && app.monitor.is_open());

    f2(&mut app);
    // Other function keys stay the chat's.
    assert_eq!(press(&app, Key::Named(Named::F5), Modifiers::empty()), None);
}
