//! The flow of the connect dialog: keys before the password, the passphrase
//! stage and what ends it.

use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::dialog::Stage;
use ncrs_core::PanelSide;
use ncrs_fs::remote::credentials::{InMemoryPasswords, PasswordStore};
use ncrs_fs::remote::session::{ConnectFailure, LoginEnv, Pool};
use ncrs_fs::remote::test_server::Fixture;
use ncrs_fs::remote::{RemotePath, Secret, ServerId};
use ncrs_i18n::Language;

struct Setup {
    pool: Arc<Pool>,
    passwords: Arc<dyn PasswordStore>,
    // The fixture owns the scratch folder the pool reads its host keys from.
    _fixture: Fixture,
    os_user: Option<String>,
}

impl Setup {
    fn new(os_user: Option<&str>) -> Self {
        let fixture = Fixture::trusted();
        Self {
            pool: Arc::clone(&fixture.pool),
            passwords: Arc::new(InMemoryPasswords::default()),
            _fixture: fixture,
            os_user: os_user.map(str::to_owned),
        }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            lang: Language::English,
            pool: &self.pool,
            passwords: &self.passwords,
            blocking_overlay_open: false,
            active_side: PanelSide::Left,
            active_panel_server: None,
            key_files: Vec::new(),
            login_env: LoginEnv {
                os_user: self.os_user.clone(),
                ..LoginEnv::default()
            },
            sandboxed: false,
            left_request_id: 1,
            right_request_id: 1,
        }
    }
}

fn alias_place() -> RemotePath {
    RemotePath::new(ServerId::new("", "alias", 22), "/srv/files")
}

fn silent_login_failed(
    remote: &mut Remote,
    setup: &Setup,
    result: Result<(), ConnectFailure>,
) -> Outcome {
    update(
        remote,
        Message::AutoConnected {
            side: PanelSide::Left,
            request_id: 1,
            remote: alias_place(),
            select: None,
            password: Secret::default(),
            result,
        },
        &setup.ctx(),
    )
}

fn dialog(remote: &Remote) -> &ConnectDialog {
    remote.dialog.as_ref().expect("the dialog is up")
}

fn locked_key() -> ConnectFailure {
    ConnectFailure::PassphraseNeeded {
        key_file: PathBuf::from("/home/test/.ssh/id_ed25519"),
    }
}

fn passphrase_stage(setup: &Setup) -> Remote {
    let mut remote = Remote::default();
    drop(silent_login_failed(&mut remote, setup, Err(locked_key())));
    remote
}

#[test]
fn with_no_stored_password_the_keys_are_tried_before_any_dialog() {
    let setup = Setup::new(None);
    let mut remote = Remote::default();
    let (_task, actions) = update(
        &mut remote,
        Message::PasswordLoaded {
            side: PanelSide::Left,
            request_id: 1,
            remote: alias_place(),
            select: None,
            result: Ok(None),
        },
        &setup.ctx(),
    );
    assert!(remote.dialog.is_none());
    assert!(actions.is_empty());
}

#[test]
fn when_the_keys_do_not_get_in_the_dialog_asks_for_a_password_and_keeps_the_typed_path() {
    let setup = Setup::new(None);
    let mut remote = Remote::default();
    drop(silent_login_failed(
        &mut remote,
        &setup,
        Err(ConnectFailure::AuthFailed),
    ));
    let asked = dialog(&remote);
    assert_eq!(*asked.stage(), Stage::Editing);
    assert_eq!(asked.error(), None);
    assert_eq!(asked.value(ConnectField::Host), "alias");
    assert_eq!(asked.then().unwrap().path, alias_place().to_path_buf());
}

#[test]
fn a_server_that_takes_no_password_says_so_in_the_dialog() {
    let setup = Setup::new(None);
    let mut remote = Remote::default();
    drop(silent_login_failed(
        &mut remote,
        &setup,
        Err(ConnectFailure::PasswordNotOffered),
    ));
    assert!(dialog(&remote).error().is_some());
}

#[test]
fn a_locked_key_opens_the_passphrase_stage_and_unlock_connects_with_it() {
    let setup = Setup::new(Some("marc"));
    let mut remote = passphrase_stage(&setup);
    assert!(
        matches!(dialog(&remote).stage(), Stage::Passphrase { key_file }
        if key_file == &PathBuf::from("/home/test/.ssh/id_ed25519"))
    );
    let before = dialog(&remote).attempt();

    drop(update(&mut remote, Message::Unlock, &setup.ctx()));
    assert!(
        matches!(dialog(&remote).stage(), Stage::Passphrase { .. }),
        "nothing typed, nothing to unlock with"
    );

    drop(update(
        &mut remote,
        Message::Passphrase(Secret::new("open sesame".to_owned())),
        &setup.ctx(),
    ));
    drop(update(&mut remote, Message::Unlock, &setup.ctx()));
    let unlocking = dialog(&remote);
    assert_eq!(*unlocking.stage(), Stage::Connecting);
    assert_ne!(unlocking.attempt(), before);
    assert!(unlocking.passphrase().is_empty(), "it left with the login");
}

#[test]
fn a_wrong_passphrase_keeps_the_stage_and_says_so() {
    let setup = Setup::new(Some("marc"));
    let mut remote = passphrase_stage(&setup);
    drop(update(
        &mut remote,
        Message::Passphrase(Secret::new("nope".to_owned())),
        &setup.ctx(),
    ));
    drop(update(&mut remote, Message::Unlock, &setup.ctx()));
    let attempt = dialog(&remote).attempt();

    drop(update(
        &mut remote,
        Message::Connected {
            attempt,
            result: Err(ConnectFailure::WrongPassphrase {
                key_file: PathBuf::from("/home/test/.ssh/id_ed25519"),
            }),
        },
        &setup.ctx(),
    ));
    let asked = dialog(&remote);
    assert!(matches!(asked.stage(), Stage::Passphrase { .. }));
    assert_eq!(
        asked.error(),
        Some("That passphrase did not unlock the key.")
    );
    assert!(asked.passphrase().is_empty());
}

#[test]
fn cancel_in_the_passphrase_stage_closes_the_dialog_without_connecting() {
    let setup = Setup::new(None);
    let mut remote = passphrase_stage(&setup);
    let (_task, actions) = update(&mut remote, Message::Cancel, &setup.ctx());
    assert!(remote.dialog.is_none());
    assert!(matches!(
        actions.as_slice(),
        [Action::StopWaiting { request_id: 1, .. }]
    ));
}

#[test]
fn the_passphrase_is_remembered_unless_the_tick_is_taken_off() {
    let setup = Setup::new(None);
    let mut remote = passphrase_stage(&setup);
    assert!(dialog(&remote).remember_passphrase());
    drop(update(
        &mut remote,
        Message::ToggleRememberPassphrase,
        &setup.ctx(),
    ));
    assert!(!dialog(&remote).remember_passphrase());
}

#[test]
fn the_dialog_needs_no_password_and_no_user_when_the_account_gives_one() {
    let setup = Setup::new(Some("marc"));
    let mut remote = Remote::default();
    drop(update(&mut remote, Message::OpenConnect, &setup.ctx()));
    drop(update(
        &mut remote,
        Message::Field(ConnectField::Host, "alias".to_owned()),
        &setup.ctx(),
    ));
    drop(update(&mut remote, Message::Submit, &setup.ctx()));
    assert_eq!(*dialog(&remote).stage(), Stage::Connecting);
}

#[test]
fn without_any_user_the_dialog_still_asks_for_one() {
    let setup = Setup::new(None);
    let mut remote = Remote::default();
    drop(update(&mut remote, Message::OpenConnect, &setup.ctx()));
    drop(update(
        &mut remote,
        Message::Field(ConnectField::Host, "alias".to_owned()),
        &setup.ctx(),
    ));
    drop(update(&mut remote, Message::Submit, &setup.ctx()));
    assert_eq!(*dialog(&remote).stage(), Stage::Editing);
}

#[test]
fn choosing_a_key_file_asks_the_app_and_takes_the_answer() {
    let setup = Setup::new(None);
    let mut remote = Remote::default();
    drop(update(&mut remote, Message::OpenConnect, &setup.ctx()));
    let (_task, actions) = update(&mut remote, Message::ChooseKeyFile, &setup.ctx());
    assert!(matches!(actions.as_slice(), [Action::PickKeyFile]));

    drop(update(
        &mut remote,
        Message::KeyFilePicked(Some(PathBuf::from("/keys/id"))),
        &setup.ctx(),
    ));
    assert_eq!(dialog(&remote).value(ConnectField::KeyFile), "/keys/id");
    drop(update(
        &mut remote,
        Message::KeyFilePicked(None),
        &setup.ctx(),
    ));
    assert_eq!(dialog(&remote).value(ConnectField::KeyFile), "/keys/id");
}
