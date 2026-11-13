//! F3 on a file a type rule sends to the viewer tab, in the real app: the
//! tab opens with the file, Tab switches to the source, Escape goes back.

use iced::keyboard::key::Named;
use iced::keyboard::Key;
use ncrs_viewer::Mode;

use super::*;

#[test]
fn f3_on_a_markdown_file_opens_the_viewer_tab_and_escape_closes_it() {
    let dir = scratch("viewer");
    write(&dir.path().join("notes.md"), "# Hello\n\nSome *text*.");
    write(&dir.path().join("plain.rs"), "fn main() {}");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "notes.md");
    session.press(Key::Named(Named::F3));

    assert!(session.app.viewer_for_test().is_open());
    assert!(session.app.viewer_is_shown_for_test());
    assert_eq!(session.app.viewer_for_test().title(), Some("notes.md"));
    assert_eq!(session.app.viewer_for_test().mode, Mode::Rendered);

    session.press(Key::Named(Named::Tab));
    assert_eq!(session.app.viewer_for_test().mode, Mode::Source);

    session.press(Key::Named(Named::Escape));
    assert!(!session.app.viewer_for_test().is_open());
    assert!(!session.app.viewer_is_shown_for_test());
}
