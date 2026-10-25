//! State of the reorganisation plan view: the entries of the plan in their
//! groups, which of them are selected to be run, and the cursor. No widgets,
//! no tasks and no file access; `App` owns it, the view is
//! `ui::reorganisation_plan`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use ncrs_index::{Finding, PlanOutcome};

use crate::reorganisation_work::{deciding_finding, PlannedItem};

/// The kind of finding an entry is listed under. `Blocked` collects what has
/// no action to run, whatever its finding was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Duplicate,
    Misplaced,
    YearMismatch,
    NameOutlier,
    Blocked,
}

impl Group {
    pub const ALL: [Self; 5] = [
        Self::Duplicate,
        Self::Misplaced,
        Self::YearMismatch,
        Self::NameOutlier,
        Self::Blocked,
    ];

    pub fn of(planned: &PlannedItem) -> Self {
        if target(planned).is_none() {
            return Self::Blocked;
        }
        match deciding_finding(&planned.item) {
            Some(Finding::Duplicate { .. }) => Self::Duplicate,
            Some(Finding::Misplaced { .. }) => Self::Misplaced,
            Some(Finding::YearMismatch { .. }) => Self::YearMismatch,
            Some(Finding::NameOutlier { .. }) => Self::NameOutlier,
            None => Self::Blocked,
        }
    }
}

/// Where the entry would put the file, below its location. Only a move or a
/// rename can be run.
pub fn target(planned: &PlannedItem) -> Option<&Path> {
    match &planned.item.outcome {
        PlanOutcome::Move { target } | PlanOutcome::Rename { target } => Some(target),
        PlanOutcome::Conflict { .. } | PlanOutcome::NoAction(_) => None,
    }
}

/// A line of the list: a group heading or an entry (by its position in
/// `rows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Header(Group),
    Item(usize),
}

/// A line of a before or after tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeLine {
    pub depth: usize,
    pub name: String,
    pub is_folder: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReorganisationView {
    rows: Vec<PlannedItem>,
    lines: Vec<Line>,
    /// Positions in `rows`; only entries with a target are ever in it.
    selected: BTreeSet<usize>,
    cursor: usize,
    /// The plan is being worked out.
    busy: bool,
    problem: Option<String>,
}

impl ReorganisationView {
    /// Opens busy: the plan is made as the view opens.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            lines: Vec::new(),
            selected: BTreeSet::new(),
            cursor: 0,
            busy: true,
            problem: None,
        }
    }

    pub fn rows(&self) -> &[PlannedItem] {
        &self.rows
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn is_selected(&self, row: usize) -> bool {
        self.selected.contains(&row)
    }

    pub fn has_selection(&self) -> bool {
        !self.selected.is_empty()
    }

    /// A new plan is made; the list stays until it is replaced.
    pub fn begin_planning(&mut self) {
        self.busy = true;
        self.problem = None;
    }

    /// The plan the work returned. Everything with a target starts selected,
    /// except what rests on a rule the user has not confirmed.
    pub fn loaded(&mut self, rows: Vec<PlannedItem>) {
        self.lines = Group::ALL
            .iter()
            .flat_map(|&group| {
                let members: Vec<usize> = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, planned)| Group::of(planned) == group)
                    .map(|(position, _)| position)
                    .collect();
                let header = (!members.is_empty()).then_some(Line::Header(group));
                header
                    .into_iter()
                    .chain(members.into_iter().map(Line::Item))
                    .collect::<Vec<_>>()
            })
            .collect();
        self.selected = rows
            .iter()
            .enumerate()
            .filter(|(_, planned)| target(planned).is_some() && !planned.unconfirmed_rule)
            .map(|(position, _)| position)
            .collect();
        self.rows = rows;
        self.cursor = 0;
        self.busy = false;
        self.problem = None;
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.problem = Some(detail);
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.lines.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }

    /// Selects or deselects what the line stands for: an entry, or every entry
    /// of a group. Entries without a target cannot be selected. The cursor
    /// moves to the line.
    pub fn toggle(&mut self, line: usize) {
        let Some(&kind) = self.lines.get(line) else {
            return;
        };
        self.cursor = line;
        if self.busy {
            return;
        }
        match kind {
            Line::Item(row) => {
                if self.can_select(row) && !self.selected.remove(&row) {
                    self.selected.insert(row);
                }
            }
            Line::Header(group) => {
                let members = self.selectable_in(group);
                if members.iter().all(|row| self.selected.contains(row)) {
                    for row in &members {
                        self.selected.remove(row);
                    }
                } else {
                    self.selected.extend(members);
                }
            }
        }
    }

    pub fn toggle_cursor(&mut self) {
        self.toggle(self.cursor);
    }

    /// How many entries of the group are selected, and how many can be.
    pub fn counts(&self, group: Group) -> (usize, usize) {
        let members = self.selectable_in(group);
        let selected = members
            .iter()
            .filter(|row| self.selected.contains(row))
            .count();
        (selected, members.len())
    }

    /// The entries to run, in plan order.
    pub fn selection(&self) -> Vec<PlannedItem> {
        self.selected
            .iter()
            .filter_map(|&row| self.rows.get(row).cloned())
            .collect()
    }

    /// The folders and files the selected entries touch, as they are now.
    pub fn before_tree(&self) -> Vec<TreeLine> {
        tree_lines(
            self.selected_rows()
                .map(|planned| in_location(planned, &planned.item.source))
                .collect(),
        )
    }

    /// The same, as they would be after running the selection.
    pub fn after_tree(&self) -> Vec<TreeLine> {
        tree_lines(
            self.selected_rows()
                .filter_map(|planned| target(planned).map(|target| in_location(planned, target)))
                .collect(),
        )
    }

    fn selected_rows(&self) -> impl Iterator<Item = &PlannedItem> {
        self.selected.iter().filter_map(|&row| self.rows.get(row))
    }

    fn can_select(&self, row: usize) -> bool {
        self.rows
            .get(row)
            .is_some_and(|planned| target(planned).is_some())
    }

    fn selectable_in(&self, group: Group) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, planned)| target(planned).is_some() && Group::of(planned) == group)
            .map(|(position, _)| position)
            .collect()
    }
}

/// A path below its location, led by the name of the location's folder so
/// that two locations do not run together.
fn in_location(planned: &PlannedItem, below: &Path) -> PathBuf {
    planned
        .location
        .file_name()
        .map_or_else(|| below.to_path_buf(), |name| Path::new(name).join(below))
}

/// Folders and files of `paths` as indented lines: every folder once, before
/// what is in it, in path order.
pub fn tree_lines(mut paths: Vec<PathBuf>) -> Vec<TreeLine> {
    paths.sort();
    paths.dedup();
    let mut lines = Vec::new();
    let mut open: Vec<OsString> = Vec::new();
    for path in &paths {
        let parts: Vec<OsString> = path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_owned()),
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => None,
            })
            .collect();
        let Some((file, folders)) = parts.split_last() else {
            continue;
        };
        let shared = open
            .iter()
            .zip(folders)
            .take_while(|(shown, wanted)| shown == wanted)
            .count();
        for (depth, folder) in folders.iter().enumerate().skip(shared) {
            lines.push(TreeLine {
                depth,
                name: folder.to_string_lossy().into_owned(),
                is_folder: true,
            });
        }
        open = folders.to_vec();
        lines.push(TreeLine {
            depth: folders.len(),
            name: file.to_string_lossy().into_owned(),
            is_folder: false,
        });
    }
    lines
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
pub mod tests {
    use super::*;
    use crate::reorganisation_work::tests::{item, sample_items};

    pub fn loaded_view() -> ReorganisationView {
        let mut view = ReorganisationView::new();
        view.loaded(sample_items());
        view
    }

    fn position(view: &ReorganisationView, source: &str) -> usize {
        view.rows()
            .iter()
            .position(|planned| planned.item.source == Path::new(source))
            .unwrap()
    }

    fn line_of(view: &ReorganisationView, source: &str) -> usize {
        let row = position(view, source);
        view.lines()
            .iter()
            .position(|line| *line == Line::Item(row))
            .unwrap()
    }

    fn header_of(view: &ReorganisationView, group: Group) -> usize {
        view.lines()
            .iter()
            .position(|line| *line == Line::Header(group))
            .unwrap()
    }

    fn names(lines: &[TreeLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                format!(
                    "{}{}{}",
                    "  ".repeat(line.depth),
                    line.name,
                    if line.is_folder { "/" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn the_entries_are_listed_under_the_kind_of_finding_that_decides() {
        let items = sample_items();

        let group = |source: &str| Group::of(item(&items, source));

        assert_eq!(group("Misc/2024-04-01_invoice.pdf"), Group::Duplicate);
        assert_eq!(
            group("Insurance/2024/2024-07-01_invoice.pdf"),
            Group::Misplaced
        );
        assert_eq!(
            group("Insurance/2024/2024-08-01_invoice.pdf"),
            Group::Misplaced
        );
        assert_eq!(
            group("Insurance/2023/2024-05-01_invoice.pdf"),
            Group::YearMismatch
        );
        assert_eq!(group("Insurance/2024/scan 0042.pdf"), Group::NameOutlier);
        assert_eq!(
            group("Insurance/2023/2024-06-01_invoice.pdf"),
            Group::Blocked
        );
        assert_eq!(group("Insurance/2024/IMG_7.pdf"), Group::Blocked);
    }

    #[test]
    fn everything_with_a_target_starts_selected_but_what_rests_on_an_unconfirmed_rule() {
        let view = loaded_view();

        let selected = |source: &str| view.is_selected(position(&view, source));

        assert!(selected("Misc/2024-04-01_invoice.pdf"));
        assert!(selected("Insurance/2024/2024-07-01_invoice.pdf"));
        assert!(selected("Insurance/2023/2024-05-01_invoice.pdf"));
        assert!(selected("Insurance/2024/scan 0042.pdf"));
        assert!(
            !selected("Insurance/2024/2024-08-01_invoice.pdf"),
            "unconfirmed rule"
        );
        assert!(
            !selected("Insurance/2023/2024-06-01_invoice.pdf"),
            "conflict"
        );
        assert!(!selected("Insurance/2024/IMG_7.pdf"), "no action");
    }

    #[test]
    fn an_entry_is_toggled_and_one_without_a_target_cannot_be() {
        let mut view = loaded_view();
        let year = line_of(&view, "Insurance/2023/2024-05-01_invoice.pdf");
        let conflict = line_of(&view, "Insurance/2023/2024-06-01_invoice.pdf");

        view.toggle(year);
        view.toggle(conflict);

        assert!(!view.is_selected(position(&view, "Insurance/2023/2024-05-01_invoice.pdf")));
        assert!(!view.is_selected(position(&view, "Insurance/2023/2024-06-01_invoice.pdf")));
        view.toggle(year);
        assert!(view.is_selected(position(&view, "Insurance/2023/2024-05-01_invoice.pdf")));
    }

    #[test]
    fn a_group_is_toggled_as_a_whole() {
        let mut view = loaded_view();
        let header = header_of(&view, Group::Misplaced);
        assert_eq!(view.counts(Group::Misplaced), (1, 2));

        view.toggle(header);
        assert_eq!(
            view.counts(Group::Misplaced),
            (2, 2),
            "a part selected: all"
        );

        view.toggle(header);
        assert_eq!(view.counts(Group::Misplaced), (0, 2), "all selected: none");
    }

    #[test]
    fn the_group_of_what_cannot_be_done_has_nothing_to_select() {
        let mut view = loaded_view();

        view.toggle(header_of(&view, Group::Blocked));

        assert_eq!(view.counts(Group::Blocked), (0, 0));
    }

    #[test]
    fn the_selection_is_what_is_handed_over_and_empty_when_nothing_is_selected() {
        let mut view = loaded_view();

        let handed = view.selection();

        assert_eq!(handed.len(), 4);
        assert!(handed.iter().all(|planned| target(planned).is_some()));
        for group in Group::ALL {
            let Some(header) = header_of_or_none(&view, group) else {
                continue;
            };
            // A group with a part selected selects the rest first.
            while view.counts(group).0 > 0 {
                view.toggle(header);
            }
        }
        assert!(!view.has_selection());
        assert!(view.selection().is_empty());
    }

    fn header_of_or_none(view: &ReorganisationView, group: Group) -> Option<usize> {
        view.lines()
            .iter()
            .position(|line| *line == Line::Header(group))
    }

    #[test]
    fn a_selected_move_is_in_the_after_tree_and_a_deselected_one_is_not() {
        let mut view = loaded_view();
        let year = line_of(&view, "Insurance/2023/2024-05-01_invoice.pdf");

        let before = names(&view.before_tree());
        let after_selected = names(&view.after_tree());
        assert!(
            before.contains(&"      2024-05-01_invoice.pdf".to_owned()),
            "{before:?}"
        );
        assert!(
            after_selected.contains(&"      2024-05-01_invoice.pdf".to_owned()),
            "{after_selected:?}"
        );

        view.toggle(year);

        let after = names(&view.after_tree());
        assert!(
            !after.iter().any(|line| line.contains("2024-05-01")),
            "{after:?}"
        );
    }

    #[test]
    fn the_trees_name_each_folder_once_before_what_is_in_it() {
        let lines = tree_lines(vec![
            PathBuf::from("Documents/Insurance/2024/b.pdf"),
            PathBuf::from("Documents/Insurance/2024/a.pdf"),
            PathBuf::from("Documents/Taxes/c.pdf"),
        ]);

        assert_eq!(
            names(&lines),
            [
                "Documents/",
                "  Insurance/",
                "    2024/",
                "      a.pdf",
                "      b.pdf",
                "  Taxes/",
                "    c.pdf",
            ]
        );
    }

    #[test]
    fn the_cursor_stays_in_the_list_and_a_new_plan_resets_it() {
        let mut view = loaded_view();

        view.move_cursor(-1);
        assert_eq!(view.cursor(), 0);
        view.move_cursor(1000);
        assert_eq!(view.cursor(), view.lines().len() - 1);

        view.begin_planning();
        view.loaded(sample_items());
        assert_eq!(view.cursor(), 0);
    }

    #[test]
    fn nothing_is_toggled_while_the_plan_is_made() {
        let mut view = loaded_view();
        let before = view.selection();

        view.begin_planning();
        view.toggle(header_of(&view, Group::Duplicate));

        assert_eq!(view.selection(), before);
    }

    #[test]
    fn a_failure_is_shown_and_ends_the_busy_state() {
        let mut view = ReorganisationView::new();
        assert!(view.is_busy());

        view.failed("index not open".to_owned());

        assert!(!view.is_busy());
        assert_eq!(view.problem(), Some("index not open"));
    }
}
