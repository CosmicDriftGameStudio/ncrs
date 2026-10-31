//! The preview of a document as a widget: the page, picture or text excerpt
//! in a frame that takes the room it is given, with a caption below. It sends
//! no messages, so any page can show it.

use iced::widget::text::Wrapping;
use std::ops::Range;

use iced::widget::{column, container, image, rich_text, text, Column};
use iced::{ContentFit, Element, Length};

use super::picker::highlight_spans;
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::document_preview::{DocumentPreview, PreviewContent, PreviewImage};
use crate::i18n::{Language, Msg};
use crate::palette::Palette;

pub fn view<'a, M: 'a>(
    preview: &'a DocumentPreview,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let (shown, caption) = match preview.content() {
        None => (note(lang.text(Msg::DocumentPreviewLoading), palette), None),
        Some(PreviewContent::Page { image, page_count }) => (
            picture(image),
            Some(fill_template(
                lang.text(Msg::DocumentPreviewCaptionPage),
                &[("{count}", &page_count.to_string())],
            )),
        ),
        Some(PreviewContent::Image(image)) => (
            picture(image),
            Some(fill_template(
                lang.text(Msg::DocumentPreviewCaptionImage),
                &[
                    ("{width}", &image.width.to_string()),
                    ("{height}", &image.height.to_string()),
                ],
            )),
        ),
        Some(PreviewContent::Text {
            excerpt,
            truncated,
            highlights,
        }) => (
            excerpt_lines(excerpt, highlights, palette),
            Some(
                lang.text(if *truncated {
                    Msg::DocumentPreviewCaptionTextShortened
                } else {
                    Msg::DocumentPreviewCaptionText
                })
                .to_owned(),
            ),
        ),
        Some(PreviewContent::Unsupported) => (
            note(lang.text(Msg::DocumentPreviewUnsupported), palette),
            None,
        ),
        Some(PreviewContent::LibraryMissing) => (
            note(lang.text(Msg::DocumentPreviewLibraryMissing), palette),
            None,
        ),
        Some(PreviewContent::TooLarge) => {
            (note(lang.text(Msg::DocumentPreviewTooLarge), palette), None)
        }
        Some(PreviewContent::TimedOut) => {
            (note(lang.text(Msg::DocumentPreviewTimedOut), palette), None)
        }
        Some(PreviewContent::Failed(detail)) => (
            note(
                fill_template(
                    lang.text(Msg::DocumentPreviewFailed),
                    &[("{detail}", detail)],
                ),
                palette,
            ),
            None,
        ),
    };
    let frame = container(shown)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(spacing::XS)
        .clip(true)
        .style(theme::outlined_in(palette.border_inactive));
    let mut body: Column<'a, M> = column![frame].spacing(spacing::XS);
    if let Some(shown_caption) = caption {
        body = body.push(
            text(shown_caption)
                .size(font_size::BODY)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        );
    }
    body.into()
}

fn picture<'a, M: 'a>(shown: &PreviewImage) -> Element<'a, M> {
    container(
        image(shown.handle.clone())
            .content_fit(ContentFit::Contain)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .center(Length::Fill)
    .into()
}

fn note<'a, M: 'a>(content: impl text::IntoFragment<'a>, palette: &Palette) -> Element<'a, M> {
    container(
        text(content)
            .size(font_size::BODY)
            .color(palette.dim_text)
            .wrapping(Wrapping::Word),
    )
    .center(Length::Fill)
    .into()
}

/// The lines of the excerpt, the highlighted places drawn in the accent color.
fn excerpt_lines<'a, M: 'a>(
    excerpt: &'a str,
    highlights: &[Range<usize>],
    palette: &Palette,
) -> Element<'a, M> {
    let mut line_start = 0;
    let mut lines = Vec::new();
    for line in excerpt.split('\n') {
        let line_end = line_start + line.len();
        let in_line: Vec<Range<usize>> = highlights
            .iter()
            .filter(|range| range.start < line_end && range.end > line_start)
            .map(|range| {
                range.start.max(line_start) - line_start..range.end.min(line_end) - line_start
            })
            .collect();
        lines.push(if line.is_empty() {
            text(" ")
                .size(font_size::BODY)
                .color(palette.text)
                .wrapping(Wrapping::Word)
                .into()
        } else {
            rich_text(highlight_spans(
                line,
                &in_line,
                false,
                palette.text,
                palette,
            ))
            .size(font_size::BODY)
            .wrapping(Wrapping::Word)
            .into()
        });
        line_start = line_end + 1;
    }
    Column::with_children(lines).into()
}
