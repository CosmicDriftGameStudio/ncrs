use super::*;
use iced::keyboard::{key::Named, Key, Modifiers};

/// Alt+F7, a typed word, two files with their line numbers, Enter on the
/// second file shows it in the other panel, and Escape brings the panel
/// back with the search still there.
#[test]
fn typing_finds_the_lines_enter_shows_the_file_and_escape_keeps_the_search() {
    let dir = scratch("search");
    write(&dir.path().join("a.txt"), "one\nhello there\nthree\n");
    write(
        &dir.path().join("sub").join("b.txt"),
        "hello first\nnothing\nsay hello\n",
    );
    write(&dir.path().join("c.txt"), "nothing to see\n");
    let mut session = session_in(dir.path());

    session.press_with(Key::Named(Named::F7), Modifiers::ALT);
    assert!(
        session.app.search_is_open_for_test(),
        "Alt+F7 opened nothing"
    );
    session.send(Message::Search(ncrs_search::SearchMessage::ToggleInFiles));
    session.type_text("hello");

    let view = session.app.search_for_test().expect("the search view");
    assert_eq!(view.query, "hello");
    assert!(!view.running, "the search did not finish");
    let found: Vec<(String, Vec<u64>)> = view
        .results
        .iter()
        .map(|result| {
            (
                result
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                result.lines.iter().map(|line| line.number).collect(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("a.txt".to_string(), vec![2]),
            ("b.txt".to_string(), vec![1, 3])
        ]
    );

    let first = &view.results[0];
    assert_eq!(
        first.size,
        std::fs::metadata(&first.path).unwrap().len(),
        "the walk read the size of the file"
    );
    assert!(first.modified.is_some());

    session.press(Key::Named(Named::ArrowDown));
    assert_eq!(
        session
            .app
            .search_for_test()
            .and_then(|search| search.cursor),
        Some(1)
    );
    session.press(Key::Named(Named::Enter));

    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(
        session.app.panel(PanelSide::Right).path,
        canonical.join("sub")
    );
    assert_eq!(
        selected(&session.app, PanelSide::Right).as_deref(),
        Some("b.txt")
    );

    session.press(Key::Named(Named::Escape));
    assert!(!session.app.search_is_open_for_test(), "Escape left it up");
    let kept = session.app.search_for_test().expect("the kept search");
    assert_eq!(kept.query, "hello");
    assert_eq!(kept.results.len(), 2);
}
