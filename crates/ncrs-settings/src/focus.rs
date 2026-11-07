//! The keyboard on the settings tab: two areas, the list of pages and the rows
//! of the page. A pure function turns a key into what happens, from the same
//! list of rows the page draws its focus frame by.

use ncrs_config::SettingsFeature;
use ncrs_ui_kit::setting_rows::RowKey;

use crate::message::SettingsMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Area {
    #[default]
    Pages,
    Rows,
}

/// Where the keyboard is. `row` counts the rows that have a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Focus {
    pub area: Area,
    pub row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusKey {
    Up,
    Down,
    Left,
    Right,
    /// Enter and Space.
    Press,
    Escape,
}

/// What a key does. Fields not named stay as they are.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub focus: Focus,
    pub select: Option<SettingsFeature>,
    /// The message of the row's button, switch or choice.
    pub press: Option<SettingsMessage>,
    /// A text field to give the keyboard to.
    pub focus_field: Option<&'static str>,
    /// A text field that may have the keyboard gives it up.
    pub release_field: bool,
}

impl Step {
    fn to(focus: Focus) -> Self {
        Self {
            focus,
            select: None,
            press: None,
            focus_field: None,
            release_field: false,
        }
    }
}

/// The step for `key`; `None` leaves the key to the tab as it was (Escape
/// closes it, Left and Right do nothing). `listed` are the pages in the list,
/// `current` the one shown and `rows` its rows that have a key.
pub fn step(
    focus: Focus,
    key: FocusKey,
    listed: &[SettingsFeature],
    current: SettingsFeature,
    rows: &[RowKey<SettingsMessage>],
) -> Option<Step> {
    match focus.area {
        Area::Pages => step_in_pages(key, listed, current, rows),
        Area::Rows if rows.is_empty() => Some(Step::to(Focus::default())),
        Area::Rows => Some(step_in_rows(focus, key, rows)),
    }
}

fn step_in_pages(
    key: FocusKey,
    listed: &[SettingsFeature],
    current: SettingsFeature,
    rows: &[RowKey<SettingsMessage>],
) -> Option<Step> {
    let at = listed.iter().position(|&page| page == current)?;
    let neighbour = |next: usize| {
        listed.get(next).map(|&page| Step {
            select: Some(page),
            ..Step::to(Focus::default())
        })
    };
    match key {
        FocusKey::Up => neighbour(at.checked_sub(1)?),
        FocusKey::Down => neighbour(at + 1),
        FocusKey::Right | FocusKey::Press if !rows.is_empty() => Some(Step::to(Focus {
            area: Area::Rows,
            row: 0,
        })),
        FocusKey::Left | FocusKey::Right | FocusKey::Press | FocusKey::Escape => None,
    }
}

fn step_in_rows(focus: Focus, key: FocusKey, rows: &[RowKey<SettingsMessage>]) -> Step {
    let row = focus.row.min(rows.len() - 1);
    let here = Focus {
        area: Area::Rows,
        row,
    };
    let to_pages = Step {
        release_field: true,
        ..Step::to(Focus::default())
    };
    let pressing = |message: &SettingsMessage| Step {
        press: Some(message.clone()),
        ..Step::to(here)
    };
    match (key, &rows[row]) {
        (FocusKey::Up, _) => Step {
            release_field: true,
            ..Step::to(Focus {
                row: row.saturating_sub(1),
                ..here
            })
        },
        (FocusKey::Down, _) => Step {
            release_field: true,
            ..Step::to(Focus {
                row: (row + 1).min(rows.len() - 1),
                ..here
            })
        },
        (FocusKey::Escape, _) => to_pages,
        (FocusKey::Left, RowKey::Choice { previous, .. }) => pressing(previous),
        (FocusKey::Left, _) => to_pages,
        (FocusKey::Right, RowKey::Choice { next, .. }) => pressing(next),
        (FocusKey::Right, _) => Step::to(here),
        (FocusKey::Press, RowKey::Press(message)) => pressing(message),
        (FocusKey::Press, RowKey::Choice { next, .. }) => pressing(next),
        (FocusKey::Press, RowKey::Text(id)) => Step {
            focus_field: Some(id),
            ..Step::to(here)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SettingsFeature::{Columns, Language, Look};

    fn rows() -> Vec<RowKey<SettingsMessage>> {
        vec![
            RowKey::Choice {
                previous: SettingsMessage::Close,
                next: SettingsMessage::Toggle,
            },
            RowKey::Press(SettingsMessage::ToggleFileListTags),
            RowKey::Text("field"),
        ]
    }

    const PAGES: [SettingsFeature; 3] = [Look, Language, Columns];
    const IN_ROWS: fn(usize) -> Focus = |row| Focus {
        area: Area::Rows,
        row,
    };

    fn pages(key: FocusKey, current: SettingsFeature) -> Option<Step> {
        step(Focus::default(), key, &PAGES, current, &rows())
    }

    fn in_rows(row: usize, key: FocusKey) -> Step {
        step(IN_ROWS(row), key, &PAGES, Look, &rows()).expect("handled")
    }

    #[test]
    fn up_and_down_in_the_list_open_the_neighbour_page() {
        assert_eq!(pages(FocusKey::Down, Look).unwrap().select, Some(Language));
        assert_eq!(pages(FocusKey::Up, Columns).unwrap().select, Some(Language));
        assert_eq!(pages(FocusKey::Up, Look), None);
        assert_eq!(pages(FocusKey::Down, Columns), None);
    }

    #[test]
    fn right_goes_into_the_page_to_its_first_row() {
        assert_eq!(pages(FocusKey::Right, Look).unwrap().focus, IN_ROWS(0));
        assert_eq!(pages(FocusKey::Press, Look).unwrap().focus, IN_ROWS(0));
    }

    #[test]
    fn a_page_without_rows_keeps_the_keyboard_in_the_list() {
        let none = step(Focus::default(), FocusKey::Right, &PAGES, Look, &[]);
        assert_eq!(none, None);
    }

    #[test]
    fn escape_in_the_list_is_left_to_the_tab() {
        assert_eq!(pages(FocusKey::Escape, Look), None);
    }

    #[test]
    fn up_and_down_in_the_rows_stop_at_the_ends() {
        assert_eq!(in_rows(0, FocusKey::Up).focus, IN_ROWS(0));
        assert_eq!(in_rows(1, FocusKey::Down).focus, IN_ROWS(2));
        assert_eq!(in_rows(2, FocusKey::Down).focus, IN_ROWS(2));
    }

    #[test]
    fn left_and_right_change_a_choice() {
        assert_eq!(
            in_rows(0, FocusKey::Left).press,
            Some(SettingsMessage::Close)
        );
        assert_eq!(
            in_rows(0, FocusKey::Right).press,
            Some(SettingsMessage::Toggle)
        );
        assert_eq!(in_rows(0, FocusKey::Left).focus, IN_ROWS(0));
    }

    #[test]
    fn left_on_any_other_row_goes_back_to_the_list() {
        assert_eq!(in_rows(1, FocusKey::Left).focus, Focus::default());
        assert_eq!(in_rows(2, FocusKey::Left).focus, Focus::default());
    }

    #[test]
    fn escape_goes_back_to_the_list_and_lets_go_of_a_field() {
        let back = in_rows(2, FocusKey::Escape);
        assert_eq!(back.focus, Focus::default());
        assert!(back.release_field);
    }

    #[test]
    fn enter_presses_and_focuses_a_text_field() {
        assert_eq!(
            in_rows(1, FocusKey::Press).press,
            Some(SettingsMessage::ToggleFileListTags)
        );
        assert_eq!(in_rows(2, FocusKey::Press).focus_field, Some("field"));
    }

    #[test]
    fn leaving_a_field_with_up_or_down_moves_to_the_next_row() {
        let up = in_rows(2, FocusKey::Up);
        assert_eq!((up.focus, up.release_field), (IN_ROWS(1), true));
    }
}
