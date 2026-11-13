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

#[test]
fn f8_and_space_in_the_viewer_tab_delete_and_tag_nothing() {
    let dir = scratch("viewer-keys");
    write(&dir.path().join("notes.md"), "# Hello");
    write(&dir.path().join("other.txt"), "x");
    let bin = tempfile::tempdir().unwrap();
    let mut session = session_with_trash(
        dir.path(),
        std::sync::Arc::new(FolderTrash(bin.path().to_path_buf())),
    );
    cursor_to(&mut session, "notes.md");
    session.press(Key::Named(Named::F3));
    assert!(session.app.viewer_is_shown_for_test());
    let (names_before, cursor_before) = (
        names(&session.app, PanelSide::Left),
        selected(&session.app, PanelSide::Left),
    );

    for key in [Named::F8, Named::Space, Named::Insert, Named::Delete] {
        session.press(Key::Named(key));
    }

    assert!(session.app.viewer_is_shown_for_test());
    assert!(!session.app.delete_dialog_is_open());
    assert!(!session.app.panel(PanelSide::Left).selection.any_tagged());
    assert_eq!(names(&session.app, PanelSide::Left), names_before);
    assert_eq!(selected(&session.app, PanelSide::Left), cursor_before);
    assert!(std::fs::read_dir(bin.path()).unwrap().next().is_none());
    assert!(dir.path().join("notes.md").exists());
}
