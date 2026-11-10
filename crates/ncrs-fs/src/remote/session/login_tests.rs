use std::path::{Path, PathBuf};

use super::super::credentials::{InMemoryPassphrases, PassphraseStore};
use super::super::test_server::{
    client_public_key, public_key_text, write_client_key, Fixture, TestServer,
};
use super::*;
use crate::remote::Secret;

const SEED: [u8; 32] = [21; 32];
const OTHER_SEED: [u8; 32] = [22; 32];
const PASSPHRASE: &str = "open sesame";

/// A pool in front of a test server, with a home folder of its own that holds
/// the `.ssh` folder, and the files for the host keys beside it.
struct Rig {
    server: Arc<TestServer>,
    pool: Arc<Pool>,
    home: tempfile::TempDir,
    passphrases: Arc<InMemoryPassphrases>,
}

impl Rig {
    fn new() -> Self {
        Self::over(TestServer::new(), true)
    }

    fn over(server: Arc<TestServer>, trusted: bool) -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".ssh")).unwrap();
        let hosts = home.path().join("hosts");
        std::fs::create_dir(&hosts).unwrap();
        let known = KnownHostsFiles {
            ssh: Some(hosts.join("ssh_known_hosts")),
            own: Some(hosts.join("known_hosts")),
        };
        if trusted {
            std::fs::write(
                known.own.as_deref().unwrap(),
                format!(
                    "test.invalid {key}\n[test.invalid]:2200 {key}\n",
                    key = public_key_text()
                ),
            )
            .unwrap();
        }
        let passphrases = Arc::new(InMemoryPassphrases::default());
        let pool = server.pool_logging_in(
            known,
            LoginEnv {
                home: Some(home.path().to_path_buf()),
                agent_socket: None,
                os_user: Some("tester".to_owned()),
            },
            Arc::clone(&passphrases) as Arc<dyn PassphraseStore>,
        );
        Self {
            server,
            pool,
            home,
            passphrases,
        }
    }

    fn ssh(&self, name: &str) -> PathBuf {
        self.home.path().join(".ssh").join(name)
    }

    /// An authorized key as `~/.ssh/<name>`.
    fn put_key(&self, name: &str, passphrase: Option<&str>) -> PathBuf {
        self.server.authorize(&client_public_key(&SEED));
        write_client_key(&self.home.path().join(".ssh"), name, &SEED, passphrase)
    }

    fn events(&self) -> Vec<&'static str> {
        self.server.control.events()
    }
}

fn keys_only() -> Login {
    Login::default()
}

fn with_passphrase(key_file: &Path, text: &str, remember: bool) -> Login {
    Login {
        passphrase: Some((key_file.to_path_buf(), Secret::new(text.to_owned()))),
        remember_passphrase: remember,
        ..Login::default()
    }
}

#[tokio::test]
async fn a_default_key_file_logs_in_without_a_password() {
    let rig = Rig::new();
    rig.put_key("id_ed25519", None);

    let result = rig.pool.connect(&Fixture::id(), &keys_only()).await;

    assert_eq!(result, Ok(()));
    assert!(!rig.events().contains(&"password"));
}

#[tokio::test]
async fn the_key_file_of_the_server_replaces_the_config_and_comes_first() {
    let rig = Rig::new();
    let own = rig.put_key("mine", None);
    write_client_key(
        &rig.home.path().join(".ssh"),
        "id_ed25519",
        &OTHER_SEED,
        None,
    );
    std::fs::write(
        rig.ssh("config"),
        "Host *\n  IdentityFile ~/.ssh/from_config\n",
    )
    .unwrap();
    write_client_key(
        &rig.home.path().join(".ssh"),
        "from_config",
        &OTHER_SEED,
        None,
    );

    let login = Login {
        key_file: Some(own),
        ..Login::default()
    };
    let result = rig.pool.connect(&Fixture::id(), &login).await;

    assert_eq!(result, Ok(()));
    assert_eq!(
        rig.events()
            .iter()
            .filter(|event| **event == "publickey")
            .count(),
        1,
        "the key that works is the first and only one offered"
    );
}

#[tokio::test]
async fn an_identity_file_of_the_config_is_offered() {
    let rig = Rig::new();
    rig.put_key("special", None);
    std::fs::write(rig.ssh("config"), "Host *\n  IdentityFile ~/.ssh/special\n").unwrap();

    assert_eq!(rig.pool.connect(&Fixture::id(), &keys_only()).await, Ok(()));
}

#[tokio::test]
async fn a_key_is_offered_once_however_often_it_is_named() {
    let rig = Rig::new();
    let unauthorized = write_client_key(
        &rig.home.path().join(".ssh"),
        "id_ed25519",
        &OTHER_SEED,
        None,
    );
    std::fs::write(
        rig.ssh("config"),
        format!("Host *\n  IdentityFile {}\n", unauthorized.display()),
    )
    .unwrap();

    let result = rig.pool.connect(&Fixture::id(), &keys_only()).await;

    assert_eq!(result, Err(ConnectFailure::AuthFailed));
    let offered = rig
        .events()
        .iter()
        .filter(|event| **event == "publickey")
        .count();
    assert_eq!(offered, 1);
}

#[tokio::test]
async fn the_password_goes_first_and_spends_no_key_attempt_when_it_works() {
    let rig = Rig::new();
    write_client_key(
        &rig.home.path().join(".ssh"),
        "id_ed25519",
        &OTHER_SEED,
        None,
    );

    let result = rig.pool.connect(&Fixture::id(), &Fixture::login()).await;

    assert_eq!(result, Ok(()));
    assert!(!rig.events().contains(&"publickey"), "{:?}", rig.events());
}

#[tokio::test]
async fn a_key_is_the_fallback_when_the_password_is_refused() {
    let rig = Rig::new();
    rig.put_key("id_ed25519", None);
    let login = Login {
        password: Secret::new("not it".to_owned()),
        ..Login::default()
    };

    let result = rig.pool.connect(&Fixture::id(), &login).await;

    assert_eq!(result, Ok(()));
    let events = rig.events();
    let password = events
        .iter()
        .position(|event| *event == "password")
        .unwrap();
    let key = events
        .iter()
        .position(|event| *event == "publickey")
        .unwrap();
    assert!(password < key, "{events:?}");
}

#[tokio::test]
async fn an_encrypted_key_without_a_passphrase_asks_for_one() {
    let rig = Rig::new();
    let key_file = rig.put_key("id_ed25519", Some(PASSPHRASE));

    let result = rig.pool.connect(&Fixture::id(), &keys_only()).await;

    assert_eq!(result, Err(ConnectFailure::PassphraseNeeded { key_file }));
    assert!(!rig.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn a_wrong_password_is_still_a_wrong_password_next_to_an_encrypted_key() {
    let rig = Rig::new();
    rig.put_key("id_ed25519", Some(PASSPHRASE));
    let login = Login {
        password: Secret::new("not it".to_owned()),
        ..Login::default()
    };

    let result = rig.pool.connect(&Fixture::id(), &login).await;

    assert_eq!(result, Err(ConnectFailure::AuthFailed));
}

#[tokio::test]
async fn the_passphrase_of_the_request_opens_the_key_and_is_not_kept_unless_asked() {
    let rig = Rig::new();
    let key_file = rig.put_key("id_ed25519", Some(PASSPHRASE));

    let result = rig
        .pool
        .connect(
            &Fixture::id(),
            &with_passphrase(&key_file, PASSPHRASE, false),
        )
        .await;

    assert_eq!(result, Ok(()));
    assert_eq!(rig.passphrases.get(&key_file).unwrap(), None);
}

#[tokio::test]
async fn a_typed_passphrase_goes_to_its_own_key_file_and_not_to_the_first_locked_one() {
    let rig = Rig::new();
    write_client_key(
        &rig.home.path().join(".ssh"),
        "id_ed25519",
        &OTHER_SEED,
        Some("another"),
    );
    let second = rig.put_key("id_ecdsa", Some(PASSPHRASE));

    let result = rig
        .pool
        .connect(&Fixture::id(), &with_passphrase(&second, PASSPHRASE, false))
        .await;

    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn the_passphrase_that_worked_is_kept_when_asked() {
    let rig = Rig::new();
    let key_file = rig.put_key("id_ed25519", Some(PASSPHRASE));

    let result = rig
        .pool
        .connect(
            &Fixture::id(),
            &with_passphrase(&key_file, PASSPHRASE, true),
        )
        .await;

    assert_eq!(result, Ok(()));
    assert_eq!(
        rig.passphrases.get(&key_file).unwrap().unwrap().expose(),
        PASSPHRASE
    );
}

#[tokio::test]
async fn a_kept_passphrase_logs_in_without_being_asked() {
    let rig = Rig::new();
    let key_file = rig.put_key("id_ed25519", Some(PASSPHRASE));
    rig.passphrases
        .set(&key_file, &Secret::new(PASSPHRASE.to_owned()))
        .unwrap();

    assert_eq!(rig.pool.connect(&Fixture::id(), &keys_only()).await, Ok(()));
}

#[tokio::test]
async fn a_wrong_passphrase_is_named_and_not_kept() {
    let rig = Rig::new();
    let key_file = rig.put_key("id_ed25519", Some(PASSPHRASE));

    let result = rig
        .pool
        .connect(&Fixture::id(), &with_passphrase(&key_file, "not it", true))
        .await;

    assert_eq!(
        result,
        Err(ConnectFailure::WrongPassphrase {
            key_file: key_file.clone()
        })
    );
    assert_eq!(rig.passphrases.get(&key_file).unwrap(), None);
}

#[tokio::test]
async fn a_kept_passphrase_that_no_longer_fits_is_dropped_and_asked_again() {
    let rig = Rig::new();
    let key_file = rig.put_key("id_ed25519", Some(PASSPHRASE));
    rig.passphrases
        .set(&key_file, &Secret::new("old".to_owned()))
        .unwrap();

    let result = rig.pool.connect(&Fixture::id(), &keys_only()).await;

    assert_eq!(
        result,
        Err(ConnectFailure::PassphraseNeeded {
            key_file: key_file.clone()
        })
    );
    assert_eq!(rig.passphrases.get(&key_file).unwrap(), None);
}

#[tokio::test]
async fn nothing_is_offered_to_a_server_whose_host_key_is_not_known() {
    let rig = Rig::over(TestServer::new(), false);
    rig.put_key("id_ed25519", None);
    let login = Login {
        password: Fixture::password(),
        ..Login::default()
    };

    let result = rig.pool.connect(&Fixture::id(), &login).await;

    assert!(
        matches!(result, Err(ConnectFailure::UnknownHost { .. })),
        "{result:?}"
    );
    let events = rig.events();
    for sent in ["none", "publickey", "password"] {
        assert!(
            !events.contains(&sent),
            "{sent} reached an unknown host: {events:?}"
        );
    }
}

#[tokio::test]
async fn a_key_only_server_without_a_usable_key_hears_that_the_password_is_not_it() {
    let rig = Rig::over(TestServer::key_login_only(), true);

    let result = rig.pool.connect(&Fixture::id(), &Fixture::login()).await;

    assert_eq!(result, Err(ConnectFailure::PasswordNotOffered));
    assert!(!rig.events().contains(&"password"));
}

#[tokio::test]
async fn a_dead_agent_is_no_failure() {
    let home = tempfile::tempdir().unwrap();
    let server = TestServer::new();
    let known = KnownHostsFiles {
        ssh: None,
        own: Some(home.path().join("known_hosts")),
    };
    std::fs::write(
        known.own.as_deref().unwrap(),
        format!("test.invalid {}\n", public_key_text()),
    )
    .unwrap();
    let pool = server.pool_logging_in(
        known,
        LoginEnv {
            home: Some(home.path().to_path_buf()),
            agent_socket: Some(home.path().join("no-agent.sock")),
            os_user: None,
        },
        Arc::new(InMemoryPassphrases::default()),
    );

    assert_eq!(
        pool.connect(&Fixture::id(), &Fixture::login()).await,
        Ok(())
    );
}

#[tokio::test]
async fn an_alias_of_the_config_is_dialled_and_judged_by_what_it_points_at() {
    let rig = Rig::new();
    std::fs::write(
        rig.ssh("config"),
        "Host files\n  HostName Test.Invalid\n  User tester\n",
    )
    .unwrap();
    let alias = ServerId::new("", "files", 22);

    let result = rig.pool.connect(&alias, &Fixture::login()).await;

    assert_eq!(result, Ok(()));
    assert!(
        rig.pool.is_connected(&alias),
        "kept under the address as given"
    );
    assert_eq!(rig.pool.connected(), vec![alias]);
}

#[tokio::test]
async fn a_host_key_accepted_for_an_alias_is_known_under_what_the_alias_points_at() {
    let rig = Rig::over(TestServer::new(), false);
    std::fs::write(
        rig.ssh("config"),
        "Host files\n  HostName test.invalid\n  Port 2200\n  User tester\n",
    )
    .unwrap();
    let alias = ServerId::new("", "files", 22);
    let Err(ConnectFailure::UnknownHost { key, .. }) =
        rig.pool.connect(&alias, &Fixture::login()).await
    else {
        panic!("the first connection must ask about the host key");
    };

    rig.pool.trust(&alias, &key).unwrap();
    let again = rig.pool.connect(&alias, &Fixture::login()).await;

    assert_eq!(again, Ok(()));
}

#[tokio::test]
async fn the_port_of_the_config_applies_when_the_address_has_the_default() {
    let rig = Rig::new();
    std::fs::write(
        rig.ssh("config"),
        "Host files\n  HostName test.invalid\n  Port 2200\n",
    )
    .unwrap();

    let at_default = ServerId::new("tester", "files", 22);
    assert_eq!(
        rig.pool.connect(&at_default, &Fixture::login()).await,
        Ok(())
    );

    let own_port = ServerId::new("tester", "files", 2222);
    let result = rig.pool.connect(&own_port, &Fixture::login()).await;
    assert!(
        matches!(result, Err(ConnectFailure::UnknownHost { .. })),
        "port 2222 is not on record for the host: {result:?}"
    );
}

#[tokio::test]
async fn an_address_without_a_user_logs_in_as_the_user_of_the_machine() {
    let rig = Rig::new();
    let bare = ServerId::new("", "test.invalid", 22);

    assert_eq!(rig.pool.connect(&bare, &Fixture::login()).await, Ok(()));
}

#[tokio::test]
async fn an_address_without_any_user_name_fails_as_a_wrong_user() {
    let home = tempfile::tempdir().unwrap();
    let pool = TestServer::new().pool_logging_in(
        KnownHostsFiles::default(),
        LoginEnv {
            home: Some(home.path().to_path_buf()),
            agent_socket: None,
            os_user: None,
        },
        Arc::new(InMemoryPassphrases::default()),
    );

    let result = pool
        .connect(&ServerId::new("", "test.invalid", 22), &Fixture::login())
        .await;

    assert_eq!(result, Err(ConnectFailure::AuthFailed));
}
