//! The rows of the Git view: a commit with its graph, ref chips, and a changed
//! file with its line counts, and the lines of a diff.

use iced::mouse::Interaction;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, row, text, Row, Space};
use iced::{alignment, Border, Color, Element, Length, Padding};

use crate::format::{estimated_width, fit_end};
use crate::git_mark::MODIFIED_ORANGE;
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};
use crate::tree_list::{LABEL_LEAD, TREE_INDENT};

const NUMBER_WIDTH: f32 = 44.0;
const COUNT_WIDTH: f32 = 90.0;
const WASH_ALPHA: f32 = 0.18;
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
    Branch,
    Remote,
    Tag,
}

fn chip_color(kind: RefChipKind, palette: &Palette) -> Color {
    match kind {
        RefChipKind::Branch => palette.accent,
        RefChipKind::Remote => palette.dim_text,
        RefChipKind::Tag => palette.success,
    }
}

/// A branch, remote branch or tag name on a commit.
pub fn ref_chip<'a, M: 'a>(label: String, kind: RefChipKind, palette: &Palette) -> Element<'a, M> {
    container(
        text(label)
            .size(font_size::SMALL)
            .color(chip_color(kind, palette))
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::XS])
    .style(theme::chip(palette, false))
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
    labels: [&str; 4],
    columns: CommitColumns,
    show_sha: bool,
    on_resize: Option<Box<dyn Fn(CommitColumn) -> M + 'a>>,
    palette: &Palette,
) -> Element<'a, M> {
    let [message, author, date, sha] = labels;
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
            container("").width(graph_width + size::COLUMN_GAP),
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
    palette: &Palette,
) -> Element<'a, M> {
    let chips_width: f32 = chips
        .iter()
        .map(|(label, _)| {
            estimated_width(label, font_size::SMALL) + 2.0 * spacing::XS + spacing::XS
        })
        .sum();
    let fitted = fit_end(message, width - chips_width, font_size::BODY);
    let chip_row = Row::with_children(
        chips
            .into_iter()
            .map(|(label, kind)| ref_chip(label, kind, palette)),
    )
    .spacing(spacing::XS);
    row![
        chip_row,
        text(fitted)
            .size(font_size::BODY)
            .color(ink)
            .wrapping(Wrapping::None)
    ]
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
    .style(theme::menu_item(palette, commit.chosen))
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
    /// The other side has no line here.
    Empty,
}

fn wash(tone: DiffTone, palette: &Palette) -> Option<Color> {
    let base = match tone {
        DiffTone::Added => palette.success,
        DiffTone::Removed => palette.error,
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

fn code_text<'a, M: 'a>(content: String, palette: &Palette) -> Element<'a, M> {
    text(content)
        .font(crate::APP_FONT)
        .size(font_size::FILE)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

/// One side of a side by side diff: a line number and the line.
pub fn diff_cell<'a, M: 'a>(
    number: Option<u32>,
    content: String,
    tone: DiffTone,
    palette: &Palette,
) -> Element<'a, M> {
    washed(
        row![number_text(number, palette), code_text(content, palette)]
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

/// Room of a worktree row: the name and, under it, the branch.
pub const WORKTREE_ROW_HEIGHT: f32 = 44.0;

/// A worktree in the sidebar of the Git view.
#[derive(Debug)]
pub struct WorktreeRow<M> {
    pub depth: usize,
    pub name: String,
    /// "WIP 7": set while the worktree has unsaved changes.
    pub wip: Option<String>,
    /// Commits ahead of and behind the main branch; `None` for the main
    /// worktree itself.
    pub distance: Option<(u32, u32)>,
    /// The branch and what is to say about it, in the dim ink.
    pub detail: String,
    pub chosen: bool,
    pub on_press: M,
}

/// The name, a chip with the unsaved changes and the distance to main on one
/// line, the branch and its state dimmed under it.
pub fn worktree_row<'a, M: Clone + 'a>(
    worktree: WorktreeRow<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let (ink, dim) = if worktree.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let behind_ink = |behind: u32| {
        if worktree.chosen {
            palette.cursor_text
        } else if behind == 0 {
            palette.dim_text
        } else {
            MODIFIED_ORANGE
        }
    };
    let chip_ink = if worktree.chosen {
        palette.cursor_text
    } else {
        MODIFIED_ORANGE
    };
    let small = |content: String, color: Color| {
        text(content)
            .size(font_size::SMALL)
            .color(color)
            .wrapping(Wrapping::None)
    };
    let first = row![text(worktree.name)
        .size(font_size::BODY)
        .color(ink)
        .wrapping(Wrapping::None),]
    .extend(worktree.wip.map(|wip| {
        container(small(wip, chip_ink))
            .padding([0.0, spacing::XS])
            .style(move |_theme| container::Style {
                border: Border {
                    color: chip_ink,
                    width: spacing::BORDER_WIDTH,
                    radius: theme::radius::CORNER.into(),
                },
                ..container::Style::default()
            })
            .into()
    }))
    .push(container("").width(Length::Fill))
    .extend(worktree.distance.map(|(ahead, behind)| {
        row![
            small(
                format!("\u{2191}{ahead}"),
                if worktree.chosen {
                    palette.cursor_text
                } else {
                    palette.success
                }
            ),
            small(format!("\u{2193}{behind}"), behind_ink(behind)),
        ]
        .spacing(spacing::XS)
        .into()
    }))
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center);
    let content = container(column![first, small(worktree.detail, dim)].spacing(2.0))
        .padding(Padding {
            left: spacing::CELL_PADDING_X + LABEL_LEAD + TREE_INDENT * worktree.depth as f32,
            right: spacing::CELL_PADDING_X,
            ..Padding::ZERO
        })
        .clip(true)
        .center_y(Length::Fill);
    button(content)
        .on_press(worktree.on_press)
        .width(Length::Fill)
        .height(WORKTREE_ROW_HEIGHT)
        .padding(0.0)
        .style(theme::menu_item(palette, worktree.chosen))
        .into()
}
