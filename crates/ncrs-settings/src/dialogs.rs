//! The question before a model is removed. The app draws it over the page.

use iced::{Element, Size};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::palette::Palette;

use crate::facts::ModelKind;
use crate::message::SettingsMessage;
use crate::settings::view::SettingsView;

type Message = SettingsMessage;

const SIZE: DialogSize = DialogSize::Small;

/// The question before the files of a model are deleted.
pub fn remove_model_dialog<'a>(
    kind: ModelKind,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let question = match kind {
        ModelKind::Ocr => Msg::SettingsModelRemoveOcr,
        ModelKind::Ai => Msg::SettingsModelRemoveAi,
    };
    Frame::new(
        lang.text(Msg::SettingsModelRemoveTitle),
        kit::paragraph(lang.text(question), palette),
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(SettingsMessage::CancelRemoveModel),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::SettingsRemove),
            Some(SettingsMessage::ConfirmRemoveModel),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(SettingsMessage::CancelRemoveModel)
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

/// The dialog the open page asks for, if any.
pub fn open_dialog<'a>(view: &SettingsView<'a>, window: Size) -> Option<Element<'a, Message>> {
    let (state, lang, palette) = (view.state, view.lang, view.palette);
    state
        .model_to_remove
        .map(|kind| remove_model_dialog(kind, window, lang, palette))
}
