//! The Updates page of the settings tab: the installed version, the switch of
//! the check, and what to do about a newer release.

use iced::widget::{column, row};
use iced::Element;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::setting_rows::{self, Group, SettingRow};
use ncrs_ui_kit::theme::spacing;

use crate::channel::Channel;
use crate::state::{Message, State};

/// The id of the field that shows the update command, so it can be selected.
pub const COMMAND_FIELD_ID: &str = "updates-command";

#[derive(Debug)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    /// Whether the check shows as on.
    pub enabled: bool,
}

/// The rows about a newer release.
fn news_rows<'a>(
    state: &State,
    lang: Language,
    palette: &'a Palette,
) -> Vec<SettingRow<'a, Message>> {
    let Some(release) = state.newer_release() else {
        return Vec::new();
    };
    let button = |label: Msg, message: Message| {
        kit::button(lang.text(label), Some(message), Tone::Secondary, palette)
    };
    let available = fill_template(
        lang.text(Msg::UpdatesAvailable),
        &[("{version}", &release.version.to_string())],
    );
    match state.channel() {
        // A development build with the check switched on gets the same news
        // as the status bar dot, so the two never disagree.
        Channel::Direct | Channel::Development => vec![SettingRow::text(
            available,
            None,
            row![
                button(Msg::UpdatesReleaseNotes, Message::OpenReleaseNotes),
                button(Msg::UpdatesSkipVersion, Message::SkipThisVersion),
            ]
            .spacing(spacing::S)
            .into(),
        )],
        Channel::PackageManager(manager) => vec![
            SettingRow::text(
                available,
                None,
                button(Msg::UpdatesSkipVersion, Message::SkipThisVersion),
            ),
            SettingRow::text(
                lang.text(Msg::UpdatesRunCommand).to_owned(),
                None,
                kit::mono_text_field(
                    "",
                    manager.update_command(),
                    COMMAND_FIELD_ID,
                    None::<fn(String) -> Message>,
                    Message::Ignored,
                    palette,
                ),
            ),
        ],
    }
}

/// The page content under the heading the settings tab draws.
pub fn view<'a>(state: &State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let ViewCtx {
        lang,
        palette,
        enabled,
    } = ctx;
    let mut rows = vec![
        setting_rows::simple_row(
            Msg::UpdatesInstalledVersion,
            None,
            setting_rows::status_text(state.display_version().to_owned(), palette.text),
            lang,
        ),
        setting_rows::simple_row(
            Msg::SettingsRowUpdateCheck,
            Some(Msg::SettingsRowUpdateCheckNote),
            setting_rows::switch(enabled, Message::SetEnabled(!enabled), lang, palette),
            lang,
        ),
    ];
    // What a check found earlier stays in updates.toml; with the check off it
    // is not news any more.
    if enabled {
        rows.extend(news_rows(state, lang, palette));
    }
    let sections = setting_rows::sections(
        vec![Group {
            intro: None,
            title: String::new(),
            rows,
        }],
        |_| true,
        None,
        palette,
    );
    let mut page = column(sections).spacing(spacing::XS);
    if enabled && state.newer_release().is_none() {
        page = page.push(kit::dim(lang.text(Msg::UpdatesUpToDate), palette));
    }
    page.into()
}
