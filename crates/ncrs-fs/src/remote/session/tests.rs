use super::super::test_server::{other_public_key_text, public_key_text, Fixture, TestServer};
use super::*;
use crate::reader::Listing;
use crate::{ReadError, SortKey};

async fn listing(fixture: &Fixture) -> Listing {
    super::super::list::list(&fixture.pool, Fixture::at("/"), SortKey::default()).await
}

#[tokio::test]
async fn host_key_is_checked_before_the_password_is_sent() {
    let unknown = Fixture::untrusted();
    let refused = unknown
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await;
    assert!(
        matches!(refused, Err(ConnectFailure::UnknownHost { .. })),
        "{refused:?}"
    );
    assert!(
        !unknown.server.control.events().contains(&"password"),
        "no password went to a server whose key is unknown"
    );

    let known = Fixture::trusted();
    known
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap();
    assert!(known.server.control.events().contains(&"password"));
    assert!(known.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn unknown_host_reports_sha256_fingerprint() {
    let fixture = Fixture::untrusted();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap_err();

    let ConnectFailure::UnknownHost { fingerprint, key } = refused else {
        panic!("{refused:?}");
    };
    let expected = PublicKey::from_openssh(&public_key_text())
        .unwrap()
        .fingerprint(HashAlg::Sha256)
        .to_string();
    assert!(fingerprint.starts_with("SHA256:"));
    assert_eq!(fingerprint, expected);
    assert_eq!(*key, PublicKey::from_openssh(&public_key_text()).unwrap());
    assert!(!fixture.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn trusting_the_key_lets_the_next_connect_through() {
    let fixture = Fixture::untrusted();
    let ConnectFailure::UnknownHost { key, .. } = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap_err()
    else {
        panic!("expected an unknown host");
    };

    fixture.pool.trust(&Fixture::id(), &key).unwrap();
    fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap();

    assert!(fixture.pool.is_connected(&Fixture::id()));
    assert!(!std::fs::exists(fixture.known.ssh.as_deref().unwrap()).unwrap());
}

#[tokio::test]
async fn changed_host_key_is_refused_without_auth() {
    let fixture = Fixture::over(TestServer::with_other_key());
    let ssh = fixture.known.ssh.clone().unwrap();
    std::fs::write(&ssh, format!("# old\ntest.invalid {}\n", public_key_text())).unwrap();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap_err();

    let ConnectFailure::HostKeyChanged {
        fingerprint,
        file,
        line,
    } = refused
    else {
        panic!("{refused:?}");
    };
    assert_eq!(file, ssh);
    assert_eq!(line, 2);
    let offered = PublicKey::from_openssh(&other_public_key_text()).unwrap();
    assert_eq!(
        fingerprint,
        offered.fingerprint(HashAlg::Sha256).to_string()
    );
    assert!(!fixture.server.control.events().contains(&"password"));
}

#[tokio::test]
async fn a_revoked_key_is_refused_without_auth() {
    let fixture = Fixture::untrusted();
    std::fs::write(
        fixture.known.ssh.as_deref().unwrap(),
        format!("@revoked test.invalid {}\n", public_key_text()),
    )
    .unwrap();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await;

    assert_eq!(refused, Err(ConnectFailure::HostKeyRevoked));
    assert!(!fixture.server.control.events().contains(&"password"));
}

#[tokio::test]
async fn wrong_password_is_auth_failed() {
    let fixture = Fixture::trusted();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Secret::new("not it".to_owned()))
        .await;

    assert_eq!(refused, Err(ConnectFailure::AuthFailed));
    assert!(!fixture.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn the_home_folder_is_the_one_the_server_names() {
    let fixture = Fixture::connected().await;

    let session = fixture.pool.session(&Fixture::id()).unwrap();

    assert_eq!(session.home(), super::super::test_server::HOME);
}

struct Hanging;

impl Dial for Hanging {
    fn dial<'a>(
        &'a self,
        _server: &'a ServerId,
    ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>> {
        Box::pin(std::future::pending())
    }
}

struct Refusing;

impl Dial for Refusing {
    fn dial<'a>(
        &'a self,
        _server: &'a ServerId,
    ) -> BoxFuture<'a, io::Result<Box<dyn AsyncReadWrite>>> {
        Box::pin(async { Err(io::Error::new(io::ErrorKind::ConnectionRefused, "refused")) })
    }
}

#[tokio::test]
async fn a_server_that_never_answers_is_a_timeout() {
    let pool = Pool::new(
        Arc::new(Hanging),
        KnownHostsFiles::default(),
        Timeouts {
            connect: Duration::from_millis(200),
            ..super::super::test_server::timeouts()
        },
    );

    let result = pool.connect(&Fixture::id(), &Fixture::password()).await;

    assert_eq!(result, Err(ConnectFailure::Timeout));
}

#[tokio::test]
async fn a_refused_connection_is_unreachable_with_the_reason() {
    let pool = Pool::new(
        Arc::new(Refusing),
        KnownHostsFiles::default(),
        super::super::test_server::timeouts(),
    );

    let result = pool.connect(&Fixture::id(), &Fixture::password()).await;

    assert!(
        matches!(&result, Err(ConnectFailure::Unreachable(reason)) if reason.contains("refused")),
        "{result:?}"
    );
}

/// Every way a connection fails, as the result of a real attempt, with a
/// password that is easy to find in any text it might leak into.
async fn failures_with(secret: &Secret) -> Vec<Result<(), ConnectFailure>> {
    let id = Fixture::id();
    let mut results = Vec::new();
    // Refused before anything: unreachable.
    let refused = Pool::new(
        Arc::new(Refusing),
        KnownHostsFiles::default(),
        super::super::test_server::timeouts(),
    );
    results.push(refused.connect(&id, secret).await);
    // The server says the password is wrong.
    results.push(Fixture::trusted().pool.connect(&id, secret).await);
    // The server is not known: the password stays here.
    results.push(Fixture::untrusted().pool.connect(&id, secret).await);
    // The server takes no password.
    let keyboard_only = Fixture::over(TestServer::without_password_login());
    std::fs::write(
        keyboard_only.known.own.as_deref().unwrap(),
        format!("test.invalid {}\n", public_key_text()),
    )
    .unwrap();
    results.push(keyboard_only.pool.connect(&id, secret).await);
    results
}

/// The results of those attempts, as a `{:?}` shows them, are the text a
/// log or a bug report would carry.
#[tokio::test]
async fn the_password_appears_in_no_result_of_a_failed_attempt() {
    let secret = Secret::new("hunter2-the-password".to_owned());

    let results = failures_with(&secret).await;

    assert_eq!(results.len(), 4);
    assert!(
        matches!(results.first(), Some(Err(ConnectFailure::Unreachable(_)))),
        "{results:?}"
    );
    assert!(matches!(
        results.get(1),
        Some(Err(ConnectFailure::AuthFailed))
    ));
    assert!(matches!(
        results.get(2),
        Some(Err(ConnectFailure::UnknownHost { .. }))
    ));
    assert!(matches!(
        results.get(3),
        Some(Err(ConnectFailure::PasswordNotOffered))
    ));
    let shown = format!("{results:?}{secret:?}");
    assert!(
        !shown.contains("hunter2"),
        "the password leaked into {shown}"
    );
    assert!(shown.contains("***"), "the secret itself shows as stars");
}

#[tokio::test]
async fn a_server_that_offers_no_password_login_gets_no_password_and_is_named() {
    let fixture = Fixture::over(TestServer::without_password_login());
    std::fs::write(
        fixture.known.own.as_deref().unwrap(),
        format!("test.invalid {}\n", public_key_text()),
    )
    .unwrap();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await;

    assert_eq!(refused, Err(ConnectFailure::PasswordNotOffered));
    assert!(!fixture.server.control.events().contains(&"password"));
}

#[tokio::test]
async fn a_key_of_another_type_than_the_one_on_record_is_refused_without_auth() {
    let fixture = Fixture::untrusted();
    let ssh = fixture.known.ssh.clone().unwrap();
    std::fs::write(
        &ssh,
        format!(
            "# the ECDSA key was recorded\ntest.invalid {}\n",
            super::super::test_server::ECDSA_PUBLIC
        ),
    )
    .unwrap();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap_err();

    let ConnectFailure::HostKeyOtherType { file, line, .. } = refused else {
        panic!("{refused:?}");
    };
    assert_eq!((file, line), (ssh, 2));
    assert!(!fixture.server.control.events().contains(&"password"));
}

#[tokio::test]
async fn the_server_is_asked_for_the_key_type_on_record_first() {
    // It has both an ed25519 and an ECDSA key and offers the first type
    // the client names that it has; ed25519 would come first by default.
    let fixture = Fixture::over(TestServer::with_two_keys());
    std::fs::write(
        fixture.known.own.as_deref().unwrap(),
        format!("test.invalid {}\n", super::super::test_server::ECDSA_PUBLIC),
    )
    .unwrap();

    fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap();

    assert!(fixture.pool.is_connected(&Fixture::id()));
}

#[test]
fn the_types_on_record_lead_the_list_and_those_russh_lacks_are_left_out() {
    let offered = russh::Preferred::default().key.to_vec();
    assert!(offered.len() > 1);
    let last = offered.last().unwrap().clone();

    let order = ask_for_known_first(&offered, &[last.clone(), Algorithm::Dsa]);

    assert_eq!(order.first(), Some(&last));
    assert!(!order.contains(&Algorithm::Dsa) || offered.contains(&Algorithm::Dsa));
    let mut sorted_in: Vec<String> = offered.iter().map(ToString::to_string).collect();
    let mut sorted_out: Vec<String> = order.iter().map(ToString::to_string).collect();
    sorted_in.sort();
    sorted_out.sort();
    assert_eq!(sorted_in, sorted_out, "nothing is lost, nothing is added");
}

#[tokio::test]
async fn a_host_key_file_that_cannot_be_read_gives_no_way_to_trust_and_sends_nothing() {
    let fixture = Fixture::untrusted();
    let ssh = fixture.known.ssh.clone().unwrap();
    std::fs::create_dir(&ssh).unwrap();

    let refused = fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap_err();

    assert!(
        matches!(&refused, ConnectFailure::KnownHostsUnreadable { file, .. } if *file == ssh),
        "{refused:?}"
    );
    assert!(!fixture.server.control.events().contains(&"password"));
}

async fn wait_for_closed(fixture: &Fixture) -> bool {
    for _ in 0..150 {
        if fixture.server.control.events().contains(&"closed") {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn forgetting_a_connection_tells_the_server_we_are_leaving() {
    let fixture = Fixture::connected().await;
    // Another holder keeps the connection from ending just by being dropped.
    let held = fixture.pool.session(&Fixture::id()).unwrap();

    fixture.pool.forget(&Fixture::id());

    assert!(wait_for_closed(&fixture).await, "the server saw no goodbye");
    drop(held);
}

#[tokio::test]
async fn closing_all_tells_every_server_and_never_waits() {
    let fixture = Fixture::connected().await;
    let held = fixture.pool.session(&Fixture::id()).unwrap();

    fixture.pool.close_all();

    assert!(!fixture.pool.is_connected(&Fixture::id()));
    assert!(wait_for_closed(&fixture).await, "the server saw no goodbye");
    drop(held);
}

#[test]
fn closing_without_a_runtime_is_quiet() {
    let pool = Pool::new(
        Arc::new(Refusing),
        KnownHostsFiles::default(),
        super::super::test_server::timeouts(),
    );
    pool.close_all();
    pool.forget(&Fixture::id());
}

#[test]
fn a_request_that_ran_out_of_time_does_not_end_the_connection_but_a_lost_one_does() {
    assert!(!RemoteError::Timeout.breaks_connection());
    assert!(!RemoteError::Refused("no".into()).breaks_connection());
    assert!(!RemoteError::NotFound.breaks_connection());
    assert!(RemoteError::Lost("gone".into()).breaks_connection());
}

#[tokio::test]
async fn cut_connection_reports_disconnected_then_reconnects() {
    let fixture = Fixture::connected().await;
    assert_eq!(fixture.server.control.dials(), 1);

    fixture.server.control.cut().await;
    for _ in 0..100 {
        if !fixture.pool.is_connected(&Fixture::id()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let after_cut = listing(&fixture).await;
    assert!(
        matches!(after_cut.error, Some(ReadError::RemoteDisconnected { .. })),
        "{:?}",
        after_cut.error
    );

    fixture
        .pool
        .connect(&Fixture::id(), &Fixture::password())
        .await
        .unwrap();
    assert_eq!(fixture.server.control.dials(), 2);
    assert_eq!(listing(&fixture).await.error, None);
}

#[tokio::test]
async fn closing_all_drops_every_connection() {
    let fixture = Fixture::connected().await;

    fixture.pool.close_all();

    assert!(!fixture.pool.is_connected(&Fixture::id()));
}

#[tokio::test]
async fn a_second_account_on_the_same_host_is_its_own_session() {
    let fixture = Fixture::connected().await;
    let other = ServerId::new("tester", "test.invalid", 2222);

    assert!(fixture.pool.is_connected(&Fixture::id()));
    assert!(!fixture.pool.is_connected(&other));
}
