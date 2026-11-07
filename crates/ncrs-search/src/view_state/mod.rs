//! The state of the search view that takes the place of a panel: the fields,
//! the results as they stream in, and the cursor in them. No widgets and no
//! tasks; `App` runs the engine and `ui::search` draws this.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use iced::mouse::ScrollDelta;

use ncrs_core::FileEntry;
use ncrs_ui_kit::layout::{HIT_LIST_ROW_HEIGHT, SEARCH_CONTENT_ROW_HEIGHT};

use crate::engine::{LineMatch, PathFilter, SearchError, SearchEvent, SearchQuery, SearchTotals};
use crate::filters::SearchFilters;

/// The line number of a snippet from the index, which does not know lines;
/// real lines count from 1.
pub const UNNUMBERED_LINE: u64 = 0;

/// Most documents of the index shown at once, after the folder and the file
/// name globs applied. Keeps one answer from filling the memory.
pub const INDEX_HIT_LIMIT: usize = 100_000;

/// Characters of a snippet kept in a result row.
const SNIPPET_CHARS: usize = 160;

/// The four text fields of the search view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    /// The name pattern at the top.
    Names,
    /// The folder the search looks in, right below the names.
    Folder,
    /// The text to find in the files; there while `in_files` is on.
    Query,
    Exclude,
}

impl SearchField {
    /// The field Alt+F7 moves to when the search is already up. The text field
    /// is skipped while it is not shown.
    pub fn next(self, in_files: bool) -> Self {
        match self {
            Self::Names => Self::Folder,
            Self::Folder if in_files => Self::Query,
            Self::Folder | Self::Query => Self::Exclude,
            Self::Exclude => Self::Names,
        }
    }
}

/// The folder a typed text names: `~` stands for the home folder. `None` when
/// it is not an existing folder.
pub fn resolve_folder(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    let expanded = match trimmed.strip_prefix('~') {
        Some("") => ncrs_fs::home_dir(),
        Some(rest) if rest.starts_with(['/', std::path::MAIN_SEPARATOR]) => {
            ncrs_fs::home_dir().join(rest.trim_start_matches(['/', std::path::MAIN_SEPARATOR]))
        }
        _ if trimmed.is_empty() => return None,
        _ => PathBuf::from(trimmed),
    };
    expanded.is_dir().then_some(expanded)
}

/// A file with matches or a folder with a matching name, in the order the
/// search found it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResult {
    pub path: PathBuf,
    pub is_dir: bool,
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
            is_dir: self.is_dir,
            is_symlink: false,
            is_parent: false,
            size: self.size,
            modified: self.modified,
            folder_size: None,
            details: Default::default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SearchView {
    pub root: PathBuf,
    /// What the folder field shows: `root`, or what the user typed on the way
    /// to another folder.
    pub folder_text: String,
    /// The typed folder is not an existing folder: nothing is searched.
    pub folder_missing: bool,
    /// Name patterns, the first field: globs separated by `;`, or plain text
    /// for a part of the name.
    pub names: String,
    /// Whether the contents are searched, too: the files whose name matches
    /// `names` and whose content has `query`.
    pub in_files: bool,
    /// The text to find in the files; only used while `in_files` is on.
    pub query: String,
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
            folder_text: root.display().to_string(),
            folder_missing: false,
            root,
            names: String::new(),
            in_files: false,
            query: String::new(),
            exclude: String::new(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
            focused: SearchField::Names,
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

    /// Searches below `folder` from now on.
    pub fn set_root(&mut self, folder: PathBuf) {
        self.folder_text = folder.display().to_string();
        self.root = folder;
        self.folder_missing = false;
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
                is_dir: false,
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
            files: results.iter().filter(|result| !result.is_dir).count(),
            folders: results.iter().filter(|result| result.is_dir).count(),
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

    /// The text the contents are searched for; empty while only names are.
    pub fn content_text(&self) -> &str {
        if self.in_files {
            &self.query
        } else {
            ""
        }
    }

    pub fn query_for_engine(&self) -> SearchQuery {
        SearchQuery {
            root: self.root.clone(),
            text: self.content_text().to_owned(),
            regex: self.regex,
            // The toggles only show next to the text field; names ignore case.
            case_sensitive: self.in_files && self.case_sensitive,
            whole_word: self.whole_word,
            include: self.names.clone(),
            exclude: self.exclude.clone(),
            same_file_system: stays_on_file_system(&self.root),
        }
    }

    /// Only names are searched: the box is off or no text is asked for.
    pub fn is_name_search(&self) -> bool {
        self.content_text().is_empty()
    }

    /// The field Alt+F7 moves on to.
    pub fn next_field(&mut self) {
        self.focused = self.focused.next(self.in_files);
    }

    /// What the hits are called as a list: the text found, else the names.
    pub fn hit_list_title(&self) -> String {
        let asked = if self.is_name_search() {
            &self.names
        } else {
            &self.query
        };
        asked.trim().to_owned()
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
                is_dir: false,
                lines,
                size,
                modified,
            }),
            SearchEvent::Folder { path, modified } => self.insert_file(FileResult {
                path,
                is_dir: true,
                lines: Vec::new(),
                size: 0,
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
            // A folder found by name comes with the files inside it: acting
            // on both would copy or delete the same file twice.
            return tagged
                .iter()
                .filter(|path| {
                    !tagged
                        .iter()
                        .any(|other| *other != **path && path.starts_with(other))
                })
                .cloned()
                .collect();
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
mod tests;
