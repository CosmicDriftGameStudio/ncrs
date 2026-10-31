//! The box Quick Open and the command palette share: a text field, a status
//! line and a list of rows with the matched characters marked, in the
//! dialog frame.

use std::ops::Range;

use iced::widget::text::{Span, Wrapping};
use iced::widget::{column, container, rich_text, span, text, Column};
use iced::{Color, Element, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::theme::{font_size, spacing};
use crate::i18n::Language;
use crate::messages::Message;
use crate::palette::Palette;

/// Both boxes list long paths and commands: the large frame, which grows with
/// the list.
pub const SIZE: DialogSize = DialogSize::Large;

/// What the box shows.
pub struct Content<'a> {
    pub title: String,
    pub title_theme: Option<String>,
    pub subtitle: Option<String>,
    pub field: Element<'a, Message>,
    pub status: String,
    pub rows: Vec<Element<'a, Message>>,
    pub on_close: Message,
}

/// The frame around the field and the rows.
pub fn frame<'a>(
    content: Content<'a>,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let status_chars = frame::content_chars(SIZE, window, 0.0);
    let status = (!content.status.is_empty()).then(|| {
        text(format::ellipsize_end(&content.status, status_chars))
            .size(font_size::BODY)
            .color(palette.dim_text)
            .wrapping(Wrapping::None)
    });
    let body = column![content.field]
        .extend(status.map(Element::from))
        .push(container(Column::with_children(content.rows)).clip(true))
        .spacing(spacing::LINE_GAP);

    let mut frame = Frame::new(content.title, body, SIZE, window, lang, palette);
    frame = frame.title_theme(content.title_theme.as_deref());
    if let Some(subtitle) = content.subtitle {
        frame = frame.subtitle(subtitle);
    }
    frame
        .on_close(content.on_close)
        // Enter runs the highlighted row, which the content defines.
        .hints(KeyHints::default())
        .into_element()
}

/// The characters one row of the list holds, less `reserved` pixels.
pub fn row_chars(window: Size, reserved: f32) -> usize {
    frame::content_chars(SIZE, window, reserved + 2.0 * spacing::CELL_PADDING_X)
}

/// `text` cut at the matched ranges, which are drawn in the accent color. A
/// text longer than `max_chars` loses its middle, and the ranges follow.
pub fn highlighted<'a>(
    text: &str,
    highlight: &[Range<usize>],
    selected: bool,
    max_chars: usize,
    palette: &Palette,
) -> Element<'a, Message> {
    let plain = if selected {
        palette.cursor_text
    } else {
        palette.text
    };
    highlighted_in(text, highlight, selected, max_chars, plain, palette)
}

/// Like [`highlighted`], with the unmarked part dimmed: a second name beside
/// the main one.
pub fn highlighted_dim<'a>(
    text: &str,
    highlight: &[Range<usize>],
    selected: bool,
    max_chars: usize,
    palette: &Palette,
) -> Element<'a, Message> {
    let plain = if selected {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    highlighted_in(text, highlight, selected, max_chars, plain, palette)
}

fn highlighted_in<'a>(
    text: &str,
    highlight: &[Range<usize>],
    selected: bool,
    max_chars: usize,
    plain: Color,
    palette: &Palette,
) -> Element<'a, Message> {
    let (shown, ranges) = fit(text, highlight, max_chars);
    rich_text(highlight_spans(&shown, &ranges, selected, plain, palette))
        .size(font_size::ROW)
        .wrapping(Wrapping::None)
        .into()
}

/// `text` shortened to `max_chars` characters in the middle, and `highlight`
/// moved to where its characters went. Matches inside the cut are dropped,
/// matches across its edge are shortened.
fn fit(text: &str, highlight: &[Range<usize>], max_chars: usize) -> (String, Vec<Range<usize>>) {
    let Some(cut) = format::middle_cut(text, max_chars) else {
        return (text.to_owned(), highlight.to_vec());
    };
    let shown = format::ellipsize_middle(text, max_chars);
    if shown.is_empty() {
        return (shown, Vec::new());
    }
    let byte_of = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(byte, _)| byte)
    };
    let head_end = byte_of(cut.head);
    let tail_start = byte_of(text.chars().count().saturating_sub(cut.tail));
    let shown_tail_start = head_end + '…'.len_utf8();

    let mut moved = Vec::new();
    for range in highlight {
        if range.start < head_end {
            moved.push(range.start..range.end.min(head_end));
        }
        if range.end > tail_start {
            let from = range.start.max(tail_start);
            moved.push(
                shown_tail_start + (from - tail_start)..shown_tail_start + (range.end - tail_start),
            );
        }
    }
    (shown, moved)
}

pub(in crate::ui) fn highlight_spans<'a>(
    text: &str,
    highlight: &[Range<usize>],
    selected: bool,
    plain: Color,
    palette: &Palette,
) -> Vec<Span<'a, ()>> {
    let mut spans = Vec::new();
    let mut at = 0;
    for range in highlight {
        let (Some(before), Some(hit)) = (text.get(at..range.start), text.get(range.clone())) else {
            continue;
        };
        if !before.is_empty() {
            spans.push(span(before.to_owned()).color(plain));
        }
        spans.push(
            span(hit.to_owned())
                .color(if selected { plain } else { palette.accent })
                .underline(true),
        );
        at = range.end;
    }
    if let Some(rest) = text.get(at..) {
        if !rest.is_empty() {
            spans.push(span(rest.to_owned()).color(plain));
        }
    }
    spans
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn marked<'a>(shown: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|range| &shown[range.clone()]).collect()
    }

    #[test]
    fn a_text_that_fits_keeps_its_highlight() {
        let (shown, ranges) = fit("src/main.rs", &[0..3, 4..8], 40);
        assert_eq!(shown, "src/main.rs");
        assert_eq!(marked(&shown, &ranges), ["src", "main"]);
    }

    #[test]
    fn a_cut_text_keeps_the_highlight_of_what_is_left() {
        let path = "documents/company-a/2024/invoices/invoice-0042.pdf";
        let start_of_name = path.rfind("invoice-0042").unwrap();
        let in_the_cut = path.find("2024").unwrap();
        let highlight = [
            0..4,
            in_the_cut..in_the_cut + 4,
            start_of_name..start_of_name + 7,
        ];
        let (shown, ranges) = fit(path, &highlight, 30);
        assert!(shown.chars().count() <= 30, "{shown}");
        assert!(shown.ends_with("/invoice-0042.pdf"), "{shown}");
        // The match inside the cut is gone; the two outside it are where the
        // characters moved.
        assert_eq!(marked(&shown, &ranges), ["docu", "invoice"]);
    }

    #[test]
    fn a_match_across_the_edge_of_the_cut_is_shortened() {
        let path = "aaaa/bbbb/cccc/dddd/eeee/ffff.txt";
        let across = 2..28;
        let (shown, ranges) = fit(path, std::slice::from_ref(&across), 20);
        let pieces = marked(&shown, &ranges);
        assert_eq!(pieces.len(), 2, "{shown}: {pieces:?}");
        assert!(
            pieces.iter().all(|piece| !piece.contains('…')),
            "{pieces:?}"
        );
    }

    #[test]
    fn non_ascii_text_is_cut_and_marked_on_characters() {
        let path = "Büro/Müller & Söhne/Übersicht-Größe-ÄÖÜ.pdf";
        let (shown, ranges) = fit(path, std::slice::from_ref(&(0..5)), 36);
        assert!(shown.chars().count() <= 36, "{shown}");
        assert_eq!(marked(&shown, &ranges), ["Büro"]);
    }
}
