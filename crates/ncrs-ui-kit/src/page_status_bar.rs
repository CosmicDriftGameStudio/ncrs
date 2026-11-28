//! The bottom bar of a page that takes the whole window (Settings, System):
//! a mode field, one line in the middle, a note at the right.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Row};
use iced::{alignment, Background, Border, Color, Element, Length, Theme};

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

/// A warning in the bar that can be pressed: orange text on a tinted segment with a bar at its left edge.
#[derive(Debug, Clone)]
pub struct WarningSegment<M> {
    pub text: String,
    pub tooltip: Option<String>,
    pub on_press: M,
}

const WARNING_TINT: f32 = 0.14;
const WARNING_EDGE: f32 = 2.0;

/// The drive is nearly full: the bar of any page shows this at its right end, and pressing it sends `on_press`.
#[derive(Debug, Clone)]
pub struct DiskWarning<M> {
    pub percent: u8,
    pub lang: ncrs_i18n::Language,
    pub on_press: M,
}

impl<M> DiskWarning<M> {
    /// The warning of a page that has the used share of the drive, when it is past the warn level.
    pub fn of(percent: Option<u8>, lang: ncrs_i18n::Language, on_press: M) -> Option<Self> {
        percent.map(|used_percent| Self {
            percent: used_percent,
            lang,
            on_press,
        })
    }
}

fn disk_segment<M>(warning: DiskWarning<M>) -> WarningSegment<M> {
    let lang = warning.lang;
    WarningSegment {
        text: ncrs_i18n::fill_template(
            lang.text(ncrs_i18n::Msg::StatusDiskHint),
            &[("{percent}", &warning.percent.to_string())],
        ),
        tooltip: Some(lang.text(ncrs_i18n::Msg::StatusDiskHintTooltip).to_owned()),
        on_press: warning.on_press,
    }
}

pub fn warning_segment<'a, M: Clone + 'a>(
    warning: WarningSegment<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let orange = theme::git::orange();
    let pressable = button(
        text(warning.text)
            .size(font_size::SMALL)
            .color(orange)
            .wrapping(Wrapping::None),
    )
    .on_press(warning.on_press)
    .padding([0.0, spacing::L])
    .height(Length::Fill)
    .style(move |_: &Theme, _| button::Style {
        background: Some(Background::Color(orange.scale_alpha(WARNING_TINT))),
        text_color: orange,
        border: Border::default(),
        ..button::Style::default()
    });
    let segment: Element<'a, M> = container(row![
        container("")
            .width(WARNING_EDGE)
            .height(Length::Fill)
            .style(theme::line(orange)),
        container(pressable).center_y(Length::Fill),
    ])
    .height(Length::Fill)
    .into();
    match warning.tooltip {
        Some(tip) => {
            crate::kit::tooltip(segment, tip, iced::widget::tooltip::Position::Top, palette)
        }
        None => segment,
    }
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
        .padding(iced::Padding {
            top: 0.0,
            right: spacing::XS + 2.0,
            bottom: 0.0,
            left: spacing::XS - 1.0,
        })
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

/// A dim line of hints above the status bar of a page.
pub fn hint_bar<'a, M: 'a>(hint: &str, palette: &Palette) -> Element<'a, M> {
    container(column![
        theme::horizontal_rule(palette.border_inactive),
        container(
            text(hint.to_owned())
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        )
        .padding([0.0, spacing::L])
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center),
    ])
    .height(size::STATUS_BAR)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}

/// The field at the left end of a bar that names the page.
pub fn mode_field<'a, M: 'a>(mode: &str, palette: &Palette) -> Element<'a, M> {
    container(
        text(mode.to_owned())
            .size(font_size::SMALL)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::L])
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::mode_field(palette))
    .into()
}

/// The bar of a page; the warning of the app (a nearly full drive) gets its own slot at the right end.
pub fn page_status_bar<'a, M: Clone + 'a>(
    bar: PageStatusBar<'_>,
    warning: Option<DiskWarning<M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let small = |content: String, color: Color| {
        container(
            text(content)
                .size(font_size::SMALL)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .padding([0.0, spacing::L])
    };
    let mode = mode_field(bar.mode, palette);
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
    if let Some(shown_warning) = warning {
        line = line.push(warning_segment(disk_segment(shown_warning), palette));
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
