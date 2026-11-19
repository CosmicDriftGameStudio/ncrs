use super::*;
use crate::messages::ClipboardKind;

fn copied() -> App {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::CopyToClipboard(ClipboardKind::Path)));
    app
}

#[test]
fn the_notice_is_drawn_in_the_status_bar() {
    let matches = simulator(&copied())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/status_notice.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the status bar notice does not match status_notice.png"
    );
}

const NOTICE_TEXT: &str = "\u{2713} Path copied";

fn status_bar<'a>(app: &'a App, failure: Option<&'a str>) -> iced::Element<'a, Message> {
    crate::ui::statusbar::view(
        app.active_panel(),
        crate::i18n::Language::English,
        &crate::palette::Palette::default(),
        crate::ui::statusbar::Activity {
            side: crate::messages::PanelSide::Left,
            theme_name: None,
            job: None,
            job_press: None,
            filing: None,
            on_blocked_press: None,
            on_failure_press: None,
            blocked_by_sandbox: false,
            notice: app.notice_for_test(),
            hermes_line: None,
            note_line: None,
            version: None,
            monitor: None,
            test_folder: None,
            date_format: ncrs_core::DateFormat::Iso,
        },
        failure,
        None,
        crate::ui::statusbar::PathBar {
            editing: None,
            on_input: Message::PathFieldInput,
            on_submit: Message::PathFieldSubmit,
            on_copy: Message::PathFieldCancel,
            hit_line: None,
        },
    )
}

fn shows_notice(view: iced::Element<'_, Message>) -> bool {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 40.0),
        view,
    )
    .find(NOTICE_TEXT)
    .is_ok()
}

#[test]
fn the_status_bar_shows_the_notice_beside_an_error() {
    let app = copied();
    assert!(shows_notice(status_bar(&app, None)));
    assert!(shows_notice(status_bar(&app, Some("Copy failed"))));
}

#[test]
fn a_config_warning_and_a_notice_are_drawn_together() {
    let mut app = App::with_fixed_panels()
        .with_config_warning("Config file config.toml, line 2: directory on panel is hard to read");
    assert!(app.status_error().is_some());
    drop(app.update(Message::CopyToClipboard(ClipboardKind::Path)));
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/status_notice_with_error.png")
        .expect("snapshot comparison");
    assert!(
        matches,
        "the status bar does not match status_notice_with_error.png"
    );
}
