//! The state of a file panel ([`PanelState`]): the rows, the cursor, the tags,
//! the sort, the history. The view is built from it in the app.
use std::path::PathBuf;

use iced::mouse::ScrollDelta;
use ncrs_core::{FileEntry, SelectionSet};
use ncrs_fs::remote::{RemotePath, ServerId};
use ncrs_fs::{sort_entries, ReadError, SortColumn, SortKey};
use ncrs_ui_kit::layout::{self, HIT_LIST_ROW_HEIGHT, ROW_HEIGHT};

use crate::hit_list::HitList;
use crate::panel_history::PanelHistory;

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

    /// The load that was just started is a Back or Forward step: arriving by it
    /// is not a new visit, so it must not clear Forward.
    pub fn mark_history_step(&mut self) {
        self.history_step_request = Some(self.request_id);
    }

    /// Marks a new load as started and returns its request id.
    pub fn begin_load(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.loading = true;
        self.request_id
    }

    /// Replaces the content with a listing in no particular order.
    #[cfg(any(test, feature = "test-support"))]
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
