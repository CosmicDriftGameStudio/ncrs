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
        .matches_reference_image(name)
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
