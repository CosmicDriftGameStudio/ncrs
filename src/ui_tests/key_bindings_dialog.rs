use super::*;
use crate::keymap::Binding;
use crate::messages::KeyBindingsMessage;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

fn opened(lang: Language) -> App {
    let mut app = App::with_fixed_panels().in_language(lang);
    drop(app.update(Message::KeyBindings(KeyBindingsMessage::Open)));
    app
}

/// Move asks for F5, which Copy holds: the dialog waits for the takeover.
fn clashing(lang: Language) -> App {
    let mut app = opened(lang);
    let bare = |named| Binding::with_modifiers(Key::Named(named), Modifiers::default());
    for message in [
        KeyBindingsMessage::SearchByKey(bare(Named::F6)),
        KeyBindingsMessage::Record,
        KeyBindingsMessage::Recorded(bare(Named::F5)),
    ] {
        drop(app.update(Message::KeyBindings(message)));
    }
    app
}

fn assert_snapshot(app: &App, name: &str) {
    let matches = simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(matches, "the shortcuts dialog does not match {name}.png");
}

#[test]
fn the_dialog_lists_the_actions_with_their_keys() {
    for (lang, name) in [
        (Language::English, "key_bindings"),
        (Language::German, "key_bindings_de"),
    ] {
        assert_snapshot(&opened(lang), name);
    }
}

#[test]
fn the_git_actions_are_listed_read_only_and_follow_the_search() {
    for (lang, name) in [
        (Language::English, "key_bindings_git"),
        (Language::German, "key_bindings_git_de"),
    ] {
        let mut app = opened(lang);
        drop(
            app.update(Message::KeyBindings(KeyBindingsMessage::SearchChanged(
                "stash".to_owned(),
            ))),
        );
        assert_snapshot(&app, name);
    }
}

#[test]
fn a_clash_names_the_action_that_holds_the_key() {
    for (lang, name) in [
        (Language::English, "key_bindings_clash"),
        (Language::German, "key_bindings_clash_de"),
    ] {
        assert_snapshot(&clashing(lang), name);
    }
}
