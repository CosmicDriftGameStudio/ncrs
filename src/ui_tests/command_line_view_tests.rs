use super::*;
use crate::messages::CommandLineMessage;

fn app_with_typed_line(language: Language) -> App {
    let mut app = App::with_fixed_panels().in_language(language);
    drop(app.update(Message::CommandLine(CommandLineMessage::Input(
        "ls -la".to_owned(),
    ))));
    app
}

#[test]
fn a_click_into_the_field_focuses_the_line_in_the_app() {
    let mut app = App::with_fixed_panels();
    let mut sim = simulator(&app);
    sim.click(iced_selector::id(crate::ui::command_line::COMMAND_LINE_ID))
        .expect("the command line field");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert_eq!(messages, [Message::CommandLine(CommandLineMessage::Focus)]);
    for message in messages {
        drop(app.update(message));
    }
    assert!(app.command_line_for_test().is_focused());
}

#[test]
fn typing_with_nothing_focused_does_not_reach_the_command_line() {
    let app = App::with_fixed_panels();
    let mut sim = simulator(&app);
    sim.typewrite("ls");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::CommandLine(CommandLineMessage::Input(_)))),
        "{messages:?}"
    );
}

#[test]
fn the_focused_command_line_matches_its_snapshot() {
    let app = app_with_typed_line(Language::English);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/command_line_focused_en.png")
        .expect("snapshot comparison");
    assert!(matches, "the command line does not match its snapshot");
}

#[test]
fn the_focused_command_line_matches_its_german_snapshot() {
    let app = app_with_typed_line(Language::German);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/command_line_focused_de.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the command line does not match its German snapshot"
    );
}
