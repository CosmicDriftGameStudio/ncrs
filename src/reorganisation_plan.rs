//! State of the reorganisation plan view: the entries of the plan in their
//! groups, which of them are selected to be run, and the cursor. No widgets,
//! no tasks and no file access; `App` owns it, the view is
//! `ui::reorganisation_plan`.

use std::collections::BTreeSet;
use std::path::Path;

use iced::mouse::ScrollDelta;

use ncrs_index::{Finding, PlanOutcome};

use crate::list_window::ListWindow;
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

/// Most lines of the plan shown at once; the list follows the cursor.
pub const VISIBLE_LINES: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct ReorganisationView {
    rows: Vec<PlannedItem>,
    lines: Vec<Line>,
    /// Positions in `rows`; only entries with a target are ever in it.
    selected: BTreeSet<usize>,
    cursor: usize,
    window: ListWindow,
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
            window: ListWindow::default(),
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

    /// The first line of the plan that is shown.
    pub fn first_visible(&self) -> usize {
        self.window.offset(self.lines.len(), VISIBLE_LINES)
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
            .filter(|(_, planned)| target(planned).is_some() && !planned.unconfirmed_rule())
            .map(|(position, _)| position)
            .collect();
        self.rows = rows;
        self.cursor = 0;
        self.window = ListWindow::default();
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
        self.window
            .ensure_visible(self.cursor, self.lines.len(), VISIBLE_LINES);
    }

    /// The wheel moves the window; the cursor follows it when it would leave.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        self.window
            .scroll(delta, self.lines.len(), VISIBLE_LINES, &mut self.cursor);
    }

    /// Puts the cursor on the line without changing the selection.
    pub fn select(&mut self, line: usize) {
        if line < self.lines.len() {
            self.cursor = line;
            self.window
                .ensure_visible(self.cursor, self.lines.len(), VISIBLE_LINES);
        }
    }

    /// How many entries are selected to be run.
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// How many entries can be run at all.
    pub fn runnable_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|planned| target(planned).is_some())
            .count()
    }

    /// How many entries cannot be run.
    pub fn blocked_count(&self) -> usize {
        self.rows.len().saturating_sub(self.runnable_count())
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
    use crate::reorganisation_work::fixtures::{item, sample_items};

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
    fn selecting_a_line_moves_the_cursor_and_keeps_the_selection() {
        let mut view = loaded_view();
        let before = view.selection();
        let year = line_of(&view, "Insurance/2023/2024-05-01_invoice.pdf");

        view.select(year);

        assert_eq!(view.cursor(), year);
        assert_eq!(view.selection(), before);
        view.select(1000);
        assert_eq!(view.cursor(), year);
    }

    #[test]
    fn the_counts_tell_what_can_and_cannot_run() {
        let view = loaded_view();

        assert_eq!(view.selected_count(), 4);
        assert_eq!(view.runnable_count(), 5);
        assert_eq!(view.blocked_count(), 2);
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
