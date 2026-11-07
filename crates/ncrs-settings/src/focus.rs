//! The keyboard on the settings tab: two areas, the list of pages and the rows
//! of the page. A pure function turns a key into what happens, from the same
//! list of rows the page draws its focus frame by.

use ncrs_config::SettingsFeature;
use ncrs_ui_kit::setting_rows::RowKey;

#[cfg(test)]
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
    /// The highlighted row of the open list of a dropdown row.
    pub open: Option<usize>,
    /// A text field of the row has the keyboard, so Tab stays in the page.
    pub field: bool,
    /// The button of a row with several that Enter presses.
    pub button: usize,
}

impl Focus {
    pub fn in_rows(row: usize) -> Self {
        Self {
            area: Area::Rows,
            row,
            ..Self::default()
        }
    }

    /// Where the rows draw their focus, while the keyboard is in them.
    pub fn row_focus(self) -> Option<ncrs_ui_kit::setting_rows::RowFocus> {
        (self.area == Area::Rows).then_some(ncrs_ui_kit::setting_rows::RowFocus {
            row: self.row,
            open: self.open,
            button: self.button,
        })
    }
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
pub struct Step<M> {
    pub focus: Focus,
    pub select: Option<SettingsFeature>,
    /// The message of the row's button, switch or choice.
    pub press: Option<M>,
    /// A text field to give the keyboard to.
    pub focus_field: Option<&'static str>,
    /// A text field that may have the keyboard gives it up.
    pub release_field: bool,
}

impl<M: Clone> Step<M> {
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
pub fn step<M: Clone>(
    focus: Focus,
    key: FocusKey,
    listed: &[SettingsFeature],
    current: SettingsFeature,
    rows: &[RowKey<M>],
) -> Option<Step<M>> {
    match focus.area {
        Area::Pages => step_in_pages(key, listed, current, rows),
        Area::Rows if rows.is_empty() => Some(Step::to(Focus::default())),
        Area::Rows => Some(step_in_rows(focus, key, rows)),
    }
}

fn step_in_pages<M: Clone>(
    key: FocusKey,
    listed: &[SettingsFeature],
    current: SettingsFeature,
    rows: &[RowKey<M>],
) -> Option<Step<M>> {
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
        FocusKey::Right | FocusKey::Press if !rows.is_empty() => Some(Step::to(Focus::in_rows(0))),
        FocusKey::Left | FocusKey::Right | FocusKey::Press | FocusKey::Escape => None,
    }
}

fn step_in_rows<M: Clone>(focus: Focus, key: FocusKey, rows: &[RowKey<M>]) -> Step<M> {
    let row = focus.row.min(rows.len() - 1);
    let here = Focus::in_rows(row);
    let to_pages = Step {
        release_field: true,
        ..Step::to(Focus::default())
    };
    let pressing = |message: &M| Step {
        press: Some(message.clone()),
        ..Step::to(here)
    };
    if let (Some(highlighted), RowKey::Dropdown { choices, .. }) = (focus.open, &rows[row]) {
        return step_in_open_list(here, highlighted, key, choices);
    }
    match (key, &rows[row]) {
        (FocusKey::Up, _) => Step {
            release_field: true,
            ..Step::to(Focus::in_rows(row.saturating_sub(1)))
        },
        (FocusKey::Down, _) => Step {
            release_field: true,
            ..Step::to(Focus::in_rows((row + 1).min(rows.len() - 1)))
        },
        (FocusKey::Escape, _) => to_pages,
        (FocusKey::Left, RowKey::Choice { previous, .. }) => pressing(previous),
        (
            FocusKey::Left,
            RowKey::Dropdown {
                choices, selected, ..
            },
        ) => choices
            .get(selected.wrapping_sub(1))
            .map_or_else(|| Step::to(here), pressing),
        (FocusKey::Left, RowKey::Buttons(_)) if focus.button > 0 => Step::to(Focus {
            button: focus.button - 1,
            ..here
        }),
        (FocusKey::Left, _) => to_pages,
        (FocusKey::Right, RowKey::Buttons(messages)) => Step::to(Focus {
            button: (focus.button + 1).min(messages.len() - 1),
            ..here
        }),
        (FocusKey::Right, RowKey::Choice { next, .. }) => pressing(next),
        (
            FocusKey::Right,
            RowKey::Dropdown {
                choices, selected, ..
            },
        ) => choices
            .get(selected + 1)
            .map_or_else(|| Step::to(here), pressing),
        (FocusKey::Right, _) => Step::to(here),
        (FocusKey::Press, RowKey::Press(message)) => pressing(message),
        (FocusKey::Press, RowKey::Buttons(messages)) => {
            let button = focus.button.min(messages.len() - 1);
            Step {
                press: Some(messages[button].clone()),
                ..Step::to(Focus { button, ..here })
            }
        }
        (FocusKey::Press, RowKey::Choice { next, .. }) => pressing(next),
        (FocusKey::Press, RowKey::Dropdown { selected, .. }) => Step::to(Focus {
            open: Some(*selected),
            ..here
        }),
        (FocusKey::Press, RowKey::Text(id)) => Step {
            focus_field: Some(id),
            ..Step::to(Focus {
                field: true,
                ..here
            })
        },
    }
}

fn step_in_open_list<M: Clone>(
    here: Focus,
    highlighted: usize,
    key: FocusKey,
    choices: &[M],
) -> Step<M> {
    let highlight = |at: usize| {
        Step::to(Focus {
            open: Some(at),
            ..here
        })
    };
    match key {
        FocusKey::Up => highlight(highlighted.saturating_sub(1)),
        FocusKey::Down => highlight((highlighted + 1).min(choices.len().saturating_sub(1))),
        FocusKey::Press => Step {
            press: choices.get(highlighted).cloned(),
            ..Step::to(here)
        },
        FocusKey::Escape | FocusKey::Left | FocusKey::Right => Step::to(here),
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
            RowKey::Dropdown {
                labels: vec!["a".into(), "b".into(), "c".into()],
                choices: vec![
                    SettingsMessage::Close,
                    SettingsMessage::Toggle,
                    SettingsMessage::ToggleFileListTags,
                ],
                selected: 1,
            },
        ]
    }

    const PAGES: [SettingsFeature; 3] = [Look, Language, Columns];
    const IN_ROWS: fn(usize) -> Focus = Focus::in_rows;

    fn pages(key: FocusKey, current: SettingsFeature) -> Option<Step<SettingsMessage>> {
        step(Focus::default(), key, &PAGES, current, &rows())
    }

    fn in_rows(row: usize, key: FocusKey) -> Step<SettingsMessage> {
        step(IN_ROWS(row), key, &PAGES, Look, &rows()).expect("handled")
    }

    #[test]
    fn up_and_down_in_the_list_open_the_neighbour_page() {
        assert_eq!(pages(FocusKey::Down, Look).unwrap().select, Some(Language));
        assert_eq!(pages(FocusKey::Up, Columns).unwrap().select, Some(Language));
        assert_eq!(pages(FocusKey::Up, Look), None);
        assert_eq!(pages(FocusKey::Down, Columns), None);
    }

    fn on_button(button: usize, key: FocusKey) -> Step<SettingsMessage> {
        let rows = vec![
            RowKey::Press(SettingsMessage::Close),
            RowKey::Buttons(vec![
                SettingsMessage::Toggle,
                SettingsMessage::ToggleFileListTags,
            ]),
        ];
        let focus = Focus {
            button,
            ..IN_ROWS(1)
        };
        step(focus, key, &PAGES, Look, &rows).expect("handled")
    }

    #[test]
    fn left_and_right_choose_the_button_of_a_row() {
        assert_eq!(on_button(0, FocusKey::Right).focus.button, 1);
        assert_eq!(on_button(1, FocusKey::Right).focus.button, 1);
        assert_eq!(on_button(1, FocusKey::Left).focus.button, 0);
        assert_eq!(on_button(1, FocusKey::Left).focus.area, Area::Rows);
    }

    #[test]
    fn left_on_the_first_button_goes_back_to_the_pages() {
        assert_eq!(on_button(0, FocusKey::Left).focus.area, Area::Pages);
    }

    #[test]
    fn enter_presses_the_chosen_button_and_a_row_change_resets_it() {
        assert_eq!(
            on_button(0, FocusKey::Press).press,
            Some(SettingsMessage::Toggle)
        );
        let second = on_button(1, FocusKey::Press);
        assert_eq!(second.press, Some(SettingsMessage::ToggleFileListTags));
        assert_eq!(second.focus.button, 1);
        assert_eq!(on_button(1, FocusKey::Down).focus.button, 0);
        assert_eq!(on_button(1, FocusKey::Up).focus.button, 0);
    }

    #[test]
    fn right_goes_into_the_page_to_its_first_row() {
        assert_eq!(pages(FocusKey::Right, Look).unwrap().focus, IN_ROWS(0));
        assert_eq!(pages(FocusKey::Press, Look).unwrap().focus, IN_ROWS(0));
    }

    #[test]
    fn a_page_without_rows_keeps_the_keyboard_in_the_list() {
        let none = step(
            Focus::default(),
            FocusKey::Right,
            &PAGES,
            Look,
            &[] as &[RowKey<SettingsMessage>],
        );
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
        assert_eq!(in_rows(3, FocusKey::Down).focus, IN_ROWS(3));
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

    fn open_at(highlighted: usize) -> Focus {
        Focus {
            open: Some(highlighted),
            ..IN_ROWS(3)
        }
    }

    fn in_open_list(highlighted: usize, key: FocusKey) -> Step<SettingsMessage> {
        step(open_at(highlighted), key, &PAGES, Look, &rows()).expect("handled")
    }

    #[test]
    fn enter_opens_a_dropdown_on_its_current_value() {
        assert_eq!(in_rows(3, FocusKey::Press).focus, open_at(1));
    }

    #[test]
    fn in_an_open_list_up_and_down_choose_and_enter_takes_the_row() {
        assert_eq!(in_open_list(1, FocusKey::Down).focus, open_at(2));
        assert_eq!(in_open_list(2, FocusKey::Down).focus, open_at(2));
        assert_eq!(in_open_list(0, FocusKey::Up).focus, open_at(0));
        let taken = in_open_list(2, FocusKey::Press);
        assert_eq!(taken.press, Some(SettingsMessage::ToggleFileListTags));
        assert_eq!(taken.focus, IN_ROWS(3));
    }

    #[test]
    fn escape_closes_the_list_and_stays_on_the_row() {
        assert_eq!(in_open_list(2, FocusKey::Escape).focus, IN_ROWS(3));
    }

    #[test]
    fn left_and_right_change_a_dropdown_without_opening_it() {
        assert_eq!(
            in_rows(3, FocusKey::Left).press,
            Some(SettingsMessage::Close)
        );
        assert_eq!(
            in_rows(3, FocusKey::Right).press,
            Some(SettingsMessage::ToggleFileListTags)
        );
    }

    #[test]
    fn a_text_field_row_marks_the_field_as_having_the_keys() {
        assert!(in_rows(2, FocusKey::Press).focus.field);
        assert!(!in_rows(2, FocusKey::Up).focus.field);
    }
}
