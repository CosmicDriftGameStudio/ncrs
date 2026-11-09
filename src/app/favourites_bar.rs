//! The favourites bar under the path tabs: what it shows, what its slots and
//! keys do. The list and its logic are `ncrs_files::favourites`; this is the
//! wiring.

use crate::app::*;
use crate::messages::FavouritesBarMessage;
use ncrs_ui_kit::favourites_bar::{self as bar, FAVOURITES_BAR_SLOTS};
use ncrs_ui_kit::layout::place;
use ncrs_ui_kit::popup_menu::{popup_menu, popup_menu_height, PopupEntry, POPUP_MENU_WIDTH};

/// The right-click menu of a slot, while it is up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::app) struct FavouriteMenu {
    pub index: usize,
    pub at: Point,
}

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
            bar::FAVOURITES_BAR_HEIGHT
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

    /// The first and the last key of the five when they are one run of keys
    /// (the same modifiers, the characters in a row).
    pub(in crate::app) fn open_keys_run(&self) -> Option<(String, String)> {
        let labels: Vec<String> = (0..FAVOURITES_BAR_SLOTS)
            .map(|index| self.open_key_label(index))
            .collect::<Option<_>>()?;
        let split = |label: &str| {
            let last = label.chars().last()?;
            Some((label[..label.len() - last.len_utf8()].to_owned(), last))
        };
        let (prefix, mut previous) = split(labels.first()?)?;
        for label in labels.iter().skip(1) {
            let (next_prefix, next) = split(label)?;
            if next_prefix != prefix || u32::from(next) != u32::from(previous) + 1 {
                return None;
            }
            previous = next;
        }
        Some((labels.first()?.clone(), labels.last()?.clone()))
    }

    fn favourites_bar_hint(&self) -> Option<String> {
        let (first, last) = self.open_keys_run()?;
        let last_character = last.chars().last()?;
        Some(format!("{first}\u{2013}{last_character}"))
    }

    /// The bar, or `None` where it is not drawn.
    pub(in crate::app) fn favourites_bar_view(&self) -> Option<Element<'_, Message>> {
        if self.favourites_own_height() == 0.0 {
            return None;
        }
        let labels = ncrs_files::favourites::slot_labels(&self.panels.favourites, Some(&self.home));
        let current = self.panels.current_slot();
        let slots = self
            .panels
            .favourites
            .iter()
            .zip(labels)
            .take(FAVOURITES_BAR_SLOTS)
            .enumerate()
            .map(|(index, (path, label))| {
                let shown = path.display().to_string();
                bar::Slot {
                    number: index + 1,
                    label,
                    is_current: current == Some(index),
                    tooltip: match self.open_key_label(index) {
                        Some(key) => format!("{shown}  {key}"),
                        None => shown,
                    },
                    on_press: Message::FavouritesBar(FavouritesBarMessage::Open(index)),
                    on_right_press: Message::FavouritesBar(FavouritesBarMessage::Menu(index)),
                }
            })
            .collect();
        Some(bar::view(slots, self.favourites_bar_hint(), &self.palette))
    }

    /// The menu of a slot with the click-away layer under it.
    pub(in crate::app) fn favourite_menu_view(&self) -> Option<Element<'_, Message>> {
        let menu = self.favourite_menu?;
        let last = self
            .panels
            .favourites
            .len()
            .min(FAVOURITES_BAR_SLOTS)
            .saturating_sub(1);
        let index = menu.index;
        let entry = |label: Msg, message: Option<FavouritesBarMessage>| PopupEntry {
            label: self.lang.text(label).to_owned(),
            key: None,
            is_selected: false,
            on_press: message.map(Message::FavouritesBar),
            on_hover: None,
        };
        let entries = vec![
            entry(
                Msg::FavouritesBarOpenInOther,
                Some(FavouritesBarMessage::OpenInOtherPanel(index)),
            ),
            entry(
                Msg::FavouritesBarMoveLeft,
                (index > 0).then_some(FavouritesBarMessage::Move { index, step: -1 }),
            ),
            entry(
                Msg::FavouritesBarMoveRight,
                (index < last).then_some(FavouritesBarMessage::Move { index, step: 1 }),
            ),
            entry(
                Msg::ContextMenuRemoveFavourite,
                Some(FavouritesBarMessage::Remove(index)),
            ),
            entry(Msg::FavouritesBarManage, Some(FavouritesBarMessage::Manage)),
        ];
        let height = popup_menu_height(entries.len());
        let origin = Point::new(
            place(menu.at.x, POPUP_MENU_WIDTH, self.window_size.width),
            place(menu.at.y, height, self.window_size.height),
        );
        // A left press beside the menu closes it; a right press passes through
        // to another slot, whose menu replaces this one.
        let click_catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_press(Message::FavouritesBar(FavouritesBarMessage::CloseMenu));
        Some(
            Stack::with_children([
                click_catcher.into(),
                popup_menu(entries, origin, &self.palette),
            ])
            .into(),
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
        if !matches!(message, FavouritesBarMessage::Menu(_)) {
            self.favourite_menu = None;
        }
        match message {
            FavouritesBarMessage::Open(index) => self.open_favourite(index, self.panels.active),
            FavouritesBarMessage::Menu(index) => {
                let at = self.input.lock().pointer;
                self.favourite_menu = Some(FavouriteMenu { index, at });
                Task::none()
            }
            FavouritesBarMessage::CloseMenu => Task::none(),
            FavouritesBarMessage::OpenInOtherPanel(index) => {
                self.open_favourite(index, self.panels.active.other())
            }
            FavouritesBarMessage::Move { index, step } => {
                self.panels.move_favourite(index, step);
                Task::none()
            }
            FavouritesBarMessage::Remove(index) => {
                self.panels.remove_favourite_index(index);
                Task::none()
            }
            FavouritesBarMessage::Manage => {
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

/// The menu keys: Escape closes it, nothing else reaches the panels behind.
pub(in crate::app) fn favourite_menu_key(key: &Key) -> Option<Message> {
    matches!(key.as_ref(), Key::Named(Named::Escape))
        .then_some(Message::FavouritesBar(FavouritesBarMessage::CloseMenu))
}
