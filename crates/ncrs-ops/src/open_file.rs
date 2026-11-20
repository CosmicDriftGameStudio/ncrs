//! The question before Enter or a double click opens a file that can run as a
//! program: asked, then handed to the system opener.
//!
//! Cancel has the focus when the dialog opens, so a held or reflex Enter never
//! starts anything. The queue does not matter: nothing here is queued.

use std::path::PathBuf;

use iced::widget::column;
use iced::{Element, Size};

use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::frame::{self, DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::spacing;

use crate::action::Action;
use crate::delete_flow::{actions, idle, Outcome};
use crate::message::OpsMessage;
use crate::state::Ops;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right: cancel is `0`, confirm is `1`, rightmost.
pub const CANCEL_FOCUS: usize = 0;
pub const CONFIRM_FOCUS: usize = 1;

/// Asks before `path` is opened. Another dialog open means nothing happens.
pub fn open_open_file_dialog(ops: &mut Ops, path: PathBuf) -> Outcome {
    if ops.open_file_dialog.is_some() || ops.transfer_dialog_is_blocked() {
        return idle();
    }
    ops.open_file_dialog = Some(path);
    actions(vec![
        Action::ClearStatus,
        Action::FocusDialogButton(CANCEL_FOCUS),
    ])
}

/// The question was confirmed: the app starts the file.
pub fn confirm_open_file(ops: &mut Ops) -> Outcome {
    let Some(path) = ops.open_file_dialog.take() else {
        return idle();
    };
    actions(vec![Action::OpenWithSystem(path)])
}

/// `focus` is the button Enter would press.
pub fn view<'a>(
    name: &str,
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let title = fill_template(
        lang.text(Msg::OpenFileTitle),
        &[("{name}", &format::ellipsize_middle(name, line_chars))],
    );
    let content = column![kit::paragraph(
        lang.text(Msg::OpenFileHint).to_owned(),
        palette
    )]
    .spacing(spacing::LINE_GAP);
    let choice = |label: &'static str, message: OpsMessage, own_focus: usize| {
        let tone = Tone::filled_if(focus == own_focus);
        if focus == own_focus {
            kit::enter_button(label, Some(message), tone, palette)
        } else {
            kit::button(label, Some(message), tone, palette)
        }
    };
    Frame::new(&title, content, SIZE, window, lang, palette)
        .buttons(vec![
            choice(
                lang.text(Msg::DialogCancel),
                OpsMessage::OpenFileCancel,
                CANCEL_FOCUS,
            ),
            choice(
                lang.text(Msg::OpenFileConfirm),
                OpsMessage::OpenFileConfirm,
                CONFIRM_FOCUS,
            ),
        ])
        .on_close(OpsMessage::OpenFileCancel)
        .hints(KeyHints {
            enter: true,
            tab: true,
        })
        .into_element()
}
