use super::*;
use crate::fs::remote::{Secret, ServerId};
use crate::messages::{ConnectField, RemoteMessage};
use crate::remote_dialog::{ConnectDialog, Stage};

fn filled() -> ConnectDialog {
    let mut dialog = ConnectDialog::new(1);
    dialog.set(ConnectField::Host, "example.org".to_owned());
    dialog.set(ConnectField::User, "marc".to_owned());
    dialog.set_password(Secret::new("hunter2".to_owned()));
    dialog
}

fn unknown_server() -> ConnectDialog {
    let mut dialog = filled();
    let key = russh::keys::PublicKey::from_openssh(
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
    )
    .unwrap();
    dialog.set_stage(Stage::HostKey {
        fingerprint: "SHA256:abc".to_owned(),
        key: Box::new(key),
        host: "example.org".to_owned(),
        port: 22,
        cert_authority_listed: false,
    });
    dialog
}

fn shows(dialog: ConnectDialog, lang: Language, label: &str) -> bool {
    simulator(&App::with_connect_dialog(dialog).in_language(lang))
        .find(label)
        .is_ok()
}

fn messages_after_click(dialog: ConnectDialog, label: &str) -> Vec<Message> {
    let app = App::with_connect_dialog(dialog);
    let mut sim = simulator(&app);
    sim.click(label).expect("the clicked label");
    sim.into_messages().filter(is_action).collect()
}

#[test]
fn the_form_names_its_fields_in_both_languages() {
    for label in ["Connect to server", "Host", "Port", "User", "Password"] {
        assert!(
            shows(ConnectDialog::new(1), Language::English, label),
            "{label}"
        );
    }
    for label in [
        "Mit Server verbinden",
        "Benutzer",
        "Passwort",
        "Startordner (optional)",
    ] {
        assert!(
            shows(ConnectDialog::new(1), Language::German, label),
            "{label}"
        );
    }
}

#[test]
fn the_buttons_cancel_and_connect() {
    assert_eq!(
        messages_after_click(filled(), "Cancel"),
        [Message::Remote(RemoteMessage::Cancel)]
    );
    assert_eq!(
        messages_after_click(filled(), "Connect"),
        [Message::Remote(RemoteMessage::Submit)]
    );
}

#[test]
fn a_locked_form_cannot_connect_again_and_says_what_it_does() {
    let mut dialog = filled();
    dialog.set_stage(Stage::Connecting);
    assert!(shows(dialog.clone(), Language::English, "Connecting…"));
    assert!(messages_after_click(dialog, "Connect").is_empty());
}

#[test]
fn an_error_is_shown_under_the_fields() {
    let mut dialog = filled();
    dialog.set_error("User name or password is wrong".to_owned());
    assert!(shows(
        dialog,
        Language::English,
        "User name or password is wrong"
    ));
}

#[test]
fn an_unknown_server_shows_its_fingerprint_and_trusts_only_on_the_button() {
    let host = ServerId::new("marc", "example.org", 22).host;
    let sentence = format!(
        "{host} is not known yet. Compare the fingerprint with the one the server's administrator gives you: SHA256:abc"
    );
    assert!(shows(unknown_server(), Language::English, "Unknown server"));
    assert!(shows(unknown_server(), Language::English, &sentence));
    assert_eq!(
        messages_after_click(unknown_server(), "Trust and connect"),
        [Message::Remote(RemoteMessage::TrustHostKey)]
    );
    assert_eq!(
        messages_after_click(unknown_server(), "Cancel"),
        [Message::Remote(RemoteMessage::Cancel)]
    );
}

#[test]
fn the_host_key_question_names_the_port_when_it_is_not_22() {
    let mut dialog = unknown_server();
    dialog.set_stage(Stage::Editing);
    dialog.set(ConnectField::Port, "2222".to_owned());
    let key = russh::keys::PublicKey::from_openssh(
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
    )
    .unwrap();
    dialog.set_stage(Stage::HostKey {
        fingerprint: "SHA256:abc".to_owned(),
        key: Box::new(key),
        host: "example.org".to_owned(),
        port: 2222,
        cert_authority_listed: false,
    });

    assert!(shows(
        dialog,
        Language::English,
        "example.org:2222 is not known yet. Compare the fingerprint with the one the server's administrator gives you: SHA256:abc"
    ));
}

#[test]
fn the_host_key_question_says_when_a_certificate_authority_is_listed() {
    let mut dialog = unknown_server();
    let Stage::HostKey {
        fingerprint,
        key,
        host,
        port,
        ..
    } = dialog.stage().clone()
    else {
        panic!("the dialog asks about the host key");
    };
    dialog.set_stage(Stage::HostKey {
        fingerprint,
        key,
        host,
        port,
        cert_authority_listed: true,
    });

    assert!(shows(
        dialog,
        Language::English,
        "A known_hosts file lists a certificate authority for this server. ncrs does not check host certificates, so the fingerprint has to be compared."
    ));
}

#[test]
fn the_host_key_question_is_worded_in_german_too() {
    assert!(shows(
        unknown_server(),
        Language::German,
        "Unbekannter Server"
    ));
    assert!(shows(
        unknown_server(),
        Language::German,
        "Vertrauen und verbinden"
    ));
}

fn matches_snapshot(dialog: ConnectDialog, language: Language, name: &str) {
    let app = App::with_connect_dialog(dialog).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the dialog does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_form_matches_its_snapshots() {
    let mut dialog = filled();
    dialog.set(ConnectField::StartPath, "/srv/files".to_owned());
    dialog.set_error("User name or password is wrong".to_owned());
    matches_snapshot(dialog.clone(), Language::English, "connect_dialog_en");
    matches_snapshot(dialog, Language::German, "connect_dialog_de");
}

#[test]
fn the_host_key_question_matches_its_snapshots() {
    matches_snapshot(unknown_server(), Language::English, "connect_hostkey_en");
    matches_snapshot(unknown_server(), Language::German, "connect_hostkey_de");
}
