//! Rows with a tick, for lists the person picks from before something is
//! deleted: a group head with a tick for the whole group, a row with tick, name
//! and reason, and the dimmed row of what stays and why.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{alignment, Color, Element, Length};

use crate::file_icon::git_branch_icon;
use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme::git::{self, faint};
use crate::theme::{font_size, size, spacing};
use crate::APP_FONT_BOLD;

/// A row with a second line (the folder of a worktree).
const TWO_LINES: f32 = 44.0;
/// The cell of the tick; a row that stays leaves it empty so the names line up.
const TICK_CELL: f32 = 28.0;
/// The cell of the branch symbol.
const GLYPH_CELL: f32 = 18.0;
/// The room of the reason, at the right of the name.
const REASON_WIDTH: f32 = 210.0;
/// What a row of a group is moved to the right of its head.
const GROUP_INDENT: f32 = 18.0;

/// One entry of the list.
pub struct CheckRow<'a, M> {
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
    /// What stands at the very right: the result of the row, say.
    pub trailing: Option<Element<'a, M>>,
}

impl<M> std::fmt::Debug for CheckRow<'_, M> {
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

pub fn check_row<'a, M: Clone + 'a>(item: CheckRow<'a, M>, palette: &Palette) -> Element<'a, M> {
    let (text_ink, dim_ink) = inks(palette, item.cursor);
    let name_ink = if item.kept { faint(text_ink) } else { text_ink };
    let tick: Element<'a, M> = if item.kept {
        Space::new().into()
    } else {
        kit::checkbox(item.check, None, palette)
    };
    let glyph: Element<'a, M> = if item.worktree {
        git_branch_icon(dim_ink, size::ICON)
    } else {
        Space::new().into()
    };
    let name_line = row![
        container(glyph).width(GLYPH_CELL),
        line(item.name, font_size::BODY, name_ink)
    ]
    .align_y(alignment::Vertical::Center);
    let mut names = column![name_line].spacing(spacing::XS);
    let height = match item.detail {
        Some(place) => {
            names = names.push(row![
                Space::new().width(GLYPH_CELL),
                line(format!("\u{25a2} {place}"), font_size::SMALL, dim_ink),
            ]);
            TWO_LINES
        }
        None => size::CHECK_ROW,
    };
    let reason_ink = if item.kept { faint(dim_ink) } else { dim_ink };
    let mut cells = row![
        Space::new().width(GROUP_INDENT),
        container(tick).width(TICK_CELL),
        container(names).width(Length::Fill).clip(true),
        container(line(item.reason, font_size::SMALL, reason_ink))
            .width(REASON_WIDTH)
            .align_x(alignment::Horizontal::Right)
            .clip(true),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(extra) = item.trailing {
        cells = cells.push(extra);
    }
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
                        trailing: None,
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
