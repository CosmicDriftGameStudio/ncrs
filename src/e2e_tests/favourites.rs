//! The favourites bar, driven with real keys: Cmd+D pins a folder, Cmd+1 comes
//! back to it, and the order the settings page gives is the order of the keys
//! and of what the session keeps on disk.

use iced::keyboard::{Key, Modifiers};

use super::*;
use crate::messages::SettingsMessage;
use ncrs_settings::FavouritesEdit;

#[test]
fn cmd_d_pins_a_folder_cmd_1_returns_to_it_and_the_settings_reorder_the_keys() {
    let dir = scratch("favourites");
    let state_file = dir.path().join("state").join("session.json");
    let left = std::fs::canonicalize(dir.path()).expect("a canonical scratch path");
    let right = left.join("home");
    let (state_for_app, left_for_app) = (state_file.clone(), left.clone());
    let mut session = Session::boot(crate::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left_for_app.clone(), right.clone());
            (app.with_state_path(state_for_app.clone()), task)
        },
    ));
    let rows_without_bar = session.app.visible_rows_for_test();

    // Pin the scratch root as favourite 1, walk into `links`, pin that too.
    session.press_with(Key::Character("d".into()), Modifiers::COMMAND);
    cursor_to(&mut session, "links");
    session.press(Key::Named(iced::keyboard::key::Named::Enter));
    assert_eq!(session.app.panel(PanelSide::Left).path, left.join("links"));
    session.press_with(Key::Character("d".into()), Modifiers::COMMAND);
    assert_eq!(
        session.app.favourites_for_test(),
        [left.clone(), left.join("links")]
    );
    assert!(
        session.app.visible_rows_for_test() < rows_without_bar,
        "the bar takes a row from the panels"
    );

    // Cmd+1 goes back to the first one.
    session.press_with(Key::Character("1".into()), Modifiers::COMMAND);
    assert_eq!(session.app.panel(PanelSide::Left).path, left);

    // The page moves `links` up: now Cmd+1 lands there and Cmd+2 in the root.
    session.send(Message::Settings(SettingsMessage::Favourites(
        FavouritesEdit::Up(1),
    )));
    session.press_with(Key::Character("1".into()), Modifiers::COMMAND);
    assert_eq!(session.app.panel(PanelSide::Left).path, left.join("links"));
    session.press_with(Key::Character("2".into()), Modifiers::COMMAND);
    assert_eq!(session.app.panel(PanelSide::Left).path, left);
    // No favourite 3: nothing happens, nothing is reported.
    session.press_with(Key::Character("3".into()), Modifiers::COMMAND);
    assert_eq!(session.app.panel(PanelSide::Left).path, left);
    assert!(session.app.job_error_for_test().is_none());

    // The debounced write has run once everything settled.
    session.settle();
    let kept = crate::session_state::load(&state_file);
    assert_eq!(
        kept.favourites,
        [
            left.join("links").display().to_string(),
            left.display().to_string()
        ]
    );
}
