// reason: a failing assertion is the signal in a test
#![allow(clippy::unwrap_used)]

use iced::widget::{column, container};
use iced::{Element, Length, Size};
use iced_test::Simulator;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::{Palette, ThemePreset};
use ncrs_ui_kit::theme;
use semver::Version;

use crate::release::TRUSTED_NOTES_PREFIX;
use crate::state::{update, Ctx, Message, State};
use crate::view::{view, ViewCtx};
use crate::{Channel, LatestRelease, PackageManager};

const PAGE: Size = Size::new(900.0, 330.0);

fn never_fetch() -> Result<LatestRelease, String> {
    Err("no network in tests".to_owned())
}

fn state_without_news(channel: Channel) -> State {
    State::new("1.2.0", channel, None)
}

fn state_with_news(channel: Channel) -> State {
    let mut state = state_without_news(channel);
    let ctx = Ctx {
        enabled: None,
        now_unix: 0,
        fetch: never_fetch,
        state_file: None,
    };
    let found = LatestRelease {
        version: Version::new(1, 3, 0),
        notes_url: format!("{TRUSTED_NOTES_PREFIX}tag/v1.3.0"),
    };
    let _ = update(&mut state, Message::Checked(Ok(found)), &ctx);
    state
}

fn page<'a>(state: &State, enabled: bool, palette: &'a Palette) -> Element<'a, Message> {
    view(
        state,
        ViewCtx {
            lang: Language::English,
            palette,
            enabled,
        },
        None,
    )
}

#[test]
fn a_direct_download_offers_the_notes_and_skipping() {
    let palette = Palette::default();
    let state = state_with_news(Channel::Direct);
    let mut sim = Simulator::new(page(&state, true, &palette));
    sim.find("Version 1.3.0 is available.").unwrap();
    sim.click("Release notes").unwrap();
    assert_eq!(
        sim.into_messages().collect::<Vec<_>>(),
        [Message::OpenReleaseNotes]
    );

    let mut skipping = Simulator::new(page(&state, true, &palette));
    skipping.click("Skip this version").unwrap();
    assert_eq!(
        skipping.into_messages().collect::<Vec<_>>(),
        [Message::SkipThisVersion]
    );
}

#[test]
fn a_package_install_names_the_command_and_has_no_notes_button() {
    let palette = Palette::default();
    let state = state_with_news(Channel::PackageManager(PackageManager::Apt));
    let mut sim = Simulator::new(page(&state, true, &palette));
    sim.find("Update with:").unwrap();
    sim.find("Release notes").unwrap_err();
    sim.find("Skip this version").unwrap();
}

#[test]
fn a_development_build_with_the_check_on_gets_the_news_like_a_download() {
    let palette = Palette::default();
    let state = state_with_news(Channel::Development);
    let mut sim = Simulator::new(page(&state, true, &palette));
    sim.find("Version 1.3.0 is available.").unwrap();
    sim.find("Release notes").unwrap();
    sim.find("Skip this version").unwrap();
}

#[test]
fn news_found_earlier_is_not_shown_while_the_check_is_off() {
    let palette = Palette::default();
    let state = state_with_news(Channel::Direct);
    let mut sim = Simulator::new(page(&state, false, &palette));
    sim.find("Installed version").unwrap();
    sim.find("Version 1.3.0 is available.").unwrap_err();
}

#[test]
fn without_news_the_page_says_so_only_while_the_check_is_on() {
    let palette = Palette::default();
    let state = state_without_news(Channel::Direct);
    let mut on = Simulator::new(page(&state, true, &palette));
    on.find("1.2.0").unwrap();
    on.find("ncrs is up to date.").unwrap();
    let mut off = Simulator::new(page(&state, false, &palette));
    off.find("ncrs is up to date.").unwrap_err();
}

fn matches_image(state: &State, enabled: bool, name: &str) {
    let palette = ThemePreset::DEFAULT.palette();
    let content = container(column![page(state, enabled, &palette)].padding(12))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root(&palette));
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        PAGE,
        content,
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(format!("tests/snapshots/{name}.png"))
        .unwrap();
    assert!(matches, "{name} differs from its image");
}

/// Named for CI, which compares pixels only on macOS.
mod ui_tests {
    use super::*;

    #[test]
    fn the_page_of_a_direct_download_with_a_newer_release() {
        matches_image(
            &state_with_news(Channel::Direct),
            true,
            "update-page-direct",
        );
    }

    #[test]
    fn the_page_of_a_package_install_with_a_newer_release() {
        matches_image(
            &state_with_news(Channel::PackageManager(PackageManager::Apt)),
            true,
            "update-page-apt",
        );
    }

    #[test]
    fn the_page_when_nothing_newer_is_known() {
        matches_image(
            &state_without_news(Channel::Direct),
            true,
            "update-page-up-to-date",
        );
    }
}
