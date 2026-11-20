use super::*;
use crate::fs::Volume;
use crate::messages::PanelSide;
use std::path::PathBuf;

fn volume(name: &str, path: &str) -> Volume {
    Volume {
        name: name.to_string(),
        path: PathBuf::from(path),
    }
}

/// A fixed list, so the image does not depend on the machine's disks.
fn app_with_menu() -> App {
    let mut app = App::with_fixed_panels();
    app.open_volume_menu_for_test(
        PanelSide::Right,
        vec![
            volume("Macintosh HD", "/"),
            volume("test", "/home/test"),
            volume(
                "iCloud Drive",
                "/home/test/Library/Mobile Documents/com~apple~CloudDocs",
            ),
            volume("Backup", "/Volumes/Backup"),
        ],
    );
    app
}

/// The menu lists the drives, names the panel and highlights the one the
/// panel is on (`test`, for `/home/test/files`).
#[test]
fn the_menu_is_drawn_over_the_panels() {
    let matches = simulator(&app_with_menu())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/volumes.png")
        .expect("snapshot comparison");
    assert!(matches, "the drive menu does not match volumes.png");
}

#[test]
fn the_menu_matches_its_german_snapshot() {
    let matches = simulator(&app_with_menu().in_language(Language::German))
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/volumes_de.png")
        .expect("snapshot comparison");
    assert!(matches, "the drive menu does not match volumes_de.png");
}

fn app_with_favourites() -> App {
    let mut app = App::with_fixed_panels();
    app.open_volume_menu_with_favourites_for_test(
        PanelSide::Right,
        vec![volume("Macintosh HD", "/"), volume("test", "/home/test")],
        &[
            ("/home/test/Documents", true),
            ("/Volumes/Gone/Photos", false),
        ],
        2,
    );
    app
}

#[test]
fn the_menu_lists_favourites_below_the_drives() {
    let matches = simulator(&app_with_favourites())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/volumes_with_favourites.png")
        .expect("snapshot comparison");
    assert!(matches, "does not match volumes_with_favourites.png");
}

#[test]
fn the_favourites_match_their_german_snapshot() {
    let matches = simulator(&app_with_favourites().in_language(Language::German))
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/volumes_with_favourites_de.png")
        .expect("snapshot comparison");
    assert!(matches, "does not match volumes_with_favourites_de.png");
}

/// A menu that is not drawn would leave the plain panels, compared with the
/// committed two-panel reference.
#[cfg(target_os = "macos")]
#[test]
fn the_menu_differs_from_the_plain_panels() {
    let same_as_panels = simulator(&app_with_menu())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/two_panels.png")
        .expect("compare with the panels");
    assert!(!same_as_panels, "the drive menu is not drawn");
}

#[test]
fn a_click_on_an_entry_chooses_it() {
    let app = app_with_menu();
    let mut sim = simulator(&app);
    // Buttons have no id (see the module comment); the row is found by its text.
    sim.click("Backup").expect("the Backup row");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert_eq!(messages, [Message::Panel(PanelMessage::VolumeMenuClick(3))]);
}
