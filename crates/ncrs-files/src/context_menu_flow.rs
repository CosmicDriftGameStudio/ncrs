//! Opening the context menu, which of its entries are usable, and running the
//! chosen one. What the entries need from outside the panels comes in as
//! facts the app fills; a chosen entry is an [`Action`] the app turns into its
//! own message.

use iced::{Point, Size};
use ncrs_core::PanelSide;
use ncrs_fs::archive::is_archive_name;
use ncrs_ui_kit::layout;

use crate::action::Action;
use crate::context_menu::{ContextAction, ContextMenu, HermesAction, HermesMenu};
use crate::folder_size::folder_size_targets;
use crate::state::Panels;
use crate::update::is_local_app_bundle;

/// What decides whether a menu may open on a row.
#[derive(Debug, Clone, Copy)]
pub struct MenuOpenCtx {
    /// A page or dialog covers the panels.
    pub blocking_overlay_is_open: bool,
    /// The side the search view stands in for, while it is open.
    pub search_side: Option<PanelSide>,
    pub visible_rows: usize,
    pub window: Size,
    /// Height of the favourites bar above the panels, 0 while it is hidden.
    pub favourites_bar_height: f32,
}

/// The Hermes column as the menu sees the file under the cursor.
#[derive(Debug, Clone, Default)]
pub struct HermesFacts {
    /// The Hermes entry is part of the menu at all.
    pub ready: bool,
    /// Sources or targets are configured.
    pub active: bool,
    pub has_live_pipeline: bool,
    pub cursor: Option<HermesCursor>,
}

/// The file or folder under the cursor, not the `..` row.
#[derive(Debug, Clone, Copy, Default)]
pub struct HermesCursor {
    pub is_dir: bool,
    /// A folder with the excluded mark, or an excluded file.
    pub excluded: bool,
    pub is_source: bool,
    pub is_target: bool,
    /// The file lies in a folder Hermes reads.
    pub lies_in_an_indexed_folder: bool,
}

/// What the entries of the menu check when it opens.
#[derive(Debug, Clone, Default)]
pub struct MenuFacts {
    /// The search results are in front: whether the cursor is on a result.
    pub search_cursor: Option<bool>,
    /// The files a copy, move or delete would take are not empty.
    pub has_sources: bool,
    pub can_copy_path: bool,
    pub can_copy_name: bool,
    pub hermes: HermesFacts,
}

/// The arrow keys, hover and close of an open menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuNav {
    Move(isize),
    First,
    Last,
    Hover(usize),
    OpenSub,
    CloseSub,
    SubHover(usize),
    Close,
}

impl MenuFacts {
    pub fn action_enabled(&self, action: ContextAction, panels: &Panels) -> bool {
        if let Some(has_cursor) = self.search_cursor {
            return match action {
                ContextAction::Open
                | ContextAction::View
                | ContextAction::Edit
                | ContextAction::ShowOnOtherSide
                | ContextAction::CopyPath
                | ContextAction::CopyName => has_cursor,
                ContextAction::Copy
                | ContextAction::Move
                | ContextAction::Trash
                | ContextAction::DeletePermanently => self.has_sources,
                ContextAction::Rename
                | ContextAction::Permissions
                | ContextAction::NewFolder
                | ContextAction::OpenTerminal
                | ContextAction::ShowPackageContents
                | ContextAction::ComputeFolderSizes
                | ContextAction::CompareFolders
                | ContextAction::GitView
                | ContextAction::GitHistory
                | ContextAction::GitBlame
                | ContextAction::AddFavourite
                | ContextAction::RemoveFavourite
                | ContextAction::NoteOnFile
                | ContextAction::ExtractHere
                | ContextAction::Pack
                | ContextAction::RemoveFromList => false,
            };
        }
        let panel = panels.active_panel();
        if (panel.archive.is_some() && !action.allowed_in_archive())
            || (panel.remote().is_some() && !action.allowed_on_server())
        {
            return false;
        }
        let cursor_row = panel.selected_entry();
        match action {
            ContextAction::Open
            | ContextAction::ShowOnOtherSide
            | ContextAction::RemoveFromList => cursor_row.is_some(),
            ContextAction::ShowPackageContents => cursor_row
                .is_some_and(|entry| entry.is_dir && is_local_app_bundle(panel, &entry.name)),
            ContextAction::View | ContextAction::Edit | ContextAction::NoteOnFile => {
                cursor_row.is_some_and(|entry| !entry.is_dir && !entry.is_parent)
            }
            ContextAction::Copy
            | ContextAction::Move
            | ContextAction::Trash
            | ContextAction::DeletePermanently
            | ContextAction::Permissions => self.has_sources,
            ContextAction::ExtractHere => {
                !panel.is_virtual()
                    && panel.action_entries().iter().any(|entry| {
                        !entry.is_dir && !entry.is_parent && is_archive_name(&entry.name)
                    })
            }
            ContextAction::Pack => self.has_sources && !panel.is_virtual(),
            ContextAction::Rename => cursor_row.is_some_and(|entry| !entry.is_parent),
            ContextAction::NewFolder | ContextAction::CompareFolders => true,
            ContextAction::GitView => panels
                .local_folder_target()
                .is_some_and(|folder| ncrs_git::repository_root_of(&folder).is_some()),
            ContextAction::GitHistory | ContextAction::GitBlame => {
                cursor_row.is_some_and(|entry| !entry.is_dir && !entry.is_parent)
                    && ncrs_git::repository_root_of(&panel.path).is_some()
            }
            ContextAction::OpenTerminal => panels.local_folder_target().is_some(),
            ContextAction::CopyPath => self.can_copy_path,
            ContextAction::CopyName => self.can_copy_name,
            ContextAction::ComputeFolderSizes => !folder_size_targets(panel).is_empty(),
            ContextAction::AddFavourite | ContextAction::RemoveFavourite => {
                panels.local_folder_target().is_some()
            }
        }
    }
}

impl HermesFacts {
    pub fn action_enabled(&self, action: HermesAction) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        match action {
            // The first source and the first target are set up before there
            // is any.
            HermesAction::WatchSource
            | HermesAction::ChangeSource
            | HermesAction::AddTarget
            | HermesAction::ChangeTarget => self.ready && cursor.is_dir,
            HermesAction::FileOnce => self.active && cursor.is_dir,
            HermesAction::ProcessAgain => {
                self.active
                    && self.has_live_pipeline
                    && !cursor.excluded
                    && cursor.lies_in_an_indexed_folder
            }
            HermesAction::Exclude | HermesAction::Include | HermesAction::Inspect => self.active,
        }
    }
}

impl Panels {
    /// Puts the cursor on the row a menu is about to open on. `None` when the
    /// menu must not open: the cursor stays where it was then. The cursor goes
    /// to the row; tags stay, so the menu acts on them as F5 and F8 do, except
    /// that a click on an untagged row drops them, as in the Finder.
    pub fn point_at_menu_row(
        &mut self,
        side: PanelSide,
        index: usize,
        anchor: Option<Point>,
        ctx: &MenuOpenCtx,
    ) -> Option<Point> {
        if ctx.blocking_overlay_is_open || ctx.search_side == Some(side) {
            return None;
        }
        // An open menu is replaced by a right click and left alone by the keys.
        if anchor.is_none() && self.context_menu.is_some() {
            return None;
        }
        self.active = side;
        let panel = self.panel_mut(side);
        let on_untagged_row = panel
            .entries
            .get(index)
            .is_some_and(|entry| !panel.selection.is_tagged(&entry.path));
        if anchor.is_some() && on_untagged_row {
            panel.selection.clear();
        }
        panel.select(index, ctx.visible_rows);
        let cursor_panel = self.panel(side);
        Some(anchor.unwrap_or_else(|| {
            layout::cursor_row_anchor(
                side == PanelSide::Right,
                cursor_panel
                    .selected
                    .saturating_sub(cursor_panel.scroll_offset),
                cursor_panel.row_height(),
                ctx.window,
                ctx.favourites_bar_height,
            )
        }))
    }

    /// Opens the menu of the row `point_at_menu_row` put the cursor on.
    pub fn show_context_menu(
        &mut self,
        side: PanelSide,
        place_at: Point,
        window: Size,
        facts: &MenuFacts,
    ) {
        let panel = self.panel(side);
        let hermes_enabled = |action: HermesAction| {
            let in_archive = panel.archive.is_some() && !action.allowed_in_archive();
            let on_server = panel.remote().is_some() && !action.allowed_on_server();
            !in_archive && !on_server && facts.hermes.action_enabled(action)
        };
        let cursor = facts.hermes.cursor.unwrap_or_default();
        let hermes = facts.hermes.ready.then_some(HermesMenu {
            excluded: cursor.excluded,
            is_source: cursor.is_source,
            is_target: cursor.is_target,
            is_enabled: &hermes_enabled,
        });
        let is_enabled = |action| facts.action_enabled(action, self);
        let menu = if self.panel(side).is_hit_list() {
            ContextMenu::for_hit_list(place_at, window, is_enabled, hermes)
        } else {
            let is_favourite = self
                .local_folder_target()
                .map(|target| self.favourites.contains(&target));
            ContextMenu::with_favourite(place_at, window, is_enabled, hermes, is_favourite)
        };
        self.context_menu = Some(menu);
    }

    /// The menu of the search results at the pointer.
    pub fn show_search_context_menu(&mut self, pointer: Point, window: Size, facts: &MenuFacts) {
        let is_enabled = |action| facts.action_enabled(action, self);
        self.context_menu = Some(ContextMenu::for_search_results(pointer, window, is_enabled));
    }

    pub fn navigate_context_menu(&mut self, nav: MenuNav) {
        if nav == MenuNav::Close {
            self.context_menu = None;
            return;
        }
        let Some(menu) = self.context_menu.as_mut() else {
            return;
        };
        match nav {
            MenuNav::Move(delta) => menu.move_by(delta),
            MenuNav::First => menu.select_first(),
            MenuNav::Last => menu.select_last(),
            MenuNav::Hover(index) => menu.hover(index),
            MenuNav::OpenSub => {
                menu.open_submenu();
            }
            MenuNav::CloseSub => {
                menu.close_submenu();
            }
            MenuNav::SubHover(index) => menu.select_sub(index),
            MenuNav::Close => {}
        }
    }

    /// A click on row `index` of the menu, or on entry `index` of its submenu
    /// when `in_submenu`.
    pub fn click_context_menu(
        &mut self,
        index: usize,
        in_submenu: bool,
        from_search: bool,
    ) -> Vec<Action> {
        if let Some(menu) = self.context_menu.as_mut() {
            if in_submenu {
                menu.select_sub(index);
            } else {
                menu.select(index);
                if menu.selected_is_hermes() {
                    menu.open_submenu();
                    return Vec::new();
                }
                menu.close_submenu();
            }
        }
        self.run_context_menu_entry(from_search)
    }

    /// Closes the menu and asks for what its highlighted entry stands for, the
    /// same thing its key does. Closed first: the actions refuse to run under
    /// an overlay.
    pub fn run_context_menu_entry(&mut self, from_search: bool) -> Vec<Action> {
        let Some(open) = self.context_menu.as_mut() else {
            return Vec::new();
        };
        if open.selected_is_hermes() && !open.submenu_is_open() {
            open.open_submenu();
            return Vec::new();
        }
        let Some(menu) = self.context_menu.take() else {
            return Vec::new();
        };
        let chosen = if menu.submenu_is_open() {
            menu.selected_hermes_action().map(Action::RunHermesEntry)
        } else {
            menu.selected_action().map(|action| match action {
                // Enter on a result shows it in the other panel.
                ContextAction::Open if from_search => Action::ActivateSearchResult,
                other => Action::RunContextEntry(other),
            })
        };
        chosen.into_iter().collect()
    }
}

#[cfg(test)]
mod hermes_folder_entry_tests {
    use super::*;

    fn facts(cursor: HermesCursor) -> HermesFacts {
        HermesFacts {
            ready: true,
            active: true,
            has_live_pipeline: true,
            cursor: Some(cursor),
        }
    }

    fn folder() -> HermesCursor {
        HermesCursor {
            is_dir: true,
            ..HermesCursor::default()
        }
    }

    const FOLDER_ENTRIES: [HermesAction; 5] = [
        HermesAction::FileOnce,
        HermesAction::WatchSource,
        HermesAction::ChangeSource,
        HermesAction::AddTarget,
        HermesAction::ChangeTarget,
    ];

    fn enabled(facts: &HermesFacts) -> [bool; 5] {
        FOLDER_ENTRIES.map(|action| facts.action_enabled(action))
    }

    #[test]
    fn a_folder_can_be_filed_and_become_a_source_or_a_target() {
        assert_eq!(enabled(&facts(folder())), [true; 5]);
    }

    #[test]
    fn a_file_gets_none_of_the_folder_entries() {
        assert_eq!(enabled(&facts(HermesCursor::default())), [false; 5]);
    }

    #[test]
    fn before_anything_is_set_up_only_the_setup_entries_are_usable() {
        let mut new = facts(folder());
        new.active = false;
        assert_eq!(enabled(&new), [false, true, true, true, true]);
        for action in [
            HermesAction::ProcessAgain,
            HermesAction::Exclude,
            HermesAction::Inspect,
        ] {
            assert!(!new.action_enabled(action), "{action:?}");
        }
    }

    #[test]
    fn with_hermes_off_or_no_row_under_the_cursor_nothing_is_usable() {
        let off = HermesFacts {
            cursor: Some(folder()),
            ..HermesFacts::default()
        };
        let no_row = HermesFacts {
            cursor: None,
            ..facts(folder())
        };
        for action in HermesAction::ALL {
            assert!(!off.action_enabled(action), "{action:?}");
            assert!(!no_row.action_enabled(action), "{action:?}");
        }
    }

    #[test]
    fn only_what_lies_in_a_folder_hermes_reads_is_read_again() {
        let again = |cursor| facts(cursor).action_enabled(HermesAction::ProcessAgain);
        let inside = HermesCursor {
            lies_in_an_indexed_folder: true,
            ..HermesCursor::default()
        };
        assert!(again(inside));
        assert!(!again(HermesCursor::default()));
        assert!(!again(HermesCursor {
            excluded: true,
            ..inside
        }));
    }
}
