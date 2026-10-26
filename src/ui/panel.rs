//! File panel: the state type ([`PanelState`]) and a pure view function
//! ([`view`]). The component owns no state; it lives in the app state and is
//! only mutated from `App::update`.
use std::path::PathBuf;

use iced::mouse::ScrollDelta;
use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Column, Row, Space};
use iced::{alignment, Element, Font, Length};

use super::format;
use super::kit::{self, Icon};
use super::layout::{
    COLUMN_HEADER_HEIGHT, DATE_COLUMN_WIDTH, HERMES_COLUMN_WIDTH, ROW_HEIGHT, SIZE_COLUMN_WIDTH,
    TAG_COLUMN_WIDTH,
};
use super::panel_history::PanelHistory;
use super::theme::{self, font_size, size, spacing};
use crate::fs::{sort_entries, FileEntry, ReadError, SortColumn, SortKey};
use crate::hermes_column::{
    abbreviated, role_message, state_label, summary_items, ColumnView, FolderRole, FolderSummary,
    HermesFileState,
};
use crate::i18n::{Language, Msg};
use crate::palette::Palette;
use crate::selection::{Selection, SelectionSet};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PanelState {
    /// Current directory.
    pub path: PathBuf,
    /// Directory contents (`..` first when not at the root).
    pub entries: Vec<FileEntry>,
    /// Index of the row the cursor is on. One per panel.
    pub selected: usize,
    /// Rows marked with Insert. An operation applies to these, or to the cursor
    /// row when nothing is tagged.
    pub selection: SelectionSet,
    /// Index of the first visible row.
    pub scroll_offset: usize,
    /// Last read error, if any. Rendered by the status bar in the UI language.
    pub error: Option<ReadError>,
    /// A directory read is in flight.
    pub loading: bool,
    /// Id of the latest load request; older results are ignored.
    pub request_id: u64,
    /// Trackpad pixels scrolled that do not yet add up to a whole row, in the
    /// direction of the list (down is positive).
    pub scroll_remainder: f32,
    /// How the rows are ordered. Belongs to the panel, so it outlives a reload
    /// and a change of directory.
    pub sort: SortKey,
    /// The folders this panel was in before, for Back and Forward.
    pub history: PanelHistory,
    /// The load a Back or Forward step started: arriving by it is not a new
    /// visit, so it must not clear Forward.
    pub history_step_request: Option<u64>,
}

impl PanelState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            selected: 0,
            selection: SelectionSet::new(),
            scroll_offset: 0,
            error: None,
            loading: false,
            request_id: 0,
            scroll_remainder: 0.0,
            sort: SortKey::default(),
            history: PanelHistory::default(),
            history_step_request: None,
        }
    }

    /// Orders the rows by `column`, or reverses them when the panel is already
    /// sorted by it. The cursor stays on the same entry and in view.
    pub fn request_sort(&mut self, column: SortColumn, visible_rows: usize) {
        self.sort = self.sort.requested(column);
        let cursor_name = self.selected_entry().map(|entry| entry.name.clone());
        sort_entries(&mut self.entries, self.sort);
        let index = cursor_name
            .and_then(|name| self.entries.iter().position(|entry| entry.name == name))
            .unwrap_or(self.selected);
        self.select(index, visible_rows);
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    /// Records the move from `previous` in the history once the listing of
    /// load `request_id` is in. A reload of the same folder and the first load
    /// are not visits, and neither is the arrival of a Back or Forward step,
    /// which moved the stacks itself.
    pub fn record_arrival(&mut self, previous: PathBuf, request_id: u64) {
        if self.path != previous && self.history_step_request != Some(request_id) {
            self.history.visit(previous);
        }
        self.history_step_request = None;
    }

    /// Marks a new load as started and returns its request id.
    pub fn begin_load(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.loading = true;
        self.request_id
    }

    /// Replaces the content with a listing in no particular order.
    #[cfg(test)]
    pub fn apply_listing(
        &mut self,
        path: PathBuf,
        entries: Vec<FileEntry>,
        error: Option<ReadError>,
        select: Option<&str>,
        visible_rows: usize,
    ) {
        self.apply_sorted_listing(path, entries, error, select, visible_rows, None);
    }

    /// Replaces the content with a freshly loaded listing that the reader
    /// sorted by `sorted_by`. It is sorted again only when the panel's own key
    /// has changed since the read began.
    pub fn apply_sorted_listing(
        &mut self,
        path: PathBuf,
        mut entries: Vec<FileEntry>,
        error: Option<ReadError>,
        select: Option<&str>,
        visible_rows: usize,
        sorted_by: Option<SortKey>,
    ) {
        if sorted_by != Some(self.sort) {
            sort_entries(&mut entries, self.sort);
        }
        let same_directory = self.path == path;
        self.path = path;
        // Read the name under the cursor *before* the rows are replaced. After
        // `self.entries = entries` this would read the new list and remember the
        // file at that position, not the file the cursor was on.
        let previous_index = self.selected;
        let previous: Option<String> = self
            .entries
            .get(self.selected)
            .map(|e| e.name.to_string_lossy().into_owned());

        // A reload replaces the rows. A tag whose name is still there stays; one
        // whose file is gone is dropped — the safe direction, since a forgotten
        // file in a copy is worse than a tag the user has to set again.
        self.selection.retain_present(&entries);
        self.entries = entries;
        self.error = error;
        self.loading = false;

        // A reload must not move the cursor. Falling back to row 0 threw the
        // user back to the top of the list after every file operation, and
        // scrolled the tagged rows out of sight. Prefer, in order: the name the
        // caller asked for, the name that was under the cursor, then — in the
        // same directory, when that file is gone — the row now at the same
        // position, which is the next file (`select` clamps to the last). The
        // scroll offset stays too, so the list does not re-centre. Another
        // directory starts at the top.
        let fallback = if same_directory { previous_index } else { 0 };
        if !same_directory {
            self.scroll_offset = 0;
        }
        let wanted = select.map(str::to_string).or(previous);
        let index = wanted
            .as_deref()
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .unwrap_or(fallback);
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

    /// Scrolls the window by a wheel or trackpad movement, in the direction iced
    /// reports it (positive `y` is towards the top of the list).
    ///
    /// Trackpad pixels are collected until they make a whole row, so a slow
    /// gesture of many small events still moves; the wheel's lines count as
    /// rows. The cursor stays where it is while it is in view and is otherwise
    /// held at the edge, so the highlighted row never leaves the screen.
    pub fn scroll(&mut self, delta: ScrollDelta, visible_rows: usize) {
        let rows_down = match delta {
            ScrollDelta::Lines { y, .. } => -y,
            ScrollDelta::Pixels { y, .. } => -y / ROW_HEIGHT,
        } + self.scroll_remainder;
        let whole = rows_down.trunc();
        self.scroll_remainder = rows_down - whole;
        self.scroll_rows(whole as isize, visible_rows);
    }

    fn scroll_rows(&mut self, rows_down: isize, visible_rows: usize) {
        let rows = visible_rows.max(1);
        let max_offset = self.entries.len().saturating_sub(rows);
        self.scroll_offset = self
            .scroll_offset
            .saturating_add_signed(rows_down)
            .min(max_offset);
        // At either end the rest of the gesture is spent, not saved up.
        let at_top = self.scroll_offset == 0 && rows_down < 0;
        let at_bottom = self.scroll_offset == max_offset && rows_down > 0;
        if at_top || at_bottom {
            self.scroll_remainder = 0.0;
        }
        let last_visible = self.scroll_offset + rows - 1;
        self.selected = self
            .selected
            .clamp(self.scroll_offset, last_visible)
            .min(self.entries.len().saturating_sub(1));
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
pub struct PanelProps<'a> {
    pub is_active: bool,
    pub visible_rows: usize,
    pub palette: Palette,
    /// `None` leaves the Hermes column out.
    pub hermes: Option<ColumnView<'a>>,
}

/// Renders a file panel. Generic over the message type: the caller decides
/// which message a row click produces, keeping the component reusable.
/// Renders one panel.
///
/// `on_row_click` is called with the row index and whether the click landed in
/// the tag column: the star toggles the tag, the rest of the row moves the
/// cursor. One callback with a flag rather than two callbacks, because a row is
/// one clickable thing with two meanings. `on_row_context` is called with the
/// row index when the row is right-clicked.
pub fn view<'a, M: Clone + 'a>(
    state: &'a PanelState,
    props: PanelProps<'a>,
    lang: Language,
    on_row_click: impl Fn(usize, bool) -> M,
    on_row_context: impl Fn(usize) -> M,
    on_sort: impl Fn(SortColumn) -> M + 'a,
    on_scroll: impl Fn(ScrollDelta) -> M + 'a,
) -> Element<'a, M> {
    let palette = &props.palette;
    let hermes = props.hermes;
    let rows = state
        .entries
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(props.visible_rows)
        .map(|(index, entry)| {
            let mark = state
                .selection
                .state_of(index, &state.entries, state.selected);
            // Two targets per row, as in NC: the star itself toggles the tag,
            // the rest of the row moves the cursor. iced gives a button one
            // message and no click position, so the star has to be its own
            // button rather than a region of the row button.
            let mut row = Row::new();
            row = row.push(
                button(
                    text(if mark.tagged { "*" } else { " " })
                        .size(font_size::FILE)
                        .font(Font::MONOSPACE)
                        .color(if mark.tagged {
                            theme::tag_star_color(
                                palette,
                                mark.cursor && props.is_active,
                                props.is_active,
                            )
                        } else {
                            palette.dim_text
                        })
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .width(Length::Fixed(TAG_COLUMN_WIDTH))
                .height(ROW_HEIGHT)
                .style(theme::tag_button(palette, mark.tagged))
                .on_press(on_row_click(index, true)),
            );
            // A button takes only the left button, so the right press reaches
            // the area around it.
            mouse_area(file_row(
                entry,
                mark,
                props.is_active,
                palette,
                on_row_click(index, false),
                row,
                hermes.map(|column| HermesCell::of(entry, column, lang, palette)),
            ))
            .on_right_press(on_row_context(index))
            .into()
        });

    // The wheel belongs to the panel under the pointer. The rows are drawn from
    // the window `scroll_offset` selects, not by a `scrollable`, so the panel
    // has to ask for the wheel itself.
    mouse_area(
        column![
            column_header(lang, palette, state.sort, hermes.is_some(), on_sort),
            Column::with_children(rows).height(Length::Fill),
        ]
        .apply_frame(palette),
    )
    .on_scroll(on_scroll)
    .into()
}

/// The column titles, each a click target that sorts by its column; the sorted
/// one carries an arrow for the direction.
fn column_header<'a, M: Clone + 'a>(
    lang: Language,
    palette: &Palette,
    sort: SortKey,
    with_hermes: bool,
    on_sort: impl Fn(SortColumn) -> M + 'a,
) -> Element<'a, M> {
    let title = |column: SortColumn, msg: Msg| {
        let arrow = match (sort.column == column, sort.descending) {
            (false, _) => "",
            (true, false) => " ▲",
            (true, true) => " ▼",
        };
        mouse_area(
            text(format!("{}{arrow}", lang.text(msg)))
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None),
        )
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(on_sort(column))
    };
    column![
        container(
            row![
                container(title(SortColumn::Name, Msg::ColumnName))
                    .padding(iced::Padding::ZERO.left(TAG_COLUMN_WIDTH))
                    .width(Length::Fill),
                container(title(SortColumn::Size, Msg::ColumnSize))
                    .width(SIZE_COLUMN_WIDTH)
                    .align_x(alignment::Horizontal::Right),
                container(title(SortColumn::Modified, Msg::ColumnModified))
                    .width(DATE_COLUMN_WIDTH)
                    .align_x(alignment::Horizontal::Right),
            ]
            .extend(with_hermes.then(|| {
                container(
                    text(lang.text(Msg::ColumnHermes))
                        .size(font_size::SMALL)
                        .color(palette.dim_text)
                        .wrapping(text::Wrapping::None),
                )
                .padding(iced::Padding::ZERO.left(HERMES_CELL_GAP))
                .width(HERMES_COLUMN_WIDTH)
                .into()
            }))
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .height(COLUMN_HEADER_HEIGHT)
    .width(Length::Fill)
    .into()
}

/// One file row: the name, size and date, clickable as a whole.
///
/// `prefix` is the tag button, built by the caller because it needs its own
/// message; the row itself is one button around the rest.
fn file_row<'a, M: Clone + 'a>(
    entry: &'a FileEntry,
    mark: Selection,
    panel_active: bool,
    palette: &Palette,
    on_press: M,
    prefix: Row<'a, M>,
    hermes: Option<HermesCell<'a, M>>,
) -> Element<'a, M> {
    // The cursor row of the active panel is the one drawn as highlighted; a
    // tagged row is marked in its own column and tinted by the row style.
    let highlighted = mark.cursor && panel_active;
    let color = theme::row_text_color(
        palette,
        entry.is_dir,
        highlighted,
        mark.tagged,
        panel_active,
    );
    let font = if entry.is_dir || (highlighted && panel_active) {
        Font {
            weight: iced::font::Weight::Bold,
            ..Font::MONOSPACE
        }
    } else {
        Font::MONOSPACE
    };

    // Display only. The identity is `entry.name` as the filesystem spells it;
    // this is the lossy form, computed here and thrown away. A name that is not
    // valid UTF-8 shows as U+FFFD, which is what the platform's own file
    // managers do.
    let name = if entry.is_dir && !entry.is_parent {
        format!("/{}", entry.name.to_string_lossy())
    } else if entry.is_symlink {
        format!("~{}", entry.name.to_string_lossy())
    } else {
        entry.name.to_string_lossy().into_owned()
    };

    let cell = |content: String| {
        text(content)
            .size(font_size::FILE)
            .font(font)
            .color(color)
            .wrapping(text::Wrapping::None)
    };

    let name_cell = row![cell(name)]
        .extend(hermes.as_ref().and_then(HermesCell::role_tag))
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    let mut content = row![
        prefix,
        container(name_cell).width(Length::Fill).clip(true),
        cell(format::entry_size(entry))
            .width(SIZE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
        cell(format::time(entry.modified))
            .width(DATE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
    ]
    .align_y(alignment::Vertical::Center);
    if let Some(status_cell) = hermes {
        content = content.push(status_cell.status());
    }

    button(content)
        .on_press(on_press)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(ROW_HEIGHT)
        .width(Length::Fill)
        .style(theme::row(palette, mark.tagged, mark.cursor, panel_active))
        .into()
}

/// Space between the date and the Hermes column.
const HERMES_CELL_GAP: f32 = 12.0;
/// Width of a digit in the small font, in font sizes, to see what fits.
const DIGIT_WIDTH_PER_SIZE: f32 = 0.6;
const SUMMARY_GAP: f32 = 6.0;
const ICON_COUNT_GAP: f32 = 2.0;
const ROLE_TAG_FONT: f32 = 10.0;
const NOT_COUNTED: &str = "…";

/// What the Hermes column shows for one row. Built from lookups only, so
/// drawing a row never reads the index.
struct HermesCell<'a, M> {
    content: HermesContent,
    role: Option<FolderRole>,
    lang: Language,
    palette: Palette,
    angle: f32,
    _message: std::marker::PhantomData<&'a M>,
}

enum HermesContent {
    None,
    File(HermesFileState),
    Folder(Option<FolderSummary>),
}

impl<'a, M: 'a> HermesCell<'a, M> {
    fn of(entry: &FileEntry, column: ColumnView<'a>, lang: Language, palette: &Palette) -> Self {
        let is_folder = entry.is_dir && !entry.is_parent;
        let content = if is_folder {
            HermesContent::Folder(column.summary_of(&entry.path).cloned())
        } else if entry.is_dir {
            HermesContent::None
        } else {
            column
                .state_of(&entry.path)
                .map_or(HermesContent::None, HermesContent::File)
        };
        Self {
            content,
            role: is_folder.then(|| column.role_of(&entry.path)).flatten(),
            lang,
            palette: *palette,
            angle: column.angle(),
            _message: std::marker::PhantomData,
        }
    }

    /// A small outlined label after a folder's name.
    fn role_tag(&self) -> Option<Element<'a, M>> {
        let role = self.role?;
        let color = theme::hermes_role_color(&self.palette, role);
        let tag = container(
            text(self.lang.text(role_message(role)))
                .size(ROLE_TAG_FONT)
                .color(color)
                .wrapping(text::Wrapping::None),
        )
        .padding([0.0, spacing::XS])
        .style(theme::outlined_in(color));
        Some(match role {
            FolderRole::Archive => hint(
                tag.into(),
                self.lang
                    .text(Msg::HermesColumnRoleArchiveTooltip)
                    .to_owned(),
                &self.palette,
            ),
            FolderRole::Inbox | FolderRole::FilingLocation => tag.into(),
        })
    }

    /// The trailing cell, always [`HERMES_COLUMN_WIDTH`] wide and cut at its
    /// edge, so it never moves the size and date columns.
    fn status(self) -> Element<'a, M> {
        let inner: Element<'a, M> = match &self.content {
            HermesContent::None => Space::new().into(),
            HermesContent::File(state) => self.state_icon(*state, true),
            HermesContent::Folder(summary) => self.summary(summary.as_ref()),
        };
        container(inner)
            .padding(iced::Padding::ZERO.left(HERMES_CELL_GAP))
            .width(HERMES_COLUMN_WIDTH)
            .clip(true)
            .into()
    }

    fn state_icon(&self, state: HermesFileState, with_hint: bool) -> Element<'a, M> {
        let color = theme::hermes_state_color(&self.palette, state);
        let glyph = match state {
            HermesFileState::Waiting => kit::icon(Icon::Clock, color),
            HermesFileState::Reading => kit::icon_rotated(Icon::Loader, color, self.angle),
            HermesFileState::Filed => kit::icon(Icon::CircleCheck, color),
            HermesFileState::Inquiry => kit::icon(Icon::CircleHelp, color),
            HermesFileState::Error => kit::icon(Icon::TriangleAlert, color),
            HermesFileState::Excluded => kit::icon(Icon::Ban, color),
        };
        if with_hint {
            hint(
                glyph,
                state_label(state, self.lang).to_owned(),
                &self.palette,
            )
        } else {
            glyph
        }
    }

    fn summary(&self, counted: Option<&FolderSummary>) -> Element<'a, M> {
        let small = |content: String, color| {
            text(content)
                .size(font_size::SMALL)
                .color(color)
                .wrapping(text::Wrapping::None)
        };
        let Some(summary) = counted else {
            return hint(
                small(NOT_COUNTED.to_owned(), self.palette.dim_text).into(),
                self.lang.text(Msg::HermesColumnNotCounted).to_owned(),
                &self.palette,
            );
        };
        if summary.excluded {
            let color = theme::hermes_state_color(&self.palette, HermesFileState::Excluded);
            return row![
                self.state_icon(HermesFileState::Excluded, false),
                small(
                    self.lang.text(Msg::HermesColumnExcludedWord).to_owned(),
                    color
                ),
            ]
            .spacing(ICON_COUNT_GAP)
            .align_y(alignment::Vertical::Center)
            .into();
        }
        let items = summary_items(summary);
        if items.is_empty() {
            return Space::new().into();
        }
        let room = HERMES_COLUMN_WIDTH - HERMES_CELL_GAP;
        let ellipsis_width = font_size::SMALL * DIGIT_WIDTH_PER_SIZE + SUMMARY_GAP;
        let mut used = 0.0;
        let mut shown = Row::new()
            .spacing(SUMMARY_GAP)
            .align_y(alignment::Vertical::Center);
        let total = items.len();
        for (index, item) in items.iter().enumerate() {
            let count = abbreviated(item.count);
            let width = size::ICON
                + ICON_COUNT_GAP
                + count.chars().count() as f32 * font_size::SMALL * DIGIT_WIDTH_PER_SIZE
                + SUMMARY_GAP;
            let reserve = if index + 1 < total {
                ellipsis_width
            } else {
                0.0
            };
            if used + width + reserve > room {
                shown = shown.push(small(NOT_COUNTED.to_owned(), self.palette.dim_text));
                break;
            }
            used += width;
            let color = theme::hermes_state_color(&self.palette, item.state);
            shown = shown.push(hint(
                row![self.state_icon(item.state, false), small(count, color)]
                    .spacing(ICON_COUNT_GAP)
                    .align_y(alignment::Vertical::Center)
                    .into(),
                state_label(item.state, self.lang).to_owned(),
                &self.palette,
            ));
        }
        shown.into()
    }
}

/// `content` with `label` in a small box on hover.
fn hint<'a, M: 'a>(content: Element<'a, M>, label: String, palette: &Palette) -> Element<'a, M> {
    tooltip(
        content,
        container(
            text(label)
                .size(font_size::SMALL)
                .wrapping(text::Wrapping::None),
        )
        .padding([2.0, 6.0])
        .style(theme::dialog(palette)),
        tooltip::Position::Top,
    )
    .into()
}

/// Small extension to wrap the panel content in its styled frame.
trait ApplyFrame<'a, M> {
    fn apply_frame(self, palette: &Palette) -> Element<'a, M>;
}

impl<'a, M: 'a> ApplyFrame<'a, M> for Column<'a, M> {
    fn apply_frame(self, palette: &Palette) -> Element<'a, M> {
        container(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::panel(palette))
            .into()
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn panel_with(n: usize) -> PanelState {
        let mut p = PanelState::new(PathBuf::from("/"));
        p.entries = (0..n)
            .map(|i| FileEntry {
                name: format!("f{i}").into(),
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

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tagging_tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str, is_parent: bool) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir: true,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
        }
    }

    fn panel_with(names: &[(&str, bool)]) -> PanelState {
        let mut p = PanelState::new(PathBuf::from("/tmp"));
        p.entries = names.iter().map(|(n, parent)| entry(n, *parent)).collect();
        p
    }

    /// Tagging is per panel, and survives navigation within that panel.
    #[test]
    fn tags_live_on_the_panel() {
        let mut p = panel_with(&[("..", true), ("a", false), ("b", false)]);
        let name = p.entries[1].name.clone();
        p.selection.toggle(&name);

        assert!(p.selection.is_tagged(&name));
        assert!(!p.selection.is_tagged(&p.entries[2].name));
    }

    /// `..` is not taggable: copying a directory into itself is not a thing
    /// anyone means, and the file manager should not offer it.
    #[test]
    fn the_parent_entry_is_not_tagged() {
        let mut p = panel_with(&[("..", true), ("a", false)]);
        p.selection.tag_all(&p.entries);
        assert!(!p.selection.is_tagged(&p.entries[0].name));
        assert!(p.selection.is_tagged(&p.entries[1].name));
    }

    /// The point of tags being names rather than positions: a reload that
    /// reorders the rows keeps the tag on the same file. With indices, a tag on
    /// "beta" would silently become a tag on whatever took position 2 — and F8
    /// would delete that.
    #[test]
    fn a_reload_keeps_the_tag_on_the_same_file() {
        let mut p = panel_with(&[("..", true), ("alpha", false), ("beta", false)]);
        p.selection.toggle(&p.entries[2].name.clone());

        // Same files, new order, one file gone.
        p.apply_listing(
            PathBuf::from("/tmp"),
            vec![
                entry("..", true),
                entry("beta", false),
                entry("gamma", false),
            ],
            None,
            None,
            10,
        );

        assert!(
            p.selection.is_tagged(&std::ffi::OsString::from("beta")),
            "the tag followed the position instead of the file"
        );
        assert_eq!(p.selection.len(), 1, "more than one row ended up tagged");

        // And the file that was tagged is still the only tagged one: an
        // index-based set would have moved the tag onto "..'s neighbour.
        let tagged: Vec<String> = p
            .entries
            .iter()
            .filter(|e| p.selection.is_tagged(&e.name))
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(tagged, vec!["beta".to_string()]);
    }

    /// A tag whose file is gone is dropped rather than left pointing at
    /// whatever took the name.
    #[test]
    fn tags_for_vanished_files_are_dropped() {
        let mut p = panel_with(&[("..", true), ("a", false), ("b", false), ("c", false)]);
        p.selection.toggle(&p.entries[3].name.clone());
        assert_eq!(p.selection.len(), 1);

        p.apply_listing(
            PathBuf::from("/tmp"),
            vec![entry("..", true)],
            None,
            None,
            10,
        );

        assert!(p.selection.is_empty(), "a tag for a deleted file survived");
    }

    /// A tag inside the new listing survives, so tagging one file and
    /// refreshing the panel does not silently drop the selection.
    #[test]
    fn tags_inside_the_new_listing_survive() {
        let mut p = panel_with(&[("..", true), ("a", false), ("b", false)]);
        p.selection.toggle(&std::ffi::OsString::from("a"));

        p.apply_listing(
            PathBuf::from("/tmp"),
            vec![entry("..", true), entry("a", false), entry("b", false)],
            None,
            None,
            10,
        );

        assert!(p.selection.is_tagged(&std::ffi::OsString::from("a")));
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod sort_tests {
    use super::*;
    use std::time::SystemTime;

    fn file(name: &str, size: u64) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from("/d").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size,
            modified: Some(SystemTime::UNIX_EPOCH),
        }
    }

    fn files() -> Vec<FileEntry> {
        vec![file("a", 30), file("b", 10), file("c", 20)]
    }

    fn loaded(rows: usize) -> PanelState {
        let mut panel = PanelState::new(PathBuf::from("/d"));
        panel.apply_listing(PathBuf::from("/d"), files(), None, None, rows);
        panel
    }

    fn names(panel: &PanelState) -> Vec<String> {
        panel
            .entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_cursor_follows_its_entry_and_stays_in_view() {
        let mut panel = loaded(2);
        panel.select(0, 2);
        panel.request_sort(SortColumn::Size, 2);
        assert_eq!(names(&panel), ["b", "c", "a"]);
        assert_eq!(panel.selected_entry().unwrap().name, "a");
        assert_eq!(panel.selected, 2);
        assert!(panel.scroll_offset <= panel.selected && panel.selected < panel.scroll_offset + 2);
    }

    #[test]
    fn tags_survive_a_sort() {
        let mut panel = loaded(5);
        panel.selection.toggle(&"b".into());
        panel.request_sort(SortColumn::Size, 5);
        panel.request_sort(SortColumn::Size, 5);
        assert!(panel.selection.is_tagged(&"b".into()));
    }

    #[test]
    fn the_sort_outlives_a_reload_and_a_change_of_directory() {
        let mut panel = loaded(5);
        panel.request_sort(SortColumn::Size, 5);
        panel.apply_listing(PathBuf::from("/d"), files(), None, None, 5);
        assert_eq!(names(&panel), ["b", "c", "a"]);
        panel.apply_listing(PathBuf::from("/e"), files(), None, None, 5);
        assert_eq!(names(&panel), ["b", "c", "a"]);
    }
}
