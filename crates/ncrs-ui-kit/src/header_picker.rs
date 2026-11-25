//! The pickers in the head of the Git tab and the repository list: a small
//! label over the bold value, and the list that opens below it.
//!
//! The closed picker is a button; its list floats below it (`list_below`).
//! The caller holds the text of the search field and which row is
//! highlighted, filters with [`filter_rows`] and shows a window of the rows
//! with [`window_rows`].

use iced::widget::{button, column, container, row, text, Column, Row};
use iced::{alignment, Element, Length};

use crate::kit::key_cap;
use crate::open_list::{window_start, OPEN_LIST_ROWS};
use crate::palette::Palette;
use crate::theme::git::{self, PickerLook};
use crate::theme::{self, font_size, size, spacing};
use crate::{APP_FONT, APP_FONT_BOLD};

const MARK_WIDTH: f32 = spacing::L;

/// The closed picker: the small `label` over the bold `value` and a triangle.
pub fn header_picker<'a, M: Clone + 'a>(
    label: &str,
    value: &str,
    look: PickerLook,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    let dim = if look == PickerLook::Open {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    button(
        column![
            text(label.to_owned())
                .size(font_size::SMALL)
                .color(dim)
                .wrapping(text::Wrapping::None),
            row![
                text(value.to_owned())
                    .size(font_size::TITLE)
                    .font(APP_FONT_BOLD)
                    .wrapping(text::Wrapping::None),
                text("\u{25be}").size(font_size::TITLE).color(dim),
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center),
        ]
        .spacing(2.0),
    )
    .on_press(on_press)
    .padding([spacing::XS, spacing::M])
    .height(size::PICKER_HEADER - spacing::L)
    .style(git::header_picker(palette, look))
    .into()
}

/// The sign between two pickers.
pub fn picker_separator<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    text("\u{203a}")
        .size(font_size::TITLE)
        .color(palette.dim_text)
        .into()
}

/// What stands before the label of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMark {
    None,
    /// The row of the current value: a tick.
    Current,
    /// The row does something instead of choosing: a plus.
    Action,
}

impl PickerMark {
    const fn sign(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Current => "\u{2713}",
            Self::Action => "+",
        }
    }
}

/// One row of the open list.
#[derive(Debug, Clone)]
pub struct PickerRow<M> {
    pub mark: PickerMark,
    pub label: String,
    /// Dim, at the right: what stands at the value, e.g. "↓2".
    pub trail: Option<String>,
    /// Orange, at the right: the key of an action.
    pub key: Option<String>,
    /// The current value is drawn bold.
    pub current: bool,
    pub highlighted: bool,
    pub on_press: M,
}

/// A line of the open list.
#[derive(Debug, Clone)]
pub enum PickerLine<M> {
    /// A small heading over a group of rows.
    Group(String),
    Row(PickerRow<M>),
    Rule,
}

/// The rows whose label contains `query`, ignoring case; the highlight goes
/// to the row at `highlighted` among them, or the last one when the filter
/// left fewer. The mark of the current value stays where it was.
pub fn filter_rows<M>(
    rows: Vec<PickerRow<M>>,
    query: &str,
    highlighted: usize,
) -> Vec<PickerRow<M>> {
    let needle = query.trim().to_lowercase();
    let mut kept: Vec<PickerRow<M>> = rows
        .into_iter()
        .filter(|row| needle.is_empty() || row.label.to_lowercase().contains(&needle))
        .collect();
    let at = highlighted.min(kept.len().saturating_sub(1));
    for (index, row) in kept.iter_mut().enumerate() {
        row.highlighted = index == at;
    }
    kept
}

/// At most [`OPEN_LIST_ROWS`] of `rows`, as near the highlighted one as the
/// ends allow.
pub fn window_rows<M>(rows: Vec<PickerRow<M>>) -> Vec<PickerRow<M>> {
    let at = rows.iter().position(|row| row.highlighted).unwrap_or(0);
    let start = window_start(rows.len(), at);
    rows.into_iter().skip(start).take(OPEN_LIST_ROWS).collect()
}

fn picker_row<'a, M: Clone + 'a>(row_data: PickerRow<M>, palette: &Palette) -> Element<'a, M> {
    let highlighted = row_data.highlighted;
    let dim = palette.dim_text;
    let label = text(row_data.label)
        .size(font_size::BODY)
        .font(if row_data.current {
            APP_FONT_BOLD
        } else {
            APP_FONT
        })
        .wrapping(text::Wrapping::None);
    let line = Row::new()
        .push(
            container(
                text(row_data.mark.sign())
                    .size(font_size::BODY)
                    .color(git::purple(palette)),
            )
            .width(MARK_WIDTH),
        )
        .push(container(label).width(Length::Fill).clip(true))
        .extend(row_data.trail.map(|trail| {
            text(trail)
                .size(font_size::SMALL)
                .color(dim)
                .wrapping(text::Wrapping::None)
                .into()
        }))
        .extend(row_data.key.map(|key| {
            text(key)
                .size(font_size::SMALL)
                .color(git::orange())
                .wrapping(text::Wrapping::None)
                .into()
        }))
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    button(line)
        .on_press(row_data.on_press)
        .width(Length::Fill)
        .height(size::ROW_TALL)
        .padding([0.0, spacing::L])
        .style(theme::menu_item(palette, highlighted))
        .into()
}

/// The open list: the `search` field on top, the `lines`, and under a rule
/// the `hints` as key caps with what the key does.
pub fn picker_list<'a, M: Clone + 'a>(
    search: Element<'a, M>,
    lines: Vec<PickerLine<M>>,
    hints: &[(&str, &str)],
    width: f32,
    palette: &Palette,
) -> Element<'a, M> {
    let body = lines.into_iter().map(|line| match line {
        PickerLine::Group(title) => container(
            text(title.to_uppercase())
                .size(font_size::SMALL)
                .color(palette.dim_text),
        )
        .padding([spacing::XS, spacing::L])
        .into(),
        PickerLine::Row(row_data) => picker_row(row_data, palette),
        PickerLine::Rule => rule(palette),
    });
    let hint_row = Row::with_children(hints.iter().flat_map(|(key, words)| {
        [
            key_cap(key, palette),
            text((*words).to_owned())
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .into(),
        ]
    }))
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center);
    container(
        column![
            container(search).padding(spacing::M),
            Column::with_children(body),
            rule(palette),
            container(hint_row).padding([spacing::XS, spacing::L]),
        ]
        .width(width),
    )
    .width(width)
    .style(theme::menu(palette))
    .into()
}

fn rule<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    container(theme::horizontal_rule(palette.border_inactive))
        .padding([spacing::XS, 0.0])
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(label: &str, current: bool) -> PickerRow<()> {
        PickerRow {
            mark: if current {
                PickerMark::Current
            } else {
                PickerMark::None
            },
            label: label.to_owned(),
            trail: None,
            key: None,
            current,
            highlighted: false,
            on_press: (),
        }
    }

    fn labels(rows: &[PickerRow<()>]) -> Vec<&str> {
        rows.iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn the_filter_matches_a_part_of_the_label_whatever_the_case() {
        let rows = vec![
            row("main", true),
            row("release/1.7.1", false),
            row("origin/MAIN", false),
        ];
        let kept = filter_rows(rows, "  Main", 0);
        assert_eq!(labels(&kept), ["main", "origin/MAIN"]);
    }

    #[test]
    fn an_empty_query_keeps_every_row() {
        let rows = vec![row("a", false), row("b", false)];
        assert_eq!(filter_rows(rows, "", 0).len(), 2);
    }

    #[test]
    fn the_highlight_stays_inside_the_filtered_rows() {
        let rows = vec![
            row("alpha", false),
            row("beta", false),
            row("gamma", false),
            row("beta2", false),
        ];
        let kept = filter_rows(rows, "beta", 3);
        assert_eq!(labels(&kept), ["beta", "beta2"]);
        assert_eq!(
            kept.iter().map(|row| row.highlighted).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn the_current_value_keeps_its_tick_after_filtering() {
        let rows = vec![row("main", true), row("feat/a", false)];
        let kept = filter_rows(rows, "ma", 0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept.first().map(|row| row.mark), Some(PickerMark::Current));
    }

    #[test]
    fn nothing_matching_leaves_no_row_and_no_highlight() {
        let rows = vec![row("main", true)];
        assert!(filter_rows(rows, "zzz", 0).is_empty());
    }

    #[test]
    fn the_window_follows_the_highlighted_row() {
        let rows: Vec<PickerRow<()>> = (0..30).map(|n| row(&n.to_string(), false)).collect();
        let kept = filter_rows(rows, "", 20);
        let shown = window_rows(kept);
        assert_eq!(shown.len(), OPEN_LIST_ROWS);
        assert!(shown.iter().any(|row| row.highlighted));
    }
}
