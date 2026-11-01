//! The state of the trash view that takes the place of a panel: the entries
//! the trash lists, the cursor in them and the tagged rows. No widgets and no
//! tasks; `App` reads and restores, `view` draws this.

use std::collections::HashSet;
use std::ffi::OsString;

use iced::mouse::ScrollDelta;

use ncrs_fs::{TrashReach, TrashedItem};
use ncrs_ui_kit::list_window::ListWindow;

pub mod view;

#[derive(Debug, Clone)]
pub struct TrashView {
    pub reach: TrashReach,
    /// Newest first, as the trash lists them.
    pub items: Vec<TrashedItem>,
    pub cursor: usize,
    /// By the key the trash knows an entry by: an entry that moves in the list
    /// keeps its tag.
    pub tagged: HashSet<OsString>,
    pub window: ListWindow,
    pub loading: bool,
    pub error: Option<String>,
}

impl TrashView {
    /// Opens while the first listing is still being read.
    pub fn new(reach: TrashReach) -> Self {
        Self {
            reach,
            items: Vec::new(),
            cursor: 0,
            tagged: HashSet::new(),
            window: ListWindow::default(),
            loading: true,
            error: None,
        }
    }

    pub fn begin_reading(&mut self) {
        self.loading = true;
    }

    /// Takes what a listing gave. The cursor stays on its row number, inside
    /// the new list; tags go, because what they marked may be gone.
    pub fn set_items(&mut self, result: Result<Vec<TrashedItem>, String>, visible_rows: usize) {
        self.loading = false;
        self.tagged.clear();
        match result {
            Ok(items) => {
                self.items = items;
                self.error = None;
            }
            Err(reason) => {
                self.items.clear();
                self.error = Some(reason);
            }
        }
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
        self.window
            .ensure_visible(self.cursor, self.items.len(), visible_rows);
    }

    pub fn move_cursor(&mut self, delta: isize, visible_rows: usize) {
        self.select(self.cursor.saturating_add_signed(delta), visible_rows);
    }

    /// `direction` pages up (negative) or down.
    pub fn page(&mut self, direction: isize, visible_rows: usize) {
        let page = isize::try_from(visible_rows.max(1)).unwrap_or(isize::MAX);
        self.move_cursor(direction.saturating_mul(page), visible_rows);
    }

    pub fn first(&mut self, visible_rows: usize) {
        self.select(0, visible_rows);
    }

    pub fn last(&mut self, visible_rows: usize) {
        self.select(usize::MAX, visible_rows);
    }

    /// Puts the cursor on row `index` (clamped) and scrolls it into view.
    pub fn select(&mut self, index: usize, visible_rows: usize) {
        self.cursor = index.min(self.items.len().saturating_sub(1));
        self.window
            .ensure_visible(self.cursor, self.items.len(), visible_rows);
    }

    pub fn scroll(&mut self, delta: ScrollDelta, visible_rows: usize) {
        self.window
            .scroll(delta, self.items.len(), visible_rows, &mut self.cursor);
    }

    pub fn is_tagged(&self, index: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|item| self.tagged.contains(&item.key))
    }

    pub fn tagged_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| self.tagged.contains(&item.key))
            .count()
    }

    pub fn toggle_tag(&mut self) {
        let Some(item) = self.items.get(self.cursor) else {
            return;
        };
        if !self.tagged.remove(&item.key) {
            self.tagged.insert(item.key.clone());
        }
    }

    /// Tags the cursor's row and moves one row on (`1`) or back (`-1`).
    pub fn tag_and_move(&mut self, delta: isize, visible_rows: usize) {
        self.toggle_tag();
        self.move_cursor(delta, visible_rows);
    }

    pub fn tag_all(&mut self) {
        self.tagged = self.items.iter().map(|item| item.key.clone()).collect();
    }

    pub fn clear_tags(&mut self) {
        self.tagged.clear();
    }

    /// What Enter acts on: the tagged rows, else the one under the cursor.
    pub fn action_items(&self) -> Vec<TrashedItem> {
        if self.tagged_count() == 0 {
            return self.items.get(self.cursor).cloned().into_iter().collect();
        }
        self.items
            .iter()
            .filter(|item| self.tagged.contains(&item.key))
            .cloned()
            .collect()
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
