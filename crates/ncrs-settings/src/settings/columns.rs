//! The edits of the Columns page, as functions of the `[columns]` setting.

use ncrs_config::panel_columns::PanelColumns;
use ncrs_core::{Column, PanelSide};

use crate::message::ColumnsEdit;

/// One row of the column list: a column and whether the panel shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnRow {
    pub column: Column,
    pub shown: bool,
}

/// The rows of the list `side` edits: the shown columns in their order, then
/// the others in the order of [`Column::ALL`]. Columns this system cannot show
/// are left out.
pub fn rows(columns: &PanelColumns, side: PanelSide) -> Vec<ColumnRow> {
    let shown = columns.of(side);
    let shown_rows = shown
        .iter()
        .copied()
        .filter(|column| column.on_this_system())
        .map(|column| ColumnRow {
            column,
            shown: true,
        });
    let hidden_rows = Column::ALL
        .into_iter()
        .filter(|column| column.on_this_system() && !shown.contains(column))
        .map(|column| ColumnRow {
            column,
            shown: false,
        });
    shown_rows.chain(hidden_rows).collect()
}

/// `current` after `edit` on the list of `side`. `ColumnsEdit::Side` changes
/// nothing in the setting.
pub fn edited(current: &PanelColumns, side: PanelSide, edit: ColumnsEdit) -> PanelColumns {
    let mut columns = current.clone();
    match edit {
        ColumnsEdit::SameInBoth(same) => {
            columns.same_in_both = same;
            // Both ways the panels start from what the left one shows.
            columns.right = columns.left.clone();
        }
        ColumnsEdit::Side(_) => {}
        ColumnsEdit::Date(format) => columns.date_format = format,
        ColumnsEdit::Size(format) => columns.size_format = format,
        ColumnsEdit::Toggle(column) => edit_list(&mut columns, side, |list| toggle(list, column)),
        ColumnsEdit::Up(column) => edit_list(&mut columns, side, |list| step(list, column, false)),
        ColumnsEdit::Down(column) => edit_list(&mut columns, side, |list| step(list, column, true)),
        // The list of the side and the widths; the formats stay.
        ColumnsEdit::Restore => {
            edit_list(&mut columns, side, |list| *list = Column::DEFAULT.to_vec());
            columns.widths.clear();
        }
    }
    columns
}

/// `current` with the list `side` edits replaced; with one list for both panels
/// that is the list of both.
pub fn with_list(current: &PanelColumns, side: PanelSide, list: Vec<Column>) -> PanelColumns {
    let mut columns = current.clone();
    edit_list(&mut columns, side, |shown| *shown = list);
    columns
}

fn edit_list(columns: &mut PanelColumns, side: PanelSide, edit: impl FnOnce(&mut Vec<Column>)) {
    let list = match side {
        PanelSide::Right if !columns.same_in_both => &mut columns.right,
        PanelSide::Left | PanelSide::Right => &mut columns.left,
    };
    edit(list);
    if columns.same_in_both {
        columns.right = columns.left.clone();
    }
}

fn toggle(list: &mut Vec<Column>, column: Column) {
    if column == Column::Name {
        return;
    }
    if let Some(at) = list.iter().position(|shown| *shown == column) {
        list.remove(at);
    } else if column == Column::Icon {
        list.insert(0, column);
    } else {
        list.push(column);
    }
}

/// Swaps `column` with the next shown column towards the end (`down`) or the
/// start; a column this system cannot show is stepped over.
fn step(list: &mut [Column], column: Column, down: bool) {
    let Some(at) = list.iter().position(|shown| *shown == column) else {
        return;
    };
    let can_show = |index: &usize| list.get(*index).is_some_and(|other| other.on_this_system());
    let to = if down {
        (at + 1..list.len()).find(can_show)
    } else {
        (0..at).rev().find(can_show)
    };
    if let Some(target) = to {
        list.swap(at, target);
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn per_panel() -> PanelColumns {
        PanelColumns {
            same_in_both: false,
            ..PanelColumns::default()
        }
    }

    #[test]
    fn the_rows_are_the_shown_columns_then_the_others() {
        let rows = rows(&PanelColumns::default(), PanelSide::Left);
        let shown: Vec<Column> = rows
            .iter()
            .filter(|row| row.shown)
            .map(|row| row.column)
            .collect();
        assert_eq!(shown, Column::DEFAULT);
        let first_hidden = rows.iter().position(|row| !row.shown).unwrap();
        assert!(rows[first_hidden..].iter().all(|row| !row.shown));
        assert_eq!(
            rows.len(),
            Column::ALL
                .into_iter()
                .filter(|column| column.on_this_system())
                .count()
        );
    }

    #[test]
    fn a_hidden_column_is_switched_on_at_the_end_and_off_again() {
        let on = edited(
            &PanelColumns::default(),
            PanelSide::Left,
            ColumnsEdit::Toggle(Column::Type),
        );
        assert_eq!(on.left.last(), Some(&Column::Type));
        let off = edited(&on, PanelSide::Left, ColumnsEdit::Toggle(Column::Type));
        assert_eq!(off.left, Column::DEFAULT);
    }

    #[test]
    fn the_icon_is_switched_on_in_front_and_the_name_stays() {
        let mut no_icon = PanelColumns::default();
        no_icon.left.remove(0);
        let on = edited(&no_icon, PanelSide::Left, ColumnsEdit::Toggle(Column::Icon));
        assert_eq!(on.left.first(), Some(&Column::Icon));
        let name = edited(&on, PanelSide::Left, ColumnsEdit::Toggle(Column::Name));
        assert_eq!(name.left, on.left);
    }

    #[test]
    fn up_and_down_swap_with_the_neighbour_and_stop_at_the_ends() {
        let start = PanelColumns::default();
        let down = edited(&start, PanelSide::Left, ColumnsEdit::Down(Column::Name));
        assert_eq!(
            down.left,
            [Column::Icon, Column::Size, Column::Name, Column::Modified]
        );
        let up = edited(&down, PanelSide::Left, ColumnsEdit::Up(Column::Name));
        assert_eq!(up.left, Column::DEFAULT);
        let first = edited(&start, PanelSide::Left, ColumnsEdit::Up(Column::Icon));
        assert_eq!(first.left, Column::DEFAULT);
        let last = edited(&start, PanelSide::Left, ColumnsEdit::Down(Column::Modified));
        assert_eq!(last.left, Column::DEFAULT);
    }

    #[test]
    fn restoring_puts_the_built_in_columns_back() {
        let mut changed = PanelColumns::default();
        changed.left = vec![Column::Name, Column::Created];
        changed.right = changed.left.clone();
        changed.widths.push((Column::Size, 90));
        let restored = edited(&changed, PanelSide::Left, ColumnsEdit::Restore);
        assert_eq!(restored.left, Column::DEFAULT);
        assert_eq!(restored.right, Column::DEFAULT);
        assert!(restored.widths.is_empty());
    }

    #[test]
    fn with_one_list_for_both_the_right_list_follows_the_left() {
        let edited = edited(
            &PanelColumns::default(),
            PanelSide::Right,
            ColumnsEdit::Toggle(Column::Type),
        );
        assert_eq!(edited.left, edited.right);
        assert!(edited.left.contains(&Column::Type));
    }

    #[test]
    fn per_panel_each_side_keeps_its_own_list() {
        let start = per_panel();
        let right = edited(&start, PanelSide::Right, ColumnsEdit::Toggle(Column::Type));
        assert!(right.right.contains(&Column::Type));
        assert_eq!(right.left, Column::DEFAULT);
        let left = edited(
            &right,
            PanelSide::Left,
            ColumnsEdit::Toggle(Column::Created),
        );
        assert!(left.left.contains(&Column::Created));
        assert!(!left.right.contains(&Column::Created));
        let restored = edited(&left, PanelSide::Right, ColumnsEdit::Restore);
        assert_eq!(restored.right, Column::DEFAULT);
        assert_eq!(restored.left, left.left);
    }

    #[test]
    fn turning_per_panel_on_starts_both_lists_alike() {
        let mut start = PanelColumns::default();
        start.left.push(Column::Created);
        let split = edited(&start, PanelSide::Left, ColumnsEdit::SameInBoth(false));
        assert!(!split.same_in_both);
        assert_eq!(split.right, split.left);
    }

    #[test]
    fn the_formats_are_set_and_the_side_changes_nothing() {
        use ncrs_core::{DateFormat, SizeFormat};
        let start = PanelColumns::default();
        let date = edited(
            &start,
            PanelSide::Left,
            ColumnsEdit::Date(DateFormat::Short),
        );
        assert_eq!(date.date_format, DateFormat::Short);
        let size = edited(&date, PanelSide::Left, ColumnsEdit::Size(SizeFormat::Bytes));
        assert_eq!(size.size_format, SizeFormat::Bytes);
        assert_eq!(
            edited(&start, PanelSide::Left, ColumnsEdit::Side(PanelSide::Right)),
            start
        );
    }
}
