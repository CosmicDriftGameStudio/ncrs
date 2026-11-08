//! Lines of text, sections, dividers and form rows.

use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{column, container, row, rule, text, Column, Row};
use iced::{alignment, Element, Length};
use ncrs_index::ConditionKind;

use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};
use ncrs_i18n::Msg;

pub fn condition_kind_name(kind: ConditionKind) -> Msg {
    match kind {
        ConditionKind::Substring => Msg::ConditionKindSubstring,
        ConditionKind::Word => Msg::ConditionKindWord,
        ConditionKind::Regex => Msg::ConditionKindRegex,
        ConditionKind::Sender => Msg::ConditionKindSender,
        ConditionKind::SenderAddress => Msg::ConditionKindSenderAddress,
        ConditionKind::Iban => Msg::ConditionKindIban,
        ConditionKind::TaxId => Msg::ConditionKindTaxId,
        ConditionKind::Recipient => Msg::ConditionKindRecipient,
        ConditionKind::Folder => Msg::ConditionKindFolder,
        ConditionKind::Tag => Msg::ConditionKindTag,
        ConditionKind::Kind => Msg::ConditionKindKind,
        ConditionKind::ReadFolder => Msg::ConditionKindReadFolder,
    }
}

/// A dimmed line of text.
pub fn dim<'a, M: Clone + 'a>(content: impl IntoFragment<'a>, palette: &Palette) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .into()
}

/// Text that stays on one line; the caller has cut it to fit.
pub fn one_line<'a, M: Clone + 'a>(content: String, palette: &Palette) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

/// Running text.
pub fn paragraph<'a, M: Clone + 'a>(
    content: impl IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::Word)
        .into()
}

pub fn error_line<'a, M: Clone + 'a>(
    content: impl IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(palette.error)
        .wrapping(Wrapping::Word)
        .into()
}

/// A line of text that warns without being wrong, in the text color.
pub fn warning_line<'a, M: Clone + 'a>(
    content: impl IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::Word)
        .into()
}

/// A notice that is neither an error nor plain text: an outlined box with its
/// line in the text color, like the warning that a name is taken.
pub fn warning_box<'a, M: Clone + 'a>(
    content: impl IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    container(
        text(content)
            .size(font_size::BODY)
            .color(palette.text)
            .wrapping(Wrapping::Word),
    )
    .padding([spacing::XS, spacing::M])
    .width(Length::Fill)
    .style(theme::outlined_in(palette.dim_text))
    .into()
}

/// How far a job is, `0.0` to `1.0`, as a thin bar.
pub fn progress_bar<'a, M: Clone + 'a>(fraction: f32, palette: &Palette) -> Element<'a, M> {
    iced::widget::progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0))
        .length(Length::Fill)
        .girth(PROGRESS_GIRTH)
        .style(theme::progress(palette))
        .into()
}

/// The thickness of the progress bar.
const PROGRESS_GIRTH: f32 = 4.0;

/// Lines of one block, one gap apart.
pub fn stack<'a, M: Clone + 'a>(lines: impl IntoIterator<Item = Element<'a, M>>) -> Column<'a, M> {
    Column::with_children(lines).spacing(spacing::LINE_GAP)
}

/// A dimmed heading above its content, set off from what comes before it.
pub fn section<'a, M: Clone + 'a>(
    heading: &str,
    content: impl Into<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    container(heading_block(heading, content.into(), palette))
        .padding(iced::Padding {
            top: spacing::SECTION_LEAD,
            ..iced::Padding::ZERO
        })
        .into()
}

/// A section at the top of a page, which has its room above already.
pub fn first_section<'a, M: Clone + 'a>(
    heading: &str,
    content: impl Into<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    heading_block(heading, content.into(), palette)
}

fn heading_block<'a, M: Clone + 'a>(
    heading: &str,
    content: Element<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    column![dim(heading.to_owned(), palette), content]
        .spacing(spacing::M)
        .into()
}

/// A line across the width of its parent.
pub fn divider<'a, M: Clone + 'a>(palette: &Palette) -> Element<'a, M> {
    let color = palette.border_inactive;
    rule::horizontal(spacing::BORDER_WIDTH)
        .style(move |_theme| rule::Style {
            color,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// One setting: its name, the name of the theme it belongs to and a
/// description on the left, the control in a column of `control_width` on the
/// right (`Length::Shrink`: only as wide as the control), with `lines_below` (a problem or a warning) under the
/// description.
pub fn form_row_with_problem<'a, M: Clone + 'a>(
    label: &str,
    theme_name: Option<&str>,
    description: Option<&str>,
    lines_below: Vec<Element<'a, M>>,
    control: Element<'a, M>,
    control_width: Length,
    palette: &Palette,
) -> Element<'a, M> {
    let mut names = Row::new()
        .push(paragraph(label.to_owned(), palette))
        .spacing(spacing::M);
    if let Some(name) = theme_name {
        names = names.push(dim(name.to_owned(), palette));
    }
    let mut left = column![names].spacing(spacing::XS / 2.0);
    if let Some(text_below) = description {
        left = left.push(dim(text_below.to_owned(), palette));
    }
    for line in lines_below {
        left = left.push(line);
    }
    column![
        row![
            container(left).width(Length::Fill),
            container(control)
                .width(control_width)
                .align_x(alignment::Horizontal::Right),
        ]
        .spacing(spacing::XL)
        .align_y(alignment::Vertical::Center)
        .padding([spacing::XS, 0.0]),
        divider(palette),
    ]
    .into()
}
