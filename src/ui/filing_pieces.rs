//! The "Move to" widget: the folders and the file name of a filing target as
//! pieces, the menus that add one, and the path of an example document. Pure
//! view over a [`TargetEditor`]; the host decides which message carries an edit.

use iced::widget::{column, row, Row};
use iced::{alignment, Element};

use super::hermes_kit::{self as kit, MenuItem, MenuLook, GAP_S, GAP_XS, TEXT_SMALL};
use crate::filing_pieces::{
    FileName, FileNameField, FileNamePiece, FilingTarget, FolderField, FolderPiece, TargetEdit,
    TargetEditor,
};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

/// The id of the field for the text of a piece of the file name.
pub const FILE_NAME_TEXT_FIELD_ID: &str = "filing-pieces-file-name-text";

/// Where the widget shows the example path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleLine {
    Hidden,
    /// A line of its own under both rows.
    Row,
    /// The note under "File name".
    UnderFileName,
}

/// What the host gives the widget.
pub struct TargetHost<'p> {
    pub wrap: fn(TargetEdit) -> Message,
    /// The id of the field for the name of a folder of its own.
    pub name_field_id: &'static str,
    /// A note under "Move to", like where the target comes from.
    pub folder_hint: Option<String>,
    pub example: ExampleLine,
    pub lang: Language,
    pub palette: &'p Palette,
}

/// The folders offered in the menu, in this order.
const OFFERED_FIELDS: [FolderField; 5] = FolderField::ALL;
const FOLDER_SEPARATOR: &str = "/";
const FILE_NAME_SEPARATOR: &str = "_";

/// The rows "Move to" and "File name" with their problems, and the example.
pub fn target_view<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    let (lang, palette) = (host.lang, host.palette);
    let mut folder_cell = column![folder_pieces(editor, host)].spacing(GAP_XS);
    if editor.own_name.is_some() {
        folder_cell = folder_cell.push(own_name_row(editor, host));
    }
    if let Some(problem) = editor.name_problem {
        folder_cell = folder_cell.push(kit::error_line(problem.text(lang), palette));
    }
    if editor.target.is_empty() {
        folder_cell = folder_cell.push(kit::error_line(
            lang.text(Msg::FilingPiecesProblemEmpty),
            palette,
        ));
    }
    let mut name_cell = column![file_name_pieces(editor, host)].spacing(GAP_XS);
    if editor.name_text.is_some() {
        name_cell = name_cell.push(name_text_row(editor, host));
    }
    if let Some(problem) = editor.name_text_problem {
        name_cell = name_cell.push(kit::error_line(problem.text(lang), palette));
    }
    let example = || example_line(&editor.target, lang, palette);
    let mut rows = column![
        kit::form_row(
            lang.text(Msg::ReviewListMoveTo),
            folder_cell.into(),
            host.folder_hint
                .clone()
                .map(|note| kit::hint(note, palette)),
            palette
        ),
        kit::form_row(
            lang.text(Msg::FilingPiecesFileNameLabel),
            name_cell.into(),
            (host.example == ExampleLine::UnderFileName).then(example),
            palette
        ),
    ]
    .spacing(GAP_S);
    if host.example == ExampleLine::Row {
        rows = rows.push(example());
    }
    rows.into()
}

/// The example path: "Example: " dimmed, the path in monospace.
pub fn example_line<'a>(
    target: &FilingTarget,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let Some(path) = target.example(lang) else {
        return kit::hint(lang.text(Msg::FilingPiecesExampleNone), palette);
    };
    let template = lang.text(Msg::FilingPiecesExample);
    let (before, after) = template.split_once("{path}").unwrap_or((template, ""));
    row![
        kit::hint(before.to_owned(), palette),
        kit::mono(path, TEXT_SMALL, palette.text),
        kit::hint(after.to_owned(), palette),
    ]
    .align_y(alignment::Vertical::Center)
    .into()
}

fn folder_label(piece: &FolderPiece, lang: Language) -> String {
    FilingTarget {
        folders: vec![piece.clone()],
        file_name: FileName::Automatic,
    }
    .in_words(lang)
}

/// The folders of a target as the parts of a path; the parts that stand for a
/// field are marked, and `/` separates them.
pub fn folder_path_parts(target: &FilingTarget, lang: Language) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    for (position, piece) in target.folders.iter().enumerate() {
        if position > 0 {
            parts.push((FOLDER_SEPARATOR.to_owned(), false));
        }
        let is_field = matches!(piece, FolderPiece::Field(_));
        parts.push((folder_label(piece, lang), is_field));
    }
    parts
}

/// The file name of a target as the parts of a name, `_` between them.
pub fn file_name_path_parts(target: &FilingTarget, lang: Language) -> Vec<(String, bool)> {
    let pieces = match &target.file_name {
        FileName::Automatic => FileName::automatic_pieces(),
        FileName::Pieces(pieces) => pieces.clone(),
        FileName::Own(_) => return vec![(target.file_name_in_words(lang), false)],
    };
    let mut parts = Vec::new();
    for (position, piece) in pieces.iter().enumerate() {
        if position > 0 {
            parts.push((FILE_NAME_SEPARATOR.to_owned(), false));
        }
        let is_field = matches!(piece, FileNamePiece::Field(_));
        parts.push((piece.label(lang).to_owned(), is_field));
    }
    parts
}

fn add_label(lang: Language) -> String {
    format!("+ {}", lang.text(Msg::RulesFormTagsAdd))
}

fn folder_pieces<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    let TargetHost {
        wrap,
        lang,
        palette,
        ..
    } = *host;
    let pieces = editor
        .target
        .folders
        .iter()
        .enumerate()
        .map(|(position, piece)| {
            kit::piece(
                &folder_label(piece, lang),
                Some(wrap(TargetEdit::Remove(position))),
                palette,
            )
        })
        .collect();
    let mut items: Vec<MenuItem> = OFFERED_FIELDS
        .into_iter()
        .map(|field| MenuItem::new(field.name(lang), wrap(TargetEdit::AddField(field))))
        .collect();
    if editor.own_name.is_none() {
        items.push(MenuItem::new(
            lang.text(Msg::FilingPiecesAddOwn),
            wrap(TargetEdit::StartOwnName),
        ));
    }
    let add = kit::menu_button(&add_label(lang), &items, MenuLook::PieceAdd, palette);
    kit::pieces(pieces, FOLDER_SEPARATOR, Some(add), palette)
}

fn file_name_pieces<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    let TargetHost {
        wrap,
        lang,
        palette,
        ..
    } = *host;
    let labels: Vec<String> = match &editor.target.file_name {
        FileName::Automatic => FileName::automatic_pieces()
            .iter()
            .map(|piece| piece.label(lang).to_owned())
            .collect(),
        FileName::Pieces(pieces) => pieces
            .iter()
            .map(|piece| piece.label(lang).to_owned())
            .collect(),
        FileName::Own(_) => vec![editor.target.file_name_in_words(lang)],
    };
    let last_one = labels.len() == 1 && matches!(editor.target.file_name, FileName::Pieces(_));
    let pieces = labels
        .iter()
        .enumerate()
        .map(|(position, label)| {
            let remove = (!last_one).then(|| wrap(TargetEdit::RemoveNamePiece(position)));
            kit::piece(label, remove, palette)
        })
        .collect();
    let add = (!matches!(editor.target.file_name, FileName::Own(_))).then(|| {
        let mut items: Vec<MenuItem> = FileNameField::ALL
            .into_iter()
            .map(|field| MenuItem::new(field.name(lang), wrap(TargetEdit::AddNameField(field))))
            .collect();
        if editor.name_text.is_none() {
            items.push(MenuItem::new(
                crate::filing_pieces::own_text_name(lang),
                wrap(TargetEdit::StartNameText),
            ));
        }
        kit::menu_button(&add_label(lang), &items, MenuLook::PieceAdd, palette)
    });
    kit::pieces(pieces, FILE_NAME_SEPARATOR, add, palette)
}

fn typing_row<'a>(
    placeholder: &str,
    typed: &str,
    id: &'static str,
    on_input: fn(String) -> TargetEdit,
    wrap: fn(TargetEdit) -> Message,
    submit: TargetEdit,
    host: &TargetHost<'_>,
) -> Element<'a, Message> {
    let (lang, palette) = (host.lang, host.palette);
    Row::new()
        .push(kit::text_field(
            placeholder,
            typed,
            id,
            Some(move |text| wrap(on_input(text))),
            wrap(submit.clone()),
            palette,
        ))
        .push(kit::secondary(
            lang.text(Msg::FilingPiecesAddOwnConfirm),
            Some(wrap(submit)),
            palette,
        ))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn own_name_row<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    typing_row(
        host.lang.text(Msg::FilingPiecesOwnPlaceholder),
        editor.own_name.as_deref().unwrap_or_default(),
        host.name_field_id,
        TargetEdit::OwnNameTyped,
        host.wrap,
        TargetEdit::OwnNameSubmit,
        host,
    )
}

fn name_text_row<'a>(editor: &TargetEditor, host: &TargetHost<'_>) -> Element<'a, Message> {
    typing_row(
        host.lang.text(Msg::FilingPiecesNameTextPlaceholder),
        editor.name_text.as_deref().unwrap_or_default(),
        FILE_NAME_TEXT_FIELD_ID,
        TargetEdit::NameTextTyped,
        host.wrap,
        TargetEdit::NameTextSubmit,
        host,
    )
}
