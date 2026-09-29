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
//!
//! Tags are **names, not indices**. An index is a position in a list that a
//! directory read replaces wholesale, so index 3 after a reload is a different
//! file than index 3 before it — and F8 would delete the wrong one. A name
//! survives the reload, or the file is genuinely gone and the tag should be.
use std::collections::BTreeSet;
use std::ffi::OsString;

use crate::fs::FileEntry;

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

/// Which rows of one panel a file operation applies to, addressed by name.
///
/// Ordering follows the name, not the row order, so `BTreeSet` is only used for
/// cheap membership and de-duplication. `ordered` gives the names back in the
/// order the rows appear, which is what the operation and the UI both need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionSet {
    tagged: BTreeSet<OsString>,
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
    pub fn toggle(&mut self, name: &OsString) -> bool {
        if self.tagged.remove(name) {
            false
        } else {
            self.tagged.insert(name.clone());
            true
        }
    }

    #[allow(dead_code)]
    pub fn is_tagged(&self, name: &OsString) -> bool {
        self.tagged.contains(name)
    }

    /// Drops every tag. `Ctrl+*` in NC.
    pub fn clear(&mut self) {
        self.tagged.clear();
    }

    /// The tagged names in row order, so the operation and the UI agree on
    /// which file is first.
    #[allow(dead_code)]
    pub fn ordered<'a>(&self, entries: &'a [FileEntry]) -> Vec<&'a FileEntry> {
        entries
            .iter()
            .filter(|e| self.tagged.contains(&e.name))
            .collect()
    }

    /// The state of one row, given where the cursor is.
    pub fn state_of(&self, index: usize, entries: &[FileEntry], cursor: usize) -> Selection {
        Selection {
            tagged: entries
                .get(index)
                .is_some_and(|e| self.tagged.contains(&e.name)),
            cursor: index == cursor,
        }
    }

    /// Whether anything is tagged, which is what decides whether the cursor row
    /// takes part in the operation.
    #[allow(dead_code)]
    pub fn any_tagged(&self) -> bool {
        !self.tagged.is_empty()
    }

    /// Removes tags for files the new listing does not contain.
    ///
    /// A reload replaces the entries, so a tag whose name is gone points at a
    /// file that no longer exists. Dropping it is the safe direction: a tag the
    /// user has to set again is better than an operation touching a file they
    /// never selected.
    pub fn retain_present(&mut self, entries: &[FileEntry]) {
        if self.tagged.is_empty() {
            return;
        }
        // BUG: verhaelt sich wie Index-Tagging — ein Tag "ueberlebt", solange
        // die neue Liste lang genug ist, egal welcher Name dort steht.
        self.tagged
            .retain(|name| entries.iter().any(|e| &e.name == name));
    }

    /// Tags every row in `entries`, as `*` does. The `..` entry is never
    /// tagged: copying the parent directory into a subdirectory of itself is
    /// not a thing anyone wants.
    pub fn tag_all(&mut self, entries: &[FileEntry]) {
        self.tagged.clear();
        for entry in entries {
            if !entry.is_parent {
                self.tagged.insert(entry.name.clone());
            }
        }
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
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str, is_parent: bool) -> FileEntry {
        FileEntry {
            name: OsString::from(name),
            path: PathBuf::from(name),
            is_dir: !is_parent,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
        }
    }

    fn listing() -> Vec<FileEntry> {
        vec![
            entry("..", true),
            entry("alpha", false),
            entry("beta", false),
            entry("gamma", false),
        ]
    }

    #[test]
    fn toggling_adds_then_removes() {
        let mut set = SelectionSet::new();
        let name = OsString::from("beta");
        assert!(set.toggle(&name), "first toggle tags the row");
        assert!(set.is_tagged(&name));
        assert!(!set.toggle(&name), "second toggle untags it");
        assert!(!set.is_tagged(&name));
        assert!(set.is_empty());
    }

    /// The rule that makes multi-selection worth having: tags win over the
    /// cursor. Without it, "copy" would be a coin toss.
    #[test]
    fn tags_take_precedence_over_the_cursor() {
        let entries = listing();
        let mut set = SelectionSet::new();
        set.toggle(&OsString::from("alpha"));
        set.toggle(&OsString::from("beta"));

        let cursor_row = set.state_of(3, &entries, 3);
        assert!(
            !cursor_row.in_operation(set.any_tagged()),
            "the cursor row is not part of the operation while rows are tagged"
        );
        for tagged in [1, 2] {
            assert!(set.state_of(tagged, &entries, 3).in_operation(true));
        }
    }

    /// With nothing tagged, the operation applies to the cursor row alone —
    /// which is what makes a plain F5 on one file work.
    #[test]
    fn without_tags_the_cursor_row_is_the_operation() {
        let entries = listing();
        let set = SelectionSet::new();
        assert!(set.state_of(2, &entries, 2).in_operation(false));
        assert!(!set.state_of(1, &entries, 2).in_operation(false));
    }

    /// A row can be tagged and under the cursor at once, and it still takes
    /// part in the operation.
    #[test]
    fn a_row_can_be_both_tagged_and_the_cursor() {
        let entries = listing();
        let mut set = SelectionSet::new();
        set.toggle(&OsString::from("beta"));
        let state = set.state_of(2, &entries, 2);
        assert!(state.tagged && state.cursor);
        assert!(state.in_operation(true));
    }

    /// The reason tags are names. After a reload the rows are a different set
    /// in a different order; a tag on "beta" still means "beta", while an index
    /// would silently point at whatever took that position.
    #[test]
    fn a_tag_survives_a_reordered_reload() {
        let mut set = SelectionSet::new();
        set.toggle(&OsString::from("beta"));

        // Same files, different order, one new file.
        let reloaded = vec![
            entry("..", true),
            entry("delta", false),
            entry("beta", false),
            entry("alpha", false),
        ];
        set.retain_present(&reloaded);

        assert!(set.is_tagged(&OsString::from("beta")), "the tag was lost");
        let tagged_rows: Vec<&str> = set
            .ordered(&reloaded)
            .iter()
            .map(|e| e.name.to_str().unwrap())
            .collect();
        assert_eq!(
            tagged_rows,
            vec!["beta"],
            "it must point at beta, not at a position"
        );
    }

    /// A tag whose file is gone has to be dropped: keeping it would make an
    /// operation act on whatever inherited the name.
    #[test]
    fn a_tag_for_a_vanished_file_is_dropped() {
        let mut set = SelectionSet::new();
        set.toggle(&OsString::from("beta"));
        set.toggle(&OsString::from("gone"));

        // "beta" is still there, "gone" is not.
        let reloaded = vec![
            entry("..", true),
            entry("alpha", false),
            entry("beta", false),
        ];
        set.retain_present(&reloaded);

        assert!(
            !set.is_tagged(&OsString::from("gone")),
            "a tag for a file that no longer exists survived"
        );
        assert!(set.is_tagged(&OsString::from("beta")));
    }

    /// A name replaced by a different file of the same name keeps the tag. That
    /// is correct: the user selected the name, and the directory has one row per
    /// name.
    #[test]
    fn a_recreated_file_keeps_the_tag() {
        let mut set = SelectionSet::new();
        set.toggle(&OsString::from("beta"));

        let reloaded = vec![entry("..", true), entry("beta", false)];
        set.retain_present(&reloaded);

        assert!(set.is_tagged(&OsString::from("beta")));
    }

    /// Tagging everything skips `..`: copying the parent into a child of itself
    /// is never what was meant.
    #[test]
    fn tag_all_skips_the_parent_entry() {
        let entries = listing();
        let mut set = SelectionSet::new();
        set.tag_all(&entries);

        let tagged: Vec<&str> = set
            .ordered(&entries)
            .iter()
            .map(|e| e.name.to_str().unwrap())
            .collect();
        assert_eq!(tagged, vec!["alpha", "beta", "gamma"]);
    }

    /// Tagging everything and then clearing returns to empty.
    #[test]
    fn tag_all_then_clear_returns_to_empty() {
        let mut set = SelectionSet::new();
        set.tag_all(&listing());
        assert!(!set.is_empty());
        set.clear();
        assert!(set.is_empty());
    }

    /// The operation walks the rows in the order the user sees them, not in
    /// name order, so the first tagged file is the one under the cursor area.
    #[test]
    fn ordered_follows_the_listing_not_the_name() {
        let entries = vec![
            entry("..", true),
            entry("zulu", false),
            entry("alpha", false),
        ];
        let mut set = SelectionSet::new();
        set.tag_all(&entries);

        let tagged: Vec<&str> = set
            .ordered(&entries)
            .iter()
            .map(|e| e.name.to_str().unwrap())
            .collect();
        assert_eq!(tagged, vec!["zulu", "alpha"]);
    }
}
