//! The Favourites page: the switch for the bar under the tabs and the list of
//! folders, whose order sets the numbers.

use ncrs_ui_kit::favourites_bar::{number_cap, number_cap_gap, FAVOURITES_BAR_SLOTS};

use super::index_page::add_folder_row;
use super::*;
use crate::folders::AddAs;
use crate::message::FavouritesEdit;

fn edit(favourites_edit: FavouritesEdit) -> Message {
    SettingsMessage::Favourites(favourites_edit)
}

pub fn favourites_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    vec![
        Group {
            intro: None,
            title: lang.text(Msg::SettingsFavouritesBarTitle).to_owned(),
            rows: vec![bar_row(view)],
        },
        Group {
            intro: Some(folders_intro(view)),
            title: lang.text(Msg::SettingsFavouritesFoldersTitle).to_owned(),
            rows: folder_rows(view),
        },
    ]
}

fn bar_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let shown = view.favourites.bar_shown;
    let note = match &view.favourites.open_keys {
        Some((first, last)) => fill_template(
            lang.text(Msg::SettingsFavouritesBarSwitchNoteKeys),
            &[("{first}", first), ("{last}", last)],
        ),
        None => lang.text(Msg::SettingsFavouritesBarSwitchNote).to_owned(),
    };
    let toggle = change(SettingChange::FavouritesBar(!shown));
    let mut row = SettingRow::text(
        lang.text(Msg::SettingsFavouritesBarSwitch).to_owned(),
        Some(note),
        switch(shown, SettingChange::FavouritesBar(!shown), view),
    );
    row.in_command_list = true;
    row.control_width = None;
    row.config_key = Some(FAVOURITES_BAR_KEY.to_owned());
    row.keyed(RowKey::Press(toggle))
}

fn folders_intro(view: &SettingsView<'_>) -> String {
    let lang = view.lang;
    match &view.favourites.add_key {
        Some(key) => fill_template(
            lang.text(Msg::SettingsFavouritesFoldersIntro),
            &[("{key}", key)],
        ),
        None => lang
            .text(Msg::SettingsFavouritesFoldersIntroUnbound)
            .to_owned(),
    }
}

fn folder_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let folders = &view.favourites.folders;
    let last = folders.len().saturating_sub(1);
    let mut rows: Vec<SettingRow<'a>> = folders
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let button = |label: Msg, edit_to: Option<FavouritesEdit>| {
                RowButton::new(lang.text(label), edit_to.map(edit), Tone::Secondary)
            };
            let buttons = vec![
                button(
                    Msg::SettingsFavouritesUp,
                    (index > 0).then_some(FavouritesEdit::Up(index)),
                ),
                button(
                    Msg::SettingsFavouritesDown,
                    (index < last).then_some(FavouritesEdit::Down(index)),
                ),
                button(
                    Msg::ContextMenuRemoveFavourite,
                    Some(FavouritesEdit::Remove(index)),
                ),
            ];
            let status = (index >= FAVOURITES_BAR_SLOTS)
                .then(|| lang.text(Msg::SettingsFavouritesMenuOnly).to_owned());
            let lead = if index < FAVOURITES_BAR_SLOTS {
                number_cap(index + 1, palette)
            } else {
                number_cap_gap()
            };
            SettingRow::listed_buttons(shown_path(path, view.home), None, status, buttons, palette)
                .with_leading(lead)
        })
        .collect();
    rows.push(add_folder_row(AddAs::Favourite, None, Vec::new(), view));
    rows
}
