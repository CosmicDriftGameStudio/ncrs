//! The Quick Open box: a text field, a status line and the best matches with
//! the matched characters marked. Pure view over [`QuickOpen`].

use iced::widget::text::Wrapping;
use iced::widget::{container, row, text};
use iced::{alignment, Element, Length, Size};

use super::kit;
use super::layout::ROW_HEIGHT;
use super::picker;
use super::theme::{font_size, spacing};
use crate::app::fill_template;
use crate::filing::{FoundDocument, TextSource};
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{Message, QuickOpenMessage};
use crate::palette::Palette;
use crate::quick_open::{Hit, QuickOpen};

pub const QUICK_OPEN_ID: &str = "quick-open-query";

fn to_quick_open(message: QuickOpenMessage) -> Message {
    Message::QuickOpen(message)
}

pub fn view<'a>(
    quick_open: &'a QuickOpen,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let directory = quick_open.root().file_name().map_or_else(
        || quick_open.root().display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let subtitle = fill_template(lang.text(Msg::QuickopenSubtitle), &[("{dir}", &directory)]);

    let field = kit::text_field(
        lang.text(Msg::QuickopenPlaceholder),
        quick_open.query(),
        QUICK_OPEN_ID,
        Some(|text| to_quick_open(QuickOpenMessage::QueryChanged(text))),
        to_quick_open(QuickOpenMessage::Activate),
        palette,
    );

    let mut rows: Vec<Element<'a, Message>> = quick_open
        .hits()
        .iter()
        .enumerate()
        .map(|(index, hit)| hit_row(quick_open, hit, index, window, palette))
        .collect();
    if !quick_open.documents().is_empty() {
        rows.push(documents_header(
            lang.text(Msg::QuickopenDocuments),
            palette,
        ));
        let first = quick_open.hits().len();
        rows.extend(
            quick_open
                .documents()
                .iter()
                .enumerate()
                .map(|(offset, document)| {
                    document_row(
                        document,
                        first + offset,
                        quick_open.cursor(),
                        window,
                        palette,
                    )
                }),
        );
    }
    if let Some(index) = quick_open.all_hits_row() {
        rows.push(all_hits_row(
            index,
            quick_open.cursor(),
            lang.text(Msg::QuickOpenAllHits),
            palette,
        ));
    }

    picker::frame(
        picker::Content {
            title: subtitle,
            title_theme: lang
                .futurama_name(FuturamaName::ActionQuickOpen)
                .map(str::to_owned),
            subtitle: None,
            field,
            status: status(quick_open, lang),
            rows,
            on_close: to_quick_open(QuickOpenMessage::Close),
        },
        window,
        lang,
        palette,
    )
}

fn status(quick_open: &QuickOpen, lang: Language) -> String {
    let count = quick_open.file_count().to_string();
    let filled = |message| fill_template(lang.text(message), &[("{count}", &count)]);
    if quick_open.has_query() && !quick_open.is_ranking() && quick_open.hits().is_empty() {
        lang.text(Msg::QuickopenNoMatch).to_owned()
    } else if quick_open.walking() {
        filled(Msg::QuickopenStatusWalking)
    } else if quick_open.truncated() {
        filled(Msg::QuickopenStatusTruncated)
    } else {
        filled(Msg::QuickopenStatusDone)
    }
}

fn hit_row<'a>(
    quick_open: &'a QuickOpen,
    hit: &Hit,
    index: usize,
    window: Size,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = index == quick_open.cursor();
    let path = quick_open.relative_path(hit);
    kit::list_row(
        picker::highlighted(
            path,
            &hit.highlight,
            selected,
            picker::row_chars(window, 0.0),
            palette,
        ),
        to_quick_open(QuickOpenMessage::RowActivated(index)),
        selected,
        true,
        palette,
    )
}

/// The last row: the documents in the search page, with the typed text.
fn all_hits_row<'a>(
    index: usize,
    cursor: usize,
    label: &str,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = index == cursor;
    kit::list_row(
        text(label.to_owned())
            .size(font_size::BODY)
            .color(if selected {
                palette.cursor_text
            } else {
                palette.dim_text
            })
            .wrapping(Wrapping::None)
            .into(),
        to_quick_open(QuickOpenMessage::RowActivated(index)),
        selected,
        true,
        palette,
    )
}

/// The heading of the documents section, not a place for the cursor.
fn documents_header<'a>(label: &str, palette: &Palette) -> Element<'a, Message> {
    container(
        text(label.to_uppercase())
            .size(font_size::COLUMN_HEADER)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
    )
    .height(ROW_HEIGHT)
    .padding([0.0, spacing::CELL_PADDING_X])
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Characters kept free for the gaps and the mark beside a document's name.
const MARK_GAP_CHARS: usize = 8;

/// A folder is never cut shorter than this.
const MIN_FOLDER_CHARS: usize = 12;

/// The mark after the name: how the text was read, nothing for plain text.
fn read_by_mark(source: TextSource) -> Option<&'static str> {
    match source {
        TextSource::Pdf => Some("PDF"),
        TextSource::Ocr => Some("OCR"),
        TextSource::Text => None,
    }
}

fn document_row<'a>(
    document: &FoundDocument,
    index: usize,
    cursor: usize,
    window: Size,
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = index == cursor;
    let name = document
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = document
        .path
        .parent()
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    let mark = read_by_mark(document.read_by).unwrap_or_default();
    let room = picker::row_chars(window, 0.0);
    let folder_room = room
        .saturating_sub(name.chars().count() + mark.len() + MARK_GAP_CHARS)
        .max(MIN_FOLDER_CHARS);
    let line = row![
        container(
            row![
                picker::highlighted(&name, &[], selected, room, palette),
                picker::highlighted_dim(&folder, &[], selected, folder_room, palette),
            ]
            .spacing(spacing::SECTION_GAP)
        )
        .width(Length::Fill),
        picker::highlighted_dim(mark, &[], selected, MARK_GAP_CHARS, palette),
    ]
    .align_y(alignment::Vertical::Center);
    kit::list_row(
        line.into(),
        to_quick_open(QuickOpenMessage::RowActivated(index)),
        selected,
        true,
        palette,
    )
}
