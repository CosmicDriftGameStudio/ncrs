use super::about_tests::{app_with_launcher, calls};
use crate::app::*;

const NEWER_RELEASE: &str = r#"{
    "tag_name": "v1.3.0",
    "html_url": "https://github.com/CosmicDriftGameStudio/ncrs/releases/tag/v1.3.0",
    "draft": false,
    "prerelease": false
}"#;

fn found(app: &mut App, body: &str) {
    let release = ncrs_updates::release::parse_latest(body).map_err(|err| err.to_string());
    drop(app.update(Message::Updates(ncrs_updates::Message::Checked(release))));
}

/// The App Store build makes no request, so it never shows the dot.
#[test]
fn a_newer_release_puts_its_version_on_the_status_bar_badge() {
    let mut app = App::with_fixed_panels();
    assert_eq!(app.newer_version_text(), None);
    found(&mut app, NEWER_RELEASE);
    assert_eq!(
        app.newer_version_text().as_deref(),
        ncrs_updates::CHECK_AVAILABLE.then_some("1.3.0")
    );
}

#[test]
fn a_release_that_is_not_newer_leaves_the_badge_as_it_was() {
    let mut app = App::with_fixed_panels();
    found(&mut app, &NEWER_RELEASE.replace("v1.3.0", "v1.2.3"));
    assert_eq!(app.newer_version_text(), None);
}

#[test]
fn turning_the_check_off_takes_the_dot_away() {
    let mut app = App::with_fixed_panels();
    found(&mut app, NEWER_RELEASE);
    app.update_check = Some(false);
    assert_eq!(app.newer_version_text(), None);
}

#[test]
fn skipping_the_version_takes_the_dot_away() {
    let mut app = App::with_fixed_panels();
    found(&mut app, NEWER_RELEASE);
    drop(app.update(Message::Updates(ncrs_updates::Message::SkipThisVersion)));
    assert_eq!(app.newer_version_text(), None);
}

#[test]
fn the_release_notes_button_launches_the_release_page() {
    let (mut app, launcher) = app_with_launcher(|| None);
    found(&mut app, NEWER_RELEASE);
    drop(app.update(Message::Updates(ncrs_updates::Message::OpenReleaseNotes)));
    let launched = calls(&launcher);
    assert_eq!(
        launched.first().and_then(|call| call.args.last()),
        Some(&std::ffi::OsString::from(
            "https://github.com/CosmicDriftGameStudio/ncrs/releases/tag/v1.3.0"
        ))
    );
}

#[cfg(not(feature = "sandbox"))]
#[test]
fn pressing_the_version_opens_the_updates_page() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::VersionPressed));
    let state = app.settings.page.as_ref().unwrap();
    assert_eq!(
        settings_ui::shown_feature(&app.settings_view(state)),
        Some(crate::messages::SettingsFeature::Updates)
    );
    assert!(app.about_for_test().is_none());
}

#[cfg(feature = "sandbox")]
#[test]
fn pressing_the_version_opens_the_about_card_where_there_is_no_update_check() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::VersionPressed));
    assert!(app.about_for_test().is_some());
    assert!(app.settings.page.is_none());
}
