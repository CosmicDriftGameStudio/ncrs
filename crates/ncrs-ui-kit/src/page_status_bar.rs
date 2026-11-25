//! The bottom bar of a page that takes the whole window (Settings, System):
//! a mode field, one line in the middle, a note at the right.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Row};
use iced::{alignment, Color, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

/// What the middle of the bar says; a failure beats a notice beats the hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStatusLine<'a> {
    Failure(&'a str),
    Notice(&'a str),
    Hint(&'a str),
    /// Keys with what they do, each key on a cap: `(key, label)`.
    Keys(&'a [(&'a str, &'a str)]),
}

#[derive(Debug, Clone, Copy)]
pub struct PageStatusBar<'a> {
    pub mode: &'a str,
    pub line: PageStatusLine<'a>,
    /// Dim text at the right end.
    pub right: Option<&'a str>,
}

fn key_line<'a, M: 'a>(keys: &[(&str, &str)], palette: &Palette) -> Element<'a, M> {
    let mut line = Row::new()
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center);
    for (key, label) in keys {
        let cap = container(
            text((*key).to_owned())
                .size(font_size::KEY_CAP)
                .wrapping(Wrapping::None),
        )
        .padding([0.0, spacing::XS - 1.0])
        .style(theme::key_cap(palette, false));
        line = line.push(
            row![
                cap,
                text((*label).to_owned())
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center),
        );
    }
    container(line).padding([0.0, spacing::L]).into()
}

pub fn page_status_bar<'a, M: 'a>(bar: PageStatusBar<'_>, palette: &Palette) -> Element<'a, M> {
    let small = |content: String, color: Color| {
        container(
            text(content)
                .size(font_size::SMALL)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .padding([0.0, spacing::L])
    };
    let mode = container(
        text(bar.mode.to_owned())
            .size(font_size::SMALL)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::L])
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::mode_field(palette));
    let middle: Element<'a, M> = match bar.line {
        PageStatusLine::Failure(failure) => small(failure.to_owned(), palette.error).into(),
        PageStatusLine::Notice(notice) => {
            small(format!("\u{2713} {notice}"), palette.success).into()
        }
        PageStatusLine::Hint(hint) => small(hint.to_owned(), palette.dim_text).into(),
        PageStatusLine::Keys(keys) => key_line(keys, palette),
    };
    let mut line =
        row![mode, middle, container("").width(Length::Fill)].align_y(alignment::Vertical::Center);
    if let Some(right) = bar.right {
        line = line.push(small(right.to_owned(), palette.dim_text));
    }
    container(column![
        theme::horizontal_rule(palette.border_inactive),
        container(line).width(Length::Fill).height(Length::Fill),
    ])
    .height(size::STATUS_BAR)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}
