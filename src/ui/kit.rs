//! The small building blocks of the dialogs, in the manner of React
//! components: every function takes what it shows and returns an `Element`,
//! and every color comes from the palette. A dialog is put together from these
//! and the [`Frame`](super::frame::Frame); it does not style widgets itself.

use iced::widget::button as widget_button;
use iced::widget::checkbox as widget_checkbox;
use iced::widget::text::{IntoFragment, LineHeight, Shaping, Wrapping};
use iced::widget::{column, container, row, rule, text, text_input, Column, Row};
use iced::{alignment, Color, Element, Length, Pixels, Theme};

use super::format;
use super::theme::{self, font_size, size, spacing, DISABLED_OPACITY};
use crate::messages::Message;
use crate::palette::Palette;

/// Horizontal room inside a button, either side of the label.
const BUTTON_PADDING_X: f32 = 10.0;

/// The height of the text inside a field: with the padding it makes
/// [`size::ROW_TALL`].
const FIELD_LINE_HEIGHT: f32 = 16.0;

/// How strongly the outline of a key cap shows on a filled button.
const CAP_OUTLINE_ALPHA: f32 = 0.35;

/// What a button stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The action the dialog is for: filled.
    Primary,
    /// Everything else: outlined.
    Secondary,
    /// An action that deletes: filled in the error color.
    Danger,
}

impl Tone {
    /// Filled for the button that is the focus of the dialog, outlined for the
    /// others.
    pub const fn filled_if(filled: bool) -> Self {
        if filled {
            Self::Primary
        } else {
            Self::Secondary
        }
    }
}

/// A key as a small outlined cap.
pub fn key_cap<'a>(label: &str, palette: &Palette) -> Element<'a, Message> {
    cap(label, false, palette)
}

fn cap<'a>(label: &str, on_fill: bool, palette: &Palette) -> Element<'a, Message> {
    let base = theme::key_cap(palette, on_fill);
    container(
        text(label.to_owned())
            .size(font_size::KEY_CAP)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::XS - 1.0])
    .style(move |theme: &Theme| {
        let mut style = base(theme);
        if on_fill {
            style.border.color = style.border.color.scale_alpha(CAP_OUTLINE_ALPHA);
        }
        style
    })
    .into()
}

fn tone_style(
    palette: &Palette,
    tone: Tone,
) -> impl Fn(&Theme, widget_button::Status) -> widget_button::Style {
    let primary = theme::dialog_button(palette, true);
    let secondary = theme::dialog_button(palette, false);
    let danger = theme::danger_button(palette);
    move |theme, status| {
        let disabled = matches!(status, widget_button::Status::Disabled);
        // A disabled button keeps its fill and outline, only fainter: without
        // them it reads as plain text.
        let shown = if disabled {
            widget_button::Status::Active
        } else {
            status
        };
        let mut style = match tone {
            Tone::Primary => primary(theme, shown),
            Tone::Secondary => secondary(theme, shown),
            Tone::Danger => danger(theme, shown),
        };
        if disabled {
            style.background = style
                .background
                .map(|background| background.scale_alpha(DISABLED_OPACITY));
            style.text_color = style.text_color.scale_alpha(DISABLED_OPACITY);
            style.border.color = style.border.color.scale_alpha(DISABLED_OPACITY);
        }
        style
    }
}

fn label_text<'a>(label: &str) -> Element<'a, Message> {
    text(label.to_owned())
        .size(font_size::BODY)
        .wrapping(Wrapping::None)
        .into()
}

fn sized_button<'a>(
    content: Element<'a, Message>,
    on_press: Option<Message>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, Message> {
    widget_button(container(content).center_y(Length::Fill))
        .padding([0.0, BUTTON_PADDING_X])
        .height(size::BUTTON)
        .on_press_maybe(on_press)
        .style(tone_style(palette, tone))
        .into()
}

/// A button of 24 px. `on_press` of `None` disables it.
pub fn button<'a>(
    label: &str,
    on_press: Option<Message>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, Message> {
    sized_button(label_text(label), on_press, tone, palette)
}

/// Like [`button`], with a small Enter cap: the button Enter presses.
pub fn enter_button<'a>(
    label: &str,
    on_press: Option<Message>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, Message> {
    let content = row![
        label_text(label),
        cap(ENTER_KEY, on_press.is_some(), palette)
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    sized_button(content.into(), on_press, tone, palette)
}

/// The label of the Enter key, on its cap and in the hints.
pub const ENTER_KEY: &str = "↵";

/// A dimmed line of text.
pub fn dim<'a>(content: impl IntoFragment<'a>, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .into()
}

/// Text that stays on one line; the caller has cut it to fit.
pub fn one_line<'a>(content: String, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

/// Running text.
pub fn paragraph<'a>(content: impl IntoFragment<'a>, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::Word)
        .into()
}

pub fn error_line<'a>(content: impl IntoFragment<'a>, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(palette.error)
        .wrapping(Wrapping::Word)
        .into()
}

/// Lines of one block, one gap apart.
pub fn stack<'a>(lines: impl IntoIterator<Item = Element<'a, Message>>) -> Column<'a, Message> {
    Column::with_children(lines).spacing(spacing::LINE_GAP)
}

/// A dimmed heading above its content, set off from what comes before it.
pub fn section<'a>(
    heading: &str,
    content: impl Into<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    container(heading_block(heading, content.into(), palette))
        .padding(iced::Padding {
            top: spacing::SECTION_LEAD,
            ..iced::Padding::ZERO
        })
        .into()
}

/// A section at the top of a page, which has its room above already.
pub fn first_section<'a>(
    heading: &str,
    content: impl Into<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    heading_block(heading, content.into(), palette)
}

fn heading_block<'a>(
    heading: &str,
    content: Element<'a, Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    column![dim(heading.to_owned(), palette), content]
        .spacing(spacing::S)
        .into()
}

/// A line across the width of its parent.
pub fn divider<'a>(palette: &Palette) -> Element<'a, Message> {
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
/// description on the left, the control in a column of fixed width on the
/// right.
pub fn form_row<'a>(
    label: &str,
    theme_name: Option<&str>,
    description: Option<&str>,
    control: Element<'a, Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    form_row_wide(
        label,
        theme_name,
        description,
        control,
        size::CONTROL_WIDTH,
        palette,
    )
}

/// Like [`form_row`], with a control column of `control_width`, for a control
/// that does not fit the usual one.
pub fn form_row_wide<'a>(
    label: &str,
    theme_name: Option<&str>,
    description: Option<&str>,
    control: Element<'a, Message>,
    control_width: f32,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut names = Row::new()
        .push(paragraph(label.to_owned(), palette))
        .spacing(spacing::S);
    if let Some(name) = theme_name {
        names = names.push(dim(name.to_owned(), palette));
    }
    let mut left = column![names].spacing(spacing::XS / 2.0);
    if let Some(text_below) = description {
        left = left.push(dim(text_below.to_owned(), palette));
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

/// A single-line text field of 24 px. Without `on_input` it takes no typing:
/// it is shown disabled.
pub fn text_field<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    let base = text_input(placeholder, value).id(id);
    let field = match on_input {
        Some(typed) => base.on_input(typed).on_submit(on_submit),
        None => base,
    };
    field
        .size(font_size::BODY)
        .line_height(LineHeight::Absolute(Pixels(FIELD_LINE_HEIGHT)))
        .padding([spacing::XS, spacing::S])
        .style(theme::field_input(palette))
        .width(Length::Fill)
        .into()
}

/// One row of a list; a click sends `on_press`.
pub fn list_row<'a>(
    content: Element<'a, Message>,
    on_press: Message,
    selected: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    widget_button(content)
        .on_press(on_press)
        .width(Length::Fill)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(size::ROW)
        .style(theme::row(palette, false, selected, true))
        .into()
}

/// The columns of a table of text that must stay on one line: each takes a
/// share of the line, and a cell is cut to the characters its column holds.
pub struct Columns<'p> {
    /// The relative widths of the columns.
    pub portions: &'p [u16],
    /// The characters of the whole line.
    pub line_chars: usize,
}

impl Columns<'_> {
    /// The characters column `index` holds, less the padding around its cell.
    pub fn chars(&self, index: usize) -> usize {
        let total: usize = self.portions.iter().copied().map(usize::from).sum();
        let portion = self.portions.get(index).copied().map_or(0, usize::from);
        self.line_chars
            .saturating_mul(portion)
            .checked_div(total)
            .unwrap_or_default()
            .saturating_sub(2)
    }

    /// The text of column `index`, cut at the end where it is too long.
    pub fn cell<'a>(&self, index: usize, content: &str, color: Color) -> Element<'a, Message> {
        self.cell_of(
            index,
            format::ellipsize_end(content, self.chars(index)),
            color,
        )
    }

    fn cell_of<'a>(&self, index: usize, shown: String, color: Color) -> Element<'a, Message> {
        self.aligned_cell_of(index, shown, color, alignment::Horizontal::Left)
    }

    fn aligned_cell_of<'a>(
        &self,
        index: usize,
        shown: String,
        color: Color,
        align: alignment::Horizontal,
    ) -> Element<'a, Message> {
        let portion = self.portions.get(index).copied().unwrap_or(1);
        container(
            text(shown)
                .size(font_size::BODY)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .width(Length::FillPortion(portion))
        .align_x(align)
        .clip(true)
        .into()
    }

    /// The text of column `index`, cut in the middle where it is too long: a
    /// path, whose end matters as much as its start.
    pub fn path_cell<'a>(&self, index: usize, content: &str, color: Color) -> Element<'a, Message> {
        self.cell_of(
            index,
            format::ellipsize_middle(content, self.chars(index)),
            color,
        )
    }

    /// The dimmed titles of the columns, one text line high: nothing selects
    /// them, so they need no list row height and stay close to the heading.
    pub fn header<'a>(&self, labels: &[&str], palette: &Palette) -> Element<'a, Message> {
        let dimmed: Vec<(&str, Color)> = labels
            .iter()
            .map(|label| (*label, palette.dim_text))
            .collect();
        self.line(&dimmed, alignment::Horizontal::Left, Length::Shrink)
    }

    /// One row of the table, every cell in `color`.
    pub fn row<'a>(&self, cells: &[&str], color: Color) -> Element<'a, Message> {
        let colored: Vec<(&str, Color)> = cells.iter().map(|cell| (*cell, color)).collect();
        self.row_colored(&colored)
    }

    /// One row of the table, each cell in its own color.
    pub fn row_colored<'a>(&self, cells: &[(&str, Color)]) -> Element<'a, Message> {
        self.line(cells, alignment::Horizontal::Left, size::ROW.into())
    }

    /// One row of the table, every cell in `color`, the last column set
    /// against its right edge: a column of times or amounts.
    pub fn row_last_right<'a>(&self, cells: &[&str], color: Color) -> Element<'a, Message> {
        let colored: Vec<(&str, Color)> = cells.iter().map(|cell| (*cell, color)).collect();
        self.line(&colored, alignment::Horizontal::Right, size::ROW.into())
    }

    fn line<'a>(
        &self,
        cells: &[(&str, Color)],
        last: alignment::Horizontal,
        height: Length,
    ) -> Element<'a, Message> {
        let missing = self.portions.len().saturating_sub(cells.len());
        let last_index = self.portions.len().saturating_sub(1);
        let shown = cells
            .iter()
            .enumerate()
            .map(|(index, (content, color))| {
                let align = if index == last_index {
                    last
                } else {
                    alignment::Horizontal::Left
                };
                self.aligned_cell_of(
                    index,
                    format::ellipsize_end(content, self.chars(index)),
                    *color,
                    align,
                )
            })
            .chain(
                (cells.len()..cells.len() + missing)
                    .map(|index| self.cell(index, "", Color::TRANSPARENT)),
            );
        Row::with_children(shown)
            .padding([0.0, spacing::CELL_PADDING_X])
            .height(height)
            .align_y(alignment::Vertical::Center)
            .into()
    }
}

/// A group of buttons of which one is on, like the load setting of Hermes.
pub fn segmented<'a>(
    options: &[(&str, Message)],
    selected: usize,
    enabled: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let buttons = options.iter().enumerate().map(|(index, (label, message))| {
        widget_button(container(label_text(label)).center_y(Length::Fill))
            .padding([0.0, BUTTON_PADDING_X])
            .height(size::ROW)
            .on_press_maybe(enabled.then(|| message.clone()))
            .style(theme::segment_button(palette, index == selected))
            .into()
    });
    container(Row::with_children(buttons))
        .style(theme::outlined(palette))
        .into()
}

/// The state of a checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    On,
    /// Some of what the box stands for is on, like a group of which only part
    /// of the entries is chosen.
    Mixed,
}

/// The side of a checkbox.
const CHECKBOX_SIDE: f32 = 14.0;

/// A checkbox without a label. `on_toggle` of `None` leaves the clicking to
/// the widget around it, like a list row.
pub fn checkbox<'a>(
    state: Check,
    on_toggle: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut boxed = widget_checkbox(state != Check::Off)
        .size(CHECKBOX_SIDE)
        .style(theme::dialog_checkbox(palette));
    if state == Check::Mixed {
        boxed = boxed.icon(widget_checkbox::Icon {
            font: iced::Font::DEFAULT,
            code_point: '-',
            size: None,
            line_height: LineHeight::default(),
            shaping: Shaping::Basic,
        });
    }
    if let Some(message) = on_toggle {
        boxed = boxed.on_toggle(move |_| message.clone());
    }
    boxed.into()
}

/// A few numbers, each under its dimmed name, side by side in an outlined box.
pub fn figures<'a>(figures: &[(&str, String)], palette: &Palette) -> Element<'a, Message> {
    container(figure_cells(figures, palette))
        .style(theme::outlined(palette))
        .into()
}

fn figure_cells<'a>(figures: &[(&str, String)], palette: &Palette) -> Row<'a, Message> {
    let cells = figures.iter().map(|(label, value)| {
        let shown: Element<'a, Message> = if value.chars().any(|c| c.is_ascii_digit()) {
            text(value.clone())
                .size(font_size::FIGURE)
                .color(palette.text)
                .wrapping(Wrapping::None)
                .into()
        } else {
            // A value without a digit is a note, not a number.
            dim(value.clone(), palette)
        };
        container(column![dim(label.to_string(), palette), shown].spacing(spacing::XS / 2.0))
            .padding([spacing::S, BUTTON_PADDING_X])
            .width(Length::FillPortion(1))
            .into()
    });
    Row::with_children(cells)
}

/// The size of a selection in a few numbers, with a button that shows or hides
/// the list behind them.
pub fn count_summary<'a>(
    figures: &[(&str, String)],
    expanded: bool,
    on_toggle: Message,
    show_label: &str,
    hide_label: &str,
    palette: &Palette,
) -> Element<'a, Message> {
    let (chevron, label) = if expanded {
        ("v", hide_label)
    } else {
        (">", show_label)
    };
    let toggle = widget_button(
        container(
            row![label_text(chevron), label_text(label)]
                .spacing(spacing::S)
                .align_y(alignment::Vertical::Center),
        )
        .center_y(Length::Fill),
    )
    .padding([0.0, BUTTON_PADDING_X])
    .height(size::ROW_TALL)
    .width(Length::Fill)
    .on_press(on_toggle)
    .style(theme::link_button(palette));
    container(column![
        figure_cells(figures, palette),
        divider(palette),
        toggle
    ])
    .style(theme::outlined(palette))
    .into()
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

    #[test]
    fn a_column_holds_its_share_of_the_line_less_the_padding() {
        let columns = Columns {
            portions: &[1, 2, 1],
            line_chars: 80,
        };
        assert_eq!(columns.chars(0), 18);
        assert_eq!(columns.chars(1), 38);
        assert_eq!(columns.chars(2), 18);
    }

    #[test]
    fn a_column_that_does_not_exist_or_a_line_without_columns_holds_nothing() {
        let columns = Columns {
            portions: &[1, 1],
            line_chars: 40,
        };
        assert_eq!(columns.chars(5), 0);
        let none = Columns {
            portions: &[],
            line_chars: 40,
        };
        assert_eq!(none.chars(0), 0);
    }

    #[test]
    fn a_narrow_line_does_not_underflow() {
        let columns = Columns {
            portions: &[1, 1],
            line_chars: 2,
        };
        assert_eq!(columns.chars(0), 0);
    }
}
