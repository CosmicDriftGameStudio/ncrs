//! What the drive menu and the context menu of a panel row send to the app.

use ncrs_core::PanelSide;

use crate::VolumeFacts;

#[derive(Debug, Clone, PartialEq)]
pub enum MenuMessage {
    /// Alt+F1 or Alt+F2: choose the drive of that panel.
    VolumeMenu(PanelSide),
    /// What the drive menu lists has been read. Dropped when `request` is not
    /// the latest one.
    VolumeMenuFacts {
        request: u64,
        side: PanelSide,
        on_favourites: bool,
        facts: VolumeFacts,
    },
    /// Right click (or Ctrl+click on a Mac) on a row: open the menu at the pointer.
    ContextMenuAt {
        side: PanelSide,
        index: usize,
    },
    /// Shift+F10 or a tap of Option: open the menu at the cursor row.
    ContextMenuKey,
    ContextMenuMove(isize),
    ContextMenuFirst,
    ContextMenuLast,
    /// Enter in the open menu: run the highlighted entry.
    ContextMenuActivate,
    /// A click on the entry at this index: run it.
    ContextMenuClick(usize),
    /// The pointer is over the entry at this index.
    ContextMenuHover(usize),
    ContextMenuClose,
    /// Right or Enter on the Hermes entry: open its submenu.
    ContextMenuOpenSub,
    /// Left: back from the submenu to the entry.
    ContextMenuCloseSub,
    /// The pointer is over the entry at this index of the submenu.
    ContextMenuSubHover(usize),
    /// A click on the entry at this index of the submenu: run it.
    ContextMenuSubClick(usize),
}
