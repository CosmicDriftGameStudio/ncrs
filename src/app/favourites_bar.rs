//! The favourites bar under the path tabs: what it shows, what its slots and
//! keys do. The list and its logic are `ncrs_files::favourites`; this is the
//! wiring.

use crate::app::*;
use crate::messages::FavouritesBarMessage;
use ncrs_files::favourites_bar::{self as bar, BarAction, BarFacts};
use ncrs_ui_kit::favourites_bar::{self as kit_bar, FAVOURITES_BAR_SLOTS};

const OPEN_ACTIONS: [KeyAction; FAVOURITES_BAR_SLOTS] = [
    KeyAction::OpenFavourite1,
    KeyAction::OpenFavourite2,
    KeyAction::OpenFavourite3,
    KeyAction::OpenFavourite4,
    KeyAction::OpenFavourite5,
];

impl App {
    /// What the bars above the panels take off them: the favourites bar,
    /// nothing while it is off or there is no favourite to show, and the
    /// compare bar while a comparison is up.
    pub(in crate::app) fn favourites_bar_height(&self) -> f32 {
        let compare_bar = if self.compare.run.is_some() {
            ncrs_ui_kit::compare_bar::COMPARE_BAR_HEIGHT
        } else {
            0.0
        };
        self.favourites_own_height() + compare_bar
    }

    fn favourites_own_height(&self) -> f32 {
        if self.favourites_bar_on && !self.panels.favourites.is_empty() {
            kit_bar::FAVOURITES_BAR_HEIGHT
        } else {
            0.0
        }
    }

    /// Sizes the lists again when the bar came or went, not only when the
    /// window was resized.
    pub(in crate::app) fn refit_for_favourites_bar(&mut self) {
        if (self.favourites_bar_applied - self.favourites_bar_height()).abs() > f32::EPSILON {
            self.apply_window_size(self.window_size);
        }
    }

    /// The panels have the keys: no dialog, menu or text field takes them and
    /// the tab shown is the files.
    fn panels_have_the_keys(&self) -> bool {
        !self.overlay_is_open()
            && !self.text_field_has_keys()
            && !self.monitor_shown()
            && self.shown_tab == ShownTab::Panels
    }

    fn open_key_label(&self, index: usize) -> Option<String> {
        self.keymap.shortcut_label_of(*OPEN_ACTIONS.get(index)?)
    }

    /// The first and the last key of the five when they are one run of keys.
    pub(in crate::app) fn open_keys_run(&self) -> Option<(String, String)> {
        bar::open_keys_run(|index| self.open_key_label(index))
    }

    /// The bar, or `None` where it is not drawn.
    pub(in crate::app) fn favourites_bar_view(&self) -> Option<Element<'_, Message>> {
        if self.favourites_own_height() == 0.0 {
            return None;
        }
        let facts = BarFacts {
            favourites: &self.panels.favourites,
            home: &self.home,
            current: self.panels.current_slot(),
            key_labels: (0..FAVOURITES_BAR_SLOTS)
                .map(|index| self.open_key_label(index))
                .collect(),
        };
        Some(bar::bar_view(&facts, &self.palette).map(Message::FavouritesBar))
    }

    /// The menu of a slot with the click-away layer under it.
    pub(in crate::app) fn favourite_menu_view(&self) -> Option<Element<'_, Message>> {
        let menu = self.favourite_menu?;
        Some(
            bar::menu_view(
                menu,
                self.panels.favourites.len(),
                self.lang,
                self.window_size,
                &self.palette,
            )
            .map(Message::FavouritesBar),
        )
    }

    /// Cmd+1..5, the command list and the menu bar. Does nothing while
    /// something else has the keys or there is no favourite with that number.
    pub(in crate::app) fn open_favourite_by_key(&mut self, index: usize) -> Task<Message> {
        if !self.panels_have_the_keys() {
            return Task::none();
        }
        self.open_favourite(index, self.panels.active)
    }

    fn open_favourite(&mut self, index: usize, side: PanelSide) -> Task<Message> {
        let actions = self.panels.open_favourite(index, side, self.lang);
        self.run_panel_actions(actions)
    }

    pub(in crate::app) fn update_favourites_bar(
        &mut self,
        message: FavouritesBarMessage,
    ) -> Task<Message> {
        let pointer = self.input.lock().pointer;
        let action = bar::update(
            &mut self.favourite_menu,
            message,
            self.panels.active,
            pointer,
        );
        match action {
            None => Task::none(),
            Some(BarAction::Open { index, side }) => self.open_favourite(index, side),
            Some(BarAction::Move { index, step }) => {
                self.panels.move_favourite(index, step);
                Task::none()
            }
            Some(BarAction::Remove(index)) => {
                self.panels.remove_favourite_index(index);
                Task::none()
            }
            Some(BarAction::Manage) => {
                self.show_setting(crate::messages::SettingsFeature::Favourites, None)
            }
        }
    }

    /// What the Favourites page shows.
    pub(in crate::app) fn favourites_facts(&self) -> ncrs_settings::facts::FavouritesFacts {
        ncrs_settings::facts::FavouritesFacts {
            folders: self.panels.favourites.clone(),
            bar_shown: self.favourites_bar_on,
            open_keys: self.open_keys_run(),
            add_key: self.keymap.shortcut_label_of(KeyAction::AddFavourite),
        }
    }

    /// A move or removal on the Favourites page.
    pub(in crate::app) fn edit_favourites(&mut self, edit: ncrs_settings::FavouritesEdit) {
        use ncrs_settings::FavouritesEdit as Edit;
        match edit {
            Edit::Up(index) => self.panels.move_favourite(index, -1),
            Edit::Down(index) => self.panels.move_favourite(index, 1),
            Edit::Remove(index) => self.panels.remove_favourite_index(index),
        };
    }

    /// A folder picked on the Favourites page joins at the end.
    pub(in crate::app) fn add_picked_favourite(&mut self, path: PathBuf) -> Task<Message> {
        let action = self.panels.add_favourite_at(self.lang, path);
        self.run_panel_actions(vec![action])
    }
}
