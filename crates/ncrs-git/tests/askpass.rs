//! A real git asks for a login over HTTP; the answer comes through the broker.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use ncrs_git::askpass::{Broker, PromptKind, Secret};

/// Answers 401 until a request carries credentials, then 404; reports the credentials header.
fn server_wanting_a_login() -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut head = Vec::new();
            let mut byte: [u8; 1] = [0; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let text = String::from_utf8_lossy(&head).to_string();
            let login = text
                .lines()
                .find(|line| line.to_lowercase().starts_with("authorization:"))
                .map(str::to_string);
            let reply = if let Some(header) = login {
                let _ = seen.send(header);
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            } else {
                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (port, receiver)
}

fn ls_remote(port: u16, endpoint: &str, token: &str) -> std::process::Output {
    let empty = std::env::temp_dir().join("ncrs-git-tests-empty-config");
    std::fs::write(&empty, "").unwrap();
    Command::new("git")
        .args(["-c", "credential.helper=", "ls-remote"])
        .arg(format!("http://127.0.0.1:{port}/repo.git"))
        .env("GIT_CONFIG_GLOBAL", &empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .env("GIT_ASKPASS", env!("CARGO_BIN_EXE_askpass-probe"))
        .env("NCRS_ASKPASS", endpoint)
        .env("NCRS_ASKPASS_TOKEN", token)
        .output()
        .unwrap()
}

#[test]
fn git_asks_for_name_and_password_and_sends_what_was_answered() {
    let (port, logins) = server_wanting_a_login();
    let (broker, requests) = Broker::start().unwrap();
    let (endpoint, token) = (broker.endpoint(), broker.token(5));
    let running = std::thread::spawn(move || ls_remote(port, &endpoint, &token));

    let first = requests.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!((first.job, first.kind), (5, PromptKind::Text));
    assert!(first.prompt.starts_with("Username for"), "{}", first.prompt);
    broker.answer(first.id, Some(Secret::new("marc")));
    let second = requests.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(second.kind, PromptKind::Secret);
    broker.answer(second.id, Some(Secret::new("s3cret")));

    running.join().unwrap();
    let header = logins.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(header.ends_with("Basic bWFyYzpzM2NyZXQ="), "{header}");
}

#[test]
fn a_cancelled_question_makes_git_fail_without_a_login() {
    let (port, logins) = server_wanting_a_login();
    let (broker, requests) = Broker::start().unwrap();
    let (endpoint, token) = (broker.endpoint(), broker.token(1));
    let running = std::thread::spawn(move || ls_remote(port, &endpoint, &token));

    let first = requests.recv_timeout(Duration::from_secs(20)).unwrap();
    broker.cancel_job(first.job);
    let output = running.join().unwrap();
    assert!(!output.status.success());
    assert!(
        logins.try_recv().is_err(),
        "no credentials may have been sent"
    );
}

#[test]
fn deny_mode_fails_at_once_and_asks_nobody() {
    let (port, logins) = server_wanting_a_login();
    let output = ls_remote(port, "deny", "");
    assert!(!output.status.success());
    logins.try_recv().unwrap_err();
}
