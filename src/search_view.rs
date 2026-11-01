//! The state of the search view that takes the place of a panel: the fields,
//! the results as they stream in, and the cursor in them. No widgets and no
//! tasks; `App` runs the engine and `ui::search` draws this.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use iced::mouse::ScrollDelta;

use crate::fs::search::{
    LineMatch, PathFilter, SearchError, SearchEvent, SearchQuery, SearchTotals,
};
use crate::fs::FileEntry;
use crate::search_filters::SearchFilters;
use crate::ui::layout::{HIT_LIST_ROW_HEIGHT, SEARCH_CONTENT_ROW_HEIGHT};

/// The line number of a snippet from the index, which does not know lines;
/// real lines count from 1.
pub const UNNUMBERED_LINE: u64 = 0;

/// Most documents of the index shown at once, after the folder and the file
/// name globs applied. Keeps one answer from filling the memory.
pub const INDEX_HIT_LIMIT: usize = 100_000;

/// Characters of a snippet kept in a result row.
const SNIPPET_CHARS: usize = 160;

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
    /// As the search found the file, so drawing a row reads no file.
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl FileResult {
    /// The line the row shows under the folder.
    pub fn first_match(&self) -> Option<&LineMatch> {
        self.lines.first()
    }

    /// Matching lines beyond the one the row shows.
    pub fn more_matches(&self) -> usize {
        self.lines.len().saturating_sub(1)
    }

    /// The row the panel's file list draws for this hit.
    pub fn entry(&self) -> Option<FileEntry> {
        Some(FileEntry {
            name: self.path.file_name()?.to_owned(),
            path: self.path.clone(),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: self.size,
            modified: self.modified,
            folder_size: None,
        })
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
    /// The rows have a third line: set once per result set, so the row height
    /// does not change while results stream in.
    pub third_line: bool,
    /// The files F5, F6 and F8 act on. By path, as in the panels, so a result
    /// that sorts in front does not shift a tag onto another file.
    pub tagged: HashSet<PathBuf>,
    /// Index in `results`.
    pub cursor: Option<usize>,
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
    /// The filters over the documents Hermes has read; there while the filing
    /// has an index to ask. With a value chosen the index answers the search
    /// and `results` mirror what it found.
    pub filters: Option<SearchFilters>,
    /// Documents of the index left out of `results` by `INDEX_HIT_LIMIT`.
    pub not_shown: usize,
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
            third_line: false,
            tagged: HashSet::new(),
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
            filters: None,
            not_shown: 0,
        }
    }

    /// The index answers the search, not the walk.
    pub fn is_filtered(&self) -> bool {
        self.filters.as_ref().is_some_and(SearchFilters::is_active)
    }

    /// Puts what the index found, narrowed to the folder of the search and
    /// its file name globs, in place of the results. The cursor stays on its
    /// file when the file is still there.
    pub fn mirror_index(&mut self, visible_rows: usize) {
        self.mirror_index_within(visible_rows, INDEX_HIT_LIMIT);
    }

    fn mirror_index_within(&mut self, visible_rows: usize, limit: usize) {
        let Some(filters) = self.filters.as_ref() else {
            return;
        };
        let paths = match PathFilter::compile(&self.query_for_engine()) {
            Ok(paths) => paths,
            Err(error) => {
                self.clear_results();
                self.error = Some(error);
                return;
            }
        };
        let mut admitted = filters.shown().filter(|hit| paths.admits_file(&hit.path));
        let results: Vec<FileResult> = admitted
            .by_ref()
            .take(limit)
            .map(|hit| FileResult {
                path: hit.path.clone(),
                size: hit.stamp.map_or(0, |stamp| stamp.size),
                modified: hit.stamp.map(|stamp| stamp.mtime),
                lines: filters
                    .snippet_of(hit)
                    .map(|snippet| {
                        let fitted = snippet.fitted(SNIPPET_CHARS);
                        LineMatch {
                            number: UNNUMBERED_LINE,
                            text: fitted.text,
                            ranges: fitted.marks,
                        }
                    })
                    .into_iter()
                    .collect(),
            })
            .collect();
        let not_shown = admitted.count();
        let kept_on = self
            .cursor
            .and_then(|file| self.results.get(file))
            .map(|result| result.path.clone());
        let busy = filters.is_busy();
        let matches = results.iter().map(|result| result.lines.len()).sum();
        self.totals = Some(SearchTotals {
            scanned: results.len(),
            files: results.len(),
            matches,
            skipped: 0,
            truncated: false,
        });
        self.results = results;
        self.third_line = true;
        self.not_shown = not_shown;
        self.error = None;
        self.running = busy;
        self.tagged
            .retain(|path| self.results.iter().any(|result| &result.path == path));
        self.cursor = kept_on
            .and_then(|path| self.results.iter().position(|result| result.path == path))
            .or_else(|| (!self.results.is_empty()).then_some(0));
        self.ensure_visible(visible_rows);
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

    /// Height of a result row: name and folder, and under them the first
    /// matching line of a content search or the tags of an index hit.
    pub fn row_height(&self) -> f32 {
        if self.third_line {
            SEARCH_CONTENT_ROW_HEIGHT
        } else {
            HIT_LIST_ROW_HEIGHT
        }
    }

    pub fn move_cursor(&mut self, delta: isize, visible_rows: usize) {
        let from = self.cursor.unwrap_or(0);
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

    /// Puts the cursor on file `index` (clamped) and scrolls it into view.
    pub fn select_row(&mut self, index: usize, visible_rows: usize) {
        let Some(last) = self.results.len().checked_sub(1) else {
            return;
        };
        self.cursor = Some(index.min(last));
        self.cursor_moved_by_user = true;
        self.ensure_visible(visible_rows);
    }

    /// Scrolls by a wheel or trackpad movement the way a panel does: pixels
    /// are collected until they make a row, and the cursor is held inside the
    /// window.
    pub fn scroll(&mut self, delta: ScrollDelta, visible_rows: usize) {
        let rows_down = match delta {
            ScrollDelta::Lines { y, .. } => -y,
            ScrollDelta::Pixels { y, .. } => -y / self.row_height(),
        } + self.scroll_remainder;
        let whole = rows_down.trunc();
        self.scroll_remainder = rows_down - whole;

        let rows = visible_rows.max(1);
        let max_offset = self.results.len().saturating_sub(rows);
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
        if let Some(index) = self.cursor {
            let held = index.clamp(self.scroll_offset, self.scroll_offset + rows - 1);
            if held != index {
                self.cursor = Some(held.min(self.results.len().saturating_sub(1)));
                self.cursor_moved_by_user = true;
            }
        }
    }

    /// Scrolls just far enough to show the cursor, and keeps the offset inside
    /// the list after rows have gone.
    pub fn ensure_visible(&mut self, visible_rows: usize) {
        let rows = visible_rows.max(1);
        if let Some(index) = self.cursor {
            if index < self.scroll_offset {
                self.scroll_offset = index;
            } else if index >= self.scroll_offset + rows {
                self.scroll_offset = index + 1 - rows;
            }
        }
        let max_offset = self.results.len().saturating_sub(rows);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Forgets the results of the search before, keeping what the user set.
    pub fn clear_results(&mut self) {
        self.results.clear();
        self.third_line = false;
        self.tagged.clear();
        self.cursor = None;
        self.cursor_moved_by_user = false;
        self.scroll_offset = 0;
        self.scroll_remainder = 0.0;
        self.running = false;
        self.scanned = 0;
        self.skipped = 0;
        self.totals = None;
        self.not_shown = 0;
        self.error = None;
    }

    /// A new search starts: its events carry `generation`.
    pub fn begin(&mut self, generation: u64) {
        self.clear_results();
        self.generation = generation;
        self.running = true;
        self.third_line = !self.is_name_search();
    }

    pub fn apply_event(&mut self, event: SearchEvent) {
        match event {
            SearchEvent::File {
                path,
                lines,
                size,
                modified,
            } => self.insert_file(FileResult {
                path,
                lines,
                size,
                modified,
            }),
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
        match self
            .results
            .binary_search_by(|result| result.path.cmp(&found.path))
        {
            Ok(existing) => {
                if let Some(slot) = self.results.get_mut(existing) {
                    *slot = found;
                }
            }
            Err(position) => {
                self.results.insert(position, found);
                // A file in front of the cursor pushes it down by one.
                if let Some(cursor) = self.cursor.as_mut() {
                    if position <= *cursor {
                        *cursor += 1;
                    }
                }
            }
        }
        if !self.cursor_moved_by_user {
            self.cursor = (!self.results.is_empty()).then_some(0);
        }
    }

    /// Every file found, in display order.
    pub fn found_files(&self) -> Vec<PathBuf> {
        self.results
            .iter()
            .map(|result| result.path.clone())
            .collect()
    }

    /// What F5, F6 and F8 act on: the tagged files in display order, else the
    /// file under the cursor, else nothing. Never every file found.
    pub fn action_files(&self) -> Vec<PathBuf> {
        let tagged: Vec<PathBuf> = self
            .results
            .iter()
            .filter(|result| self.tagged.contains(&result.path))
            .map(|result| result.path.clone())
            .collect();
        if !tagged.is_empty() {
            return tagged;
        }
        self.cursor_target()
            .map(|(path, _)| vec![path])
            .unwrap_or_default()
    }

    pub fn is_tagged(&self, file: usize) -> bool {
        self.results
            .get(file)
            .is_some_and(|result| self.tagged.contains(&result.path))
    }

    /// Toggles the tag on the cursor's file and moves the cursor to the next
    /// file (`delta` > 0) or the previous one. At either end the cursor stays.
    pub fn tag_and_move(&mut self, delta: isize, visible_rows: usize) {
        let Some(file) = self.cursor else {
            return;
        };
        let Some(result) = self.results.get(file) else {
            return;
        };
        if !self.tagged.remove(&result.path) {
            self.tagged.insert(result.path.clone());
        }
        let neighbour = file.checked_add_signed(delta.signum());
        if let Some(next) = neighbour.filter(|next| *next < self.results.len()) {
            self.cursor = Some(next);
            self.cursor_moved_by_user = true;
            self.ensure_visible(visible_rows);
        }
    }

    pub fn tag_all(&mut self) {
        self.tagged = self.found_files().into_iter().collect();
    }

    /// The file under the cursor, and where its first match is for an editor
    /// to open it at.
    pub fn cursor_target(&self) -> Option<(PathBuf, Option<u64>)> {
        let result = self.results.get(self.cursor?)?;
        let line = result
            .first_match()
            .map(|first| first.number)
            .filter(|number| *number != UNNUMBERED_LINE);
        Some((result.path.clone(), line))
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
            size: 0,
            modified: None,
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

    fn three_files() -> SearchView {
        view_with(vec![
            found("/root/a.txt", &[1, 2]),
            found("/root/b.txt", &[3]),
            found("/root/c.txt", &[]),
        ])
    }

    #[test]
    fn without_tags_the_actions_take_the_cursors_file() {
        let mut view = three_files();
        view.select_row(1, 10);
        assert_eq!(view.cursor, Some(1));
        assert_eq!(view.action_files(), [PathBuf::from("/root/b.txt")]);
    }

    #[test]
    fn with_tags_the_actions_take_exactly_the_tagged_files_in_display_order() {
        let mut view = three_files();
        view.tagged.insert(PathBuf::from("/root/c.txt"));
        view.tagged.insert(PathBuf::from("/root/a.txt"));
        assert_eq!(
            view.action_files(),
            [PathBuf::from("/root/a.txt"), PathBuf::from("/root/c.txt")]
        );
    }

    #[test]
    fn without_a_cursor_and_tags_the_actions_take_nothing() {
        let mut view = three_files();
        view.cursor = None;
        assert!(view.action_files().is_empty());
    }

    #[test]
    fn tagging_moves_to_the_next_file() {
        let mut view = three_files();
        view.tag_and_move(1, 10);
        assert_eq!(view.cursor, Some(1));
        view.tag_and_move(1, 10);
        assert_eq!(view.cursor, Some(2));
        assert!(view.is_tagged(0) && view.is_tagged(1) && !view.is_tagged(2));
        // At the last file the tag still toggles and the cursor stays.
        view.tag_and_move(1, 10);
        assert_eq!(view.cursor, Some(2));
        assert!(view.is_tagged(2));
    }

    #[test]
    fn tagging_upwards_goes_to_the_previous_file() {
        let mut view = three_files();
        view.select_row(1, 10);
        view.tag_and_move(-1, 10);
        assert_eq!(view.cursor, Some(0));
        assert!(view.is_tagged(1));
    }

    #[test]
    fn tagging_twice_removes_the_tag() {
        let mut view = three_files();
        view.tag_and_move(1, 10);
        view.first(10);
        view.tag_and_move(1, 10);
        assert!(!view.is_tagged(0));
    }

    #[test]
    fn tag_all_and_a_new_search_clear_it() {
        let mut view = three_files();
        view.tag_all();
        assert_eq!(view.tagged.len(), 3);
        view.tagged.clear();
        assert_eq!(view.action_files().len(), 1);
        view.tag_all();
        view.begin(2);
        assert!(view.tagged.is_empty());
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
        assert_eq!(view.cursor, Some(0));
        view.apply_event(found("/root/a.txt", &[1]));
        assert_eq!(view.cursor, Some(0));
        assert_eq!(
            view.cursor_target().unwrap().0,
            PathBuf::from("/root/a.txt")
        );
    }

    #[test]
    fn after_the_user_moved_the_cursor_it_stays_on_its_file_when_files_sort_in_front() {
        let mut view = view_with(vec![found("/root/m.txt", &[4, 9])]);
        view.move_cursor(0, 10);
        assert_eq!(view.cursor, Some(0));

        view.apply_event(found("/root/a.txt", &[1]));
        view.apply_event(found("/root/z.txt", &[1]));

        assert_eq!(view.cursor, Some(1));
        assert_eq!(
            view.cursor_target(),
            Some((PathBuf::from("/root/m.txt"), Some(4)))
        );
    }

    #[test]
    fn the_cursor_is_clamped_and_scrolls_into_view() {
        let events = (0..20)
            .map(|i| found(&format!("/root/f{i:02}.txt"), &[]))
            .collect();
        let mut view = view_with(events);
        view.move_cursor(-5, 5);
        assert_eq!((view.cursor, view.scroll_offset), (Some(0), 0));
        view.move_cursor(7, 5);
        assert_eq!((view.cursor, view.scroll_offset), (Some(7), 3));
        view.last(5);
        assert_eq!((view.cursor, view.scroll_offset), (Some(19), 15));
        view.page(-1, 5);
        assert_eq!(view.cursor, Some(14));
        view.first(5);
        assert_eq!((view.cursor, view.scroll_offset), (Some(0), 0));
    }

    #[test]
    fn an_empty_result_list_does_not_panic() {
        let mut view = SearchView::new(PathBuf::from("/root"));
        view.move_cursor(3, 5);
        view.last(5);
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
        assert_eq!(view.cursor, Some(3));
        view.scroll(ScrollDelta::Lines { x: 0.0, y: -100.0 }, 5);
        assert_eq!(view.scroll_offset, 15);
        let half_a_row = HIT_LIST_ROW_HEIGHT / 2.0;
        view.scroll(
            ScrollDelta::Pixels {
                x: 0.0,
                y: half_a_row,
            },
            5,
        );
        assert_eq!(view.scroll_offset, 15, "half a row does not move yet");
        view.scroll(
            ScrollDelta::Pixels {
                x: 0.0,
                y: half_a_row,
            },
            5,
        );
        assert_eq!(view.scroll_offset, 14);
    }

    #[test]
    fn the_index_answer_is_cut_at_the_limit_and_the_rest_is_counted() {
        let mut view = SearchView::new(PathBuf::from("archive"));
        view.filters = Some(crate::search_filters::tests::loaded_search());

        view.mirror_index_within(10, 2);

        assert_eq!((view.results.len(), view.not_shown), (2, 4));
        view.mirror_index_within(10, 10);
        assert_eq!((view.results.len(), view.not_shown), (6, 0));
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
    fn the_cursor_target_names_the_file_and_its_first_match() {
        let view = view_with(vec![
            found("/root/a.txt", &[7, 9]),
            found("/root/b.txt", &[]),
        ]);
        assert_eq!(
            view.cursor_target(),
            Some((PathBuf::from("/root/a.txt"), Some(7)))
        );
        let mut moved = view;
        moved.move_cursor(1, 10);
        assert_eq!(
            moved.cursor_target(),
            Some((PathBuf::from("/root/b.txt"), None))
        );
    }

    #[test]
    fn a_content_row_shows_the_first_match_and_counts_the_others() {
        let view = view_with(vec![
            found("/root/a.txt", &[7, 9, 12]),
            found("/root/b.txt", &[3]),
        ]);
        let several = &view.results[0];
        assert_eq!(several.first_match().map(|m| m.number), Some(7));
        assert_eq!(several.more_matches(), 2);
        let single = &view.results[1];
        assert_eq!(single.first_match().map(|m| m.number), Some(3));
        assert_eq!(single.more_matches(), 0);
    }

    #[test]
    fn a_name_search_has_no_matching_line_to_show() {
        let view = view_with(vec![found("/root/a.txt", &[])]);
        assert_eq!(view.results[0].first_match(), None);
        assert_eq!(view.results[0].more_matches(), 0);
    }

    #[test]
    fn a_content_search_has_three_line_rows_and_a_name_search_two() {
        let height_is =
            |view: &SearchView, expected: f32| view.row_height().total_cmp(&expected).is_eq();
        let mut view = SearchView::new(PathBuf::from("/root"));
        view.begin(1);
        assert!(height_is(&view, HIT_LIST_ROW_HEIGHT));
        view.query = "needle".into();
        view.begin(2);
        assert!(height_is(&view, SEARCH_CONTENT_ROW_HEIGHT));
        view.query.clear();
        assert!(
            height_is(&view, SEARCH_CONTENT_ROW_HEIGHT),
            "the height is fixed for the result set"
        );
        view.begin(3);
        assert!(height_is(&view, HIT_LIST_ROW_HEIGHT));
    }

    #[test]
    fn the_entry_of_a_result_carries_its_size_and_date() {
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400);
        let result = FileResult {
            path: PathBuf::from("/root/sub/a.txt"),
            lines: Vec::new(),
            size: 42,
            modified: Some(modified),
        };
        let entry = result.entry().unwrap();
        assert_eq!(entry.name, std::ffi::OsString::from("a.txt"));
        assert_eq!((entry.size, entry.modified), (42, Some(modified)));
        assert!(!entry.is_dir);
    }
}
