//! The small building blocks of the dialogs, in the manner of React
//! components: every function takes what it shows and returns an `Element`,
//! and every color comes from the palette. A dialog is put together from these
//! and the [`Frame`](super::frame::Frame); it does not style widgets itself.

use iced::mouse::ScrollDelta;
use iced::widget::button as widget_button;
use iced::widget::checkbox as widget_checkbox;
use iced::widget::svg::{self, Svg};
use iced::widget::text::{IntoFragment, LineHeight, Shaping, Wrapping};
use iced::widget::{
    column, container, mouse_area, pick_list, row, rule, text, text_input, Column, Row, Space,
};
use iced::{
    alignment, Background, Color, Element, Font, Length, Padding, Pixels, Radians, Rotation, Theme,
};
use ncrs_index::ConditionKind;
use std::sync::LazyLock;

use super::format;
use super::frame::{AVERAGE_GLYPH_WIDTH, SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use super::theme::{self, font_size, size, spacing, DISABLED_OPACITY};
use crate::i18n::Msg;
use crate::messages::Message;
use crate::palette::Palette;

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
    }
}

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

/// The Lucide icons the main window draws (assets/icons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Clock,
    Loader,
    CircleCheck,
    CircleHelp,
    TriangleAlert,
    Ban,
    ChevronRight,
    CircleArrowRight,
}

impl Icon {
    pub const ALL: [Self; 8] = [
        Self::Clock,
        Self::Loader,
        Self::CircleCheck,
        Self::CircleHelp,
        Self::TriangleAlert,
        Self::Ban,
        Self::ChevronRight,
        Self::CircleArrowRight,
    ];

    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Clock => include_bytes!("../../assets/icons/clock.svg"),
            Self::Loader => include_bytes!("../../assets/icons/loader-circle.svg"),
            Self::CircleCheck => include_bytes!("../../assets/icons/circle-check.svg"),
            Self::CircleHelp => include_bytes!("../../assets/icons/circle-question-mark.svg"),
            Self::TriangleAlert => include_bytes!("../../assets/icons/triangle-alert.svg"),
            Self::Ban => include_bytes!("../../assets/icons/ban.svg"),
            Self::ChevronRight => include_bytes!("../../assets/icons/chevron-right.svg"),
            Self::CircleArrowRight => include_bytes!("../../assets/icons/circle-arrow-right.svg"),
        }
    }

    /// Built once: the handle's id is what the renderer caches the parsed
    /// vector by, so a new handle per frame would parse the file every frame.
    fn handle(self) -> svg::Handle {
        static HANDLES: LazyLock<[svg::Handle; 8]> =
            LazyLock::new(|| Icon::ALL.map(|icon| svg::Handle::from_memory(icon.bytes())));
        HANDLES
            .get(self as usize)
            .cloned()
            .unwrap_or_else(|| svg::Handle::from_memory(self.bytes()))
    }
}

/// An icon in `color`, [`size::ICON`] square.
pub fn icon<'a, M: 'a>(icon: Icon, color: Color) -> Element<'a, M> {
    icon_rotated(icon, color, 0.0)
}

/// As [`icon`], turned by `radians` around its center (the spinner).
pub fn icon_rotated<'a, M: 'a>(icon: Icon, color: Color, radians: f32) -> Element<'a, M> {
    Svg::new(icon.handle())
        .width(size::ICON)
        .height(size::ICON)
        .rotation(Rotation::Floating(Radians(radians)))
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// A short label on a tinted, rounded ground, one line: the reason of a
/// document. `on_fill` for a chip on the highlighted row of a list.
pub fn chip<'a, M: 'a>(label: &str, on_fill: bool, palette: &Palette) -> Element<'a, M> {
    container(
        text(label.to_owned())
            .size(font_size::SMALL)
            .wrapping(Wrapping::None),
    )
    .padding([1.0, spacing::S])
    .style(theme::chip(palette, on_fill))
    .into()
}

/// A chip the user can take away again: the label and a cross. The chip
/// has an accent outline.
pub fn removable_chip<'a>(
    label: &str,
    on_remove: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    let cross = widget_button(text("×").size(font_size::SMALL))
        .padding([0.0, spacing::XS])
        .on_press(on_remove)
        .style(theme::close_button(palette));
    container(
        row![
            text(label.to_owned())
                .size(font_size::SMALL)
                .wrapping(Wrapping::None),
            cross
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center),
    )
    .padding([1.0, spacing::S])
    .style(theme::own_chip(palette))
    .into()
}

/// A key as a small outlined cap.
pub fn key_cap<'a>(label: &str, palette: &Palette) -> Element<'a, Message> {
    cap(label, false, palette)
}

/// A key cap in a menu row; on the highlighted (filled) row it is drawn in
/// the highlight's text color so it stays readable.
pub fn key_cap_in_row<'a>(
    label: &str,
    highlighted: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    cap(label, highlighted, palette)
}

/// The one key cap of a highlighted row that an edit acts on: filled in the
/// highlight's text color, so it stands out from the outlined caps beside it.
pub fn key_cap_focused<'a>(label: &str, palette: &Palette) -> Element<'a, Message> {
    let base = theme::key_cap(palette, true);
    let (fill, ink) = (palette.cursor_text, palette.cursor);
    container(
        text(label.to_owned())
            .size(font_size::KEY_CAP)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::XS - 1.0])
    .style(move |theme: &Theme| {
        let mut style = base(theme);
        style.background = Some(fill.into());
        style.text_color = Some(ink);
        style
    })
    .into()
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

/// A flat button in the accent color for a jump to another page, sized like
/// a line of text.
pub fn link<'a>(label: &str, on_press: Message, palette: &Palette) -> Element<'a, Message> {
    widget_button(label_text(label))
        .padding([0.0, spacing::S])
        .on_press(on_press)
        .style(theme::link_button(palette))
        .into()
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

/// A notice that is neither an error nor plain text: an outlined box with its
/// line in the accent color, like the warning that a name is taken.
pub fn warning_box<'a>(content: impl IntoFragment<'a>, palette: &Palette) -> Element<'a, Message> {
    container(
        text(content)
            .size(font_size::BODY)
            .color(palette.accent)
            .wrapping(Wrapping::Word),
    )
    .padding([spacing::XS, spacing::M])
    .width(Length::Fill)
    .style(theme::outlined_in(palette.accent))
    .into()
}

/// How far a job is, `0.0` to `1.0`, as a thin bar.
pub fn progress_bar<'a>(fraction: f32, palette: &Palette) -> Element<'a, Message> {
    iced::widget::progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0))
        .length(Length::Fill)
        .girth(PROGRESS_GIRTH)
        .style(theme::progress(palette))
        .into()
}

/// The thickness of the progress bar.
const PROGRESS_GIRTH: f32 = 4.0;

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
    form_row_with_problem(
        label,
        theme_name,
        description,
        None,
        control,
        control_width,
        palette,
    )
}

/// [`form_row_wide`] with what is wrong with the value in the error color
/// under the description.
pub fn form_row_with_problem<'a>(
    label: &str,
    theme_name: Option<&str>,
    description: Option<&str>,
    problem: Option<&str>,
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
    if let Some(wrong) = problem {
        left = left.push(error_line(wrong.to_owned(), palette));
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
    field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Plain,
        palette,
    )
}

/// A text field that shows dots instead of what is typed, for a password.
pub fn secret_field<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Secret,
        palette,
    )
}

/// A text field for a path or a name, set in the monospace font.
pub fn mono_text_field<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Mono,
        palette,
    )
}

/// How a text field shows what is typed.
#[derive(Clone, Copy)]
enum Look {
    Plain,
    /// The monospace font, for paths and names.
    Mono,
    /// Dots, for a password.
    Secret,
}

fn field_in<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    look: Look,
    palette: &Palette,
) -> Element<'a, Message> {
    let plain = text_input(placeholder, value)
        .id(id)
        .secure(matches!(look, Look::Secret));
    let fonted = match look {
        Look::Mono => plain.font(Font::MONOSPACE),
        Look::Plain | Look::Secret => plain,
    };
    let field = match on_input {
        Some(typed) => fonted.on_input(typed).on_submit(on_submit),
        None => fonted,
    };
    field
        .size(font_size::BODY)
        .line_height(LineHeight::Absolute(Pixels(FIELD_LINE_HEIGHT)))
        .padding([spacing::XS, spacing::S])
        .style(theme::field_input(palette))
        .width(Length::Fill)
        .into()
}

/// One row of a list; a click sends `on_press`. The row under the cursor is
/// filled while the list has the keyboard focus (`focused`), else only
/// outlined.
pub fn list_row<'a>(
    content: Element<'a, Message>,
    on_press: Message,
    selected: bool,
    focused: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    list_row_of_height(content, on_press, selected, focused, size::ROW, palette)
}

/// [`list_row`] for rows of more than one line.
pub fn list_row_of_height<'a>(
    content: Element<'a, Message>,
    on_press: Message,
    selected: bool,
    focused: bool,
    row_height: f32,
    palette: &Palette,
) -> Element<'a, Message> {
    widget_button(content)
        .on_press(on_press)
        .width(Length::Fill)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(row_height)
        .style(theme::row(palette, false, selected, focused))
        .into()
}

/// Room kept right of a table for the bar of a scrolling list.
pub const SCROLL_LANE: f32 = SCROLLBAR_WIDTH + SCROLLBAR_MARGIN;

/// The columns of a table of text that must stay on one line: each takes a
/// share of the line, and a cell is cut to the pixels its column holds. The
/// width of a text is estimated, see [`format::estimated_width`].
pub struct Columns<'p> {
    /// The relative widths of the columns.
    pub portions: &'p [u16],
    /// The characters of the whole line, at the average width of a glyph.
    pub line_chars: usize,
}

impl Columns<'_> {
    /// The pixels the cells of the line share: the line less the padding of a
    /// row and the lane of the scroll bar.
    fn cells_width(&self) -> f32 {
        (self.line_chars as f32 * AVERAGE_GLYPH_WIDTH - 2.0 * spacing::CELL_PADDING_X - SCROLL_LANE)
            .max(0.0)
    }

    /// The pixels the text of column `index` may take: the share of the
    /// column less the gap to the next one.
    pub fn text_width(&self, index: usize) -> f32 {
        let total: f32 = self.portions.iter().copied().map(f32::from).sum();
        let portion = self.portions.get(index).copied().map_or(0.0, f32::from);
        if total <= 0.0 {
            return 0.0;
        }
        (self.cells_width() * portion / total - spacing::S).max(0.0)
    }

    /// The text of column `index`, cut at the end where it is too long.
    pub fn cell<'a>(&self, index: usize, content: &str, color: Color) -> Element<'a, Message> {
        self.cell_of(
            index,
            format::fit_end(content, self.text_width(index), font_size::BODY),
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
        .padding(Padding {
            right: spacing::S,
            ..Padding::ZERO
        })
        .align_x(align)
        .clip(true)
        .into()
    }

    /// A [`chip`] in column `index`, its label cut at the end where it is too
    /// long for the column.
    pub fn chip_cell<'a>(
        &self,
        index: usize,
        label: &str,
        on_fill: bool,
        palette: &Palette,
    ) -> Element<'a, Message> {
        let portion = self.portions.get(index).copied().unwrap_or(1);
        let room = (self.text_width(index) - 2.0 * spacing::S).max(0.0);
        container(chip(
            &format::fit_end(label, room, font_size::SMALL),
            on_fill,
            palette,
        ))
        .width(Length::FillPortion(portion))
        .padding(Padding {
            right: spacing::S,
            ..Padding::ZERO
        })
        .clip(true)
        .into()
    }

    /// The text of column `index`, cut in the middle where it is too long: a
    /// path, whose end matters as much as its start.
    pub fn path_cell<'a>(&self, index: usize, content: &str, color: Color) -> Element<'a, Message> {
        self.cell_of(
            index,
            format::fit_middle(content, self.text_width(index), font_size::BODY),
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
                    format::fit_end(content, self.text_width(index), font_size::BODY),
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

/// The shortest the thumb of the bar of a list gets.
const MIN_THUMB: f32 = 12.0;

/// Where the thumb starts and how long it is on a track of `track` pixels, for
/// a window of `visible` rows at `offset` in a list of `len`.
fn thumb_extent(len: usize, visible: usize, offset: usize, track: f32) -> (f32, f32) {
    let thumb = (track * visible as f32 / len.max(1) as f32).clamp(MIN_THUMB.min(track), track);
    let scrollable = len.saturating_sub(visible).max(1) as f32;
    let top = (track - thumb) * (offset as f32 / scrollable).min(1.0);
    (top, thumb)
}

/// A table whose rows are a window into a longer list: the header, the rows
/// shown, and right of them a bar that shows where the window is. The wheel
/// over the rows (not over the header or the rest of the page) sends
/// `on_scroll`.
pub fn scroll_list<'a>(
    header: Element<'a, Message>,
    rows: Vec<Element<'a, Message>>,
    window: ListView,
    on_scroll: impl Fn(ScrollDelta) -> Message + 'a,
    palette: &Palette,
) -> Element<'a, Message> {
    scroll_list_of_height(header, rows, window, size::ROW, on_scroll, palette)
}

/// [`scroll_list`] for rows of `row_height` pixels, as made by
/// [`list_row_of_height`].
pub fn scroll_list_of_height<'a>(
    header: Element<'a, Message>,
    rows: Vec<Element<'a, Message>>,
    window: ListView,
    row_height: f32,
    on_scroll: impl Fn(ScrollDelta) -> Message + 'a,
    palette: &Palette,
) -> Element<'a, Message> {
    let shown = rows.len();
    let track = row_height * shown as f32;
    let list = mouse_area(
        container(Column::with_children(rows))
            .width(Length::Fill)
            .height(track)
            .clip(true),
    )
    .on_scroll(on_scroll);
    let lane = if window.len > window.visible {
        let (top, thumb) = thumb_extent(window.len, window.visible, window.offset, track);
        let (track_color, thumb_color) = (palette.border_inactive, palette.dim_text);
        let fill = |color: Color| {
            move |_theme: &Theme| container::Style {
                background: Some(Background::Color(color)),
                border: iced::Border {
                    radius: SCROLLBAR_WIDTH.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            }
        };
        container(column![
            Space::new().height(top),
            container("")
                .width(SCROLLBAR_WIDTH)
                .height(thumb)
                .style(fill(thumb_color)),
        ])
        .width(SCROLLBAR_WIDTH)
        .height(track)
        .style(fill(track_color))
    } else {
        container("").width(SCROLLBAR_WIDTH).height(track)
    };
    column![
        container(header).padding(Padding {
            right: SCROLL_LANE,
            ..Padding::ZERO
        }),
        row![
            list,
            container(lane).padding(Padding {
                left: SCROLLBAR_MARGIN,
                ..Padding::ZERO
            })
        ],
    ]
    .into()
}

/// Which rows of a list a window shows, for [`scroll_list`].
#[derive(Debug, Clone, Copy)]
pub struct ListView {
    pub len: usize,
    pub visible: usize,
    pub offset: usize,
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

/// An entry of a dropdown: equal to another by its position, so that two
/// entries with the same label stay apart.
#[derive(Debug, Clone)]
struct DropdownChoice {
    label: String,
    position: usize,
    message: Message,
}

impl PartialEq for DropdownChoice {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
    }
}

impl std::fmt::Display for DropdownChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn dropdown_choices(options: &[(&str, Message)]) -> Vec<DropdownChoice> {
    options
        .iter()
        .enumerate()
        .map(|(position, (label, message))| DropdownChoice {
            label: (*label).to_owned(),
            position,
            message: message.clone(),
        })
        .collect()
}

/// A closed list of which one entry is shown and the others drop down on a
/// click; the same shape as [`segmented`], for a list that may grow long.
/// Disabled, it shows the chosen entry fainter and does not open.
pub fn dropdown<'a>(
    options: &[(&str, Message)],
    selected: usize,
    enabled: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let choices = dropdown_choices(options);
    let chosen = choices.get(selected).cloned();
    if !enabled {
        let label = chosen.map(|choice| choice.label).unwrap_or_default();
        let dimmed = palette.text.scale_alpha(DISABLED_OPACITY);
        return container(
            text(label)
                .size(font_size::BODY)
                .wrapping(Wrapping::None)
                .color(dimmed),
        )
        .padding([spacing::XS, spacing::S])
        .center_y(size::ROW_TALL)
        .width(Length::Fill)
        .style(theme::outlined(palette))
        .into();
    }
    pick_list(choices, chosen, |choice| choice.message)
        .width(Length::Fill)
        .padding([spacing::XS, spacing::S])
        .text_size(font_size::BODY)
        .text_line_height(LineHeight::Absolute(Pixels(FIELD_LINE_HEIGHT)))
        .style(theme::dropdown_field(palette))
        .menu_style(theme::dropdown_menu(palette))
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
/// the list behind them; `note` goes between the two, set off by a divider.
pub fn count_summary<'a>(
    figures: &[(&str, String)],
    note: Option<Element<'a, Message>>,
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
    let mut parts = column![figure_cells(figures, palette), divider(palette)];
    if let Some(between) = note {
        parts = parts.push(container(between).padding([spacing::S, BUTTON_PADDING_X]));
        parts = parts.push(divider(palette));
    }
    container(parts.push(toggle))
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
    fn a_picked_dropdown_choice_sends_the_message_of_its_option() {
        let options = [("a", Message::PageUp), ("b", Message::PageDown)];
        let choices = dropdown_choices(&options);
        let picked = choices.get(1).cloned().map(|choice| choice.message);
        assert_eq!(picked, Some(Message::PageDown));
        assert_ne!(choices.first(), choices.get(1));
    }

    #[test]
    fn a_column_holds_its_share_of_the_line_less_the_gap() {
        let columns = Columns {
            portions: &[1, 2, 1],
            line_chars: 80,
        };
        let cells = 80.0 * AVERAGE_GLYPH_WIDTH - 2.0 * spacing::CELL_PADDING_X - SCROLL_LANE;
        let gap = spacing::S;
        assert!((columns.text_width(0) - (cells / 4.0 - gap)).abs() < 0.01);
        assert!((columns.text_width(1) - (cells / 2.0 - gap)).abs() < 0.01);
        assert!(columns.text_width(1) > columns.text_width(0));
    }

    #[test]
    fn a_column_that_does_not_exist_or_a_line_without_columns_holds_nothing() {
        let columns = Columns {
            portions: &[1, 1],
            line_chars: 40,
        };
        assert_eq!(columns.text_width(5), 0.0);
        let none = Columns {
            portions: &[],
            line_chars: 40,
        };
        assert_eq!(none.text_width(0), 0.0);
    }

    #[test]
    fn a_narrow_line_does_not_underflow() {
        let columns = Columns {
            portions: &[1, 1],
            line_chars: 2,
        };
        assert_eq!(columns.text_width(0), 0.0);
    }

    #[test]
    fn the_thumb_follows_the_offset_and_never_leaves_the_track() {
        let track = 5.0 * size::ROW;
        let (top, thumb) = thumb_extent(2421, 5, 0, track);
        assert_eq!(top, 0.0);
        assert!(thumb >= MIN_THUMB);
        let (end_top, end_thumb) = thumb_extent(2421, 5, 2416, track);
        assert!((end_top + end_thumb - track).abs() < 0.01);
        let (middle, _) = thumb_extent(100, 10, 45, track);
        assert!(middle > 0.0 && middle < track);
    }

    #[test]
    fn every_icon_is_a_24_unit_svg() {
        for icon in Icon::ALL {
            let source = std::str::from_utf8(icon.bytes()).unwrap_or_default();
            assert!(source.trim_start().starts_with("<svg"), "{icon:?}");
            assert!(source.contains("viewBox=\"0 0 24 24\""), "{icon:?}");
        }
    }

    #[test]
    fn every_icon_has_its_own_cached_handle() {
        for (index, icon) in Icon::ALL.into_iter().enumerate() {
            assert_eq!(icon as usize, index);
            assert_eq!(icon.handle().id(), icon.handle().id());
        }
    }
}
