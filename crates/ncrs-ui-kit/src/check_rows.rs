//! Rows with a tick, for lists the person picks from before something is
//! deleted: a group head with a tick for the whole group, a row with tick, name
//! and reason, and the dimmed row of what stays and why.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, rich_text, row, span, text, Space};
use iced::{alignment, Color, Element, Length, Renderer, Theme};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::file_icon::git_branch_icon;
use crate::format::ellipsize_middle;
use crate::kit::{self, icon, Check, Icon};
use crate::palette::Palette;
use crate::theme::git::{self, faint};
use crate::theme::{self, font_size, size, spacing};
use crate::APP_FONT_BOLD;

/// A row with a second line (the folder of a worktree).
const TWO_LINES: f32 = 44.0;
/// The cell of the tick; a row that stays leaves it empty so the names line up.
const TICK_CELL: f32 = 28.0;
/// The cell of the branch symbol.
const GLYPH_CELL: f32 = 18.0;
/// What fits under the name in the narrowest list the rows are used in.
const DETAIL_CHARS: usize = 52;
/// The room of the reason, at the right of the name.
const REASON_WIDTH: f32 = 240.0;
/// What a row of a group is moved to the right of its head.
const GROUP_INDENT: f32 = 18.0;

/// How many ignored files are named; the rest is a number.
const NAMED_FILES: usize = 2;

/// `.env, local.toml and 3 more`.
pub fn named_files(files: &[String], lang: Language) -> String {
    let shown: Vec<&str> = files.iter().take(NAMED_FILES).map(String::as_str).collect();
    let names = shown.join(", ");
    match files.len().saturating_sub(NAMED_FILES) {
        0 => names,
        more => fill_template(
            lang.text(Msg::GitCleanupIgnoredMore),
            &[("{count}", &more.to_string()), ("{names}", &names)],
        ),
    }
}

/// Where a worktree lives; with ignored files the folder alone, and them.
pub fn worktree_detail(path: &Path, ignored_files: &[String], lang: Language) -> String {
    let folder = match path.file_name() {
        Some(name) if !ignored_files.is_empty() => name.to_string_lossy(),
        _ => return path.display().to_string(),
    };
    fill_template(
        lang.text(Msg::GitCleanupWorktreeIgnored),
        &[
            ("{folder}", &folder),
            ("{files}", &named_files(ignored_files, lang)),
        ],
    )
}

/// What became of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOutcome {
    /// A green chip with this word in the place of the reason.
    Done(String),
    /// A red chip with this reason in the place of the reason.
    Failed(String),
}

/// One entry of the list.
pub struct CheckRow<M> {
    pub check: Check,
    pub name: String,
    /// A second line under the name: where a worktree lives.
    pub detail: Option<String>,
    /// Why it is offered, or why it stays.
    pub reason: String,
    /// A worktree: shown with the branch symbol and a second line.
    pub worktree: bool,
    /// Never offered: no tick, the row is dimmed and `reason` says why it stays.
    pub kept: bool,
    /// The row the keys are on.
    pub cursor: bool,
    /// A click on the row ticks or unticks it.
    pub on_toggle: Option<M>,
    pub outcome: Option<RowOutcome>,
}

impl<M> std::fmt::Debug for CheckRow<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckRow")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// The head of a group of entries.
#[derive(Debug)]
pub struct CheckGroupHead<M> {
    pub title: String,
    /// The folder of the repository, or what is inside while the group is shut.
    pub subtitle: String,
    /// How many entries the group offers.
    pub count: usize,
    pub open: bool,
    pub check: Check,
    /// The row the keys are on.
    pub cursor: bool,
    pub on_toggle_open: M,
    pub on_toggle_check: M,
}

fn line<'a, M: 'a>(content: String, size: f32, ink: Color) -> Element<'a, M> {
    text(content)
        .size(size)
        .color(ink)
        .wrapping(Wrapping::None)
        .into()
}

fn inks(palette: &Palette, cursor: bool) -> (Color, Color) {
    if cursor {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    }
}

/// An icon and a word in one ink: what became of an entry.
fn chip<'a, M: 'a>(sign: Icon, words: String, ink: Color) -> Element<'a, M> {
    row![icon(sign, ink), line(words, font_size::SMALL, ink)]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into()
}

pub fn check_row<'a, M: Clone + 'a>(item: CheckRow<M>, palette: &Palette) -> Element<'a, M> {
    let (text_ink, dim_ink) = inks(palette, item.cursor);
    let name_ink = if item.kept { faint(text_ink) } else { text_ink };
    let tick: Element<'a, M> = match (&item.outcome, item.kept) {
        (_, true) => Space::new().into(),
        (Some(RowOutcome::Done(_)), false) => icon(Icon::CircleCheck, palette.success),
        (Some(RowOutcome::Failed(_)), false) => icon(Icon::CircleX, palette.error),
        (None, false) => kit::checkbox(item.check, None, palette),
    };
    let is_done = matches!(item.outcome, Some(RowOutcome::Done(_)));
    let glyph: Element<'a, M> = if item.worktree {
        git_branch_icon(dim_ink, size::ICON)
    } else {
        Space::new().into()
    };
    let reason_ink = if item.kept { faint(dim_ink) } else { dim_ink };
    let slot: Element<'a, M> = match item.outcome {
        Some(RowOutcome::Done(done)) => chip(Icon::CircleCheck, done, palette.success),
        Some(RowOutcome::Failed(failure)) => chip(Icon::CircleX, failure, palette.error),
        None => line(item.reason, font_size::SMALL, reason_ink),
    };
    // A finished row steps back: dimmed, and the name struck through.
    let name_cell: Element<'a, M> = if is_done {
        let dimmed = faint(name_ink);
        rich_text::<(), M, Theme, Renderer>([span(item.name).color(dimmed).strikethrough(true)])
            .size(font_size::BODY)
            .wrapping(Wrapping::None)
            .into()
    } else {
        line(item.name, font_size::BODY, name_ink)
    };
    let name_line = row![
        container(glyph).width(GLYPH_CELL),
        container(name_cell).width(Length::Fill).clip(true),
        container(slot)
            .width(REASON_WIDTH)
            .align_x(alignment::Horizontal::Right)
            .clip(true),
    ]
    .align_y(alignment::Vertical::Center);
    let second = item.detail.map(|place| {
        let shown_place = ellipsize_middle(&place, DETAIL_CHARS);
        (format!("\u{25a2} {shown_place}"), dim_ink)
    });
    let mut lines = column![name_line].spacing(spacing::XS);
    let mut height = size::CHECK_ROW;
    if let Some((content, ink)) = second {
        lines = lines.push(row![
            Space::new().width(GLYPH_CELL),
            line(content, font_size::SMALL, ink),
        ]);
        height = TWO_LINES;
    }
    let cells = row![
        Space::new().width(GROUP_INDENT),
        container(tick).width(TICK_CELL),
        container(lines).width(Length::Fill).clip(true),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let framed = container(cells)
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        .height(height)
        .align_y(alignment::Vertical::Center)
        .style(git::marked_row(palette, item.cursor, false));
    match item.on_toggle.filter(|_| !item.kept) {
        Some(message) => mouse_area(framed).on_press(message).into(),
        None => framed.into(),
    }
}

pub fn check_group_head<'a, M: Clone + 'a>(
    head: CheckGroupHead<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let (text_ink, dim_ink) = inks(palette, head.cursor);
    let arrow = if head.open { "\u{25be}" } else { "\u{25b8}" };
    let count_ink = if head.cursor {
        palette.cursor_text
    } else {
        git::purple(palette)
    };
    let cells = row![
        line(arrow.to_owned(), font_size::SMALL, dim_ink),
        text(head.title)
            .size(font_size::BODY)
            .font(APP_FONT_BOLD)
            .color(text_ink)
            .wrapping(Wrapping::None),
        container(line(head.subtitle, font_size::SMALL, dim_ink))
            .width(Length::Fill)
            .clip(true),
        text(head.count.to_string())
            .size(font_size::BODY)
            .font(APP_FONT_BOLD)
            .color(count_ink)
            .wrapping(Wrapping::None),
        kit::checkbox(head.check, Some(head.on_toggle_check), palette),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let framed = container(cells)
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        .height(size::CHECK_ROW)
        .align_y(alignment::Vertical::Center)
        .style(git::marked_row(palette, head.cursor, false));
    mouse_area(framed).on_press(head.on_toggle_open).into()
}

/// The "×" at the right of the head of a pane; without a message it is inert.
pub fn close_cross<'a, M: Clone + 'a>(on_press: Option<M>, palette: &Palette) -> Element<'a, M> {
    const SIDE: f32 = 22.0;
    button(container(text("\u{d7}").size(font_size::TITLE)).center(Length::Fill))
        .padding(0.0)
        .width(SIDE)
        .height(SIDE)
        .on_press_maybe(on_press)
        .style(theme::close_button(palette))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_and_a_head_build_for_every_state() {
        let palette = crate::palette::ThemePreset::ALL[0].palette();
        for kept in [false, true] {
            for cursor in [false, true] {
                let _: Element<'_, ()> = check_row(
                    CheckRow {
                        check: Check::On,
                        name: "fix/x".to_owned(),
                        detail: kept.then(|| "~/code/x".to_owned()),
                        reason: "in main".to_owned(),
                        worktree: kept,
                        kept,
                        cursor,
                        on_toggle: Some(()),
                        outcome: None,
                    },
                    &palette,
                );
            }
        }
        for check in [Check::Off, Check::On, Check::Mixed] {
            let _: Element<'_, ()> = check_group_head(
                CheckGroupHead {
                    title: "infra".to_owned(),
                    subtitle: "~/code/infra".to_owned(),
                    count: 5,
                    open: true,
                    check,
                    cursor: false,
                    on_toggle_open: (),
                    on_toggle_check: (),
                },
                &palette,
            );
        }
    }
}
