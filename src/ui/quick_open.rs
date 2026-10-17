//! The Quick Open box: a text field, a status line and the best matches with
//! the matched characters marked. Pure view over [`QuickOpen`].

use iced::widget::text::{Span, Wrapping};
use iced::widget::{button, column, container, rich_text, span, text, text_input, Column};
use iced::{Element, Length};

use super::layout::ROW_HEIGHT;
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, QuickOpenMessage};
use crate::palette::Palette;
use crate::quick_open::{Hit, QuickOpen, MAX_HITS};

pub const QUICK_OPEN_ID: &str = "quick-open-query";

/// Wider than the other dialogs: it shows paths.
const WIDTH: f32 = 560.0;

fn to_quick_open(message: QuickOpenMessage) -> Message {
    Message::QuickOpen(message)
}

pub fn view<'a>(
    quick_open: &'a QuickOpen,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let directory = quick_open.root().file_name().map_or_else(
        || quick_open.root().display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let title = fill_template(lang.text(Msg::QuickopenTitle), &[("{dir}", &directory)]);

    let field = text_input(lang.text(Msg::QuickopenPlaceholder), quick_open.query())
        .id(QUICK_OPEN_ID)
        .on_input(|text| to_quick_open(QuickOpenMessage::QueryChanged(text)))
        .on_submit(to_quick_open(QuickOpenMessage::Activate))
        .size(font_size::ROW)
        .padding([3.0, 6.0])
        .style(theme::search_input(palette))
        .width(Length::Fill);

    let rows = quick_open
        .hits()
        .iter()
        .enumerate()
        .map(|(index, hit)| hit_row(quick_open, hit, index, palette));

    let body = column![
        text(title).size(font_size::TITLE).color(palette.accent),
        field,
        text(status(quick_open, lang))
            .size(font_size::STATUS)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
        container(Column::with_children(rows))
            .height(ROW_HEIGHT * MAX_HITS as f32)
            .clip(true),
    ]
    .spacing(spacing::SECTION_GAP);

    container(body)
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(WIDTH)
        .style(theme::dialog(palette))
        .into()
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
    palette: &Palette,
) -> Element<'a, Message> {
    let selected = index == quick_open.cursor();
    let path = quick_open.relative_path(hit);
    let spans = path_spans(path, &hit.highlight, selected, palette);
    button(
        rich_text(spans)
            .size(font_size::ROW)
            .wrapping(Wrapping::None),
    )
    .on_press(to_quick_open(QuickOpenMessage::RowActivated(index)))
    .width(Length::Fill)
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(ROW_HEIGHT)
    .style(theme::row(palette, false, selected, true))
    .into()
}

/// The path cut at the matched ranges, which are drawn in the accent color.
fn path_spans<'a>(
    path: &'a str,
    highlight: &[std::ops::Range<usize>],
    selected: bool,
    palette: &Palette,
) -> Vec<Span<'a, ()>> {
    let plain = if selected {
        palette.cursor_text
    } else {
        palette.text
    };
    let mut spans = Vec::new();
    let mut at = 0;
    for range in highlight {
        let (Some(before), Some(hit)) = (path.get(at..range.start), path.get(range.clone())) else {
            continue;
        };
        if !before.is_empty() {
            spans.push(span(before).color(plain));
        }
        spans.push(
            span(hit)
                .color(if selected { plain } else { palette.accent })
                .underline(true),
        );
        at = range.end;
    }
    if let Some(rest) = path.get(at..) {
        if !rest.is_empty() {
            spans.push(span(rest).color(plain));
        }
    }
    spans
}
