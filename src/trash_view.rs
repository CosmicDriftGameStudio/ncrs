//! The state of the trash view that takes the place of a panel: the entries
//! the trash lists, the cursor in them and the tagged rows. No widgets and no
//! tasks; `App` reads and restores, `ui::trash` draws this.

use std::collections::HashSet;
use std::ffi::OsString;

use iced::mouse::ScrollDelta;

use crate::fs::{TrashReach, TrashedItem};
use crate::list_window::ListWindow;

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

    const VISIBLE: usize = 3;

    fn item(name: &str, deleted_at: i64) -> TrashedItem {
        TrashedItem {
            original: format!("/home/me/{name}").into(),
            deleted_at,
            key: format!("/trash/{name}").into(),
        }
    }

    fn view_of(names: &[&str]) -> TrashView {
        let mut view = TrashView::new(TrashReach::Everything);
        let items = names
            .iter()
            .enumerate()
            .map(|(index, name)| item(name, i64::try_from(names.len() - index).unwrap()))
            .collect();
        view.set_items(Ok(items), VISIBLE);
        view
    }

    #[test]
    fn a_new_view_is_reading_until_the_listing_arrives() {
        let mut view = TrashView::new(TrashReach::OwnDeletions);
        assert!(view.loading);

        view.set_items(Ok(vec![item("a", 1)]), VISIBLE);

        assert!(!view.loading);
        assert_eq!(view.items.len(), 1);
    }

    #[test]
    fn the_cursor_moves_inside_the_list_and_scrolls_with_it() {
        let mut view = view_of(&["a", "b", "c", "d", "e"]);

        view.move_cursor(-1, VISIBLE);
        assert_eq!(view.cursor, 0);
        view.last(VISIBLE);
        assert_eq!(view.cursor, 4);
        assert_eq!(view.window.offset(5, VISIBLE), 2);
        view.page(-1, VISIBLE);
        assert_eq!(view.cursor, 1);
        view.first(VISIBLE);
        assert_eq!(view.cursor, 0);
        assert_eq!(view.window.offset(5, VISIBLE), 0);
    }

    #[test]
    fn enter_acts_on_the_cursor_row_when_nothing_is_tagged() {
        let mut view = view_of(&["a", "b", "c"]);
        view.move_cursor(1, VISIBLE);

        assert_eq!(view.action_items(), vec![item("b", 2)]);
    }

    #[test]
    fn enter_acts_on_the_tagged_rows_only() {
        let mut view = view_of(&["a", "b", "c"]);
        view.tag_and_move(1, VISIBLE);
        view.move_cursor(1, VISIBLE);
        view.toggle_tag();

        assert_eq!(view.action_items(), vec![item("a", 3), item("c", 1)]);
        assert_eq!(view.tagged_count(), 2);
        assert!(view.is_tagged(0) && !view.is_tagged(1) && view.is_tagged(2));
    }

    #[test]
    fn tagging_twice_untags_and_all_and_none_work() {
        let mut view = view_of(&["a", "b"]);
        view.toggle_tag();
        view.toggle_tag();
        assert_eq!(view.tagged_count(), 0);

        view.tag_all();
        assert_eq!(view.tagged_count(), 2);
        view.clear_tags();
        assert_eq!(view.tagged_count(), 0);
    }

    #[test]
    fn a_new_listing_clamps_the_cursor_and_drops_the_tags() {
        let mut view = view_of(&["a", "b", "c"]);
        view.last(VISIBLE);
        view.tag_all();

        view.set_items(Ok(vec![item("a", 3)]), VISIBLE);

        assert_eq!(view.cursor, 0);
        assert_eq!(view.tagged_count(), 0);
    }

    #[test]
    fn a_failed_listing_shows_the_reason_and_no_rows() {
        let mut view = view_of(&["a"]);

        view.set_items(Err("no access".to_string()), VISIBLE);

        assert!(view.items.is_empty());
        assert_eq!(view.error.as_deref(), Some("no access"));
        assert!(view.action_items().is_empty());
    }

    #[test]
    fn scrolling_holds_the_cursor_inside_the_window() {
        let mut view = view_of(&["a", "b", "c", "d", "e", "f"]);

        view.scroll(ScrollDelta::Lines { x: 0.0, y: -3.0 }, VISIBLE);

        assert_eq!(view.window.offset(6, VISIBLE), 3);
        assert_eq!(view.cursor, 3);
    }
}
