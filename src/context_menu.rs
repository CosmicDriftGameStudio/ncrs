//! The context menu of a panel row: which entries it has, which are usable, and
//! the two input sources the keyboard route cannot see (Option tap, pointer).

use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::{event, mouse, window, Point, Size};

use crate::fs::OpenKind;
use crate::hermes_column::FolderRole;
use crate::i18n::Msg;
use crate::messages::{
    ClipboardKind, CommandLineMessage, CommandPaletteMessage, HermesFileMessage,
    KeyBindingsMessage, Message, QuickFixMessage, QuickOpenMessage, ReorganisationPlanMessage,
    ReviewListMessage, RuleSuggestionsMessage, RulesMessage, SavedQueriesMessage, SearchMessage,
    SettingsMessage, TagsMessage, ThemeFetchMessage, TransferKind,
};
use crate::transfer_dialog::TransferDialogMessage;
use crate::ui::layout::{
    MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_SUB_WIDTH, MENU_WIDTH,
};

/// What an entry does. Each one is exactly the message its key sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    Open,
    View,
    Edit,
    Copy,
    Move,
    NewFolder,
    CopyPath,
    CopyName,
    ComputeFolderSizes,
    Trash,
    DeletePermanently,
}

impl ContextAction {
    #[cfg(test)]
    pub const ALL: [ContextAction; 11] = [
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::NewFolder,
        ContextAction::CopyPath,
        ContextAction::CopyName,
        ContextAction::ComputeFolderSizes,
        ContextAction::Trash,
        ContextAction::DeletePermanently,
    ];

    /// Whether a panel inside an archive may run the entry; see
    /// `KeyAction::allowed_in_archive`.
    #[cfg(test)]
    pub fn allowed_in_archive(self) -> bool {
        match self {
            ContextAction::Open
            | ContextAction::Copy
            | ContextAction::CopyPath
            | ContextAction::CopyName
            | ContextAction::Trash
            | ContextAction::DeletePermanently
            | ContextAction::View => true,
            ContextAction::Edit
            | ContextAction::Move
            | ContextAction::NewFolder
            | ContextAction::ComputeFolderSizes => false,
        }
    }

    /// Whether a panel on an SFTP server may run the entry. See
    /// `KeyAction::allowed_on_server`.
    #[cfg(test)]
    pub fn allowed_on_server(self) -> bool {
        match self {
            ContextAction::Open
            | ContextAction::Copy
            | ContextAction::CopyPath
            | ContextAction::CopyName
            | ContextAction::View => true,
            ContextAction::Edit
            | ContextAction::Move
            | ContextAction::NewFolder
            | ContextAction::ComputeFolderSizes
            | ContextAction::Trash
            | ContextAction::DeletePermanently => false,
        }
    }

    pub fn message(self) -> Message {
        match self {
            ContextAction::Open => Message::OpenSelected,
            ContextAction::View => Message::OpenExternal(OpenKind::View),
            ContextAction::Edit => Message::OpenExternal(OpenKind::Edit),
            ContextAction::Copy => Message::Transfer(TransferKind::Copy),
            ContextAction::Move => Message::Transfer(TransferKind::Move),
            ContextAction::NewFolder => Message::CreateDirPrompt,
            ContextAction::CopyPath => Message::CopyToClipboard(ClipboardKind::Path),
            ContextAction::CopyName => Message::CopyToClipboard(ClipboardKind::Name),
            ContextAction::ComputeFolderSizes => Message::ComputeFolderSizes,
            ContextAction::Trash => Message::Delete { permanent: false },
            ContextAction::DeletePermanently => Message::Delete { permanent: true },
        }
    }

    pub fn label(self) -> Msg {
        match self {
            ContextAction::Open => Msg::ContextMenuOpen,
            ContextAction::View => Msg::ContextMenuView,
            ContextAction::Edit => Msg::ContextMenuEdit,
            ContextAction::Copy => Msg::ContextMenuCopy,
            ContextAction::Move => Msg::ContextMenuMove,
            ContextAction::NewFolder => Msg::ContextMenuNewFolder,
            ContextAction::CopyPath => Msg::ContextMenuCopyPath,
            ContextAction::CopyName => Msg::ContextMenuCopyName,
            ContextAction::ComputeFolderSizes => Msg::ContextMenuComputeFolderSizes,
            ContextAction::Trash => Msg::ContextMenuTrash,
            ContextAction::DeletePermanently => Msg::ContextMenuDeletePermanently,
        }
    }
}

const GROUPS: [&[ContextAction]; 5] = [
    &[
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
    ],
    &[
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::NewFolder,
    ],
    &[ContextAction::CopyPath, ContextAction::CopyName],
    &[ContextAction::ComputeFolderSizes],
    &[ContextAction::Trash, ContextAction::DeletePermanently],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Separator,
    Item {
        action: ContextAction,
        enabled: bool,
    },
    /// The entry that opens the Hermes submenu.
    Hermes,
}

impl MenuEntry {
    fn is_usable(self) -> bool {
        matches!(
            self,
            MenuEntry::Item { enabled: true, .. } | MenuEntry::Hermes
        )
    }
}

/// What an entry of the Hermes submenu does with the file under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesAction {
    ShowSuggestion,
    HandToInbox,
    ProcessAgain,
    ReprocessFolder,
    SetInbox,
    SetFilingLocation,
    SetArchive,
    Exclude,
    Include,
    Inspect,
}

impl HermesAction {
    #[cfg(test)]
    pub const ALL: [HermesAction; 10] = [
        HermesAction::ShowSuggestion,
        HermesAction::HandToInbox,
        HermesAction::ProcessAgain,
        HermesAction::ReprocessFolder,
        HermesAction::SetInbox,
        HermesAction::SetFilingLocation,
        HermesAction::SetArchive,
        HermesAction::Exclude,
        HermesAction::Include,
        HermesAction::Inspect,
    ];

    /// Every Hermes entry works on the file or folder under the cursor, whose
    /// path inside an archive is no place Hermes knows.
    #[cfg(test)]
    pub fn allowed_in_archive(self) -> bool {
        match self {
            HermesAction::ShowSuggestion
            | HermesAction::HandToInbox
            | HermesAction::ProcessAgain
            | HermesAction::ReprocessFolder
            | HermesAction::SetInbox
            | HermesAction::SetFilingLocation
            | HermesAction::SetArchive
            | HermesAction::Exclude
            | HermesAction::Include
            | HermesAction::Inspect => false,
        }
    }

    /// Hermes knows no place on a server either.
    #[cfg(test)]
    pub fn allowed_on_server(self) -> bool {
        match self {
            HermesAction::ShowSuggestion
            | HermesAction::HandToInbox
            | HermesAction::ProcessAgain
            | HermesAction::ReprocessFolder
            | HermesAction::SetInbox
            | HermesAction::SetFilingLocation
            | HermesAction::SetArchive
            | HermesAction::Exclude
            | HermesAction::Include
            | HermesAction::Inspect => false,
        }
    }

    pub fn message(self) -> Message {
        match self {
            HermesAction::ShowSuggestion => Message::QuickFix(QuickFixMessage::Open),
            HermesAction::HandToInbox => Message::HermesFile(HermesFileMessage::HandToInbox),
            HermesAction::ProcessAgain => Message::HermesFile(HermesFileMessage::ProcessAgain),
            HermesAction::ReprocessFolder => {
                Message::HermesFile(HermesFileMessage::ReprocessFolder)
            }
            HermesAction::SetInbox => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Inbox))
            }
            HermesAction::SetFilingLocation => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::FilingLocation))
            }
            HermesAction::SetArchive => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Archive))
            }
            HermesAction::Exclude | HermesAction::Include => {
                Message::HermesFile(HermesFileMessage::ToggleExclusion)
            }
            HermesAction::Inspect => Message::HermesFile(HermesFileMessage::ShowInInspection),
        }
    }

    pub fn label(self) -> Msg {
        match self {
            HermesAction::ShowSuggestion => Msg::HermesMenuShowSuggestion,
            HermesAction::HandToInbox => Msg::HermesMenuHandToInbox,
            HermesAction::ProcessAgain => Msg::HermesMenuProcessAgain,
            HermesAction::ReprocessFolder => Msg::HermesMenuReprocessFolder,
            HermesAction::SetInbox => Msg::HermesMenuSetInbox,
            HermesAction::SetFilingLocation => Msg::HermesMenuSetFilingLocation,
            HermesAction::SetArchive => Msg::HermesMenuSetArchive,
            HermesAction::Exclude => Msg::HermesMenuExclude,
            HermesAction::Include => Msg::HermesMenuInclude,
            HermesAction::Inspect => Msg::HermesMenuInspect,
        }
    }

    /// The dimmed word after the label.
    pub fn note(self) -> Option<Msg> {
        match self {
            HermesAction::HandToInbox => Some(Msg::HermesMenuNoteInbox),
            HermesAction::SetArchive => Some(Msg::HermesMenuNoteReadOnly),
            HermesAction::ShowSuggestion
            | HermesAction::ProcessAgain
            | HermesAction::ReprocessFolder
            | HermesAction::SetInbox
            | HermesAction::SetFilingLocation
            | HermesAction::Exclude
            | HermesAction::Include
            | HermesAction::Inspect => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubEntry {
    Separator,
    Item { action: HermesAction, enabled: bool },
}

impl SubEntry {
    fn is_usable(self) -> bool {
        matches!(self, SubEntry::Item { enabled: true, .. })
    }
}

/// How the Hermes entry is built: which of its commands apply to the file.
pub struct HermesMenu<'a> {
    /// The file is excluded, so the menu offers to include it.
    pub excluded: bool,
    pub is_enabled: &'a dyn Fn(HermesAction) -> bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Submenu {
    entries: Vec<SubEntry>,
    selected: usize,
    open: bool,
}

fn sub_groups(excluded: bool) -> [Vec<HermesAction>; 4] {
    [
        vec![HermesAction::ShowSuggestion],
        vec![
            HermesAction::HandToInbox,
            HermesAction::ProcessAgain,
            HermesAction::ReprocessFolder,
        ],
        vec![
            HermesAction::SetInbox,
            HermesAction::SetFilingLocation,
            HermesAction::SetArchive,
        ],
        vec![
            if excluded {
                HermesAction::Include
            } else {
                HermesAction::Exclude
            },
            HermesAction::Inspect,
        ],
    ]
}

/// The usable index after (or before, for a negative `delta`) `from`, going
/// round: past either end it continues at the other one. Stays at `from` when
/// no other entry can be used.
fn next_usable_index(is_usable: &[bool], from: usize, delta: isize) -> usize {
    let count = is_usable.len();
    (1..count)
        .map(|step| {
            if delta < 0 {
                (from + count - step % count) % count
            } else {
                (from + step) % count
            }
        })
        .find(|&index| is_usable.get(index).copied().unwrap_or(false))
        .unwrap_or(from)
}

fn step_usable(entries: &[SubEntry], from: usize, delta: isize) -> usize {
    let usable: Vec<bool> = entries.iter().map(|entry| entry.is_usable()).collect();
    next_usable_index(&usable, from, delta)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    origin: Point,
    entries: Vec<MenuEntry>,
    /// Index into `entries`; always a usable item.
    selected: usize,
    window: Size,
    hermes: Option<Submenu>,
}

impl ContextMenu {
    /// A menu next to `anchor` that stays inside `window`: it opens to the
    /// right and below, and folds to the left or up where there is no room.
    #[cfg(test)]
    pub fn new(anchor: Point, window: Size, is_enabled: impl Fn(ContextAction) -> bool) -> Self {
        Self::with_hermes(anchor, window, is_enabled, None)
    }

    /// As [`Self::new`], with the Hermes entry after the file groups when
    /// `hermes` says the column is up.
    pub fn with_hermes(
        anchor: Point,
        window: Size,
        is_enabled: impl Fn(ContextAction) -> bool,
        hermes: Option<HermesMenu<'_>>,
    ) -> Self {
        let mut entries = Vec::new();
        for (group_index, group) in GROUPS.iter().enumerate() {
            if group_index > 0 {
                entries.push(MenuEntry::Separator);
            }
            entries.extend(group.iter().map(|&action| MenuEntry::Item {
                action,
                enabled: is_enabled(action),
            }));
        }
        let submenu = hermes.map(|menu| {
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::Hermes);
            Submenu::new(&menu)
        });
        let selected = entries
            .iter()
            .position(|entry| entry.is_usable())
            .unwrap_or(0);
        let size = Self::size_of(&entries);
        Self {
            origin: Point::new(
                place(anchor.x, size.width, window.width),
                place(anchor.y, size.height, window.height),
            ),
            entries,
            selected,
            window,
            hermes: submenu,
        }
    }

    pub fn size_of(entries: &[MenuEntry]) -> Size {
        let height: f32 = entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                MenuEntry::Item { .. } | MenuEntry::Hermes => MENU_ITEM_HEIGHT,
            })
            .sum();
        Size::new(MENU_WIDTH, height + 2.0 * MENU_PADDING)
    }

    pub fn origin(&self) -> Point {
        self.origin
    }

    pub fn entries(&self) -> &[MenuEntry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_action(&self) -> Option<ContextAction> {
        match self.entries.get(self.selected) {
            Some(MenuEntry::Item {
                action,
                enabled: true,
            }) => Some(*action),
            _ => None,
        }
    }

    /// Highlights the entry at `index` if it can be used.
    pub fn select(&mut self, index: usize) {
        if self
            .entries
            .get(index)
            .is_some_and(|entry| entry.is_usable())
        {
            self.selected = index;
        }
    }

    /// The pointer is over the entry at `index`: the Hermes entry opens its
    /// submenu, any other closes it.
    pub fn hover(&mut self, index: usize) {
        self.select(index);
        if self.selected == index {
            if self.selected_is_hermes() {
                self.open_submenu();
            } else {
                self.close_submenu();
            }
        }
    }

    /// Moves to the next usable entry in the direction of `delta`, skipping
    /// separators and greyed-out ones, and goes round at either end. In the
    /// open submenu it moves there.
    pub fn move_by(&mut self, delta: isize) {
        if let Some(sub) = self.open_sub_mut() {
            sub.selected = step_usable(&sub.entries, sub.selected, delta);
            return;
        }
        let usable: Vec<bool> = self.entries.iter().map(|entry| entry.is_usable()).collect();
        self.selected = next_usable_index(&usable, self.selected, delta);
    }

    pub fn select_first(&mut self) {
        if let Some(sub) = self.open_sub_mut() {
            sub.selected = sub.first_usable();
            return;
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.is_usable()) {
            self.selected = index;
        }
    }

    pub fn select_last(&mut self) {
        if let Some(sub) = self.open_sub_mut() {
            sub.selected = sub
                .entries
                .iter()
                .rposition(|entry| entry.is_usable())
                .unwrap_or(sub.selected);
            return;
        }
        if let Some(index) = self.entries.iter().rposition(|entry| entry.is_usable()) {
            self.selected = index;
        }
    }

    fn open_sub_mut(&mut self) -> Option<&mut Submenu> {
        self.hermes.as_mut().filter(|sub| sub.open)
    }

    pub fn selected_is_hermes(&self) -> bool {
        matches!(self.entries.get(self.selected), Some(MenuEntry::Hermes))
    }

    pub fn submenu_is_open(&self) -> bool {
        self.hermes.as_ref().is_some_and(|sub| sub.open)
    }

    /// Opens the submenu of the highlighted Hermes entry, with its first
    /// usable entry highlighted. False when the highlight is on another entry.
    pub fn open_submenu(&mut self) -> bool {
        if !self.selected_is_hermes() {
            return false;
        }
        match self.hermes.as_mut() {
            Some(sub) => {
                if !sub.open {
                    sub.selected = sub.first_usable();
                }
                sub.open = true;
                true
            }
            None => false,
        }
    }

    /// True when a submenu was open.
    pub fn close_submenu(&mut self) -> bool {
        match self.hermes.as_mut() {
            Some(sub) if sub.open => {
                sub.open = false;
                true
            }
            _ => false,
        }
    }

    pub fn sub_entries(&self) -> &[SubEntry] {
        self.hermes
            .as_ref()
            .map_or(&[], |sub| sub.entries.as_slice())
    }

    pub fn sub_selected(&self) -> usize {
        self.hermes.as_ref().map_or(0, |sub| sub.selected)
    }

    /// Highlights the entry of the open submenu at `index` if it can be used.
    pub fn select_sub(&mut self, index: usize) {
        if let Some(sub) = self.open_sub_mut() {
            if sub
                .entries
                .get(index)
                .is_some_and(|entry| entry.is_usable())
            {
                sub.selected = index;
            }
        }
    }

    pub fn selected_hermes_action(&self) -> Option<HermesAction> {
        let sub = self.hermes.as_ref().filter(|sub| sub.open)?;
        match sub.entries.get(sub.selected) {
            Some(SubEntry::Item {
                action,
                enabled: true,
            }) => Some(*action),
            _ => None,
        }
    }

    /// Where the open submenu goes: beside the Hermes entry, to the right or,
    /// where that has no room, to the left, and up as far as it must to stay
    /// in the window. `None` while it is closed.
    pub fn submenu_origin(&self) -> Option<Point> {
        let sub = self.hermes.as_ref().filter(|sub| sub.open)?;
        let height = sub_height(&sub.entries);
        let row_top = self.origin.y
            + MENU_PADDING
            + self
                .entries
                .iter()
                .take(self.selected)
                .map(entry_height)
                .sum::<f32>();
        let right = self.origin.x + MENU_WIDTH;
        let x = if right + MENU_SUB_WIDTH > self.window.width {
            self.origin.x - MENU_SUB_WIDTH
        } else {
            right
        };
        Some(Point::new(
            x.max(0.0),
            place(row_top - MENU_PADDING, height, self.window.height),
        ))
    }
}

impl Submenu {
    fn new(menu: &HermesMenu<'_>) -> Self {
        let mut entries = Vec::new();
        for (group_index, group) in sub_groups(menu.excluded).iter().enumerate() {
            if group_index > 0 {
                entries.push(SubEntry::Separator);
            }
            entries.extend(group.iter().map(|&action| SubEntry::Item {
                action,
                enabled: (menu.is_enabled)(action),
            }));
        }
        let mut sub = Self {
            entries,
            selected: 0,
            open: false,
        };
        sub.selected = sub.first_usable();
        sub
    }

    fn first_usable(&self) -> usize {
        self.entries
            .iter()
            .position(|entry| entry.is_usable())
            .unwrap_or(0)
    }
}

fn entry_height(entry: &MenuEntry) -> f32 {
    match entry {
        MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
        MenuEntry::Item { .. } | MenuEntry::Hermes => MENU_ITEM_HEIGHT,
    }
}

pub fn sub_height(entries: &[SubEntry]) -> f32 {
    let inner: f32 = entries
        .iter()
        .map(|entry| match entry {
            SubEntry::Separator => MENU_SEPARATOR_HEIGHT,
            SubEntry::Item { .. } => MENU_ITEM_HEIGHT,
        })
        .sum();
    inner + 2.0 * MENU_PADDING
}

/// Where a span of `length` starts so it stays within `0..limit`: at `anchor`,
/// or ending there when it would not fit.
pub fn place(anchor: f32, length: f32, limit: f32) -> f32 {
    let start = if anchor + length > limit {
        anchor - length
    } else {
        anchor
    };
    start.min(limit - length).max(0.0)
}

/// A tap of Option (Alt) on its own: pressed and released with no other key,
/// modifier or mouse button in between.
#[derive(Debug, Default)]
pub struct AltTap {
    armed: bool,
    held: Modifiers,
}

impl AltTap {
    /// Returns true when the release completes a tap.
    pub fn modifiers_changed(&mut self, now: Modifiers) -> bool {
        let mut tapped = false;
        if now == Modifiers::ALT {
            if self.held.is_empty() {
                self.armed = true;
            }
        } else if now.is_empty() {
            tapped = self.armed;
            self.armed = false;
        } else {
            self.armed = false;
        }
        self.held = now;
        tapped
    }

    /// Any key other than the Alt key itself spoils the tap, bound or not.
    pub fn key_pressed(&mut self, key: &Key) {
        if !matches!(key, Key::Named(Named::Alt | Named::AltGraph)) {
            self.armed = false;
        }
    }

    pub fn cancel(&mut self) {
        self.armed = false;
    }
}

/// What the window's raw events leave behind that no widget reports: where the
/// pointer is, and whether Option was just tapped.
#[derive(Debug, Default)]
pub struct InputState {
    pub pointer: Point,
    pub alt_tap: AltTap,
    /// The path field is being edited, so Escape and Tab leave it. Kept here
    /// because the text field captures those keys before the key routing.
    pub path_editing: bool,
    /// The command line has the focus: its text field captures Escape before
    /// the key routing.
    pub command_line: bool,
    /// The search view is up and nothing is on top of it: Escape closes it,
    /// and an Option tap opens no menu on the panel it hides.
    pub search_open: bool,
    /// A job is running, so Escape stops it before it closes the search.
    pub job_running: bool,
    /// Quick Open is up: Escape closes it, even with a job running, and an
    /// Option tap opens no menu.
    pub quick_open: bool,
    /// The command palette is up: same as Quick Open.
    pub command_palette: bool,
    /// The keyboard shortcuts dialog is up and records no key: same as Quick
    /// Open. While it records, Escape is the routing's, which cancels.
    pub key_bindings: bool,
    /// The theme download card is up: same as Quick Open.
    pub theme_fetch: bool,
    /// The rule suggestions view is up: same as Quick Open.
    pub rule_suggestions: bool,
    /// The rules page is up: same as Quick Open.
    pub rules: bool,
    pub review_list: bool,
    pub saved_queries: bool,
    /// The tags page is up: same as Quick Open.
    pub tags: bool,
    /// The document search page is up: same as Quick Open.
    /// The sidebar of the Hermes card has the arrows, not the page.
    pub hermes_sidebar: bool,
    /// The copy or move confirmation is up: its target field takes Escape.
    pub transfer_dialog: bool,
    /// The reorganisation plan view is up: same as Quick Open.
    pub reorganisation_plan: bool,
    /// The settings tab is up with nothing on top: Escape closes it, even from
    /// its search field, and an Option tap opens no menu.
    pub settings: bool,
    /// A row is held with the left button: the pointer and the button decide
    /// whether it becomes a drag.
    pub panel_drag_armed: bool,
}

impl InputState {
    /// The overlay Escape closes, first match wins: the order is the stacking
    /// order, so an overlay on top closes before the one below it.
    fn closes_overlay(&self, key: &Key) -> Option<Message> {
        if !matches!(key, Key::Named(Named::Escape)) {
            return None;
        }
        [
            (
                self.command_line,
                Message::CommandLine(CommandLineMessage::Leave),
            ),
            (self.quick_open, Message::QuickOpen(QuickOpenMessage::Close)),
            (
                self.command_palette,
                Message::CommandPalette(CommandPaletteMessage::Close),
            ),
            (
                self.key_bindings,
                Message::KeyBindings(KeyBindingsMessage::Close),
            ),
            (
                self.theme_fetch,
                Message::ThemeFetch(ThemeFetchMessage::Close),
            ),
            (
                self.rule_suggestions,
                Message::RuleSuggestions(RuleSuggestionsMessage::Close),
            ),
            (self.rules, Message::Rules(RulesMessage::Close)),
            (
                self.review_list,
                Message::ReviewList(ReviewListMessage::Close),
            ),
            (
                self.transfer_dialog,
                Message::TransferDialog(TransferDialogMessage::Cancel),
            ),
            (
                self.saved_queries,
                Message::SavedQueries(SavedQueriesMessage::Close),
            ),
            (self.tags, Message::Tags(TagsMessage::Close)),
            (
                self.reorganisation_plan,
                Message::ReorganisationPlan(ReorganisationPlanMessage::Close),
            ),
            (self.settings, Message::Settings(SettingsMessage::Close)),
        ]
        .into_iter()
        .find_map(|(open, close)| open.then_some(close))
    }

    pub fn handle(&mut self, event: &event::Event, status: event::Status) -> Option<Message> {
        match event {
            event::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                self.pointer = *position;
                self.panel_drag_armed
                    .then_some(Message::PanelDragMoved(*position))
            }
            event::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.panel_drag_armed =>
            {
                Some(Message::PanelDragReleased)
            }
            event::Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                self.alt_tap.cancel();
                // A press nothing took (not a row, not a menu, not the field)
                // is a click elsewhere.
                (status == event::Status::Ignored).then_some(Message::ClickedOutside)
            }
            event::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                self.alt_tap.key_pressed(key);
                let leaves_field = matches!(key, Key::Named(Named::Escape | Named::Tab));
                if self.path_editing && leaves_field {
                    return Some(Message::PathFieldCancel);
                }
                if let Some(close) = self.closes_overlay(key) {
                    return Some(close);
                }
                // The search's text fields capture Escape, so the routing
                // never sees it there.
                if !(self.search_open && matches!(key, Key::Named(Named::Escape))) {
                    return None;
                }
                Some(if self.job_running {
                    Message::AbortJob
                } else {
                    Message::Search(SearchMessage::Close)
                })
            }
            event::Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                self.alt_tap.cancel();
                None
            }
            event::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                // Still fed, so the tap's bookkeeping stays right, but a tap
                // while a path is being typed opens nothing.
                let tapped = self.alt_tap.modifiers_changed(*modifiers);
                (tapped
                    && !self.path_editing
                    && !self.command_line
                    && !self.search_open
                    && !self.quick_open
                    && !self.command_palette
                    && !self.key_bindings
                    && !self.theme_fetch
                    && !self.rule_suggestions
                    && !self.rules
                    && !self.review_list
                    && !self.saved_queries
                    && !self.tags
                    && !self.transfer_dialog
                    && !self.reorganisation_plan
                    && !self.settings)
                    .then_some(Message::ContextMenuKey)
            }
            event::Event::Window(window::Event::Unfocused) => {
                self.alt_tap.cancel();
                None
            }
            _ => None,
        }
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{key::Physical, Location};

    const WINDOW: Size = Size::new(1200.0, 760.0);

    fn menu_with(disabled: &[ContextAction]) -> ContextMenu {
        ContextMenu::new(Point::new(10.0, 10.0), WINDOW, |action| {
            !disabled.contains(&action)
        })
    }

    fn selected_action(menu: &ContextMenu) -> ContextAction {
        menu.selected_action().unwrap()
    }

    fn key_pressed(key: Key) -> event::Event {
        event::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::ALT,
            text: None,
            repeat: false,
        })
    }

    fn modifiers(modifiers: Modifiers) -> event::Event {
        event::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers))
    }

    fn feed(input: &mut InputState, events: &[event::Event]) -> Vec<Message> {
        events
            .iter()
            .filter_map(|event| input.handle(event, event::Status::Ignored))
            .collect()
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
        assert_eq!(menu.entries().len(), 11 + 4);
        assert_eq!(selected_action(&menu), ContextAction::Open);
    }

    #[test]
    fn navigation_skips_separators_and_greyed_out_entries() {
        let mut menu = menu_with(&[ContextAction::View, ContextAction::Edit]);
        menu.move_by(1);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
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
        menu.select(4);
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
            |_| true,
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

    #[test]
    fn each_entry_sends_the_message_of_its_key() {
        assert_eq!(ContextAction::Open.message(), Message::OpenSelected);
        assert_eq!(
            ContextAction::Trash.message(),
            Message::Delete { permanent: false }
        );
        assert_eq!(
            ContextAction::DeletePermanently.message(),
            Message::Delete { permanent: true }
        );
        for action in GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .filter(|action| **action != ContextAction::ComputeFolderSizes)
        {
            assert!(
                crate::keymap::Keymap::built_in()
                    .shortcut_label(&action.message())
                    .is_some(),
                "{action:?} has no key to show"
            );
        }
    }

    #[test]
    fn tapping_option_opens_the_menu() {
        let mut input = InputState::default();
        let messages = feed(
            &mut input,
            &[
                modifiers(Modifiers::ALT),
                key_pressed(Key::Named(Named::Alt)),
                modifiers(Modifiers::empty()),
            ],
        );
        assert_eq!(messages, [Message::ContextMenuKey]);
    }

    #[test]
    fn option_with_any_key_does_not_open_it() {
        for key in [
            Key::Named(Named::F1),
            Key::Character("c".into()),
            Key::Named(Named::F11),
            Key::Named(Named::Super),
            Key::Named(Named::ArrowLeft),
            Key::Named(Named::ArrowRight),
        ] {
            let mut input = InputState::default();
            let messages = feed(
                &mut input,
                &[
                    modifiers(Modifiers::ALT),
                    key_pressed(key.clone()),
                    modifiers(Modifiers::empty()),
                ],
            );
            assert!(messages.is_empty(), "{key:?} did not cancel the tap");
        }
    }

    #[test]
    fn option_with_another_modifier_does_not_open_it() {
        let mut input = InputState::default();
        let with_alt_first = feed(
            &mut input,
            &[
                modifiers(Modifiers::ALT),
                modifiers(Modifiers::ALT | Modifiers::SHIFT),
                modifiers(Modifiers::ALT),
                modifiers(Modifiers::empty()),
            ],
        );
        assert!(with_alt_first.is_empty());

        let with_shift_first = feed(
            &mut input,
            &[
                modifiers(Modifiers::SHIFT),
                modifiers(Modifiers::SHIFT | Modifiers::ALT),
                modifiers(Modifiers::SHIFT),
                modifiers(Modifiers::empty()),
            ],
        );
        assert!(with_shift_first.is_empty());
    }

    #[test]
    fn option_with_a_mouse_click_does_not_open_it() {
        let mut input = InputState::default();
        let _ = feed(&mut input, &[modifiers(Modifiers::ALT)]);
        let click = event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert_eq!(
            input.handle(&click, event::Status::Captured),
            None,
            "a click on a row is not a click beside the menu"
        );
        assert!(feed(&mut input, &[modifiers(Modifiers::empty())]).is_empty());
    }

    #[test]
    fn losing_focus_while_option_is_held_does_not_open_it_later() {
        let mut input = InputState::default();
        let _ = feed(&mut input, &[modifiers(Modifiers::ALT)]);
        let _ = feed(
            &mut input,
            &[event::Event::Window(window::Event::Unfocused)],
        );
        assert!(feed(&mut input, &[modifiers(Modifiers::empty())]).is_empty());
    }

    #[test]
    fn a_wheel_turn_while_option_is_held_spoils_the_tap() {
        let mut input = InputState::default();
        let wheel = event::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
        let messages = feed(
            &mut input,
            &[
                modifiers(Modifiers::ALT),
                wheel,
                modifiers(Modifiers::empty()),
            ],
        );
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn a_tap_while_the_path_is_edited_opens_nothing() {
        let mut input = InputState {
            path_editing: true,
            ..InputState::default()
        };
        let while_editing = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert!(while_editing.is_empty(), "{while_editing:?}");
        input.path_editing = false;
        let after_editing = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert_eq!(
            after_editing,
            [Message::ContextMenuKey],
            "the next tap still works"
        );
    }

    #[test]
    fn escape_closes_the_search_whatever_captured_it_and_a_tap_opens_no_menu_over_it() {
        let mut input = InputState::default();
        let escape = key_pressed(Key::Named(Named::Escape));
        assert_eq!(input.handle(&escape, event::Status::Captured), None);

        input.search_open = true;
        for status in [event::Status::Captured, event::Status::Ignored] {
            assert_eq!(
                input.handle(&escape, status),
                Some(Message::Search(SearchMessage::Close))
            );
        }
        input.job_running = true;
        assert_eq!(
            input.handle(&escape, event::Status::Ignored),
            Some(Message::AbortJob),
            "a running job is stopped before the search is closed"
        );
        input.job_running = false;
        let letter = key_pressed(Key::Character("a".into()));
        assert_eq!(input.handle(&letter, event::Status::Ignored), None);
        let tap = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert!(tap.is_empty(), "{tap:?}");
    }

    #[test]
    fn escape_closes_quick_open_even_with_a_job_running_and_a_tap_opens_no_menu() {
        let mut input = InputState::default();
        let escape = key_pressed(Key::Named(Named::Escape));
        input.quick_open = true;
        input.job_running = true;
        for status in [event::Status::Captured, event::Status::Ignored] {
            assert_eq!(
                input.handle(&escape, status),
                Some(Message::QuickOpen(QuickOpenMessage::Close))
            );
        }
        let tap = feed(
            &mut input,
            &[modifiers(Modifiers::ALT), modifiers(Modifiers::empty())],
        );
        assert!(tap.is_empty(), "{tap:?}");
    }

    #[test]
    fn the_pointer_position_is_remembered_and_unclaimed_clicks_close() {
        let mut input = InputState::default();
        let moved = event::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(12.0, 34.0),
        });
        assert_eq!(input.handle(&moved, event::Status::Ignored), None);
        assert_eq!(input.pointer, Point::new(12.0, 34.0));
        let click = event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
        assert_eq!(
            input.handle(&click, event::Status::Ignored),
            Some(Message::ClickedOutside)
        );
    }

    #[test]
    fn escape_leaves_the_command_line_and_tab_does_not() {
        let mut input = InputState {
            command_line: true,
            ..InputState::default()
        };
        assert_eq!(
            input.handle(
                &key_pressed(Key::Named(Named::Escape)),
                event::Status::Captured
            ),
            Some(Message::CommandLine(CommandLineMessage::Leave))
        );
        assert_eq!(
            input.handle(
                &key_pressed(Key::Named(Named::Tab)),
                event::Status::Captured
            ),
            None
        );
    }

    #[test]
    fn a_tap_while_the_command_line_has_the_focus_opens_nothing() {
        let mut input = InputState {
            command_line: true,
            ..InputState::default()
        };
        let tap = [modifiers(Modifiers::ALT), modifiers(Modifiers::empty())];
        assert!(feed(&mut input, &tap).is_empty());
    }

    #[test]
    fn escape_and_tab_leave_the_path_field_whatever_captured_them() {
        let mut input = InputState::default();
        let escape = key_pressed(Key::Named(Named::Escape));
        assert_eq!(input.handle(&escape, event::Status::Captured), None);
        input.path_editing = true;
        for key in [Named::Escape, Named::Tab] {
            assert_eq!(
                input.handle(&key_pressed(Key::Named(key)), event::Status::Captured),
                Some(Message::PathFieldCancel)
            );
        }
        let letter = key_pressed(Key::Character("a".into()));
        assert_eq!(input.handle(&letter, event::Status::Ignored), None);
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
        let mut menu = hermes_menu(&[
            HermesAction::ProcessAgain,
            HermesAction::ReprocessFolder,
            HermesAction::SetInbox,
        ]);
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
}
