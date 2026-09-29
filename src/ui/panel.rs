//! File panel: the state type ([`PanelState`]) and a pure view function
//! ([`view`]). The component owns no state; it lives in the app state and is
//! only mutated from `App::update`.

use std::path::PathBuf;

use iced::widget::{button, column, container, row, text, Column};
use iced::{alignment, Element, Font, Length};

use super::format;
use super::layout::{
    COLUMN_HEADER_HEIGHT, DATE_COLUMN_WIDTH, PANEL_TITLE_HEIGHT, ROW_HEIGHT, SIZE_COLUMN_WIDTH,
};
use super::theme::{self, colors, font_size, spacing};
use crate::fs::{FileEntry, ReadError};
use crate::i18n::{Language, Msg};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PanelState {
    /// Current directory.
    pub path: PathBuf,
    /// Directory contents (`..` first when not at the root).
    pub entries: Vec<FileEntry>,
    /// Index of the highlighted row.
    pub selected: usize,
    /// Index of the first visible row.
    pub scroll_offset: usize,
    /// Last read error, if any. Rendered by the status bar in the UI language.
    pub error: Option<ReadError>,
    /// A directory read is in flight.
    pub loading: bool,
    /// Id of the latest load request; older results are ignored.
    pub request_id: u64,
}

impl PanelState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            error: None,
            loading: false,
            request_id: 0,
        }
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    /// Marks a new load as started and returns its request id.
    pub fn begin_load(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.loading = true;
        self.request_id
    }

    /// Replaces the content with a freshly loaded listing.
    pub fn apply_listing(
        &mut self,
        path: PathBuf,
        entries: Vec<FileEntry>,
        error: Option<ReadError>,
        select: Option<&str>,
        visible_rows: usize,
    ) {
        self.path = path;
        self.entries = entries;
        self.error = error;
        self.loading = false;
        self.scroll_offset = 0;
        let index = select
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .unwrap_or(0);
        self.select(index, visible_rows);
    }

    /// Moves the selection by `delta` rows, clamped to the list bounds.
    pub fn move_selection(&mut self, delta: isize, visible_rows: usize) {
        let target = self.selected.saturating_add_signed(delta);
        self.select(target, visible_rows);
    }

    pub fn select_last(&mut self, visible_rows: usize) {
        self.select(usize::MAX, visible_rows);
    }

    /// Selects `index` (clamped) and scrolls it into view.
    pub fn select(&mut self, index: usize, visible_rows: usize) {
        self.selected = index.min(self.entries.len().saturating_sub(1));
        self.ensure_visible(visible_rows);
    }

    /// Adjusts `scroll_offset` so the selected row is visible.
    pub fn ensure_visible(&mut self, visible_rows: usize) {
        let rows = visible_rows.max(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + rows {
            self.scroll_offset = self.selected + 1 - rows;
        }
        let max_offset = self.entries.len().saturating_sub(rows);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

/// Display options for a panel that are not part of its state.
#[derive(Debug, Clone, Copy)]
pub struct PanelProps {
    pub is_active: bool,
    pub visible_rows: usize,
}

/// Renders a file panel. Generic over the message type: the caller decides
/// which message a row click produces, keeping the component reusable.
pub fn view<'a, M: Clone + 'a>(
    state: &'a PanelState,
    props: PanelProps,
    lang: Language,
    on_row_click: impl Fn(usize) -> M,
) -> Element<'a, M> {
    let rows = state
        .entries
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(props.visible_rows)
        .map(|(index, entry)| {
            file_row(
                entry,
                index == state.selected,
                props.is_active,
                on_row_click(index),
            )
        });

    column![
        title_bar(state, props.is_active),
        column_header(lang),
        Column::with_children(rows).height(Length::Fill),
    ]
    .apply_frame(props.is_active)
}

fn title_bar<'a, M: 'a>(state: &'a PanelState, active: bool) -> Element<'a, M> {
    let mut title = state.path.display().to_string();
    if state.loading {
        title.push_str("  …");
    }
    container(
        text(title)
            .size(font_size::TITLE)
            .wrapping(text::Wrapping::None),
    )
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(PANEL_TITLE_HEIGHT)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .clip(true)
    .style(theme::panel_title(active))
    .into()
}

fn column_header<'a, M: 'a>(lang: Language) -> Element<'a, M> {
    let label = |s: &'static str| text(s).size(font_size::COLUMN_HEADER).color(colors::ACCENT);
    container(
        row![
            label(lang.text(Msg::ColumnName)).width(Length::Fill),
            label(lang.text(Msg::ColumnSize))
                .width(SIZE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
            label(lang.text(Msg::ColumnModified))
                .width(DATE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(COLUMN_HEADER_HEIGHT)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}

fn file_row<'a, M: Clone + 'a>(
    entry: &'a FileEntry,
    selected: bool,
    panel_active: bool,
    on_press: M,
) -> Element<'a, M> {
    let color = theme::row_text_color(entry.is_dir, selected, panel_active);
    let font = if entry.is_dir || (selected && panel_active) {
        Font {
            weight: iced::font::Weight::Bold,
            ..Font::MONOSPACE
        }
    } else {
        Font::MONOSPACE
    };

    let name = if entry.is_dir && !entry.is_parent {
        format!("/{}", entry.name)
    } else if entry.is_symlink {
        format!("~{}", entry.name)
    } else {
        entry.name.clone()
    };

    let cell = |content: String| {
        text(content)
            .size(font_size::ROW)
            .font(font)
            .color(color)
            .wrapping(text::Wrapping::None)
    };

    let content = row![
        container(cell(name)).width(Length::Fill).clip(true),
        cell(format::entry_size(entry))
            .width(SIZE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
        cell(format::time(entry.modified))
            .width(DATE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
    ]
    .align_y(alignment::Vertical::Center);

    button(content)
        .on_press(on_press)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(ROW_HEIGHT)
        .width(Length::Fill)
        .style(theme::row(selected, panel_active))
        .into()
}

/// Small extension to wrap the panel content in its styled frame.
trait ApplyFrame<'a, M> {
    fn apply_frame(self, active: bool) -> Element<'a, M>;
}

impl<'a, M: 'a> ApplyFrame<'a, M> for Column<'a, M> {
    fn apply_frame(self, active: bool) -> Element<'a, M> {
        container(self)
            .padding(spacing::BORDER_WIDTH)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::panel(active))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel_with(n: usize) -> PanelState {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = (0..n)
            .map(|i| FileEntry {
                name: format!("f{i}"),
                path: PathBuf::from(format!("/f{i}")),
                is_dir: false,
                is_symlink: false,
                is_parent: false,
                size: 0,
                modified: None,
            })
            .collect();
        p
    }

    #[test]
    fn selection_is_clamped_and_scrolls() {
        let mut p = panel_with(20);
        p.move_selection(-5, 5);
        assert_eq!((p.selected, p.scroll_offset), (0, 0));
        p.move_selection(7, 5);
        assert_eq!((p.selected, p.scroll_offset), (7, 3));
        p.select_last(5);
        assert_eq!((p.selected, p.scroll_offset), (19, 15));
        p.select(2, 5);
        assert_eq!((p.selected, p.scroll_offset), (2, 2));
    }

    #[test]
    fn empty_panel_does_not_panic() {
        let mut p = panel_with(0);
        p.move_selection(3, 5);
        p.select_last(5);
        assert_eq!((p.selected, p.scroll_offset), (0, 0));
        assert!(p.selected_entry().is_none());
    }
}
