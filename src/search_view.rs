//! The state of the search view that takes the place of a panel: the fields,
//! the results as they stream in, and the cursor in them. No widgets and no
//! tasks; `App` runs the engine and `ui::search` draws this.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use iced::mouse::ScrollDelta;

use crate::fs::search::{LineMatch, SearchError, SearchEvent, SearchQuery, SearchTotals};
use crate::ui::layout::ROW_HEIGHT;

/// The three text fields of the search view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Query,
    Include,
    Exclude,
}

impl SearchField {
    /// The field Alt+F7 moves to when the search is already up.
    pub fn next(self) -> Self {
        match self {
            Self::Query => Self::Include,
            Self::Include => Self::Exclude,
            Self::Exclude => Self::Query,
        }
    }
}

/// A file with matches, in the order the search found it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResult {
    pub path: PathBuf,
    /// Empty for a search by file name.
    pub lines: Vec<LineMatch>,
}

/// One row of the result list, by position in `SearchView::results`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRow {
    File(usize),
    /// File index, then line index in that file.
    Line(usize, usize),
}

impl SearchRow {
    fn file(self) -> usize {
        match self {
            Self::File(file) | Self::Line(file, _) => file,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchView {
    pub root: PathBuf,
    pub query: String,
    pub include: String,
    pub exclude: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// The field the keys type into, and the one the next `OpenSearch` leaves.
    pub focused: SearchField,
    /// Kept sorted by path, whatever order the files arrive in.
    pub results: Vec<FileResult>,
    /// Files whose lines are folded away. Survives a new search, so a file the
    /// user folded stays folded when the query changes.
    pub collapsed: HashSet<PathBuf>,
    pub cursor: Option<SearchRow>,
    /// Until the user moves the cursor it stays on the first row, however many
    /// files sort in front of it; afterwards it stays on the item it is on.
    pub cursor_moved_by_user: bool,
    pub scroll_offset: usize,
    pub scroll_remainder: f32,
    /// Answers the running search's events; one from an older search is stale.
    pub generation: u64,
    pub running: bool,
    pub scanned: usize,
    pub skipped: usize,
    pub totals: Option<SearchTotals>,
    pub error: Option<SearchError>,
}

/// Whether the walk may leave the file system `root` is on.
///
/// From `/` on macOS it may not: the volume with `/Users` is another file
/// system than the read-only system volume that `/` is, so staying on one
/// would search nothing the user means.
pub fn stays_on_file_system(root: &Path) -> bool {
    !(cfg!(target_os = "macos") && root == Path::new("/"))
}

impl SearchView {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            query: String::new(),
            include: String::new(),
            exclude: String::new(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
            focused: SearchField::Query,
            results: Vec::new(),
            collapsed: HashSet::new(),
            cursor: None,
            cursor_moved_by_user: false,
            scroll_offset: 0,
            scroll_remainder: 0.0,
            generation: 0,
            running: false,
            scanned: 0,
            skipped: 0,
            totals: None,
            error: None,
        }
    }

    pub fn query_for_engine(&self) -> SearchQuery {
        SearchQuery {
            root: self.root.clone(),
            text: self.query.clone(),
            regex: self.regex,
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            same_file_system: stays_on_file_system(&self.root),
        }
    }

    /// Nothing typed in the search field: the Files field names what to list.
    pub fn is_name_search(&self) -> bool {
        self.query.is_empty()
    }

    /// The rows in display order: each file, followed by its lines unless the
    /// file is folded. Lazy, so drawing a window of them does not build all.
    pub fn rows_iter(&self) -> impl Iterator<Item = SearchRow> + '_ {
        self.results.iter().enumerate().flat_map(|(file, result)| {
            let lines = if self.collapsed.contains(&result.path) {
                0
            } else {
                result.lines.len()
            };
            std::iter::once(SearchRow::File(file))
                .chain((0..lines).map(move |line| SearchRow::Line(file, line)))
        })
    }

    pub fn rows(&self) -> Vec<SearchRow> {
        self.rows_iter().collect()
    }

    pub fn cursor_row_index(&self) -> Option<usize> {
        let cursor = self.cursor?;
        self.rows_iter().position(|row| row == cursor)
    }

    pub fn move_cursor(&mut self, delta: isize, visible_rows: usize) {
        let from = self.cursor_row_index().unwrap_or(0);
        self.select_row(from.saturating_add_signed(delta), visible_rows);
    }

    /// `direction` pages up (negative) or down.
    pub fn page(&mut self, direction: isize, visible_rows: usize) {
        self.move_cursor(
            direction.saturating_mul(visible_rows.max(1) as isize),
            visible_rows,
        );
    }

    pub fn first(&mut self, visible_rows: usize) {
        self.select_row(0, visible_rows);
    }

    pub fn last(&mut self, visible_rows: usize) {
        self.select_row(usize::MAX, visible_rows);
    }

    /// Puts the cursor on row `index` (clamped) and scrolls it into view.
    pub fn select_row(&mut self, index: usize, visible_rows: usize) {
        let rows = self.rows();
        let Some(last) = rows.len().checked_sub(1) else {
            return;
        };
        self.cursor = rows.get(index.min(last)).copied();
        self.cursor_moved_by_user = true;
        self.ensure_visible(visible_rows);
    }

    /// Right unfolds and Left folds the cursor's file. Left on a line folds
    /// its file and moves the cursor up to it, so the cursor never sits on a
    /// row that is gone.
    pub fn set_expanded(&mut self, expanded: bool, visible_rows: usize) {
        let Some(file) = self.cursor.map(SearchRow::file) else {
            return;
        };
        self.set_group(file, expanded, visible_rows);
    }

    pub fn toggle_group(&mut self, file: usize, visible_rows: usize) {
        let Some(result) = self.results.get(file) else {
            return;
        };
        let expanded = self.collapsed.contains(&result.path);
        self.set_group(file, expanded, visible_rows);
    }

    fn set_group(&mut self, file: usize, expanded: bool, visible_rows: usize) {
        let Some(result) = self.results.get(file) else {
            return;
        };
        if expanded {
            self.collapsed.remove(&result.path);
        } else if !result.lines.is_empty() {
            self.collapsed.insert(result.path.clone());
            if self.cursor.map(SearchRow::file) == Some(file) {
                self.cursor = Some(SearchRow::File(file));
            }
        }
        self.ensure_visible(visible_rows);
    }

    /// Scrolls by a wheel or trackpad movement the way a panel does: pixels
    /// are collected until they make a row, and the cursor is held inside the
    /// window.
    pub fn scroll(&mut self, delta: ScrollDelta, visible_rows: usize) {
        let rows_down = match delta {
            ScrollDelta::Lines { y, .. } => -y,
            ScrollDelta::Pixels { y, .. } => -y / ROW_HEIGHT,
        } + self.scroll_remainder;
        let whole = rows_down.trunc();
        self.scroll_remainder = rows_down - whole;

        let rows = visible_rows.max(1);
        let all = self.rows();
        let max_offset = all.len().saturating_sub(rows);
        let moved = whole as isize;
        self.scroll_offset = self
            .scroll_offset
            .saturating_add_signed(moved)
            .min(max_offset);
        // At either end the rest of the gesture is spent, not saved up.
        if (self.scroll_offset == 0 && moved < 0) || (self.scroll_offset == max_offset && moved > 0)
        {
            self.scroll_remainder = 0.0;
        }
        if let Some(index) = self.cursor_row_index() {
            let held = index.clamp(self.scroll_offset, self.scroll_offset + rows - 1);
            if held != index {
                self.cursor = all.get(held).copied();
                self.cursor_moved_by_user = true;
            }
        }
    }

    /// Scrolls just far enough to show the cursor, and keeps the offset inside
    /// the list after rows have gone.
    pub fn ensure_visible(&mut self, visible_rows: usize) {
        let rows = visible_rows.max(1);
        if let Some(index) = self.cursor_row_index() {
            if index < self.scroll_offset {
                self.scroll_offset = index;
            } else if index >= self.scroll_offset + rows {
                self.scroll_offset = index + 1 - rows;
            }
        }
        let max_offset = self.rows_iter().count().saturating_sub(rows);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Forgets the results of the search before, keeping what the user set.
    pub fn clear_results(&mut self) {
        self.results.clear();
        self.cursor = None;
        self.cursor_moved_by_user = false;
        self.scroll_offset = 0;
        self.scroll_remainder = 0.0;
        self.running = false;
        self.scanned = 0;
        self.skipped = 0;
        self.totals = None;
        self.error = None;
    }

    /// A new search starts: its events carry `generation`.
    pub fn begin(&mut self, generation: u64) {
        self.clear_results();
        self.generation = generation;
        self.running = true;
    }

    pub fn apply_event(&mut self, event: SearchEvent) {
        match event {
            SearchEvent::File { path, lines } => self.insert_file(FileResult { path, lines }),
            SearchEvent::Progress { scanned, skipped } => {
                self.scanned = scanned;
                self.skipped = skipped;
            }
            SearchEvent::Finished(totals) => {
                self.scanned = totals.scanned;
                self.skipped = totals.skipped;
                self.totals = Some(totals);
                self.running = false;
            }
        }
    }

    fn insert_file(&mut self, found: FileResult) {
        let at = match self
            .results
            .binary_search_by(|result| result.path.cmp(&found.path))
        {
            Ok(existing) => {
                if let Some(slot) = self.results.get_mut(existing) {
                    *slot = found;
                }
                existing
            }
            Err(position) => {
                self.results.insert(position, found);
                // A file in front of the cursor pushes it down by one.
                if let Some(cursor) = self.cursor.as_mut() {
                    if position <= cursor.file() {
                        *cursor = match *cursor {
                            SearchRow::File(file) => SearchRow::File(file + 1),
                            SearchRow::Line(file, line) => SearchRow::Line(file + 1, line),
                        };
                    }
                }
                position
            }
        };
        if !self.cursor_moved_by_user {
            self.cursor = (!self.results.is_empty()).then_some(SearchRow::File(0));
        } else if let Some(SearchRow::Line(file, line)) = self.cursor {
            // A replaced file may have fewer lines than the cursor's index.
            if file == at && self.results.get(at).is_some_and(|r| line >= r.lines.len()) {
                self.cursor = Some(SearchRow::File(at));
            }
        }
    }

    /// Every file found, in display order: what F5, F6 and F8 act on.
    pub fn found_files(&self) -> Vec<PathBuf> {
        self.results
            .iter()
            .map(|result| result.path.clone())
            .collect()
    }

    /// The file under the cursor, and the line when the cursor is on one.
    pub fn cursor_target(&self) -> Option<(PathBuf, Option<u64>)> {
        match self.cursor? {
            SearchRow::File(file) => Some((self.results.get(file)?.path.clone(), None)),
            SearchRow::Line(file, line) => {
                let result = self.results.get(file)?;
                Some((result.path.clone(), Some(result.lines.get(line)?.number)))
            }
        }
    }
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

    fn line(number: u64) -> LineMatch {
        LineMatch {
            number,
            text: format!("line {number}"),
            ranges: std::iter::once(0..4).collect(),
        }
    }

    fn found(path: &str, lines: &[u64]) -> SearchEvent {
        SearchEvent::File {
            path: PathBuf::from(path),
            lines: lines.iter().map(|&number| line(number)).collect(),
        }
    }

    fn view_with(events: Vec<SearchEvent>) -> SearchView {
        let mut view = SearchView::new(PathBuf::from("/root"));
        view.begin(1);
        for event in events {
            view.apply_event(event);
        }
        view
    }

    fn paths(view: &SearchView) -> Vec<&str> {
        view.results
            .iter()
            .map(|result| result.path.to_str().unwrap())
            .collect()
    }

    #[test]
    fn the_fields_cycle() {
        assert_eq!(SearchField::Query.next(), SearchField::Include);
        assert_eq!(SearchField::Include.next(), SearchField::Exclude);
        assert_eq!(SearchField::Exclude.next(), SearchField::Query);
    }

    #[test]
    fn found_files_lists_every_file_in_display_order() {
        let view = view_with(vec![
            found("/root/b.txt", &[1]),
            found("/root/a.txt", &[2, 3]),
        ]);
        assert_eq!(
            view.found_files(),
            [PathBuf::from("/root/a.txt"), PathBuf::from("/root/b.txt")]
        );
    }

    #[test]
    fn files_are_kept_sorted_by_path_whatever_order_they_arrive_in() {
        let view = view_with(vec![
            found("/root/b.txt", &[1]),
            found("/root/a.txt", &[1]),
            found("/root/sub/c.txt", &[1]),
        ]);
        assert_eq!(
            paths(&view),
            ["/root/a.txt", "/root/b.txt", "/root/sub/c.txt"]
        );
    }

    #[test]
    fn the_cursor_stays_on_the_first_row_until_the_user_moves_it() {
        let mut view = view_with(vec![found("/root/m.txt", &[1])]);
        assert_eq!(view.cursor, Some(SearchRow::File(0)));
        view.apply_event(found("/root/a.txt", &[1]));
        assert_eq!(view.cursor, Some(SearchRow::File(0)));
        assert_eq!(
            view.cursor_target().unwrap().0,
            PathBuf::from("/root/a.txt")
        );
    }

    #[test]
    fn after_the_user_moved_the_cursor_it_stays_on_its_item_when_files_sort_in_front() {
        let mut view = view_with(vec![found("/root/m.txt", &[4, 9])]);
        view.move_cursor(2, 10);
        assert_eq!(view.cursor, Some(SearchRow::Line(0, 1)));

        view.apply_event(found("/root/a.txt", &[1]));
        view.apply_event(found("/root/z.txt", &[1]));

        assert_eq!(view.cursor, Some(SearchRow::Line(1, 1)));
        assert_eq!(
            view.cursor_target(),
            Some((PathBuf::from("/root/m.txt"), Some(9)))
        );
    }

    #[test]
    fn rows_list_each_file_with_its_lines() {
        let view = view_with(vec![
            found("/root/a.txt", &[1, 2]),
            found("/root/b.txt", &[3]),
        ]);
        assert_eq!(
            view.rows(),
            [
                SearchRow::File(0),
                SearchRow::Line(0, 0),
                SearchRow::Line(0, 1),
                SearchRow::File(1),
                SearchRow::Line(1, 0),
            ]
        );
    }

    #[test]
    fn a_name_search_has_only_file_rows() {
        let view = view_with(vec![found("/root/a.txt", &[]), found("/root/b.txt", &[])]);
        assert_eq!(view.rows(), [SearchRow::File(0), SearchRow::File(1)]);
    }

    #[test]
    fn a_folded_file_hides_its_lines_and_unfolds_again() {
        let mut view = view_with(vec![
            found("/root/a.txt", &[1, 2]),
            found("/root/b.txt", &[3]),
        ]);
        view.toggle_group(0, 10);
        assert_eq!(
            view.rows(),
            [
                SearchRow::File(0),
                SearchRow::File(1),
                SearchRow::Line(1, 0)
            ]
        );
        view.toggle_group(0, 10);
        assert_eq!(view.rows().len(), 5);
    }

    #[test]
    fn left_on_a_line_folds_its_file_and_puts_the_cursor_on_it() {
        let mut view = view_with(vec![
            found("/root/a.txt", &[1, 2]),
            found("/root/b.txt", &[3]),
        ]);
        view.select_row(2, 10);
        assert_eq!(view.cursor, Some(SearchRow::Line(0, 1)));

        view.set_expanded(false, 10);

        assert_eq!(view.cursor, Some(SearchRow::File(0)));
        assert!(view.collapsed.contains(Path::new("/root/a.txt")));
        view.set_expanded(true, 10);
        assert!(view.collapsed.is_empty());
        assert_eq!(view.cursor, Some(SearchRow::File(0)));
    }

    #[test]
    fn folding_survives_a_new_search() {
        let mut view = view_with(vec![found("/root/a.txt", &[1])]);
        view.toggle_group(0, 10);
        view.begin(2);
        assert!(view.collapsed.contains(Path::new("/root/a.txt")));
        assert!(view.results.is_empty());
        assert!(view.running);
        assert_eq!(view.generation, 2);
    }

    #[test]
    fn the_cursor_is_clamped_and_scrolls_into_view() {
        let events = (0..20)
            .map(|i| found(&format!("/root/f{i:02}.txt"), &[]))
            .collect();
        let mut view = view_with(events);
        view.move_cursor(-5, 5);
        assert_eq!((view.cursor_row_index(), view.scroll_offset), (Some(0), 0));
        view.move_cursor(7, 5);
        assert_eq!((view.cursor_row_index(), view.scroll_offset), (Some(7), 3));
        view.last(5);
        assert_eq!(
            (view.cursor_row_index(), view.scroll_offset),
            (Some(19), 15)
        );
        view.page(-1, 5);
        assert_eq!(view.cursor_row_index(), Some(14));
        view.first(5);
        assert_eq!((view.cursor_row_index(), view.scroll_offset), (Some(0), 0));
    }

    #[test]
    fn an_empty_result_list_does_not_panic() {
        let mut view = SearchView::new(PathBuf::from("/root"));
        view.move_cursor(3, 5);
        view.last(5);
        view.set_expanded(false, 5);
        view.toggle_group(4, 5);
        view.scroll(ScrollDelta::Lines { x: 0.0, y: -3.0 }, 5);
        assert_eq!(view.cursor, None);
        assert_eq!(view.cursor_target(), None);
    }

    #[test]
    fn the_wheel_scrolls_by_rows_and_holds_the_cursor_inside() {
        let events = (0..20)
            .map(|i| found(&format!("/root/f{i:02}.txt"), &[]))
            .collect();
        let mut view = view_with(events);
        view.scroll(ScrollDelta::Lines { x: 0.0, y: -3.0 }, 5);
        assert_eq!(view.scroll_offset, 3);
        assert_eq!(view.cursor_row_index(), Some(3));
        view.scroll(ScrollDelta::Lines { x: 0.0, y: -100.0 }, 5);
        assert_eq!(view.scroll_offset, 15);
        view.scroll(ScrollDelta::Pixels { x: 0.0, y: 11.0 }, 5);
        assert_eq!(view.scroll_offset, 15, "half a row does not move yet");
        view.scroll(ScrollDelta::Pixels { x: 0.0, y: 11.0 }, 5);
        assert_eq!(view.scroll_offset, 14);
    }

    #[test]
    fn progress_and_the_end_are_recorded() {
        let mut view = view_with(vec![]);
        view.apply_event(SearchEvent::Progress {
            scanned: 40,
            skipped: 2,
        });
        assert_eq!((view.scanned, view.skipped, view.running), (40, 2, true));
        let totals = SearchTotals {
            scanned: 90,
            files: 1,
            matches: 3,
            skipped: 5,
            truncated: false,
        };
        view.apply_event(SearchEvent::Finished(totals));
        assert_eq!(view.totals, Some(totals));
        assert_eq!((view.scanned, view.skipped, view.running), (90, 5, false));
    }

    #[test]
    fn the_engine_query_carries_the_fields() {
        let mut view = SearchView::new(PathBuf::from("/root"));
        view.query = "needle".into();
        view.include = "*.rs".into();
        view.exclude = "target/".into();
        view.regex = true;
        view.case_sensitive = true;
        let query = view.query_for_engine();
        assert_eq!(query.root, PathBuf::from("/root"));
        assert_eq!(query.text, "needle");
        assert_eq!(query.include, "*.rs");
        assert_eq!(query.exclude, "target/");
        assert!(query.regex && query.case_sensitive && !query.whole_word);
        assert!(query.same_file_system);
    }

    #[test]
    fn only_the_macos_root_leaves_its_file_system() {
        assert!(stays_on_file_system(Path::new("/Users")));
        assert!(stays_on_file_system(Path::new("/tmp/x")));
        assert_eq!(
            stays_on_file_system(Path::new("/")),
            !cfg!(target_os = "macos")
        );
    }

    #[test]
    fn the_cursor_target_names_the_file_and_the_line() {
        let mut view = view_with(vec![found("/root/a.txt", &[7])]);
        assert_eq!(
            view.cursor_target(),
            Some((PathBuf::from("/root/a.txt"), None))
        );
        view.move_cursor(1, 10);
        assert_eq!(
            view.cursor_target(),
            Some((PathBuf::from("/root/a.txt"), Some(7)))
        );
    }
}
