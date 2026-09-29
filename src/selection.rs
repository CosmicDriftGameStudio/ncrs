//! Multi-selection: which rows an operation applies to.
//!
//! Norton Commander has three visual states per row, and the difference matters
//! for what a file operation does:
//!
//! - the **cursor** is where Enter goes, one per panel
//! - a **tagged** row was marked with Insert and is included in the operation
//! - a row can be both: tagged *and* under the cursor
//!
//! Without this, "Copy" would either do nothing or copy something the user did
//! not mark. The rule is the one from NC: if anything is tagged, the operation
//! applies to the tagged rows; otherwise to the single row under the cursor.

use std::collections::BTreeSet;

/// A row's selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// Tagged with Insert. Part of a file operation.
    pub tagged: bool,
    /// The row the cursor is on. Where Enter goes.
    pub cursor: bool,
}

impl Selection {
    /// Whether the row takes part in a file operation. A tagged row always
    /// does; an untagged one only when it carries the cursor and nothing else
    /// is tagged.
    ///
    /// Not called from the view: the view draws cursor and tag separately, and
    /// which rows an operation covers is decided where the operation is built
    /// (T7). Part of this type's contract, and tested as such, so the rule
    /// cannot be lost before it has a caller.
    #[allow(dead_code)]
    pub fn in_operation(&self, any_tagged: bool) -> bool {
        if any_tagged {
            self.tagged
        } else {
            self.cursor
        }
    }
}

/// Which rows of one panel a file operation applies to.
///
/// A set of indices rather than flags on `FileEntry`, because indices survive a
/// reload better than tags would: a directory read replaces `entries` entirely,
/// and an index set can be intersected with the new listing in one place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionSet {
    tagged: BTreeSet<usize>,
}

impl SelectionSet {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tagged.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tagged.len()
    }

    /// Toggles one row, as Insert does.
    pub fn toggle(&mut self, index: usize) -> bool {
        if self.tagged.remove(&index) {
            false
        } else {
            self.tagged.insert(index);
            true
        }
    }

    pub fn is_tagged(&self, index: usize) -> bool {
        self.tagged.contains(&index)
    }

    /// Drops every tag. `Ctrl+*` in NC.
    pub fn clear(&mut self) {
        self.tagged.clear();
    }

    /// The tagged indices, in the order the rows appear.
    #[allow(dead_code)]
    pub fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.tagged.iter().copied()
    }

    /// The state of one row, given where the cursor is.
    pub fn state_of(&self, index: usize, cursor: usize) -> Selection {
        Selection {
            tagged: self.is_tagged(index),
            cursor: index == cursor,
        }
    }

    /// Whether anything is tagged, which is what decides whether the cursor row
    /// takes part in the operation.
    #[allow(dead_code)]
    pub fn any_tagged(&self) -> bool {
        !self.tagged.is_empty()
    }

    /// Removes indices that no longer exist, after a reload.
    ///
    /// A reload replaces `entries`, so an index that pointed at a file before
    /// may point at a different one now — or past the end. Dropping the stale
    /// ones is the safe direction: a forgotten file in a copy is worse than a
    /// tag the user has to set again.
    pub fn retain_valid(&mut self, len: usize) {
        self.tagged.retain(|i| *i < len);
    }

    /// Tags every row in `0..len`, as `*` does. The `..` entry is never
    /// tagged: copying the parent directory into a subdirectory of itself is
    /// not a thing anyone wants.
    pub fn tag_all(&mut self, len: usize, entries: &[crate::fs::FileEntry]) {
        self.tagged.clear();
        for i in 0..len {
            if !entries.get(i).is_some_and(|e| e.is_parent) {
                self.tagged.insert(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_adds_then_removes() {
        let mut set = SelectionSet::new();
        assert!(set.toggle(3), "first toggle tags the row");
        assert!(set.is_tagged(3));
        assert!(!set.toggle(3), "second toggle untags it");
        assert!(!set.is_tagged(3));
        assert!(set.is_empty());
    }

    /// The rule that makes multi-selection worth having: tags win over the
    /// cursor. Without it, "copy" would be a coin toss.
    #[test]
    fn tags_take_precedence_over_the_cursor() {
        let mut set = SelectionSet::new();
        set.toggle(1);
        set.toggle(2);

        let cursor_row = set.state_of(5, 5);
        assert!(
            !cursor_row.in_operation(set.any_tagged()),
            "the cursor row is not part of the operation while rows are tagged"
        );
        for tagged in [1, 2] {
            assert!(set.state_of(tagged, 5).in_operation(true));
        }
    }

    /// With nothing tagged, the operation applies to the cursor row alone —
    /// which is what makes a plain F5 on one file work.
    #[test]
    fn without_tags_the_cursor_row_is_the_operation() {
        let set = SelectionSet::new();
        assert!(set.state_of(5, 5).in_operation(false));
        assert!(!set.state_of(4, 5).in_operation(false));
    }

    /// A row can be tagged and under the cursor at once, and it still takes
    /// part in the operation.
    #[test]
    fn a_row_can_be_both_tagged_and_the_cursor() {
        let mut set = SelectionSet::new();
        set.toggle(5);
        let state = set.state_of(5, 5);
        assert!(state.tagged && state.cursor);
        assert!(state.in_operation(true));
    }

    /// After a reload the row count can shrink. A stale index would point at a
    /// different file, so it is dropped.
    #[test]
    fn stale_indices_are_dropped_after_a_reload() {
        let mut set = SelectionSet::new();
        for i in 0..10 {
            set.toggle(i);
        }
        set.retain_valid(4);

        assert_eq!(set.len(), 4);
        assert_eq!(set.indices().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert!(!set.is_tagged(9));
    }

    /// Tagging everything skips `..`: copying the parent into a child of itself
    /// is never what was meant.
    #[test]
    fn tag_all_skips_the_parent_entry() {
        let entries = vec![
            crate::fs::FileEntry::parent(std::path::Path::new("/tmp")),
            crate::fs::FileEntry {
                name: "a".into(),
                path: "/tmp/a".into(),
                is_dir: true,
                is_symlink: false,
                is_parent: false,
                size: 0,
                modified: None,
            },
        ];
        let mut set = SelectionSet::new();
        set.tag_all(entries.len(), &entries);

        assert_eq!(set.indices().collect::<Vec<_>>(), vec![1]);
    }

    /// Tagging everything and then untagging it is a round trip back to empty.
    #[test]
    fn tag_all_then_clear_returns_to_empty() {
        let mut set = SelectionSet::new();
        set.tag_all(5, &[]);
        assert!(!set.is_empty());
        set.clear();
        assert!(set.is_empty());
    }

    /// Indices come back in row order, so the operation and the UI agree on
    /// which file is first.
    #[test]
    fn indices_are_in_row_order() {
        let mut set = SelectionSet::new();
        for i in [7, 2, 5, 0] {
            set.toggle(i);
        }
        assert_eq!(set.indices().collect::<Vec<_>>(), vec![0, 2, 5, 7]);
    }
}
