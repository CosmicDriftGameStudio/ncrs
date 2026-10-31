//! The "Move to" widget: the folders of a filing target as chips, the buttons
//! that add one, the file name and the path of an example document. Pure view
//! over a [`TargetEditor`]; the host decides which message carries an edit.

use iced::widget::{column, row, Row};
use iced::{alignment, Element};

use super::kit::{self, dim, error_line, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::filing_pieces::{FilingTarget, FolderField, FolderPiece, TargetEdit, TargetEditor};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

/// What the host gives the widget.
pub struct TargetHost<'p> {
    pub wrap: fn(TargetEdit) -> Message,
    /// The id of the field for the name of a folder of its own.
    pub name_field_id: &'static str,
    pub lang: Language,
    pub palette: &'p Palette,
}

/// The folders offered as buttons, in this order.
const OFFERED_FIELDS: [FolderField; 5] = FolderField::ALL;

pub fn target_view<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    let TargetHost { lang, palette, .. } = *host;
    let mut content = column![
        folder_chips(&editor.target, host),
        add_buttons(editor, host)
    ]
    .spacing(spacing::S);
    if editor.own_name.is_some() {
        content = content.push(own_name_row(editor, host));
    }
    if let Some(problem) = editor.name_problem {
        content = content.push(error_line(problem.text(lang), palette));
    }
    if editor.target.is_empty() {
        content = content.push(error_line(
            lang.text(Msg::FilingPiecesProblemEmpty),
            palette,
        ));
    }
    content
        .push(dim(
            format!(
                "{}{}",
                lang.text(Msg::FilingPiecesFileNamePrefix),
                editor.target.file_name_in_words(lang)
            ),
            palette,
        ))
        .push(example_line(&editor.target, lang, palette))
        .into()
}

/// Only the example line, for the places that show the target read-only.
pub fn example_line<'a>(
    target: &FilingTarget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    dim(
        match target.example(lang) {
            Some(path) => fill_template(lang.text(Msg::FilingPiecesExample), &[("{path}", &path)]),
            None => lang.text(Msg::FilingPiecesExampleNone).to_owned(),
        },
        palette,
    )
}

fn folder_label(piece: &FolderPiece, lang: Language) -> String {
    FilingTarget {
        folders: vec![piece.clone()],
        file_name: crate::filing_pieces::FileName::Automatic,
    }
    .in_words(lang)
}

/// The folders as chips with a cross, a dimmed slash between them.
fn folder_chips<'a>(target: &FilingTarget, host: &TargetHost<'_>) -> Element<'a, Message> {
    let TargetHost {
        wrap,
        lang,
        palette,
        ..
    } = *host;
    let mut chips = Row::new()
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    for (position, piece) in target.folders.iter().enumerate() {
        if position > 0 {
            chips = chips.push(dim("/", palette));
        }
        chips = chips.push(kit::removable_chip(
            &folder_label(piece, lang),
            wrap(TargetEdit::Remove(position)),
            palette,
        ));
    }
    chips.wrap().into()
}

fn add_buttons<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    let TargetHost {
        wrap,
        lang,
        palette,
        ..
    } = *host;
    let mut buttons = Row::new()
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .push(dim(lang.text(Msg::FilingPiecesAddFolder), palette))
        .push(kit::button(
            lang.text(Msg::FilingPiecesAddOwn),
            editor
                .own_name
                .is_none()
                .then(|| wrap(TargetEdit::StartOwnName)),
            Tone::Secondary,
            palette,
        ));
    for field in OFFERED_FIELDS {
        buttons = buttons.push(kit::button(
            field.name(lang),
            Some(wrap(TargetEdit::AddField(field))),
            Tone::Secondary,
            palette,
        ));
    }
    buttons.wrap().into()
}

fn own_name_row<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    let TargetHost {
        wrap,
        name_field_id,
        lang,
        palette,
    } = *host;
    let typed = editor.own_name.as_deref().unwrap_or_default();
    row![
        kit::text_field(
            lang.text(Msg::FilingPiecesOwnPlaceholder),
            typed,
            name_field_id,
            Some(move |text| wrap(TargetEdit::OwnNameTyped(text))),
            wrap(TargetEdit::OwnNameSubmit),
            palette,
        ),
        kit::button(
            lang.text(Msg::FilingPiecesAddOwnConfirm),
            Some(wrap(TargetEdit::OwnNameSubmit)),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .into()
}
