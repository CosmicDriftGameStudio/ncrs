//! The stages of the connect dialog that ask for more than the form: the
//! passphrase of a locked key, and the form with what the machine provides.

use std::path::PathBuf;

use super::hermes_chat_ui_tests::matches_snapshot;
use crate::app::*;
use crate::i18n::Language;
use crate::messages::ConnectField;
use ncrs_remote::dialog::{ConnectDialog, SshHint, Stage};

fn locked_key() -> ConnectDialog {
    let mut dialog = ConnectDialog::for_server(1, &ServerId::new("marc", "example.org", 22), None);
    dialog.set_surroundings(false, Some(PathBuf::from("/home/test")));
    dialog.set_stage(Stage::Passphrase {
        key_file: PathBuf::from("/home/test/.ssh/id_ed25519"),
    });
    dialog
}

fn shown(dialog: ConnectDialog, lang: Language, name: &str) {
    matches_snapshot(&App::with_connect_dialog(dialog).in_language(lang), name);
}

#[test]
fn the_passphrase_stage_matches_its_snapshots() {
    shown(locked_key(), Language::English, "connect_passphrase_en");
    shown(locked_key(), Language::German, "connect_passphrase_de");
}

#[test]
fn the_passphrase_stage_after_a_wrong_passphrase_matches_its_snapshot() {
    let mut dialog = locked_key();
    dialog.set_error("That passphrase did not unlock the key.".to_owned());
    shown(dialog, Language::English, "connect_passphrase_wrong_en");
}

#[test]
fn the_form_tells_what_the_ssh_config_gives() {
    let mut dialog = ConnectDialog::new(1);
    dialog.set(ConnectField::Host, "work".to_owned());
    dialog.set_ssh(
        SshHint {
            host_name: Some("example.org".to_owned()),
            port: Some(2222),
            user: Some("marc".to_owned()),
        },
        true,
    );
    shown(dialog, Language::English, "connect_dialog_ssh_config_en");
}

#[test]
fn the_form_of_the_app_store_build_asks_for_the_key_file() {
    let mut dialog = ConnectDialog::new(1);
    dialog.set(ConnectField::Host, "example.org".to_owned());
    dialog.set(ConnectField::User, "marc".to_owned());
    dialog.set_surroundings(true, None);
    shown(dialog, Language::English, "connect_dialog_app_store_en");
}
