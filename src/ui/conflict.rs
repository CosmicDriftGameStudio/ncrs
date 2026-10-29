//! The dialog shown when a copy or move runs into a name that is taken.
//!
//! The "for all files" answer is the point of the dialog. Without it, copying
//! fifty files into a directory that has some of them means fifty identical
//! questions; with it, the user answers once and the rest follow. That was in
//! the requirements before anything here was built.

use iced::widget::{checkbox, column, Row};
use iced::{alignment, Element, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::kit::{self, one_line, paragraph, Tone};
use super::theme::{self, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::{ConflictChoice, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right, the focus numbers they have.
pub const CANCEL_FOCUS: usize = 0;
pub const KEEP_FOCUS: usize = 1;
pub const OVERWRITE_FOCUS: usize = 2;

/// Renders the conflict dialog.
///
/// `name` is the file that is in the way. `all` is whether "for all files" is
/// ticked; the app holds it so the tick survives the dialog being rebuilt every
/// frame.
///
/// `focus` is the button Enter would press.
///
/// The keys are routed in `app`, not here: Esc answers "keep", the abort key
/// (Ctrl+C on a Mac) answers "cancel the whole operation", Space ticks
/// "for all files".
pub fn view<'a>(
    name: &str,
    lang: Language,
    palette: &Palette,
    all: bool,
    focus: usize,
    window: Size,
    on_choice: impl Fn(ConflictChoice) -> Message + 'a,
) -> Element<'a, Message> {
    let subject = one_line(
        format::ellipsize_end(
            &lang.text(Msg::ConflictName).replace("{name}", name),
            frame::content_chars(SIZE, window, 0.0),
        ),
        palette,
    );

    // The checkbox state comes from the app, not from here: the view is pure,
    // and the tick has to survive the dialog being rebuilt every frame.
    let all_row = Row::with_children([
        checkbox(all)
            .on_toggle(|_| Message::ToggleConflictAll)
            .style(theme::dialog_checkbox(palette))
            .into(),
        paragraph(lang.text(Msg::ConflictApplyAll), palette),
    ])
    .spacing(spacing::CELL_PADDING_X)
    .align_y(alignment::Vertical::Center);

    let choice_button = |label: Msg, choice: ConflictChoice, button_focus: usize| {
        let tone = Tone::filled_if(focus == button_focus);
        if focus == button_focus {
            kit::enter_button(lang.text(label), Some(on_choice(choice)), tone, palette)
        } else {
            kit::button(lang.text(label), Some(on_choice(choice)), tone, palette)
        }
    };

    Frame::new(
        lang.text(Msg::ConflictSubtitle),
        column![subject, all_row].spacing(spacing::LINE_GAP),
        SIZE,
        window,
        lang,
        palette,
    )
    .subtitle(lang.text(Msg::ConflictTitle))
    .buttons(vec![
        choice_button(Msg::ConflictCancel, ConflictChoice::Cancel, CANCEL_FOCUS),
        choice_button(Msg::ConflictKeep, ConflictChoice::ThisKeep, KEEP_FOCUS),
        choice_button(
            Msg::ConflictOverwrite,
            ConflictChoice::ThisOverwrite,
            OVERWRITE_FOCUS,
        ),
    ])
    // Esc answers "keep", the no that loses no data; the cross does the same.
    .on_close(on_choice(ConflictChoice::ThisKeep))
    .hints(KeyHints {
        enter: true,
        tab: true,
    })
    .into_element()
}
