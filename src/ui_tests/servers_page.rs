use super::*;
use crate::messages::{SettingChange, SettingsFeature, SettingsMessage};
use std::path::PathBuf;

fn page(source: &str) -> App {
    App::with_settings_file(
        SettingsFeature::Servers,
        Ok(crate::config::from_text(source).unwrap()),
    )
}

const TWO: &str = "[[servers]]\nname = \"Work\"\nhost = \"files.example.org\"\nuser = \"marc\"\npath = \"/srv\"\n\n[[servers]]\nhost = \"home.example.org\"\nport = 2222\nuser = \"marc\"\n";

fn messages_after_click(app: &App, label: &str) -> Vec<Message> {
    let mut sim = simulator(app);
    sim.click(label).expect("the clicked label");
    sim.into_messages().collect()
}

#[test]
fn the_page_lists_each_server_with_its_address() {
    let app = page(TWO);
    let mut sim = simulator(&app);
    for label in [
        "Work",
        "marc@files.example.org /srv",
        "marc@home.example.org:2222",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
    sim.find("Add server…").unwrap();
}

#[test]
fn connect_opens_the_server_and_remove_writes_the_list_without_it() {
    let app = page(TWO);
    assert_eq!(
        messages_after_click(&app, "Connect"),
        [Message::Settings(SettingsMessage::FocusThen(
            0,
            Some(Box::new(SettingsMessage::OpenServer(PathBuf::from(
                "sftp://marc@files.example.org/srv"
            ))))
        ))]
    );
    let sent = messages_after_click(&app, "Remove");
    let [Message::Settings(SettingsMessage::FocusThen(_, Some(change)))] = sent.as_slice() else {
        panic!("{sent:?}");
    };
    let SettingsMessage::Change(SettingChange::Servers(rest)) = change.as_ref() else {
        panic!("{sent:?}");
    };
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].host, "home.example.org");
}

#[test]
fn an_empty_list_says_so_and_offers_to_add() {
    let app = page("");
    simulator(&app).find("No saved servers").unwrap();
    assert_eq!(
        messages_after_click(&app, "Add…"),
        [Message::Settings(SettingsMessage::FocusThen(
            0,
            Some(Box::new(SettingsMessage::OpenServerAdd))
        ))]
    );
}

#[test]
fn the_page_is_worded_in_german_too() {
    let app = page(TWO).in_language(Language::German);
    let mut sim = simulator(&app);
    sim.find("Entfernen").unwrap();
    sim.find("Verbinden").unwrap();
}
