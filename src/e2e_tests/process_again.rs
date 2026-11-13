//! "Process again" on marked files and folders, in the real app: Hermes runs
//! with a live pipeline over a read-only location, a plain index and an
//! in-memory key store. No AI is configured, the text steps need no model.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::time::Duration;

use iced::keyboard::key::Named;
use iced::keyboard::Key;
use ncrs_i18n::{fill_template, Language, Msg};

use super::{cursor_to, write};
use crate::app::App;
use crate::e2e_tests::Session;
use crate::filing::{FilingMessage, Phase};
use crate::hermes_protocol::real_start::{boot_with, empty_store, pump_for};
use crate::hermes_protocol::thread::{documents_indexed, is_ready};
use crate::messages::{HermesFileMessage, Message, PanelSide};
use crate::session_state::SessionState;

fn pump_until<P>(session: &mut Session<P>, seconds: u64, until: fn(&App) -> bool)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    pump_for(session, Duration::from_secs(seconds), until);
    assert!(
        until(&session.app),
        "not reached within {seconds} s, notice {:?}",
        session.app.notice_for_test()
    );
}

/// `cursor_to` only goes down, so start from the top.
fn go_to<P>(session: &mut Session<P>, name: &str)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    session.press(Key::Named(Named::Home));
    cursor_to(session, name);
}

/// Delivers the message without `send`'s settle: that waits for the notice to
/// expire, and the notice is what the test looks at.
fn process_again<P>(session: &mut Session<P>)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    let task = session.send_held(Message::HermesFile(HermesFileMessage::ProcessAgain));
    session.drive(task);
}

fn tag_current<P>(session: &mut Session<P>, name: &str)
where
    P: iced::Program<State = App, Message = Message> + 'static,
{
    go_to(session, name);
    session.press(Key::Named(Named::Insert));
}

#[test]
fn process_again_takes_the_marked_files_and_folders_and_skips_the_excluded() {
    let data = tempfile::TempDir::new().unwrap();
    let documents = tempfile::TempDir::new().unwrap();
    let root = std::fs::canonicalize(documents.path()).unwrap();
    for name in ["one", "two", "three"] {
        write(
            &root.join("Haus").join(format!("{name}.txt")),
            &format!("Mietvertrag Haus {name}"),
        );
    }
    write(&root.join("excluded.txt"), "Rechnung, nicht verarbeiten");
    write(&root.join("single.txt"), "Rechnung Einzeldatei");
    // Excluding needs the target archive; the folder under test stays read-only.
    let target = std::fs::canonicalize(tempfile::TempDir::new().unwrap().keep()).unwrap();
    std::fs::write(
        data.path().join("config.toml"),
        format!(
            "version = 1\n[modules.filing]\nenabled = true\n\
             [[modules.filing.locations]]\npath = {root:?}\nmode = \"archive\"\n\
             [[modules.filing.locations]]\npath = {target:?}\nmode = \"archive\"\ntarget = true\n"
        ),
    )
    .unwrap();
    drop(ncrs_index::Index::create_plain(&data.path().join("index.db")).unwrap());

    let mut saved = SessionState::default();
    saved.left.path = Some(root.to_string_lossy().into_owned());
    let mut session = boot_with(data.path(), empty_store(), saved);
    let mut confirmed = false;
    for _ in 0..300 {
        pump_for(&mut session, Duration::from_millis(200), is_ready);
        if is_ready(&session.app) {
            break;
        }
        if !confirmed
            && session
                .app
                .filing_module_for_test()
                .is_some_and(|module| matches!(module.phase(), Phase::PlainFileWarning))
        {
            confirmed = true;
            session.send(Message::Filing(FilingMessage::ConfirmPlainFile));
        }
    }
    pump_until(&mut session, 1, is_ready);
    for _ in 0..300 {
        pump_for(&mut session, Duration::from_millis(200), |_| false);
        if documents_indexed(&session.app) >= Some(5) {
            break;
        }
    }
    assert!(
        documents_indexed(&session.app) >= Some(5),
        "the index never learned the five documents"
    );

    go_to(&mut session, "excluded.txt");
    session.send(Message::HermesFile(HermesFileMessage::ToggleExclusion));
    let index_path = data.path().join("index.db");
    let excluded_file = root.join("excluded.txt");
    let mut excluded = false;
    for _ in 0..100 {
        pump_for(&mut session, Duration::from_millis(200), |_| false);
        let index = ncrs_index::Index::open(&index_path, empty_store().as_ref()).unwrap();
        excluded = index.is_excluded(&excluded_file).unwrap();
        if excluded {
            break;
        }
    }
    assert!(excluded, "the file was never excluded from Hermes");

    tag_current(&mut session, "Haus");
    tag_current(&mut session, "single.txt");
    tag_current(&mut session, "excluded.txt");
    process_again(&mut session);
    pump_until(&mut session, 20, |app| {
        app.notice_for_test()
            .is_some_and(|notice| notice.contains("processed again"))
    });
    assert_eq!(
        session.app.notice_for_test(),
        Some(
            fill_template(
                Language::default().text(Msg::StatusHermesProcessingAgainMany),
                &[("{count}", "4")]
            )
            .as_str()
        )
    );

    for name in ["Haus", "single.txt", "excluded.txt"] {
        tag_current(&mut session, name);
    }
    assert!(!session.app.panel(PanelSide::Left).selection.any_tagged());

    go_to(&mut session, "single.txt");
    process_again(&mut session);
    pump_until(&mut session, 20, |app| {
        app.notice_for_test()
            .is_some_and(|notice| notice.contains("single.txt"))
    });
    assert_eq!(
        session.app.notice_for_test(),
        Some(
            fill_template(
                Language::default().text(Msg::StatusHermesProcessingAgain),
                &[("{name}", "single.txt")]
            )
            .as_str()
        )
    );
}
