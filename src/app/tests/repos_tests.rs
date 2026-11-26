//! The keys of the Repositories tab, from the key press to the shown tab.

use crate::app::*;

#[tokio::test]
async fn enter_shows_the_git_tab_of_the_repository_under_the_cursor() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenRepos));
    drop(app.update(Message::Repos(ncrs_repos::Message::Learn(
        root.path().to_path_buf(),
    ))));
    let pressed = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Enter),
        Modifiers::empty(),
    );
    drop(app.update(pressed.unwrap()));
    assert!(matches!(app.shown_tab, ShownTab::Git(_)));
}

#[tokio::test]
async fn the_search_of_the_settings_does_not_stop_the_one_of_the_opened_tab() {
    let mut app = App::with_fixed_panels();
    app.repos_settings.sources = vec![app.home.join("code")];
    drop(app.scan_repos());
    let scan_ended = ncrs_repos::Message::Scanned(Box::default());
    drop(app.update_repos(scan_ended));
    assert!(app.repos.as_ref().unwrap().is_scanning());
}

#[tokio::test]
async fn the_menu_of_a_row_opens_and_runs_from_the_keyboard() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenRepos));
    drop(app.update(Message::Repos(ncrs_repos::Message::Learn(
        root.path().to_path_buf(),
    ))));
    let press = |target: &mut App, key: Named, modifiers: Modifiers| {
        let pressed = route_key(&target.prompt_key_state(), Key::Named(key), modifiers);
        drop(target.update(pressed.unwrap()));
    };
    if cfg!(target_os = "macos") {
        press(&mut app, Named::Enter, Modifiers::CTRL);
        assert_eq!(
            app.repos.as_ref().unwrap().menu.as_deref(),
            Some(root.path())
        );
        // Escape comes through `InputState`, not the key routing.
        drop(app.update(Message::Repos(ncrs_repos::Message::Escape)));
        assert!(app.repos.as_ref().unwrap().menu.is_none());
    }
    press(&mut app, Named::F10, Modifiers::SHIFT);
    let repos = app.repos.as_ref().unwrap();
    assert_eq!(repos.menu.as_deref(), Some(root.path()));
    assert_eq!(repos.menu_cursor, 0);
    press(&mut app, Named::ArrowDown, Modifiers::empty());
    assert_eq!(app.repos.as_ref().unwrap().menu_cursor, 1);
    press(&mut app, Named::ArrowUp, Modifiers::empty());
    press(&mut app, Named::Enter, Modifiers::empty());
    assert!(app.repos.as_ref().unwrap().menu.is_none());
    assert!(matches!(app.shown_tab, ShownTab::Git(_)));
}

#[tokio::test]
async fn a_tap_of_option_opens_the_menu_of_the_row_as_in_the_panels() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenRepos));
    drop(app.update(Message::Repos(ncrs_repos::Message::Learn(
        root.path().to_path_buf(),
    ))));
    let tap = |target: &mut App| {
        target.publish_key_state();
        let mut input = target.input.lock();
        let modifiers = |held| iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(held));
        assert_eq!(
            input.handle(&modifiers(Modifiers::ALT), event::Status::Ignored),
            None
        );
        input.handle(&modifiers(Modifiers::empty()), event::Status::Ignored)
    };
    let tapped = tap(&mut app).expect("the tap opens nothing");
    drop(app.update(tapped));
    assert_eq!(
        app.repos.as_ref().unwrap().menu.as_deref(),
        Some(root.path())
    );
    // A second tap while the menu is open does not open it again.
    assert_eq!(tap(&mut app), None);
}

#[test]
fn a_held_arrow_repeats_in_the_repo_list_without_the_page_listing_it() {
    let state = PromptKeyState {
        repos: true,
        ..PromptKeyState::default()
    };
    let held = |key: Named, modifiers: Modifiers| {
        route_press(
            &state,
            Key::Named(key),
            iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::ArrowDown),
            modifiers,
            true,
        )
    };
    assert!(matches!(
        held(Named::ArrowDown, Modifiers::empty()),
        Some(Message::Repos(_))
    ));
    assert!(matches!(
        held(Named::PageDown, Modifiers::empty()),
        Some(Message::Repos(_))
    ));
    assert_eq!(held(Named::ArrowDown, Modifiers::COMMAND), None);
}
