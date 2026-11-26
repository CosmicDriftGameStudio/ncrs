//! Which rows of a list the user has marked, and where the cursor is. Rows are
//! known by a key (a path, a name), not by their place: a list that is read
//! again keeps its marks, whatever order the rows come in.
//!
//! Space marks or unmarks the cursor row; Shift with the arrow keys marks the
//! rows between the anchor, where the Shift run began, and the cursor.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedList<K: Ord + Clone> {
    cursor: Option<K>,
    anchor: Option<K>,
    marked: BTreeSet<K>,
}

impl<K: Ord + Clone> Default for MarkedList<K> {
    fn default() -> Self {
        Self {
            cursor: None,
            anchor: None,
            marked: BTreeSet::new(),
        }
    }
}

impl<K: Ord + Clone> MarkedList<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The place of the cursor in `keys`; the first row while it has no row
    /// (nothing chosen yet, or its row is gone).
    pub fn cursor_index(&self, keys: &[K]) -> usize {
        self.cursor
            .as_ref()
            .and_then(|cursor| keys.iter().position(|key| key == cursor))
            .unwrap_or(0)
    }

    pub fn cursor<'a>(&self, keys: &'a [K]) -> Option<&'a K> {
        keys.get(self.cursor_index(keys))
    }

    pub fn is_marked(&self, key: &K) -> bool {
        self.marked.contains(key)
    }

    pub fn marked_count(&self) -> usize {
        self.marked.len()
    }

    /// The marked keys in key order.
    pub fn marked(&self) -> impl Iterator<Item = &K> {
        self.marked.iter()
    }

    /// The marked rows, or the cursor row when none is marked: what an action
    /// on the list acts on.
    pub fn targets(&self, keys: &[K]) -> Vec<K> {
        if self.marked.is_empty() {
            self.cursor(keys).cloned().into_iter().collect()
        } else {
            keys.iter()
                .filter(|key| self.marked.contains(*key))
                .cloned()
                .collect()
        }
    }

    pub fn clear(&mut self) {
        self.marked.clear();
        self.anchor = None;
    }

    pub fn mark_all(&mut self, keys: &[K]) {
        self.marked = keys.iter().cloned().collect();
        self.anchor = None;
    }

    /// Moves the cursor by `delta` rows, within the list; the run of Shift ends.
    pub fn move_by(&mut self, delta: isize, keys: &[K]) {
        self.anchor = None;
        self.cursor = self.moved(delta, keys);
    }

    /// Puts the cursor on `key`; the run of Shift ends.
    pub fn move_to(&mut self, key: K) {
        self.anchor = None;
        self.cursor = Some(key);
    }

    /// Space: marks the cursor row, or unmarks it. The cursor stays.
    pub fn toggle(&mut self, keys: &[K]) {
        self.anchor = None;
        let Some(key) = self.cursor(keys).cloned() else {
            return;
        };
        if !self.marked.remove(&key) {
            self.marked.insert(key);
        }
    }

    /// Shift with an arrow key: the cursor moves by `delta` and the rows from
    /// the anchor to it are marked. Moving back toward the anchor unmarks the
    /// row left behind.
    pub fn extend_by(&mut self, delta: isize, keys: &[K]) {
        let from = self.cursor_index(keys);
        let anchor = self
            .anchor
            .as_ref()
            .and_then(|anchor| keys.iter().position(|key| key == anchor))
            .unwrap_or(from);
        let target = self.moved(delta, keys);
        let to = target
            .as_ref()
            .and_then(|key| keys.iter().position(|row| row == key))
            .unwrap_or(from);
        let covered = |at: usize, end: usize| at >= anchor.min(end) && at <= anchor.max(end);
        for (at, key) in keys.iter().enumerate() {
            if covered(at, to) {
                self.marked.insert(key.clone());
            } else if covered(at, from) {
                self.marked.remove(key);
            }
        }
        self.anchor = keys.get(anchor).cloned();
        self.cursor = target;
    }

    /// The list was read again as `keys`: marks of rows that are gone are
    /// dropped, all others stay; the cursor follows its row.
    pub fn reload(&mut self, keys: &[K]) {
        self.marked.retain(|key| keys.contains(key));
        if self
            .anchor
            .as_ref()
            .is_some_and(|anchor| !keys.contains(anchor))
        {
            self.anchor = None;
        }
        let cursor_gone = self
            .cursor
            .as_ref()
            .is_some_and(|cursor| !keys.contains(cursor));
        if cursor_gone {
            self.cursor = keys.first().cloned();
        }
    }

    fn moved(&self, delta: isize, keys: &[K]) -> Option<K> {
        let last = keys.len().checked_sub(1)?;
        let at = self
            .cursor_index(keys)
            .saturating_add_signed(delta)
            .min(last);
        keys.get(at).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<String> {
        ["a", "b", "c", "d", "e"].map(str::to_owned).to_vec()
    }

    fn marked(list: &MarkedList<String>) -> Vec<&str> {
        list.marked().map(String::as_str).collect()
    }

    #[test]
    fn space_marks_and_unmarks_the_cursor_row() {
        let keys = keys();
        let mut list = MarkedList::new();
        list.move_by(1, &keys);
        list.toggle(&keys);
        assert_eq!(marked(&list), ["b"]);
        list.toggle(&keys);
        assert!(marked(&list).is_empty());
    }

    #[test]
    fn the_cursor_stops_at_the_ends() {
        let keys = keys();
        let mut list = MarkedList::new();
        list.move_by(-3, &keys);
        assert_eq!(list.cursor_index(&keys), 0);
        list.move_by(99, &keys);
        assert_eq!(list.cursor_index(&keys), 4);
    }

    #[test]
    fn shift_down_marks_from_the_anchor_and_shift_up_takes_it_back() {
        let keys = keys();
        let mut list = MarkedList::new();
        list.move_by(1, &keys);
        list.extend_by(1, &keys);
        list.extend_by(1, &keys);
        assert_eq!(marked(&list), ["b", "c", "d"]);
        list.extend_by(-1, &keys);
        assert_eq!(marked(&list), ["b", "c"]);
        assert_eq!(list.cursor_index(&keys), 2);
    }

    #[test]
    fn shift_up_past_the_anchor_marks_the_other_way() {
        let keys = keys();
        let mut list = MarkedList::new();
        list.move_by(2, &keys);
        list.extend_by(-1, &keys);
        list.extend_by(-1, &keys);
        assert_eq!(marked(&list), ["a", "b", "c"]);
    }

    #[test]
    fn a_plain_move_ends_the_run_and_keeps_the_marks() {
        let keys = keys();
        let mut list = MarkedList::new();
        list.extend_by(1, &keys);
        list.move_by(2, &keys);
        list.extend_by(1, &keys);
        assert_eq!(marked(&list), ["a", "b", "d", "e"]);
    }

    #[test]
    fn marks_survive_a_reload_in_another_order() {
        let mut list = MarkedList::new();
        let first = keys();
        list.move_by(2, &first);
        list.toggle(&first);
        let second: Vec<String> = ["e", "c", "a", "z"].map(str::to_owned).to_vec();
        list.reload(&second);
        assert_eq!(marked(&list), ["c"]);
        assert_eq!(list.cursor_index(&second), 1);
    }

    #[test]
    fn a_reload_drops_the_marks_of_rows_that_are_gone() {
        let mut list = MarkedList::new();
        let first = keys();
        list.mark_all(&first);
        list.reload(&["b".to_owned(), "x".to_owned()]);
        assert_eq!(marked(&list), ["b"]);
    }

    #[test]
    fn an_action_takes_the_cursor_row_when_nothing_is_marked() {
        let keys = keys();
        let mut list = MarkedList::new();
        list.move_by(3, &keys);
        assert_eq!(list.targets(&keys), ["d"]);
        list.move_by(-1, &keys);
        list.toggle(&keys);
        list.move_by(-1, &keys);
        list.toggle(&keys);
        assert_eq!(list.targets(&keys), ["b", "c"]);
    }
}
