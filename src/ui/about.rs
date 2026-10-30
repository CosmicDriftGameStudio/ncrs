//! The about card: what ncrs is, its version and licence, and the way to the
//! licences of the libraries it is built from. Modal. Pure view over
//! [`AboutCard`].

use iced::widget::column;
use iced::{Element, Size};

use super::frame::{DialogSize, Frame, KeyHints};
use super::kit::{self, paragraph, Tone};
use super::theme::spacing;
use crate::about::{third_party_licenses_url, AboutCard, REPOSITORY_URL};
use crate::app::fill_template;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{AboutMessage, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right.
pub const REPOSITORY_FOCUS: usize = 0;
pub const LICENSES_FOCUS: usize = 1;
pub const CLOSE_FOCUS: usize = 2;

/// `focus` is the button Enter would press.
pub fn view<'a>(
    card: &AboutCard,
    version: &str,
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut body = column![
        paragraph(
            fill_template(lang.text(Msg::AboutVersion), &[("{version}", version)]),
            palette
        ),
        paragraph(lang.text(Msg::AboutLicense), palette),
        paragraph(REPOSITORY_URL, palette),
    ]
    .spacing(spacing::LINE_GAP);
    if card.licenses_missing() {
        body = body.push(paragraph(
            fill_template(
                lang.text(Msg::AboutLicensesMissing),
                &[("{url}", third_party_licenses_url(version).as_str())],
            ),
            palette,
        ));
    }

    Frame::new(
        lang.text(Msg::AboutTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::AboutTitle))
    .buttons(vec![
        kit::button(
            lang.text(Msg::AboutRepository),
            Some(Message::About(AboutMessage::OpenRepository)),
            Tone::filled_if(focus == REPOSITORY_FOCUS),
            palette,
        ),
        kit::button(
            lang.text(Msg::AboutLicenses),
            Some(Message::About(AboutMessage::OpenLicenses)),
            Tone::filled_if(focus == LICENSES_FOCUS),
            palette,
        ),
        kit::button(
            lang.text(Msg::AboutClose),
            Some(Message::About(AboutMessage::Close)),
            Tone::filled_if(focus == CLOSE_FOCUS),
            palette,
        ),
    ])
    .on_close(Message::About(AboutMessage::Close))
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}
