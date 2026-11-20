use super::*;
use iced::keyboard::{key::Named, Key, Modifiers};

/// Ctrl+P, a few letters of a path, Enter: the active panel shows the
/// file's directory with the cursor on the file, and the box is gone.
#[test]
fn typing_scattered_letters_and_enter_jump_to_the_file() {
    let dir = scratch("quick-open");
    write(&dir.path().join("src").join("config.rs"), "x");
    write(&dir.path().join("src").join("main.rs"), "x");
    write(&dir.path().join("README.md"), "x");
    let mut session = session_in(dir.path());

    session.press_with(Key::Character("p".into()), Modifiers::COMMAND);
    assert!(
        session.app.quick_open_for_test().is_some(),
        "Ctrl+P opened nothing"
    );
    session.type_into_widgets("cfgrs");
    session.settle();

    let quick_open = session.app.quick_open_for_test().expect("the box");
    assert!(!quick_open.walking(), "the walk did not finish");
    let shown: Vec<&str> = quick_open
        .hits()
        .iter()
        .map(|hit| quick_open.relative_path(hit))
        .collect();
    assert_eq!(
        shown,
        [Path::new("src").join("config.rs").to_string_lossy()]
    );

    session.press_in_widgets(Key::Named(Named::Enter), Modifiers::default());
    session.settle();

    assert!(
        session.app.quick_open_for_test().is_none(),
        "Enter left it up"
    );
    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(
        session.app.panel(PanelSide::Left).path,
        canonical.join("src")
    );
    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("config.rs")
    );
}

#[test]
fn escape_closes_it_and_leaves_the_panel_alone() {
    let dir = scratch("quick-open-escape");
    write(&dir.path().join("a.txt"), "x");
    let mut session = session_in(dir.path());
    let canonical = std::fs::canonicalize(dir.path()).unwrap();

    session.press_with(Key::Character("p".into()), Modifiers::COMMAND);
    session.press(Key::Named(Named::Escape));

    assert!(
        session.app.quick_open_for_test().is_none(),
        "Escape left it up"
    );
    assert_eq!(session.app.panel(PanelSide::Left).path, canonical);
}
