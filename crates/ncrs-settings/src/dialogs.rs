//! The two small dialogs of the Index and Hermes pages: what Hermes does with
//! a read folder, and the question before a model is removed. The app draws
//! them over the page.

use iced::widget::{column, row, text};
use iced::{Element, Size};
use ncrs_config::config::{FilingLocation, LocationTransfer};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_intake::LocationMode;
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::{font_size, spacing};

use crate::ai_consent::announced_items;
use crate::facts::ModelKind;
use crate::message::SettingsMessage;
use crate::settings::view::{server_dialog_body, SettingsView};

type Message = SettingsMessage;

const SIZE: DialogSize = DialogSize::Small;

/// The dialog of one read folder. Every choice is written at once, so the one
/// button only closes it.
pub fn folder_dialog<'a>(
    location: &FilingLocation,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let edit = |edited: FilingLocation| SettingsMessage::ReplaceFolder(edited);
    let when = kit::segmented(
        &[
            (
                lang.text(Msg::SettingsFolderDialogAlways),
                edit(FilingLocation {
                    mode: LocationMode::Inbox,
                    ..location.clone()
                }),
            ),
            (
                lang.text(Msg::SettingsFolderDialogOnRequest),
                edit(FilingLocation {
                    mode: LocationMode::Archive,
                    ..location.clone()
                }),
            ),
        ],
        usize::from(location.mode == LocationMode::Archive),
        true,
        palette,
    );
    let transfer = kit::segmented(
        &[
            (
                lang.text(Msg::SettingsFolderDialogCopy),
                edit(FilingLocation {
                    transfer: LocationTransfer::Copy,
                    ..location.clone()
                }),
            ),
            (
                lang.text(Msg::SettingsFolderDialogMove),
                edit(FilingLocation {
                    transfer: LocationTransfer::Move,
                    ..location.clone()
                }),
            ),
        ],
        usize::from(location.transfer == LocationTransfer::Move),
        true,
        palette,
    );
    let images = row![
        kit::checkbox(
            if location.images {
                Check::On
            } else {
                Check::Off
            },
            Some(edit(FilingLocation {
                images: !location.images,
                ..location.clone()
            })),
            palette,
        ),
        text(lang.text(Msg::SettingsFolderDialogImages).to_owned())
            .size(font_size::BODY)
            .color(palette.text),
    ]
    .spacing(spacing::M);
    let body = column![
        kit::paragraph(lang.text(Msg::SettingsFolderDialogWhen), palette),
        when,
        kit::paragraph(lang.text(Msg::SettingsFolderDialogTransfer), palette),
        transfer,
        images,
    ]
    .spacing(spacing::LINE_GAP);
    Frame::new(
        lang.text(Msg::SettingsFolderDialogTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .subtitle(
        location
            .path
            .file_name()
            .unwrap_or(location.path.as_os_str())
            .to_string_lossy()
            .into_owned(),
    )
    .buttons(vec![kit::button(
        lang.text(Msg::DialogDone),
        Some(SettingsMessage::EditFolder(None)),
        Tone::Primary,
        palette,
    )])
    .on_close(SettingsMessage::EditFolder(None))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

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

/// The own server: the switch back to the local model, address, key and model
/// name. Save writes the address and model and keeps the key.
pub fn server_dialog<'a>(view: &SettingsView<'a>, window: Size) -> Element<'a, Message> {
    Frame::new(
        view.lang.text(Msg::SettingsServerDialogTitle),
        server_dialog_body(view),
        DialogSize::Wide,
        window,
        view.lang,
        view.palette,
    )
    .buttons(vec![
        kit::button(
            view.lang.text(Msg::DialogCancel),
            Some(SettingsMessage::ServerDialog(false)),
            Tone::Secondary,
            view.palette,
        ),
        kit::button(
            view.lang.text(Msg::SettingsServerDialogSave),
            Some(SettingsMessage::SaveServer),
            Tone::Primary,
            view.palette,
        ),
    ])
    .on_close(SettingsMessage::ServerDialog(false))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

/// The question before the model is switched to a server on another
/// computer: everything that would go there. Nothing is written before the
/// yes.
pub fn ai_consent_dialog<'a>(
    host: &str,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body = column![
        kit::paragraph(
            fill_template(lang.text(Msg::SettingsAiConsentIntro), &[("{host}", host)]),
            palette,
        ),
        kit::bullet_list(
            announced_items().map(|item| lang.text(item).to_owned()),
            palette,
        ),
        kit::warning_box(lang.text(Msg::SettingsAiConsentOutro), palette),
    ]
    .spacing(spacing::LINE_GAP);
    Frame::new(
        lang.text(Msg::SettingsAiConsentTitle),
        body,
        DialogSize::Wide,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(SettingsMessage::CancelAiConsent),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            lang.text(Msg::SettingsAiConsentConfirm),
            Some(SettingsMessage::ConfirmAiConsent),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(SettingsMessage::CancelAiConsent)
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

/// The dialog the open page asks for, if any.
pub fn open_dialog<'a>(view: &SettingsView<'a>, window: Size) -> Option<Element<'a, Message>> {
    let (state, lang, palette) = (view.state, view.lang, view.palette);
    if let Some(ask) = &state.consent_pending {
        return Some(ai_consent_dialog(&ask.host, window, lang, palette));
    }
    if state.server_dialog {
        return Some(server_dialog(view, window));
    }
    if let Some(kind) = state.model_to_remove {
        return Some(remove_model_dialog(kind, window, lang, palette));
    }
    let path = state.folder_dialog.as_ref()?;
    let location = state
        .config()?
        .filing
        .locations
        .iter()
        .find(|location| &location.path == path)?;
    Some(folder_dialog(location, window, lang, palette))
}
