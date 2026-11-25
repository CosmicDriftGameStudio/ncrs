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
