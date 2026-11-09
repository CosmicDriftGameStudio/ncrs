//! The dialog that connects to an SFTP server, and the stage of it that asks
//! whether to trust a server for the first time. Modal. A pure view over
//! [`ConnectDialog`]; what the buttons do is decided by `update`.

use std::path::Path;

use iced::widget::{column, row, Row};
use iced::{Element, Length, Size};

use crate::dialog::{ConnectDialog, Stage};
use crate::message::{ConnectField, Message};
use ncrs_fs::remote::Secret;
use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, dim, error_line, paragraph, Check, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::spacing;

const SIZE: DialogSize = DialogSize::Medium;

/// The width of the port field.
const PORT_WIDTH: f32 = 90.0;

/// The id of the field of the passphrase stage.
pub const PASSPHRASE_ID: &str = "remote-passphrase";

/// The id of a field, so the app can put the focus on it and a test can find it.
pub const fn field_id(field: ConnectField) -> &'static str {
    match field {
        ConnectField::Host => "remote-host",
        ConnectField::Port => "remote-port",
        ConnectField::User => "remote-user",
        ConnectField::Password => "remote-password",
        ConnectField::KeyFile => "remote-key-file",
        ConnectField::StartPath => "remote-start-path",
    }
}

const fn field_label(field: ConnectField) -> Msg {
    match field {
        ConnectField::Host => Msg::RemoteHost,
        ConnectField::Port => Msg::RemotePort,
        ConnectField::User => Msg::RemoteUser,
        ConnectField::Password => Msg::RemotePassword,
        ConnectField::KeyFile => Msg::RemoteKeyFile,
        ConnectField::StartPath => Msg::RemoteStartPath,
    }
}

pub fn view<'a>(
    dialog: &'a ConnectDialog,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    match dialog.stage() {
        Stage::HostKey { fingerprint, .. } => host_key(dialog, fingerprint, window, lang, palette),
        Stage::Passphrase { key_file } => passphrase(dialog, key_file, window, lang, palette),
        Stage::Editing | Stage::Connecting => form(dialog, window, lang, palette),
    }
}

fn labelled<'a>(
    field: ConnectField,
    dialog: &ConnectDialog,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let label = lang.text(field_label(field));
    let typing = !dialog.is_locked();
    let submit = Message::Submit;
    let input = if field == ConnectField::Password {
        kit::secret_field(
            label,
            dialog.value(field),
            field_id(field),
            typing.then_some(|text: String| Message::Password(Secret::new(text))),
            submit,
            palette,
        )
    } else if field == ConnectField::KeyFile {
        // The App Store build finds nothing by itself, so nothing is promised.
        let placeholder = if dialog.sandboxed() {
            ""
        } else {
            lang.text(Msg::RemoteKeyFilePlaceholder)
        };
        row![
            kit::mono_text_field(
                placeholder,
                dialog.value(field),
                field_id(field),
                typing.then_some(move |text: String| Message::Field(field, text)),
                submit,
                palette,
            ),
            kit::button(
                lang.text(Msg::RemoteKeyFileChoose),
                typing.then_some(Message::ChooseKeyFile),
                Tone::Secondary,
                palette,
            ),
        ]
        .spacing(spacing::XS)
        .into()
    } else {
        kit::mono_text_field(
            label,
            dialog.value(field),
            field_id(field),
            typing.then_some(move |text: String| Message::Field(field, text)),
            submit,
            palette,
        )
    };
    column![dim(label, palette), input]
        .spacing(spacing::XS)
        .width(Length::Fill)
        .into()
}

fn tick<'a>(
    label: &str,
    checked: bool,
    on_toggle: Message,
    locked: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let state = if checked { Check::On } else { Check::Off };
    let toggle = (!locked).then_some(on_toggle);
    let row: Row<'a, Message> = row![
        kit::checkbox(state, toggle, palette),
        paragraph(label.to_owned(), palette)
    ]
    .spacing(spacing::XS);
    row.into()
}

fn form<'a>(
    dialog: &'a ConnectDialog,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let locked = dialog.is_locked();
    let host_and_port = row![
        labelled(ConnectField::Host, dialog, lang, palette),
        column![labelled(ConnectField::Port, dialog, lang, palette)].width(PORT_WIDTH),
    ]
    .spacing(spacing::M);
    let mut body = column![host_and_port].spacing(spacing::LINE_GAP);
    if let Some(line) = ssh_config_line(dialog, lang) {
        body = body.push(dim(line, palette));
    }
    body = body
        .push(labelled(ConnectField::User, dialog, lang, palette))
        .push(labelled(ConnectField::Password, dialog, lang, palette))
        .push(labelled(ConnectField::KeyFile, dialog, lang, palette));
    if dialog.sandboxed() {
        body = body.push(dim(lang.text(Msg::RemoteKeyFileStoreNote), palette));
    }
    body = body
        .push(labelled(ConnectField::StartPath, dialog, lang, palette))
        .push(tick(
            lang.text(Msg::RemoteRemember),
            dialog.remember(),
            Message::ToggleRemember,
            locked,
            palette,
        ))
        .push(tick(
            lang.text(Msg::RemoteSave),
            dialog.save(),
            Message::ToggleSave,
            locked,
            palette,
        ));
    if locked {
        body = body.push(dim(lang.text(Msg::RemoteConnecting), palette));
    }
    if let Some(error) = dialog.error() {
        body = body.push(error_line(error.to_owned(), palette));
    }
    let cancel = Message::Cancel;
    Frame::new(
        lang.text(Msg::RemoteDialogTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(cancel.clone()),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::RemoteConnect),
            (!locked).then_some(Message::Submit),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(cancel)
    .hints(KeyHints {
        enter: true,
        tab: true,
    })
    .into_element()
}

/// The server as the question names it: the host, with `:port` when the port is
/// not 22, so that two servers on one host are told apart.
fn shown_host(dialog: &ConnectDialog) -> String {
    let typed = dialog.value(ConnectField::Host).trim();
    let host = if typed.contains(':') {
        format!("[{typed}]")
    } else {
        typed.to_owned()
    };
    match dialog.value(ConnectField::Port).trim() {
        "" | "22" => host,
        port => format!("{host}:{port}"),
    }
}

fn host_key<'a>(
    dialog: &ConnectDialog,
    fingerprint: &str,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let host = shown_host(dialog);
    let body = column![paragraph(
        fill_template(
            lang.text(Msg::RemoteHostkeyBody),
            &[("{host}", &host), ("{fingerprint}", fingerprint)],
        ),
        palette,
    )]
    .spacing(spacing::LINE_GAP);
    let cancel = Message::Cancel;
    Frame::new(
        lang.text(Msg::RemoteHostkeyTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(cancel.clone()),
            Tone::Primary,
            palette,
        ),
        kit::button(
            lang.text(Msg::RemoteHostkeyTrust),
            Some(Message::TrustHostKey),
            Tone::Secondary,
            palette,
        ),
    ])
    .on_close(cancel)
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .into_element()
}

/// What `~/.ssh/config` gives for the typed host, as one line; `None` when it
/// gives nothing.
fn ssh_config_line(dialog: &ConnectDialog, lang: Language) -> Option<String> {
    let hint = dialog.ssh_hint();
    let parts: Vec<String> = [
        hint.host_name.clone(),
        hint.port.map(|port| {
            fill_template(
                lang.text(Msg::RemoteSshConfigPort),
                &[("{port}", &port.to_string())],
            )
        }),
        hint.user
            .as_ref()
            .map(|user| fill_template(lang.text(Msg::RemoteSshConfigUser), &[("{user}", user)])),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| {
        fill_template(
            lang.text(Msg::RemoteSshConfig),
            &[("{details}", &parts.join(", "))],
        )
    })
}

fn passphrase<'a>(
    dialog: &'a ConnectDialog,
    key_file: &Path,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut body = column![
        paragraph(
            fill_template(
                lang.text(Msg::RemotePassphraseBody),
                &[("{key}", &dialog.shown_path(key_file))],
            ),
            palette,
        ),
        kit::secret_field(
            lang.text(Msg::RemotePassphraseTitle),
            dialog.passphrase().expose(),
            PASSPHRASE_ID,
            Some(|text: String| Message::Passphrase(Secret::new(text))),
            Message::Unlock,
            palette,
        ),
        tick(
            lang.text(Msg::RemotePassphraseRemember),
            dialog.remember_passphrase(),
            Message::ToggleRememberPassphrase,
            false,
            palette,
        ),
    ]
    .spacing(spacing::LINE_GAP);
    if let Some(error) = dialog.error() {
        body = body.push(error_line(error.to_owned(), palette));
    }
    let cancel = Message::Cancel;
    Frame::new(
        lang.text(Msg::RemotePassphraseTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(cancel.clone()),
            Tone::Secondary,
            palette,
        ),
        kit::enter_button(
            lang.text(Msg::RemotePassphraseUnlock),
            Some(Message::Unlock),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(cancel)
    .hints(KeyHints {
        enter: true,
        tab: false,
    })
    .into_element()
}
