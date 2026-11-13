// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::Size;
use ncrs_i18n::Language;

use crate::load::{load, Loaded};
use crate::{update, viewer_key, Action, Ctx, Message, Mode, State, MAX_BYTES};

const CTX: Ctx = Ctx {
    lang: Language::English,
    window_size: Size::new(1000.0, 800.0),
};

fn opened(dir: &tempfile::TempDir, name: &str, text: &str) -> State {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    let mut state = State::default();
    let _ = update(&mut state, Message::Loaded(Box::new(load(path))), &CTX);
    state
}

fn action(state: &mut State, message: Message) -> Option<Action> {
    update(state, message, &CTX).1
}

#[test]
fn markdown_opens_rendered_and_toggles_to_source() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(&dir, "README.md", "# Title\n\nSome **text**.\n");
    assert_eq!(state.mode, Mode::Rendered);
    assert!(!state.doc.as_ref().unwrap().items.is_empty());
    action(&mut state, Message::ToggleMode);
    assert_eq!(state.mode, Mode::Source);
    action(&mut state, Message::ToggleMode);
    assert_eq!(state.mode, Mode::Rendered);
}

#[test]
fn other_text_is_source_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(&dir, "notes.txt", "plain");
    assert_eq!(state.mode, Mode::Source);
    action(&mut state, Message::ToggleMode);
    assert_eq!(state.mode, Mode::Source);
}

#[test]
fn a_file_that_is_not_utf8_is_shown_lossy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin.txt");
    fs::write(&path, [b'a', 0xE9, b'b']).unwrap();
    let Loaded::Doc(doc) = load(path) else {
        panic!("not loaded");
    };
    assert_eq!(doc.text, "a\u{FFFD}b");
}

#[test]
fn a_file_over_the_cap_asks_for_the_system_instead() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.md");
    fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
    let mut state = State::default();
    let (_, action) = update(
        &mut state,
        Message::Loaded(Box::new(load(path.clone()))),
        &CTX,
    );
    assert!(!state.is_open());
    let Some(Action::OpenSystemInstead {
        path: asked,
        notice,
    }) = action
    else {
        panic!("no action: {action:?}");
    };
    assert_eq!(asked, path);
    assert!(notice.contains("big.md"), "{notice}");
}

#[test]
fn a_missing_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = State::default();
    let loaded = load(dir.path().join("gone.md"));
    let (_, action) = update(&mut state, Message::Loaded(Box::new(loaded)), &CTX);
    assert!(matches!(action, Some(Action::Error(_))), "{action:?}");
    assert!(!state.is_open());
}

#[tokio::test]
async fn links_go_to_the_right_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(&dir, "README.md", "x");
    let click =
        |viewer: &mut State, target: &str| action(viewer, Message::LinkClicked(target.to_owned()));
    assert_eq!(
        click(&mut state, "https://example.org"),
        Some(Action::OpenUrl("https://example.org".into()))
    );
    assert_eq!(click(&mut state, "#top"), None);
    assert_eq!(
        click(&mut state, "scan.pdf"),
        Some(Action::Reveal(dir.path().join("scan.pdf")))
    );
    // A Markdown link loads in the same tab: no action, the answer comes as a message.
    assert_eq!(click(&mut state, "other.md"), None);
}

#[test]
fn escape_closes_the_tab_and_asks_the_app_to_leave() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(&dir, "README.md", "x");
    let message = viewer_key(&Key::Named(Named::Escape), Modifiers::empty(), true).unwrap();
    assert_eq!(action(&mut state, message), Some(Action::Close));
    assert!(!state.is_open());
}

#[test]
fn tab_toggles_only_for_markdown() {
    let tab = Key::Named(Named::Tab);
    assert!(matches!(
        viewer_key(&tab, Modifiers::empty(), true),
        Some(Message::ToggleMode)
    ));
    assert!(viewer_key(&tab, Modifiers::empty(), false).is_none());
    assert!(viewer_key(&tab, Modifiers::SHIFT, true).is_none());
}

#[test]
fn external_and_edit_name_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(&dir, "README.md", "x");
    let path = dir.path().join("README.md");
    assert_eq!(
        action(&mut state, Message::OpenExternal),
        Some(Action::OpenExternal(path.clone()))
    );
    assert_eq!(action(&mut state, Message::Edit), Some(Action::Edit(path)));
}
