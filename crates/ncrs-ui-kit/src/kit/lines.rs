//! Lines of text, sections, dividers and form rows.

use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{button, column, container, row, rule, scrollable, text, Column, Row};
use iced::{alignment, Element, Length};
use ncrs_index_types::ConditionKind;

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

/// Dim text that stays on one line; the caller has cut it to fit.
pub fn dim_line<'a, M: Clone + 'a>(content: String, palette: &Palette) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(palette.dim_text)
        .wrapping(Wrapping::None)
        .into()
}

/// A file name in the file font, on one line; the caller has cut it to fit.
pub fn file_name<'a, M: Clone + 'a>(name: String, palette: &Palette) -> Element<'a, M> {
    text(name)
        .font(crate::APP_FONT)
        .size(font_size::FILE)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

/// The names a dialog is about, one under the other in a bordered box. With
/// `scroll_height` the box is that tall and scrolls; `more` is a dimmed last
/// line for the names left out.
pub fn name_list<'a, M: Clone + 'a>(
    names: Vec<String>,
    more: Option<String>,
    scroll_height: Option<f32>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut lines: Column<'a, M> = Column::new().spacing(spacing::XS / 2.0);
    for name in names {
        lines = lines.push(file_name(name, palette));
    }
    if let Some(more) = more {
        lines = lines.push(dim_line(more, palette));
    }
    let body = container(lines).padding([spacing::M, spacing::M]);
    let inside: Element<'a, M> = match scroll_height {
        Some(height) => scrollable(body).height(height).into(),
        None => body.into(),
    };
    container(inside)
        .width(Length::Fill)
        .style(theme::outlined(palette))
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

/// The title above a group of rows in a list, under a dividing line.
pub fn group_heading<'a, M: Clone + 'a>(
    title: impl IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    column![
        divider(palette),
        text(title).size(font_size::BODY).color(palette.dim_text),
    ]
    .padding([0.0, spacing::CELL_PADDING_X])
    .spacing(spacing::LINE_GAP)
    .into()
}

/// A row of a list of places: the name, a smaller share of the width for
/// where it is. `present` is false for a place that is gone, drawn dim.
pub fn place_row<'a, M: Clone + 'a>(
    name: String,
    path: String,
    present: bool,
    is_selected: bool,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    let name_color = match (is_selected, present) {
        (true, _) => palette.cursor_text,
        (false, true) => palette.directory,
        (false, false) => palette.dim_text,
    };
    let path_color = if is_selected {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let cell = |shown: String, color, portion| {
        container(
            text(shown)
                .size(font_size::BODY)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .width(Length::FillPortion(portion))
        .clip(true)
        .into()
    };
    let content = Row::with_children([cell(name, name_color, 2), cell(path, path_color, 3)])
        .spacing(spacing::CELL_PADDING_X);
    button(content)
        .on_press(on_press)
        .width(Length::Fill)
        .style(theme::row(palette, false, is_selected, true))
        .into()
}

/// The frame around the body of a panel: fills its space in the panel colors.
pub fn panel_frame<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel(palette))
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

/// `content` centred over the window on a dimmed backdrop, so clicks outside it
/// do not reach what is below.
pub fn scrim<'a, M: 'a>(content: iced::Element<'a, M>) -> iced::Element<'a, M> {
    iced::widget::container(content)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(|_theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(crate::theme::SCRIM)),
            ..iced::widget::container::Style::default()
        })
        .into()
}

/// `content` with `label` in a small box on hover.
pub fn tooltip<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    position: iced::widget::tooltip::Position,
    palette: &Palette,
) -> Element<'a, M> {
    iced::widget::tooltip(
        content,
        container(text(label).size(font_size::SMALL).wrapping(Wrapping::None))
            .padding([spacing::XS, spacing::M])
            .style(theme::dialog(palette)),
        position,
    )
    .into()
}
