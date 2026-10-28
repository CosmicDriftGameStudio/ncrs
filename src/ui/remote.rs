//! The dialog that connects to an SFTP server, and the stage of it that asks
//! whether to trust a server for the first time. Modal. A pure view over
//! [`ConnectDialog`]; what the buttons do is decided by `App`.

use iced::widget::{column, row, Row};
use iced::{Element, Length, Size};

use super::frame::{DialogSize, Frame, KeyHints};
use super::kit::{self, dim, error_line, paragraph, Check, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::fs::remote::Secret;
use crate::i18n::{Language, Msg};
use crate::messages::{ConnectField, Message, RemoteMessage};
use crate::palette::Palette;
use crate::remote_dialog::{ConnectDialog, Stage};

const SIZE: DialogSize = DialogSize::Medium;

/// The width of the port field.
const PORT_WIDTH: f32 = 90.0;

/// The id of a field, so the app can put the focus on it and a test can find it.
pub const fn field_id(field: ConnectField) -> &'static str {
    match field {
        ConnectField::Host => "remote-host",
        ConnectField::Port => "remote-port",
        ConnectField::User => "remote-user",
        ConnectField::Password => "remote-password",
        ConnectField::StartPath => "remote-start-path",
    }
}

const fn field_label(field: ConnectField) -> Msg {
    match field {
        ConnectField::Host => Msg::RemoteHost,
        ConnectField::Port => Msg::RemotePort,
        ConnectField::User => Msg::RemoteUser,
        ConnectField::Password => Msg::RemotePassword,
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
    let submit = Message::Remote(RemoteMessage::Submit);
    let input = if field == ConnectField::Password {
        kit::secret_field(
            label,
            dialog.value(field),
            field_id(field),
            typing.then_some(|text: String| {
                Message::Remote(RemoteMessage::Password(Secret::new(text)))
            }),
            submit,
            palette,
        )
    } else {
        kit::mono_text_field(
            label,
            dialog.value(field),
            field_id(field),
            typing
                .then_some(move |text: String| Message::Remote(RemoteMessage::Field(field, text))),
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
    on_toggle: RemoteMessage,
    locked: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let state = if checked { Check::On } else { Check::Off };
    let toggle = (!locked).then_some(Message::Remote(on_toggle));
    let row: Row<'a, Message> = row![
        kit::checkbox(state, toggle, palette),
        paragraph(label.to_owned(), palette)
    ]
    .spacing(spacing::S);
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
    .spacing(spacing::S);
    let mut body = column![
        host_and_port,
        labelled(ConnectField::User, dialog, lang, palette),
        labelled(ConnectField::Password, dialog, lang, palette),
        labelled(ConnectField::StartPath, dialog, lang, palette),
        tick(
            lang.text(Msg::RemoteRemember),
            dialog.remember(),
            RemoteMessage::ToggleRemember,
            locked,
            palette,
        ),
        tick(
            lang.text(Msg::RemoteSave),
            dialog.save(),
            RemoteMessage::ToggleSave,
            locked,
            palette,
        ),
    ]
    .spacing(spacing::LINE_GAP);
    if locked {
        body = body.push(dim(lang.text(Msg::RemoteConnecting), palette));
    }
    if let Some(error) = dialog.error() {
        body = body.push(error_line(error.to_owned(), palette));
    }
    let cancel = Message::Remote(RemoteMessage::Cancel);
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
            (!locked).then_some(Message::Remote(RemoteMessage::Submit)),
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
    let cancel = Message::Remote(RemoteMessage::Cancel);
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
            Some(Message::Remote(RemoteMessage::TrustHostKey)),
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
