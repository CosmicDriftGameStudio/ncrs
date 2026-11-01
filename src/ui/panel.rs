//! File panel: the state type ([`PanelState`]) and a pure view function
//! ([`view`]). The component owns no state; it lives in the app state and is
//! only mutated from `App::update`.
use std::path::{Path, PathBuf};

use iced::mouse::ScrollDelta;
use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Column, Row, Space};
use iced::{alignment, Element, Font, Length};

use super::format;
use super::kit::{self, Icon};
use super::layout::{
    self, COLUMN_HEADER_HEIGHT, DATE_COLUMN_WIDTH, HERMES_COLUMN_WIDTH, HIT_LIST_ROW_HEIGHT,
    ROW_HEIGHT, SIZE_COLUMN_WIDTH, TAG_COLUMN_WIDTH,
};
use super::panel_history::PanelHistory;
use super::theme::{self, font_size, size, spacing};
use crate::file_list_tags::ShownTags;
use crate::fs::remote::{RemotePath, ServerId};
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

/// A flat list of search hits that stands in for the folder listing. The
/// entries are a snapshot; `scope` is the folder the search ran in and the
/// path the panel reports, `return_to` the folder the panel showed before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitList {
    pub query: String,
    pub scope: PathBuf,
    pub return_to: PathBuf,
}

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
    /// The archive file the panel is standing in right now; set from the
    /// listing that put it there.
    pub archive: Option<PathBuf>,
    /// Set while the panel lists search hits instead of a folder.
    pub hit_list: Option<HitList>,
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
            archive: None,
            hit_list: None,
        }
    }

    /// Orders the rows by `column`, or reverses them when the panel is already
    /// sorted by it. The cursor stays on the same entry and in view.
    pub fn request_sort(&mut self, column: SortColumn, visible_rows: usize) {
        self.sort = self.sort.requested(column);
        self.resort(visible_rows);
    }

    /// Sorts the rows by the current key; the cursor stays on its entry.
    pub fn resort(&mut self, visible_rows: usize) {
        let cursor_path = self.selected_entry().map(|entry| entry.path.clone());
        sort_entries(&mut self.entries, self.sort);
        let index = cursor_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .unwrap_or(self.selected);
        self.select(index, visible_rows);
    }

    pub fn is_hit_list(&self) -> bool {
        self.hit_list.is_some()
    }

    /// Height of one row: a hit list shows the folder below the name.
    pub fn row_height(&self) -> f32 {
        if self.is_hit_list() {
            HIT_LIST_ROW_HEIGHT
        } else {
            ROW_HEIGHT
        }
    }

    /// Rows this panel shows where a folder shows `folder_rows`. Every method
    /// that takes `visible_rows` expects the folder count and narrows it here,
    /// so a caller cannot forget the taller rows of a hit list.
    pub fn rows_in_view(&self, folder_rows: usize) -> usize {
        if self.is_hit_list() {
            layout::hit_list_rows(folder_rows)
        } else {
            folder_rows.max(1)
        }
    }

    /// Turns the panel into an empty hit list that is being read. The request
    /// id moves on, so a directory read still in flight cannot replace it.
    pub fn begin_hit_list(&mut self, list: HitList) -> u64 {
        self.path.clone_from(&list.scope);
        self.hit_list = Some(list);
        self.archive = None;
        self.entries.clear();
        self.selection.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.scroll_remainder = 0.0;
        self.error = None;
        self.begin_load()
    }

    /// Leaves the hit list without a visit in the history: the panel counts as
    /// standing in the folder it came from.
    pub fn drop_hit_list(&mut self) -> Option<HitList> {
        let list = self.hit_list.take()?;
        self.path.clone_from(&list.return_to);
        Some(list)
    }

    /// Replaces the snapshot with `entries` (freshly read), keeping the sort,
    /// the cursor and the tags, all by full path.
    pub fn apply_hit_entries(&mut self, mut entries: Vec<FileEntry>, visible_rows: usize) {
        sort_entries(&mut entries, self.sort);
        let cursor_path = self.selected_entry().map(|entry| entry.path.clone());
        let previous_index = self.selected;
        self.selection.retain_present(&entries);
        self.entries = entries;
        self.loading = false;
        let index = cursor_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .unwrap_or(previous_index);
        self.select(index, visible_rows);
    }

    /// Takes the rows at `paths` out of the list; the files are untouched.
    pub fn remove_hits(&mut self, paths: &[PathBuf], visible_rows: usize) {
        self.entries.retain(|entry| !paths.contains(&entry.path));
        self.selection.retain_present(&self.entries);
        self.select(self.selected, visible_rows);
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    /// The server the panel shows a folder of. Told from the text of the path,
    /// never from the disk.
    pub fn remote(&self) -> Option<ServerId> {
        RemotePath::parse(&self.path).map(|place| place.server().clone())
    }

    /// The panel shows something that is not a folder on this computer: the
    /// inside of an archive, or a folder on a server.
    pub fn is_virtual(&self) -> bool {
        self.archive.is_some() || self.remote().is_some()
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
            ScrollDelta::Pixels { y, .. } => -y / self.row_height(),
        } + self.scroll_remainder;
        let whole = rows_down.trunc();
        self.scroll_remainder = rows_down - whole;
        self.scroll_rows(whole as isize, visible_rows);
    }

    fn scroll_rows(&mut self, rows_down: isize, visible_rows: usize) {
        let rows = self.rows_in_view(visible_rows);
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
        let rows = self.rows_in_view(visible_rows);
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

/// What the pointer does over a panel: the wheel, and entering and leaving.
#[derive(Debug)]
pub struct PanelPointer<M, S> {
    pub scrolled: S,
    pub entered: M,
    pub exited: M,
}

/// Renders a file panel. Generic over the message type: the caller decides
/// which message a row click produces, keeping the component reusable.
/// Renders one panel.
///
/// `on_row_click` is called with the row index and what the click did. One
/// callback rather than one per kind, because a row is one clickable thing
/// with several meanings. `on_row_context` is called with the row index when
/// the row is right-clicked.
pub fn view<'a, M: Clone + 'a>(
    state: &'a PanelState,
    props: PanelProps<'a>,
    lang: Language,
    on_row_click: impl Fn(usize, RowClick) -> M,
    on_row_context: impl Fn(usize) -> M,
    on_sort: impl Fn(SortColumn) -> M + 'a,
    pointer: PanelPointer<M, impl Fn(ScrollDelta) -> M + 'a>,
) -> Element<'a, M> {
    let palette = &props.palette;
    let hermes = props.hermes;
    let rows = state
        .entries
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(state.rows_in_view(props.visible_rows))
        .map(|(index, entry)| {
            let mark = state
                .selection
                .state_of(index, &state.entries, state.selected);
            // Two targets per row, as in NC: the star itself toggles the tag,
            // the rest of the row moves the cursor. iced gives a button one
            // message and no click position, so the star has to be its own
            // button rather than a region of the row button.
            let row = Row::new().push(tag_star(
                mark,
                props.is_active,
                palette,
                state.row_height(),
                on_row_click(index, RowClick::Tag),
            ));
            // A button takes only the left button, so the right press reaches
            // the area around it.
            mouse_area(file_row(
                entry,
                mark,
                props.is_active,
                palette,
                |click| on_row_click(index, click),
                row,
                RowExtras {
                    third_line: None,
                    hermes: hermes.map(|column| HermesCell::of(entry, column, lang, palette)),
                    hit_scope: state
                        .hit_list
                        .as_ref()
                        .map(|list| (list.scope.as_path(), state.row_height())),
                },
            ))
            .on_right_press(on_row_context(index))
            .into()
        });

    // The wheel belongs to the panel under the pointer. The rows are drawn from
    // the window `scroll_offset` selects, not by a `scrollable`, so the panel
    // has to ask for the wheel itself.
    mouse_area(
        column![
            column_header(
                lang,
                palette,
                Some(state.sort),
                hermes.is_some(),
                move |column| Some(on_sort(column)),
            ),
            Column::with_children(rows).height(Length::Fill),
        ]
        .apply_frame(palette),
    )
    .on_scroll(pointer.scrolled)
    .on_enter(pointer.entered)
    .on_exit(pointer.exited)
    .into()
}

/// The column titles, each a click target that sorts by its column; the sorted
/// one carries an arrow for the direction.
fn column_header<'a, M: Clone + 'a>(
    lang: Language,
    palette: &Palette,
    sort: Option<SortKey>,
    with_hermes: bool,
    on_sort: impl Fn(SortColumn) -> Option<M> + 'a,
) -> Element<'a, M> {
    let title = |column: SortColumn, msg: Msg| {
        let arrow = match sort.filter(|key| key.column == column) {
            None => "",
            Some(key) if key.descending => " ▼",
            Some(_) => " ▲",
        };
        let label = mouse_area(
            text(format!("{}{arrow}", lang.text(msg)))
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None),
        );
        match on_sort(column) {
            Some(pressed) => label
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(pressed),
            None => label,
        }
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

/// The column titles of a list that a click does not sort.
pub fn plain_column_header<'a, M: Clone + 'a>(lang: Language, palette: &Palette) -> Element<'a, M> {
    column_header(lang, palette, None, false, |_| None)
}

/// The star column of a row. Its own button so a click there tags the row
/// instead of moving the cursor.
pub fn tag_star<'a, M: Clone + 'a>(
    mark: Selection,
    panel_active: bool,
    palette: &Palette,
    row_height: f32,
    pressed: M,
) -> Element<'a, M> {
    button(
        text(if mark.tagged { "*" } else { " " })
            .size(font_size::FILE)
            .font(Font::MONOSPACE)
            .color(if mark.tagged {
                theme::tag_star_color(palette, mark.cursor && panel_active, panel_active)
            } else {
                palette.dim_text
            })
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fixed(TAG_COLUMN_WIDTH))
    .height(row_height)
    .style(theme::tag_button(palette, mark.tagged))
    .on_press(pressed)
    .into()
}

/// A file found by a search, drawn as a row of the hit list: the name, the
/// folder below `scope`, size and date, and optionally a third line.
pub struct HitRow<'a, M> {
    pub entry: FileEntry,
    pub mark: Selection,
    pub scope: &'a Path,
    pub height: f32,
    pub third_line: Option<Element<'a, M>>,
}

pub fn hit_row<'a, M: Clone + 'a>(
    hit: HitRow<'a, M>,
    panel_active: bool,
    palette: &Palette,
    on_click: impl Fn(RowClick) -> M,
    prefix: Row<'a, M>,
) -> Element<'a, M> {
    let HitRow {
        entry,
        mark,
        scope,
        height,
        third_line,
    } = hit;
    file_row(
        &entry,
        mark,
        panel_active,
        palette,
        on_click,
        prefix,
        RowExtras {
            hermes: None,
            hit_scope: Some((scope, height)),
            third_line,
        },
    )
}

/// What a click on a row of the panel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowClick {
    /// The star in the tag column: toggles the tag.
    Tag,
    /// The rest of the row: moves the cursor there.
    Cursor,
    /// A double click on the row: opens it like Enter.
    Open,
}

/// What only some rows have: the filing cell, and for a hit list the folder
/// the scope is measured from with the row height.
struct RowExtras<'a, M> {
    hermes: Option<HermesCell<'a, M>>,
    hit_scope: Option<(&'a Path, f32)>,
    /// Under the folder of a hit.
    third_line: Option<Element<'a, M>>,
}

/// One file row: the name, size and date, clickable as a whole.
///
/// `prefix` is the tag button, built by the caller because it needs its own
/// message; the row itself is one button around the rest.
fn file_row<'a, M: Clone + 'a>(
    entry: &FileEntry,
    mark: Selection,
    panel_active: bool,
    palette: &Palette,
    on_click: impl Fn(RowClick) -> M,
    prefix: Row<'a, M>,
    extras: RowExtras<'a, M>,
) -> Element<'a, M> {
    let RowExtras {
        hermes,
        hit_scope,
        third_line,
    } = extras;
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

    let fitted = hermes.as_ref().map(|cell_of| {
        HermesCell::<M>::fit_tags(cell_of.tags(), cell_of.panel_width(), name.chars().count())
    });
    let shown_name = match hermes.as_ref() {
        Some(cell_of) if cell_of.has_tags() => {
            format::ellipsize_middle(&name, HermesCell::<M>::name_room(cell_of.panel_width()))
        }
        _ => name,
    };
    let name_cell = row![cell(shown_name)]
        .extend(hermes.as_ref().and_then(HermesCell::role_tag))
        .extend(
            hermes
                .as_ref()
                .zip(fitted.as_ref())
                .into_iter()
                .flat_map(|(cell_of, tags)| cell_of.tag_chips(tags, highlighted)),
        )
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    let name_column: Element<'a, M> = match hit_scope {
        Some((scope, _)) => column![
            name_cell,
            text(format::folder_below(&entry.path, scope))
                .size(font_size::SMALL)
                .font(Font::MONOSPACE)
                .color(if highlighted { color } else { palette.dim_text })
                .wrapping(text::Wrapping::None),
        ]
        .into(),
        None => name_cell.into(),
    };
    let name_element: Element<'a, M> = container(name_column).width(Length::Fill).clip(true).into();
    let size_cell: Element<'a, M> = cell(format::entry_size(entry))
        .width(SIZE_COLUMN_WIDTH)
        .align_x(alignment::Horizontal::Right)
        .into();
    let date_cell: Element<'a, M> = cell(format::time(entry.modified))
        .width(DATE_COLUMN_WIDTH)
        .align_x(alignment::Horizontal::Right)
        .into();
    // The third line runs under size and date too, beside the tag column.
    let mut content = match third_line {
        Some(third) => row![
            prefix,
            column![
                row![name_element, size_cell, date_cell].align_y(alignment::Vertical::Center),
                container(third).width(Length::Fill).clip(true),
            ]
            .width(Length::Fill),
        ],
        None => row![prefix, name_element, size_cell, date_cell],
    }
    .align_y(alignment::Vertical::Center);
    if let Some(status_cell) = hermes {
        content = content.push(status_cell.status());
    }

    // A button captures the press, so the double click is read inside it: the
    // area takes both presses and the button only draws the row.
    let clicks = mouse_area(content.width(Length::Fill))
        .on_press(on_click(RowClick::Cursor))
        .on_double_click(on_click(RowClick::Open));
    button(clicks)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(hit_scope.map_or(ROW_HEIGHT, |(_, height)| height))
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
/// Room kept free beside the cells of a row, for borders and rounding.
const NAME_SAFETY_MARGIN: f32 = 4.0;
/// A name is never cut shorter than this, whatever the chips need.
const MIN_NAME_CHARS: f32 = 8.0;
const NOT_COUNTED: &str = "…";

/// What the Hermes column shows for one row. Built from lookups only, so
/// drawing a row never reads the index.
struct HermesCell<'a, M> {
    content: HermesContent,
    role: Option<FolderRole>,
    lang: Language,
    palette: Palette,
    angle: f32,
    tags: ShownTags,
    panel_width: f32,
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
            tags: if is_folder || entry.is_dir {
                ShownTags::default()
            } else {
                column.tags_after(&entry.path)
            },
            panel_width: column.panel_width(),
            _message: std::marker::PhantomData,
        }
    }

    fn tags(&self) -> &ShownTags {
        &self.tags
    }

    fn has_tags(&self) -> bool {
        !self.tags.chips.is_empty() || self.tags.more > 0
    }

    const fn panel_width(&self) -> f32 {
        self.panel_width
    }

    /// How many characters of the name fit in the row. The font of the name
    /// is monospaced, so this is exact enough; the row clips what is left.
    fn name_room(panel_width: f32) -> usize {
        (Self::room_beside_name(panel_width) / Self::name_char_width()).max(MIN_NAME_CHARS) as usize
    }

    const fn name_char_width() -> f32 {
        font_size::FILE * DIGIT_WIDTH_PER_SIZE
    }

    fn room_beside_name(panel_width: f32) -> f32 {
        let fixed = TAG_COLUMN_WIDTH
            + SIZE_COLUMN_WIDTH
            + DATE_COLUMN_WIDTH
            + HERMES_COLUMN_WIDTH
            + 2.0 * spacing::CELL_PADDING_X
            + NAME_SAFETY_MARGIN;
        panel_width - fixed
    }

    fn chip_width(label: &str) -> f32 {
        format::estimated_width(label, ROLE_TAG_FONT)
            + 2.0 * (spacing::XS + spacing::BORDER_WIDTH)
            + spacing::XS
    }

    /// The chips that fit behind a name of `name_chars` characters, in order.
    /// The name keeps its width; the chips left out are counted in `more`, and
    /// the count shows only when it fits as well.
    fn fit_tags(tags: &ShownTags, panel_width: f32, name_chars: usize) -> ShownTags {
        let left =
            Self::room_beside_name(panel_width) - name_chars as f32 * Self::name_char_width();
        let total = tags.chips.len() + tags.more;
        let fits = |kept: usize| {
            let chips: f32 = tags
                .chips
                .iter()
                .take(kept)
                .map(|chip| Self::chip_width(&chip.tag))
                .sum();
            let count = if total > kept {
                Self::chip_width(&format!("+{}", total - kept))
            } else {
                0.0
            };
            chips + count <= left
        };
        let fitting = (0..=tags.chips.len()).rev().find(|count| fits(*count));
        match fitting {
            Some(kept) => ShownTags {
                chips: tags.chips.iter().take(kept).cloned().collect(),
                more: total - kept,
            },
            None => ShownTags::default(),
        }
    }

    /// The tags of the file after its name, small, the ones the user set with
    /// an outline in the accent color.
    fn tag_chips(&self, tags: &ShownTags, on_fill: bool) -> Vec<Element<'a, M>> {
        let mut chips: Vec<Element<'a, M>> = tags
            .chips
            .iter()
            .map(|chip| {
                let label = text(chip.tag.clone())
                    .size(ROLE_TAG_FONT)
                    .wrapping(text::Wrapping::None);
                let boxed = container(label).padding([0.0, spacing::XS]);
                if chip.own {
                    boxed.style(theme::own_chip(&self.palette)).into()
                } else {
                    boxed.style(theme::chip(&self.palette, on_fill)).into()
                }
            })
            .collect();
        if tags.more > 0 {
            chips.push(
                text(format!("+{}", tags.more))
                    .size(ROLE_TAG_FONT)
                    .color(self.palette.dim_text)
                    .wrapping(text::Wrapping::None)
                    .into(),
            );
        }
        chips
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
            HermesFileState::Unread => kit::icon(Icon::Clock, color),
            HermesFileState::Planned => kit::icon(Icon::CircleArrowRight, color),
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
                folder_size: None,
            })
            .collect();
        p
    }

    fn chips_of(labels: &[&str], more: usize) -> ShownTags {
        ShownTags {
            chips: labels
                .iter()
                .map(|label| crate::file_list_tags::FileChip {
                    tag: (*label).to_owned(),
                    own: false,
                })
                .collect(),
            more,
        }
    }

    #[test]
    fn a_short_name_keeps_all_its_chips() {
        let tags = chips_of(&["invoice", "telekom", "2026"], 0);
        let fitted = HermesCell::<()>::fit_tags(&tags, 1200.0, 10);
        assert_eq!(fitted, tags);
    }

    #[test]
    fn a_long_name_keeps_its_text_and_the_chips_turn_into_a_count() {
        let tags = chips_of(&["invoice", "telekom", "2026"], 0);
        let room = HermesCell::<()>::name_room(600.0);
        let fitted = HermesCell::<()>::fit_tags(&tags, 600.0, room - 6);
        assert!(fitted.chips.len() < 3);
        assert_eq!(fitted.chips.len() + fitted.more, 3);
        assert!(fitted.more > 0);
    }

    #[test]
    fn a_name_wider_than_the_room_has_no_chips() {
        let tags = chips_of(&["invoice", "2026"], 1);
        let room = HermesCell::<()>::name_room(600.0);
        let fitted = HermesCell::<()>::fit_tags(&tags, 600.0, room + 5);
        assert_eq!(fitted, ShownTags::default());
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
    use std::path::{Path, PathBuf};

    fn entry(name: &str, is_parent: bool) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir: true,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
            folder_size: None,
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
        let name = p.entries[1].path.clone();
        p.selection.toggle(&name);

        assert!(p.selection.is_tagged(&name));
        assert!(!p.selection.is_tagged(&p.entries[2].path));
    }

    /// `..` is not taggable: copying a directory into itself is not a thing
    /// anyone means, and the file manager should not offer it.
    #[test]
    fn the_parent_entry_is_not_tagged() {
        let mut p = panel_with(&[("..", true), ("a", false)]);
        p.selection.tag_all(&p.entries);
        assert!(!p.selection.is_tagged(&p.entries[0].path));
        assert!(p.selection.is_tagged(&p.entries[1].path));
    }

    /// The point of tags being names rather than positions: a reload that
    /// reorders the rows keeps the tag on the same file. With indices, a tag on
    /// "beta" would silently become a tag on whatever took position 2 — and F8
    /// would delete that.
    #[test]
    fn a_reload_keeps_the_tag_on_the_same_file() {
        let mut p = panel_with(&[("..", true), ("alpha", false), ("beta", false)]);
        p.selection.toggle(&p.entries[2].path.clone());

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
            p.selection.is_tagged(Path::new("beta")),
            "the tag followed the position instead of the file"
        );
        assert_eq!(p.selection.len(), 1, "more than one row ended up tagged");

        // And the file that was tagged is still the only tagged one: an
        // index-based set would have moved the tag onto "..'s neighbour.
        let tagged: Vec<String> = p
            .entries
            .iter()
            .filter(|e| p.selection.is_tagged(&e.path))
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(tagged, vec!["beta".to_string()]);
    }

    /// A tag whose file is gone is dropped rather than left pointing at
    /// whatever took the name.
    #[test]
    fn tags_for_vanished_files_are_dropped() {
        let mut p = panel_with(&[("..", true), ("a", false), ("b", false), ("c", false)]);
        p.selection.toggle(&p.entries[3].path.clone());
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
        p.selection.toggle(Path::new("a"));

        p.apply_listing(
            PathBuf::from("/tmp"),
            vec![entry("..", true), entry("a", false), entry("b", false)],
            None,
            None,
            10,
        );

        assert!(p.selection.is_tagged(Path::new("a")));
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
    use std::path::Path;
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
            folder_size: None,
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
        panel.selection.toggle(Path::new("/d/b"));
        panel.request_sort(SortColumn::Size, 5);
        panel.request_sort(SortColumn::Size, 5);
        assert!(panel.selection.is_tagged(Path::new("/d/b")));
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
