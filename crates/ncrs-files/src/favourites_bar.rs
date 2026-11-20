//! The favourites bar under the path tabs: its slots and the right-click menu
//! of a slot. The list and its labels are `crate::favourites`; the app owns
//! the favourites themselves and runs what `update` asks for.

use std::path::{Path, PathBuf};

use iced::{Element, Point, Size};
use ncrs_core::PanelSide;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::favourites_bar::{self as bar, slots_that_fit, FAVOURITE_KEYS};
use ncrs_ui_kit::layout::place;
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::popup_menu::{
    popup_layer, popup_menu, popup_menu_height, PopupEntry, RightPress, POPUP_MENU_WIDTH,
};

/// What the favourites bar sends: a click on a slot and the entries of its
/// right-click menu. The number is the favourite's place in the list, from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavouritesBarMessage {
    Open(usize),
    /// A right click: the menu of this slot at the pointer.
    Menu(usize),
    /// The button at the end of the bar: the menu of the favourites that did
    /// not fit, at the pointer.
    Overflow,
    CloseMenu,
    OpenInOtherPanel(usize),
    Move {
        index: usize,
        step: isize,
    },
    Remove(usize),
    Manage,
}

/// Whose menu is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    /// The right-click menu of the slot of this favourite.
    Slot(usize),
    /// The favourites the bar had no room for.
    Overflow,
}

/// A menu of the bar, while it is up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FavouriteMenu {
    pub target: MenuTarget,
    pub at: Point,
}

/// What the app does for the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAction {
    Open {
        index: usize,
        side: PanelSide,
    },
    Move {
        index: usize,
        step: isize,
    },
    Remove(usize),
    /// Show the Favourites page of the settings.
    Manage,
}

/// Applies `message`; `active` is the panel a plain click opens in and
/// `pointer` where a menu opens.
pub fn update(
    menu: &mut Option<FavouriteMenu>,
    message: FavouritesBarMessage,
    active: PanelSide,
    pointer: Point,
) -> Option<BarAction> {
    let opened = match message {
        FavouritesBarMessage::Menu(index) => Some(MenuTarget::Slot(index)),
        FavouritesBarMessage::Overflow => Some(MenuTarget::Overflow),
        _ => None,
    };
    if let Some(target) = opened {
        *menu = Some(FavouriteMenu {
            target,
            at: pointer,
        });
        return None;
    }
    *menu = None;
    match message {
        FavouritesBarMessage::Open(index) => Some(BarAction::Open {
            index,
            side: active,
        }),
        FavouritesBarMessage::OpenInOtherPanel(index) => Some(BarAction::Open {
            index,
            side: active.other(),
        }),
        FavouritesBarMessage::Move { index, step } => Some(BarAction::Move { index, step }),
        FavouritesBarMessage::Remove(index) => Some(BarAction::Remove(index)),
        FavouritesBarMessage::Manage => Some(BarAction::Manage),
        FavouritesBarMessage::Menu(_)
        | FavouritesBarMessage::Overflow
        | FavouritesBarMessage::CloseMenu => None,
    }
}

/// The menu keys: Escape closes it, nothing else reaches the panels behind.
pub fn menu_key(key: &iced::keyboard::Key) -> Option<FavouritesBarMessage> {
    use iced::keyboard::key::Named;
    matches!(key.as_ref(), iced::keyboard::Key::Named(Named::Escape))
        .then_some(FavouritesBarMessage::CloseMenu)
}

/// The first and the last key of the nine when they are one run of keys (the
/// same modifiers, the characters in a row). `label` is the key of a slot.
pub fn open_keys_run(label: impl Fn(usize) -> Option<String>) -> Option<(String, String)> {
    let labels: Vec<String> = (0..FAVOURITE_KEYS).map(label).collect::<Option<_>>()?;
    let split = |key: &str| {
        let last = key.chars().last()?;
        Some((key[..key.len() - last.len_utf8()].to_owned(), last))
    };
    let (prefix, mut previous) = split(labels.first()?)?;
    for key in labels.iter().skip(1) {
        let (next_prefix, next) = split(key)?;
        if next_prefix != prefix || u32::from(next) != u32::from(previous) + 1 {
            return None;
        }
        previous = next;
    }
    Some((labels.first()?.clone(), labels.last()?.clone()))
}

/// What the bar shows: the favourites, which one is the active panel's folder
/// and the key of each slot, in the order of the slots. `width` is the width
/// of the window the bar spans.
#[derive(Debug)]
pub struct BarFacts<'a> {
    pub favourites: &'a [PathBuf],
    pub home: &'a Path,
    pub current: Option<usize>,
    pub key_labels: Vec<Option<String>>,
    pub width: f32,
    pub lang: Language,
}

/// The bar: the favourites that fit, then the button for the rest.
pub fn bar_view<'a>(facts: &BarFacts<'_>, palette: &Palette) -> Element<'a, FavouritesBarMessage> {
    let labels = crate::favourites::slot_labels(facts.favourites, Some(facts.home));
    let fitting = slots_that_fit(&labels, facts.width);
    let all_fit = fitting == labels.len();
    let slots = facts
        .favourites
        .iter()
        .zip(labels)
        .take(fitting)
        .enumerate()
        .map(|(index, (path, label))| {
            let shown = path.display().to_string();
            bar::Slot {
                number: (index < FAVOURITE_KEYS).then_some(index + 1),
                label,
                is_current: facts.current == Some(index),
                tooltip: match facts.key_labels.get(index).cloned().flatten() {
                    Some(key) => format!("{shown}  {key}"),
                    None => shown,
                },
                on_press: FavouritesBarMessage::Open(index),
                on_right_press: FavouritesBarMessage::Menu(index),
            }
        })
        .collect();
    let overflow = (!all_fit).then(|| bar::Overflow {
        tooltip: facts
            .lang
            .text(Msg::FavouritesBarOverflowTooltip)
            .to_owned(),
        on_press: FavouritesBarMessage::Overflow,
    });
    let hint = open_keys_run(|index| facts.key_labels.get(index).cloned().flatten())
        .filter(|_| all_fit)
        .and_then(|(first, last)| {
            let last_character = last.chars().last()?;
            Some(format!("{first}\u{2013}{last_character}"))
        });
    bar::view(slots, overflow, hint, palette)
}

/// The menu of the bar with the click-away layer under it: the menu of a slot,
/// or the list of the favourites past the ones that fit, for `favourites` in
/// a window of `window`.
pub fn menu_view<'a>(
    menu: FavouriteMenu,
    favourites: &[PathBuf],
    home: &Path,
    lang: Language,
    window: Size,
    palette: &Palette,
) -> Element<'a, FavouritesBarMessage> {
    let entry = |label: String, message: Option<FavouritesBarMessage>| PopupEntry {
        label,
        key: None,
        is_selected: false,
        on_press: message,
        on_hover: None,
    };
    let entries = match menu.target {
        MenuTarget::Slot(index) => slot_menu_entries(index, favourites.len(), lang, entry),
        MenuTarget::Overflow => {
            let labels = crate::favourites::slot_labels(favourites, Some(home));
            let fitting = slots_that_fit(&labels, window.width);
            labels
                .into_iter()
                .enumerate()
                .skip(fitting)
                .map(|(index, label)| entry(label, Some(FavouritesBarMessage::Open(index))))
                .collect()
        }
    };
    let height = popup_menu_height(entries.len());
    let origin = Point::new(
        place(menu.at.x, POPUP_MENU_WIDTH, window.width),
        place(menu.at.y, height, window.height),
    );
    // A right press passes through to another slot, whose menu replaces this
    // one.
    popup_layer(
        popup_menu(entries, origin, palette),
        Some(FavouritesBarMessage::CloseMenu),
        RightPress::PassThrough,
    )
}

fn slot_menu_entries(
    index: usize,
    favourites: usize,
    lang: Language,
    entry: impl Fn(String, Option<FavouritesBarMessage>) -> PopupEntry<FavouritesBarMessage>,
) -> Vec<PopupEntry<FavouritesBarMessage>> {
    let last = favourites.saturating_sub(1);
    let plain = |label: Msg, message: Option<FavouritesBarMessage>| {
        entry(lang.text(label).to_owned(), message)
    };
    vec![
        plain(
            Msg::FavouritesBarOpenInOther,
            Some(FavouritesBarMessage::OpenInOtherPanel(index)),
        ),
        plain(
            Msg::FavouritesBarMoveLeft,
            (index > 0).then_some(FavouritesBarMessage::Move { index, step: -1 }),
        ),
        plain(
            Msg::FavouritesBarMoveRight,
            (index < last).then_some(FavouritesBarMessage::Move { index, step: 1 }),
        ),
        plain(
            Msg::ContextMenuRemoveFavourite,
            Some(FavouritesBarMessage::Remove(index)),
        ),
        plain(Msg::FavouritesBarManage, Some(FavouritesBarMessage::Manage)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_right_click_opens_the_menu_and_any_other_message_closes_it() {
        let mut menu = None;
        let at = Point::new(10.0, 20.0);
        assert_eq!(
            update(
                &mut menu,
                FavouritesBarMessage::Menu(2),
                PanelSide::Left,
                at
            ),
            None
        );
        assert_eq!(
            menu,
            Some(FavouriteMenu {
                target: MenuTarget::Slot(2),
                at
            })
        );
        assert_eq!(
            update(
                &mut menu,
                FavouritesBarMessage::OpenInOtherPanel(2),
                PanelSide::Left,
                at
            ),
            Some(BarAction::Open {
                index: 2,
                side: PanelSide::Right
            })
        );
        assert_eq!(menu, None);
    }

    #[test]
    fn the_nine_keys_are_a_run_only_when_the_characters_follow_each_other() {
        let digits = |index: usize| Some(format!("Cmd+{}", index + 1));
        assert_eq!(
            open_keys_run(digits),
            Some(("Cmd+1".to_owned(), "Cmd+9".to_owned()))
        );
        let gap = |index: usize| Some(format!("Cmd+{}", index * 2));
        assert_eq!(open_keys_run(gap), None);
        assert_eq!(
            open_keys_run(|index| (index < 8).then(|| "Cmd+1".to_owned())),
            None
        );
    }

    #[test]
    fn the_overflow_button_opens_its_own_menu_at_the_pointer() {
        let mut menu = None;
        let at = Point::new(5.0, 6.0);
        assert_eq!(
            update(
                &mut menu,
                FavouritesBarMessage::Overflow,
                PanelSide::Left,
                at
            ),
            None
        );
        assert_eq!(
            menu,
            Some(FavouriteMenu {
                target: MenuTarget::Overflow,
                at
            })
        );
    }
}
