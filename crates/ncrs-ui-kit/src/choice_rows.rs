//! Rows that pick one of a few values or switch a list entry on and off.

use iced::widget::Space;
use iced::{Element, Length};

use crate::hermes_kit::{icon_button, CONTROL_HEIGHT};
use crate::palette::Palette;
use crate::setting_rows::{status_text, switch, SettingRow};
use ncrs_i18n::Language;

/// What the row of one entry of a reorderable list shows and sends.
#[derive(Debug, Clone)]
pub struct ListEntry<M> {
    pub label: String,
    pub description: String,
    pub on: bool,
    /// A word in place of the switch for an entry that cannot be switched off.
    pub always: Option<String>,
    pub on_toggle: M,
    /// `None` for the first entry, which cannot move up.
    pub on_up: Option<M>,
    /// `None` for the last entry, which cannot move down.
    pub on_down: Option<M>,
}

/// The row of an entry of a list the user switches on and off and puts in
/// order: the switch, then the two arrows. The arrows an entry cannot use are
/// left blank so the switches stay in one column.
pub fn list_entry_row<'a, M: Clone + 'a>(
    entry: ListEntry<M>,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, M> {
    let toggle = match entry.always {
        Some(word) => status_text(word, palette.dim_text),
        None => switch(entry.on, entry.on_toggle, lang, palette),
    };
    let arrow = |label: &str, on_press: Option<M>| -> Element<'a, M> {
        match on_press {
            Some(message) => icon_button(label, message, palette),
            None => Space::new()
                .width(Length::Fixed(CONTROL_HEIGHT))
                .height(Length::Fixed(CONTROL_HEIGHT))
                .into(),
        }
    };
    SettingRow::listed(
        entry.label,
        Some(entry.description),
        None,
        vec![toggle, arrow("↑", entry.on_up), arrow("↓", entry.on_down)],
        palette,
    )
}
