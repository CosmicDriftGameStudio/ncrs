//! Rendered Markdown: parse a text once, draw the parsed items in the colors of
//! the window, and turn a click on a link into the caller's message. Shared by
//! the file viewer and the notes, so Markdown looks the same in both.

use iced::widget::markdown;
use iced::{border, Element};

use crate::palette::Palette;
use crate::theme::font_size;

/// One parsed block of a Markdown text.
pub use markdown::Item as MarkdownItem;

/// The blocks of `source`; keep them in the state, parsing is not for `view`.
pub fn parse_markdown(source: &str) -> Vec<MarkdownItem> {
    markdown::parse(source).collect()
}

/// `items` drawn in the palette. A click on a link (or an image) gives
/// `on_link` the address as written in the text.
pub fn markdown_view<'a, M: 'a>(
    items: &'a [MarkdownItem],
    palette: &Palette,
    on_link: impl Fn(String) -> M + 'a,
) -> Element<'a, M> {
    let style = markdown::Style {
        font: crate::APP_FONT,
        inline_code_highlight: markdown::Highlight {
            background: palette.panel.into(),
            border: border::rounded(crate::theme::radius::CORNER),
        },
        inline_code_padding: iced::padding::left(1).right(1),
        inline_code_color: palette.text,
        inline_code_font: crate::APP_FONT,
        code_block_font: crate::APP_FONT,
        link_color: palette.accent,
    };
    let settings = markdown::Settings::with_text_size(font_size::ROW, style);
    markdown::view(items, settings).map(on_link)
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_a_paragraph_and_a_code_block_are_three_items() {
        let items = parse_markdown("# Title\n\nSome *text*.\n\n```\ncode\n```\n");
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0], MarkdownItem::Heading(..)));
        assert!(matches!(items[1], MarkdownItem::Paragraph(_)));
        assert!(matches!(items[2], MarkdownItem::CodeBlock { .. }));
    }

    #[test]
    fn the_view_builds_for_a_text_with_a_link_and_a_table() {
        let items = parse_markdown("[a](b.md)\n\n| x | y |\n|---|---|\n| 1 | 2 |\n");
        let _view: Element<'_, String> =
            markdown_view(&items, &Palette::default(), |address| address);
    }
}
