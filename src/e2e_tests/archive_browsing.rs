use super::*;
use crate::fs::archive::fixtures::{write_archive, Item};
use iced::keyboard::key::Named;

fn zip_with_photos(dir: &Path) {
    write_archive(
        &dir.join("photos.zip"),
        &[
            Item::Dir("2024"),
            Item::File("2024/a.txt", b"from the zip"),
            Item::File("top.txt", b"top"),
        ],
    );
}

#[test]
fn an_archive_opens_like_a_folder_copies_out_and_leaves_with_the_cursor_on_it() {
    let dir = scratch("archive");
    zip_with_photos(dir.path());
    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    let mut session = session_in(dir.path());

    cursor_to(&mut session, "photos.zip");
    session.press(Key::Named(Named::Enter));
    let archive = canonical.join("photos.zip");
    assert_eq!(session.app.panel(PanelSide::Left).path, archive);
    assert_eq!(
        session.app.panel(PanelSide::Left).archive.as_deref(),
        Some(archive.as_path())
    );

    cursor_to(&mut session, "2024");
    session.press(Key::Named(Named::Enter));
    assert_eq!(
        session.app.panel(PanelSide::Left).path,
        archive.join("2024")
    );
    cursor_to(&mut session, "a.txt");

    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));
    let copied = canonical.join("home").join("a.txt");
    assert_eq!(
        std::fs::read_to_string(&copied).unwrap(),
        "from the zip",
        "F5 did not copy the entry out of the archive"
    );

    session.send(Message::Panel(PanelMessage::GoUp));
    session.send(Message::Panel(PanelMessage::GoUp));
    assert_eq!(session.app.panel(PanelSide::Left).path, canonical);
    assert!(session.app.panel(PanelSide::Left).archive.is_none());
    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("photos.zip"),
        "leaving the archive did not put the cursor back on it"
    );
}
