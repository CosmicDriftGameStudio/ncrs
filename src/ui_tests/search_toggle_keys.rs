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
