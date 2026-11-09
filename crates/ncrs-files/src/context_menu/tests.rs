// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

const WINDOW: Size = Size::new(1200.0, 760.0);

/// The menu of an ordinary row: the bundle entry is left out.
fn menu_with(disabled: &[ContextAction]) -> ContextMenu {
    ContextMenu::new(Point::new(10.0, 10.0), WINDOW, |action| {
        action != ContextAction::ShowPackageContents && !disabled.contains(&action)
    })
}

fn selected_action(menu: &ContextMenu) -> ContextAction {
    menu.selected_action().unwrap()
}

#[test]
fn the_menu_has_the_five_groups_with_separators() {
    let menu = menu_with(&[]);
    let separators = menu
        .entries()
        .iter()
        .filter(|entry| **entry == MenuEntry::Separator)
        .count();
    assert_eq!(separators, 4);
    assert_eq!(menu.entries().len(), 14 + 4);
    assert_eq!(selected_action(&menu), ContextAction::Open);
}

#[test]
fn the_favourites_entry_fits_the_folder() {
    let actions_of = |is_favourite: Option<bool>| {
        ContextMenu::with_favourite(Point::new(10.0, 10.0), WINDOW, |_| true, None, is_favourite)
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item { action, .. } => Some(*action),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let adds = actions_of(Some(false));
    assert!(adds.contains(&ContextAction::AddFavourite));
    assert!(!adds.contains(&ContextAction::RemoveFavourite));
    let removes = actions_of(Some(true));
    assert!(removes.contains(&ContextAction::RemoveFavourite));
    assert!(!removes.contains(&ContextAction::AddFavourite));
    let neither = actions_of(None);
    assert!(!neither.contains(&ContextAction::AddFavourite));
    assert!(!neither.contains(&ContextAction::RemoveFavourite));
}

#[test]
fn navigation_skips_separators_and_greyed_out_entries() {
    let mut menu = menu_with(&[ContextAction::View, ContextAction::Edit]);
    menu.move_by(1);
    assert_eq!(selected_action(&menu), ContextAction::OpenTerminal);
    menu.move_by(1);
    assert_eq!(selected_action(&menu), ContextAction::Copy);
    menu.move_by(-1);
    assert_eq!(selected_action(&menu), ContextAction::OpenTerminal);
    menu.move_by(-1);
    assert_eq!(selected_action(&menu), ContextAction::Open);
    menu.move_by(-1);
    assert_eq!(
        selected_action(&menu),
        ContextAction::DeletePermanently,
        "up from the top goes round to the last entry"
    );
    menu.move_by(1);
    assert_eq!(
        selected_action(&menu),
        ContextAction::Open,
        "down from the bottom goes round to the first entry"
    );
    menu.select_last();
    assert_eq!(selected_action(&menu), ContextAction::DeletePermanently);
    menu.move_by(-1);
    assert_eq!(selected_action(&menu), ContextAction::Trash);
    menu.move_by(-1);
    assert_eq!(selected_action(&menu), ContextAction::ComputeFolderSizes);
    menu.move_by(-1);
    assert_eq!(selected_action(&menu), ContextAction::CopyName);
    menu.select_first();
    assert_eq!(selected_action(&menu), ContextAction::Open);
}

#[test]
fn going_round_in_the_menu_skips_greyed_out_entries_at_both_ends() {
    let mut menu = menu_with(&[ContextAction::Open, ContextAction::DeletePermanently]);
    let first = selected_action(&menu);
    assert_ne!(first, ContextAction::Open);
    menu.move_by(-1);
    assert_eq!(selected_action(&menu), ContextAction::Trash);
    menu.move_by(1);
    assert_eq!(selected_action(&menu), first);
}

#[test]
fn going_round_in_the_submenu_skips_greyed_out_entries_at_both_ends() {
    let mut menu = hermes_menu(&[HermesAction::ShowSuggestion, HermesAction::Inspect]);
    menu.select_last();
    menu.open_submenu();
    menu.select_first();
    let first = menu.selected_hermes_action();
    assert_eq!(first, Some(HermesAction::HandToInbox));
    menu.move_by(-1);
    assert_eq!(menu.selected_hermes_action(), Some(HermesAction::Exclude));
    menu.move_by(1);
    assert_eq!(menu.selected_hermes_action(), first);
}

#[test]
fn going_round_skips_greyed_out_entries_at_both_ends() {
    let usable = [false, true, false, true, false];
    assert_eq!(next_usable_index(&usable, 3, 1), 1, "past the end");
    assert_eq!(next_usable_index(&usable, 1, -1), 3, "before the start");
    assert_eq!(next_usable_index(&usable, 1, 1), 3);
    assert_eq!(next_usable_index(&[true], 0, 1), 0, "alone");
    assert_eq!(next_usable_index(&[false, false], 1, -1), 1, "none usable");
    assert_eq!(next_usable_index(&[], 0, 1), 0, "empty");
}

#[test]
fn only_usable_entries_can_be_highlighted() {
    let mut menu = menu_with(&[ContextAction::View]);
    menu.select(1);
    assert_eq!(selected_action(&menu), ContextAction::Open);
    menu.select(5);
    assert_eq!(selected_action(&menu), ContextAction::Copy);
    let separator = menu
        .entries()
        .iter()
        .position(|entry| *entry == MenuEntry::Separator)
        .unwrap();
    menu.select(separator);
    assert_eq!(selected_action(&menu), ContextAction::Copy);
}

#[test]
fn the_first_usable_entry_is_highlighted_when_open_is_unusable() {
    let menu = menu_with(&[ContextAction::Open, ContextAction::View]);
    assert_eq!(selected_action(&menu), ContextAction::Edit);
}

#[test]
fn the_menu_folds_back_into_the_window() {
    let size = ContextMenu::size_of(menu_with(&[]).entries());
    let corner = ContextMenu::new(
        Point::new(WINDOW.width - 5.0, WINDOW.height - 5.0),
        WINDOW,
        |action| action != ContextAction::ShowPackageContents,
    );
    assert_eq!(
        corner.origin(),
        Point::new(
            WINDOW.width - 5.0 - size.width,
            WINDOW.height - 5.0 - size.height
        )
    );
    let inside = ContextMenu::new(Point::new(40.0, 50.0), WINDOW, |_| true);
    assert_eq!(inside.origin(), Point::new(40.0, 50.0));
    let tiny = ContextMenu::new(Point::new(40.0, 50.0), Size::new(100.0, 100.0), |_| true);
    assert_eq!(tiny.origin(), Point::ORIGIN);
}

fn hermes_menu(disabled: &'static [HermesAction]) -> ContextMenu {
    let enabled = move |action: HermesAction| !disabled.contains(&action);
    ContextMenu::with_hermes(
        Point::new(10.0, 10.0),
        WINDOW,
        |_| true,
        Some(HermesMenu {
            excluded: false,
            is_enabled: &enabled,
        }),
    )
}

#[test]
fn the_hermes_entry_is_last_and_only_there_with_the_column() {
    let menu = hermes_menu(&[]);
    assert_eq!(menu.entries().last(), Some(&MenuEntry::Hermes));
    assert!(!menu_with(&[]).entries().contains(&MenuEntry::Hermes));
}

#[test]
fn right_opens_the_submenu_on_the_hermes_entry_and_left_closes_it() {
    let mut menu = hermes_menu(&[]);
    assert!(!menu.open_submenu(), "the highlight is on Open");
    menu.select_last();
    assert!(menu.selected_is_hermes());
    assert!(menu.open_submenu());
    assert_eq!(
        menu.selected_hermes_action(),
        Some(HermesAction::ShowSuggestion)
    );
    assert!(menu.close_submenu());
    assert!(!menu.submenu_is_open());
    assert!(!menu.close_submenu());
}

#[test]
fn the_submenu_skips_separators_and_greyed_entries() {
    let mut menu = hermes_menu(&[HermesAction::ProcessAgain, HermesAction::SetInbox]);
    menu.select_last();
    menu.open_submenu();
    menu.move_by(1);
    assert_eq!(
        menu.selected_hermes_action(),
        Some(HermesAction::HandToInbox)
    );
    menu.move_by(1);
    assert_eq!(
        menu.selected_hermes_action(),
        Some(HermesAction::SetFilingLocation)
    );
    menu.select_last();
    assert_eq!(menu.selected_hermes_action(), Some(HermesAction::Inspect));
    menu.move_by(1);
    assert_eq!(
        menu.selected_hermes_action(),
        Some(HermesAction::ShowSuggestion),
        "down from the last entry goes round to the first"
    );
    menu.move_by(-1);
    assert_eq!(
        menu.selected_hermes_action(),
        Some(HermesAction::Inspect),
        "up from the first entry goes round to the last"
    );
    menu.select_first();
    assert_eq!(
        menu.selected_hermes_action(),
        Some(HermesAction::ShowSuggestion)
    );
    assert!(menu.selected_is_hermes(), "the parent keeps its highlight");
}

#[test]
fn hovering_another_entry_closes_the_submenu() {
    let mut menu = hermes_menu(&[]);
    menu.select_last();
    let hermes = menu.selected();
    menu.hover(hermes);
    assert!(menu.submenu_is_open());
    menu.hover(0);
    assert!(!menu.submenu_is_open());
}

#[test]
fn the_submenu_folds_to_the_left_where_there_is_no_room() {
    let mut menu = hermes_menu(&[]);
    menu.select_last();
    menu.open_submenu();
    let origin = menu.submenu_origin().unwrap();
    assert_eq!(origin.x, menu.origin().x + MENU_WIDTH);

    let mut right = ContextMenu::with_hermes(
        Point::new(WINDOW.width, 10.0),
        WINDOW,
        |_| true,
        Some(HermesMenu {
            excluded: true,
            is_enabled: &|_| true,
        }),
    );
    right.select_last();
    right.open_submenu();
    assert_eq!(
        right.submenu_origin().unwrap().x,
        right.origin().x - MENU_SUB_WIDTH
    );
    assert!(right.sub_entries().iter().any(|entry| matches!(
        entry,
        SubEntry::Item {
            action: HermesAction::Include,
            ..
        }
    )));
}

fn actions_of(menu: &ContextMenu) -> Vec<ContextAction> {
    menu.entries()
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item { action, .. } => Some(*action),
            MenuEntry::Separator | MenuEntry::Hermes => None,
        })
        .collect()
}

#[test]
fn permissions_sit_after_rename_in_the_menu_of_a_folder() {
    let actions = actions_of(&menu_with(&[]));
    let rename = actions
        .iter()
        .position(|action| *action == ContextAction::Rename)
        .unwrap();
    assert_eq!(actions[rename + 1], ContextAction::Permissions);
}

#[test]
fn hit_lists_and_search_results_do_not_offer_permissions() {
    let point = Point::new(10.0, 10.0);
    let hits = ContextMenu::for_hit_list(point, WINDOW, |_| true, None);
    let results = ContextMenu::for_search_results(point, WINDOW, |_| true);
    assert!(!actions_of(&hits).contains(&ContextAction::Permissions));
    assert!(!actions_of(&results).contains(&ContextAction::Permissions));
}

#[test]
fn archives_and_servers_do_not_run_permissions() {
    assert!(!ContextAction::Permissions.allowed_in_archive());
    assert!(!ContextAction::Permissions.allowed_on_server());
    assert_eq!(ContextAction::Permissions.label(), Msg::ActionPermissions);
}

#[test]
fn show_package_contents_follows_open_only_where_it_applies() {
    let entries_where = |applies: bool| {
        ContextMenu::new(Point::new(10.0, 10.0), WINDOW, |action| {
            action != ContextAction::ShowPackageContents || applies
        })
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item { action, .. } => Some(*action),
            _ => None,
        })
        .take(2)
        .collect::<Vec<_>>()
    };
    assert_eq!(
        entries_where(true),
        [ContextAction::Open, ContextAction::ShowPackageContents]
    );
    assert_eq!(
        entries_where(false),
        [ContextAction::Open, ContextAction::View]
    );
}
