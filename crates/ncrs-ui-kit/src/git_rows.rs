//! The rows of the Git view: a commit with its graph, ref chips, and a changed
//! file with its line counts, and the lines of a diff.

use iced::mouse::Interaction;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, container, mouse_area, responsive, row, text, Row, Space};
use iced::{alignment, mouse, Color, Element, Length, Padding, Point, Rectangle, Renderer, Theme};

use crate::format::{estimated_text_width, estimated_width, fit_end};
use crate::palette::Palette;
use crate::theme::{self, font_size, git, size, spacing};
use crate::tree_list::TREE_INDENT;

const NUMBER_WIDTH: f32 = 44.0;
pub const COUNT_WIDTH: f32 = 90.0;
const SIGN_WIDTH: f32 = 16.0;
const WASH_ALPHA: f32 = 0.18;
/// The hatching of the side that has no line: thin diagonals, one every 6 px.
const HATCH_ALPHA: f32 = 0.22;
const HATCH_STEP: f32 = 6.0;
const DIMMED_ALPHA: f32 = 0.55;
/// Room the scroll bar of the list takes from the message.
const SCROLL_BAR_ROOM: f32 = 12.0;
/// A column dragged narrower or wider stops here.
pub const COMMIT_COLUMN_MIN: u16 = 50;
pub const COMMIT_COLUMN_MAX: u16 = 600;

/// The commit columns after the message, in pixels; the message takes what is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitColumns {
    pub author: u16,
    pub date: u16,
    pub sha: u16,
}

impl CommitColumns {
    pub const STANDARD: Self = Self {
        author: 130,
        date: 130,
        sha: 70,
    };

    pub fn of(self, column: CommitColumn) -> u16 {
        match column {
            CommitColumn::Author => self.author,
            CommitColumn::Date => self.date,
            CommitColumn::Sha => self.sha,
        }
    }

    pub fn with(self, column: CommitColumn, wanted: u16) -> Self {
        let width = wanted.clamp(COMMIT_COLUMN_MIN, COMMIT_COLUMN_MAX);
        match column {
            CommitColumn::Author => Self {
                author: width,
                ..self
            },
            CommitColumn::Date => Self {
                date: width,
                ..self
            },
            CommitColumn::Sha => Self { sha: width, ..self },
        }
    }

    /// `column` after its left edge was dragged `delta` pixels to the right.
    pub fn dragged(self, column: CommitColumn, delta: f32) -> Self {
        let wanted = (f32::from(self.of(column)) - delta)
            .round()
            .clamp(f32::from(COMMIT_COLUMN_MIN), f32::from(COMMIT_COLUMN_MAX));
        self.with(column, wanted as u16)
    }

    /// What the columns, the gaps in front of them and the SHA column when shown take.
    fn taken(self, show_sha: bool) -> f32 {
        let columns = f32::from(self.author) + f32::from(self.date);
        let sha = if show_sha { f32::from(self.sha) } else { 0.0 };
        let gaps = if show_sha { 3.0 } else { 2.0 };
        columns + sha + gaps * size::COLUMN_GAP
    }
}

/// The columns of the commit list whose width can be dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitColumn {
    Author,
    Date,
    Sha,
}

/// How wide the message cell of a commit row is in a list `pane_width` wide.
pub fn commit_message_width(
    pane_width: f32,
    graph_width: f32,
    columns: CommitColumns,
    show_sha: bool,
) -> f32 {
    (pane_width
        - 2.0 * (spacing::XXL + spacing::CELL_PADDING_X)
        - graph_width
        - size::COLUMN_GAP
        - columns.taken(show_sha)
        - SCROLL_BAR_ROOM)
        .max(0.0)
}

/// A layer over the whole view while a column is dragged: the pointer moves
/// and the release are seen wherever the pointer is.
pub fn drag_capture<'a, M: Clone + 'a>(
    on_move: impl Fn(f32) -> M + 'a,
    on_release: M,
) -> Element<'a, M> {
    mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_move(move |point| on_move(point.x))
        .on_release(on_release)
        .interaction(Interaction::ResizingHorizontally)
        .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefChipKind {
    /// The checked-out branch: filled, bold, with a tick.
    Head,
    /// A local branch.
    Branch,
    Remote,
    Tag,
}

fn ref_look(kind: RefChipKind) -> git::RefLook {
    match kind {
        RefChipKind::Head => git::RefLook::Head,
        RefChipKind::Branch => git::RefLook::Local,
        RefChipKind::Remote => git::RefLook::Remote,
        RefChipKind::Tag => git::RefLook::Tag,
    }
}

/// What a chip shows: the head carries a tick.
fn chip_text(label: &str, kind: RefChipKind) -> String {
    match kind {
        RefChipKind::Head => format!("\u{2713} {label}"),
        RefChipKind::Branch | RefChipKind::Remote | RefChipKind::Tag => label.to_owned(),
    }
}

/// A branch, remote branch or tag name on a commit.
pub fn ref_chip<'a, M: 'a>(label: String, kind: RefChipKind, palette: &Palette) -> Element<'a, M> {
    chip(label, kind, false, palette)
}

/// A chip on the chosen row: the row is filled in the accent, so the chip is outlined in its ink.
fn chip<'a, M: 'a>(
    label: String,
    kind: RefChipKind,
    on_chosen: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let font = if kind == RefChipKind::Head {
        crate::APP_FONT_BOLD
    } else {
        crate::APP_FONT
    };
    container(
        text(chip_text(&label, kind))
            .size(font_size::SMALL)
            .font(font)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::XS])
    .style(git::ref_label(
        palette,
        if on_chosen {
            git::RefLook::OnChosen
        } else {
            ref_look(kind)
        },
    ))
    .into()
}

pub struct CommitRow<'a, M> {
    pub graph: Element<'a, M>,
    pub chips: Vec<(String, RefChipKind)>,
    pub message: String,
    pub author: String,
    pub date: String,
    pub sha: String,
    pub columns: CommitColumns,
    /// How wide the message cell is, from [`commit_message_width`]: a longer
    /// message is cut with an ellipsis.
    pub message_width: f32,
    /// The SHA column is left out where the message needs the room.
    pub show_sha: bool,
    pub chosen: bool,
    /// A merge commit is drawn in the dim ink.
    pub dimmed: bool,
    /// The checked-out commit: tinted in purple and bold.
    pub is_head: bool,
    pub on_press: M,
}

impl<M> std::fmt::Debug for CommitRow<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitRow").finish_non_exhaustive()
    }
}

/// The titles above the commit columns; `graph_width` is the width of the graph cells.
/// With `on_resize` the gap before each column after the message can be
/// pressed and dragged to change that column's width.
pub fn commit_header<'a, M: Clone + 'a>(
    graph_width: f32,
    labels: [&str; 5],
    columns: CommitColumns,
    show_sha: bool,
    on_resize: Option<Box<dyn Fn(CommitColumn) -> M + 'a>>,
    palette: &Palette,
) -> Element<'a, M> {
    let [graph, message, author, date, sha] = labels;
    let label = |label: &str| {
        container(
            text(label.to_owned())
                .size(font_size::COLUMN_HEADER)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        )
        .width(Length::Fill)
        .clip(true)
    };
    let strip = |column: CommitColumn| -> Element<'a, M> {
        let line = container(theme::vertical_rule(palette.border_inactive))
            .width(size::COLUMN_GAP)
            .height(Length::Fill)
            .center_x(size::COLUMN_GAP);
        match &on_resize {
            Some(pressed) => mouse_area(line)
                .on_press(pressed(column))
                .interaction(Interaction::ResizingHorizontally)
                .into(),
            None => line.into(),
        }
    };
    let cell = |title: &str, column: CommitColumn| -> Element<'a, M> {
        row![strip(column), label(title)]
            .align_y(alignment::Vertical::Center)
            .width(f32::from(columns.of(column)) + size::COLUMN_GAP)
            .into()
    };
    container(
        row![
            container(
                text(graph.to_owned())
                    .size(font_size::COLUMN_HEADER)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None)
            )
            .width(graph_width + size::COLUMN_GAP),
            label(message),
            cell(author, CommitColumn::Author),
            cell(date, CommitColumn::Date),
        ]
        .extend(show_sha.then(|| cell(sha, CommitColumn::Sha)))
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::XXL + spacing::CELL_PADDING_X])
    .height(size::COLUMN_HEADER)
    .into()
}

/// The chips and the message after them, the message cut to what is left of `width`.
fn message_cell<'a, M: 'a>(
    chips: Vec<(String, RefChipKind)>,
    message: &str,
    width: f32,
    ink: Color,
    bold: bool,
    on_chosen: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let chips_width: f32 = chips
        .iter()
        .map(|(label, kind)| {
            estimated_width(&chip_text(label, *kind), font_size::SMALL)
                + 2.0 * spacing::XS
                + spacing::XS
        })
        .sum();
    let fitted = fit_end(message, width - chips_width, font_size::BODY);
    // No empty row before the text: its gap would push the message right.
    let chip_row = (!chips.is_empty()).then(|| {
        Row::with_children(
            chips
                .into_iter()
                .map(|(label, kind)| chip(label, kind, on_chosen, palette)),
        )
        .spacing(spacing::XS)
    });
    Row::new()
        .extend(chip_row.map(Element::from))
        .push(
            text(fitted)
                .size(font_size::BODY)
                .font(if bold {
                    crate::APP_FONT_BOLD
                } else {
                    crate::APP_FONT
                })
                .color(ink)
                .wrapping(Wrapping::None),
        )
        .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}

pub fn commit_row<'a, M: Clone + 'a>(
    commit: CommitRow<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let ink = if commit.chosen {
        palette.cursor_text
    } else if commit.dimmed {
        Color {
            a: palette.text.a * DIMMED_ALPHA,
            ..palette.text
        }
    } else {
        palette.text
    };
    let dim = if commit.chosen {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let cell = |content: String, color: Color, width: u16| {
        container(
            text(content)
                .size(font_size::BODY)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .width(f32::from(width))
        .clip(true)
    };
    let message = message_cell(
        commit.chips,
        &commit.message,
        commit.message_width,
        ink,
        commit.is_head,
        commit.chosen,
        palette,
    );
    let line = row![
        commit.graph,
        container(message).width(Length::Fill).clip(true),
        cell(commit.author, dim, commit.columns.author),
        cell(commit.date, dim, commit.columns.date),
    ]
    .extend(
        commit
            .show_sha
            .then(|| Element::from(cell(commit.sha, dim, commit.columns.sha))),
    )
    .spacing(size::COLUMN_GAP)
    .align_y(alignment::Vertical::Center);
    button(
        container(line)
            .padding([0.0, spacing::CELL_PADDING_X])
            .center_y(Length::Fill),
    )
    .on_press(commit.on_press)
    .width(Length::Fill)
    .height(size::ROW)
    .padding(0.0)
    .style(git::graph_row(palette, commit.chosen, commit.is_head))
    .into()
}

#[derive(Debug)]
pub struct ChangeRow<M> {
    pub depth: usize,
    pub label: String,
    pub added: u32,
    pub removed: u32,
    /// Binary or too large: no counts.
    pub no_counts: bool,
    pub chosen: bool,
    pub on_press: M,
}

/// A changed file with its added and removed lines.
pub fn change_row<'a, M: Clone + 'a>(change: ChangeRow<M>, palette: &Palette) -> Element<'a, M> {
    let ink = if change.chosen {
        palette.cursor_text
    } else {
        palette.text
    };
    let (plus, minus) = if change.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.success, palette.error)
    };
    let counts = if change.no_counts {
        row![text("").size(font_size::SMALL)]
    } else {
        row![
            text(format!("+{}", change.added))
                .size(font_size::SMALL)
                .color(plus),
            text(format!("\u{2212}{}", change.removed))
                .size(font_size::SMALL)
                .color(minus),
        ]
        .spacing(spacing::XS)
    };
    let line = row![
        container(
            text(change.label)
                .size(font_size::BODY)
                .color(ink)
                .wrapping(Wrapping::None)
        )
        .width(Length::Fill)
        .clip(true),
        container(counts)
            .width(COUNT_WIDTH)
            .align_x(alignment::Horizontal::Right),
    ]
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center);
    button(
        container(line)
            .padding(Padding {
                left: spacing::CELL_PADDING_X + TREE_INDENT * change.depth as f32,
                right: spacing::CELL_PADDING_X,
                ..Padding::ZERO
            })
            .center_y(Length::Fill),
    )
    .on_press(change.on_press)
    .width(Length::Fill)
    .height(size::ROW)
    .padding(0.0)
    .style(theme::menu_item(palette, change.chosen))
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTone {
    Context,
    Added,
    Removed,
    /// A line of the checked-out side of a conflict.
    Mine,
    /// A line of the merged side of a conflict: the accent of the tags, not the red of a deletion.
    Theirs,
    /// The other side has no line here.
    Empty,
}

fn wash(tone: DiffTone, palette: &Palette) -> Option<Color> {
    let base = match tone {
        DiffTone::Added => palette.success,
        DiffTone::Removed => palette.error,
        DiffTone::Mine => palette.success,
        DiffTone::Theirs => palette.tagged,
        DiffTone::Context | DiffTone::Empty => return None,
    };
    Some(Color {
        a: WASH_ALPHA,
        ..base
    })
}

fn number_text<'a, M: 'a>(number: Option<u32>, palette: &Palette) -> Element<'a, M> {
    container(
        text(number.map(|count| count.to_string()).unwrap_or_default())
            .size(font_size::SMALL)
            .color(palette.dim_text),
    )
    .width(NUMBER_WIDTH)
    .padding([0.0, spacing::XS])
    .align_x(alignment::Horizontal::Right)
    .into()
}

fn washed<'a, M: 'a>(
    content: Element<'a, M>,
    tone: DiffTone,
    width: Length,
    palette: &Palette,
) -> Element<'a, M> {
    let fill = wash(tone, palette);
    container(content)
        .width(width)
        .height(size::ROW)
        .center_y(size::ROW)
        .clip(true)
        .style(move |_theme| container::Style {
            background: fill.map(Into::into),
            ..container::Style::default()
        })
        .into()
}

/// `content` cut at the end with `…` to fit `width_px` of the monospace font.
fn fit_code(content: &str, width_px: f32) -> String {
    let room = estimated_text_width(1, font_size::FILE);
    let fits = (width_px / room).floor().max(0.0) as usize;
    if content.chars().count() <= fits {
        return content.to_owned();
    }
    let mut cut: String = content.chars().take(fits.saturating_sub(1)).collect();
    if fits > 0 {
        cut.push('…');
    }
    cut
}

/// The line of a diff: it takes the room that is left and ends in `…` where it is cut.
fn code_text<'a, M: 'a>(content: String, palette: &Palette) -> Element<'a, M> {
    let ink = palette.text;
    responsive(move |bounds| {
        text(fit_code(&content, bounds.width))
            .font(crate::APP_FONT)
            .size(font_size::FILE)
            .color(ink)
            .height(Length::Fill)
            .align_y(alignment::Vertical::Center)
            .wrapping(Wrapping::None)
            .into()
    })
    .width(Length::Fill)
    .height(size::ROW)
    .into()
}

/// The column of the sign: `+` for a line that is new, `−` for one that is gone.
fn sign_text<'a, M: 'a>(tone: DiffTone, palette: &Palette) -> Element<'a, M> {
    let (sign, ink) = match tone {
        DiffTone::Added => ("+", palette.success),
        DiffTone::Removed => ("\u{2212}", palette.error),
        _ => ("", palette.dim_text),
    };
    container(
        text(sign)
            .font(crate::APP_FONT_BOLD)
            .size(font_size::FILE)
            .color(ink),
    )
    .width(SIGN_WIDTH)
    .align_x(alignment::Horizontal::Center)
    .into()
}

#[derive(Debug)]
struct Hatch {
    color: Color,
}

impl<M> canvas::Program<M> for Hatch {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let mut left = -bounds.height;
        while left < bounds.width {
            frame.stroke(
                &Path::line(
                    Point::new(left, bounds.height),
                    Point::new(left + bounds.height, 0.0),
                ),
                Stroke::default().with_color(self.color).with_width(1.0),
            );
            left += HATCH_STEP;
        }
        vec![frame.into_geometry()]
    }
}

/// The side of a diff row that has no line: hatched, so it reads as "nothing here".
fn hatched<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    Canvas::new(Hatch {
        color: Color {
            a: HATCH_ALPHA,
            ..palette.dim_text
        },
    })
    .width(Length::FillPortion(1))
    .height(size::ROW)
    .into()
}

/// One side of a side by side diff: a line number and the line.
pub fn diff_cell<'a, M: 'a>(
    number: Option<u32>,
    content: String,
    tone: DiffTone,
    palette: &Palette,
) -> Element<'a, M> {
    if tone == DiffTone::Empty {
        return hatched(palette);
    }
    washed(
        row![
            number_text(number, palette),
            sign_text(tone, palette),
            code_text(content, palette)
        ]
        .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center)
            .into(),
        tone,
        Length::FillPortion(1),
        palette,
    )
}

/// A line of a unified diff: both line numbers and the line.
pub fn diff_unified_line<'a, M: 'a>(
    old: Option<u32>,
    new: Option<u32>,
    content: String,
    tone: DiffTone,
    palette: &Palette,
) -> Element<'a, M> {
    washed(
        row![
            number_text(old, palette),
            number_text(new, palette),
            sign_text(tone, palette),
            code_text(content, palette)
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into(),
        tone,
        Length::Fill,
        palette,
    )
}

/// The line that opens a hunk: where it starts in both files.
pub fn diff_hunk_header<'a, M: 'a>(content: String, palette: &Palette) -> Element<'a, M> {
    container(
        text(content)
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(size::ROW)
    .center_y(size::ROW)
    .width(Length::Fill)
    .into()
}

/// A dim line under a diff column, such as "End of file", set where the code
/// of the lines above starts. `Fill` portion 1, so two sit side by side.
pub fn diff_note<'a, M: 'a>(content: String, palette: &Palette) -> Element<'a, M> {
    container(
        row![
            Space::new().width(NUMBER_WIDTH + SIGN_WIDTH + 2.0 * spacing::XS),
            text(content)
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .height(size::ROW)
    .center_y(size::ROW)
    .width(Length::FillPortion(1))
    .into()
}
