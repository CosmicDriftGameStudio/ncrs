use crate::app::*;

impl App {
    pub(in crate::app) fn close_context_menu(&mut self) {
        self.panels.context_menu = None;
    }

    pub(in crate::app) fn update_files_menu(&mut self, message: MenuMessage) -> Task<Message> {
        match message {
            MenuMessage::VolumeMenu(side) => self.request_volume_menu(side, false),
            MenuMessage::VolumeMenuFacts {
                request,
                side,
                on_favourites,
                facts,
            } => {
                if request == self.panels.volume_menu_request {
                    let overlay_is_open = self.overlay_is_open();
                    self.panels
                        .open_volume_menu_on(side, on_favourites, overlay_is_open, facts);
                }
                Task::none()
            }
            context_menu_message => self.update_context_menu(context_menu_message),
        }
    }

    fn update_context_menu(&mut self, message: MenuMessage) -> Task<Message> {
        let nav = match message {
            MenuMessage::ContextMenuAt { side, index } => {
                let pointer = self.input.lock().pointer;
                self.open_context_menu(side, index, Some(pointer));
                return Task::none();
            }
            MenuMessage::ContextMenuKey => {
                let side = self.panels.active;
                let index = self.panel(side).selected;
                self.open_context_menu(side, index, None);
                return Task::none();
            }
            MenuMessage::ContextMenuActivate => return self.run_context_menu_entry(),
            MenuMessage::ContextMenuClick(index) => return self.click_context_menu(index, false),
            MenuMessage::ContextMenuSubClick(index) => return self.click_context_menu(index, true),
            MenuMessage::ContextMenuMove(delta) => MenuNav::Move(delta),
            MenuMessage::ContextMenuFirst => MenuNav::First,
            MenuMessage::ContextMenuLast => MenuNav::Last,
            MenuMessage::ContextMenuHover(index) => MenuNav::Hover(index),
            MenuMessage::ContextMenuOpenSub => MenuNav::OpenSub,
            MenuMessage::ContextMenuCloseSub => MenuNav::CloseSub,
            MenuMessage::ContextMenuSubHover(index) => MenuNav::SubHover(index),
            MenuMessage::ContextMenuClose => MenuNav::Close,
            _ => return Task::none(),
        };
        self.panels.navigate_context_menu(nav);
        Task::none()
    }

    pub(in crate::app) fn context_menu_facts(&self) -> MenuFacts {
        let can_copy = ncrs_ops::has_rows(rows_of(self.active_panel()));
        MenuFacts {
            search_cursor: self
                .open_search_view()
                .map(|search| search.cursor_target().is_some()),
            has_sources: !self.action_sources().is_empty(),
            can_copy_path: can_copy,
            can_copy_name: can_copy,
            hermes: self.hermes_facts(),
        }
    }

    fn menu_open_ctx(&self) -> MenuOpenCtx {
        MenuOpenCtx {
            blocking_overlay_is_open: self.blocking_overlay_is_open(),
            search_side: self.search.open.then_some(self.search.side),
            visible_rows: self.visible_rows,
            window: self.window_size,
            favourites_bar_height: self.favourites_bar_height(),
        }
    }

    /// The menu of the search results at the pointer, for the file the cursor
    /// is on or the tagged ones.
    pub(in crate::app) fn open_search_context_menu(&mut self) {
        if self.blocking_overlay_is_open() || !self.search_has_keys() {
            return;
        }
        self.job_error = None;
        self.notice = None;
        let pointer = self.input.lock().pointer;
        let facts = self.context_menu_facts();
        self.panels
            .show_search_context_menu(pointer, self.window_size, &facts);
    }

    /// Opens the context menu on row `index` of `side`, at `anchor` or, without
    /// one, below that row.
    pub(in crate::app) fn open_context_menu(
        &mut self,
        side: PanelSide,
        index: usize,
        anchor: Option<Point>,
    ) {
        let ctx = self.menu_open_ctx();
        let Some(place_at) = self.panels.point_at_menu_row(side, index, anchor, &ctx) else {
            return;
        };
        self.job_error = None;
        self.notice = None;
        let facts = self.context_menu_facts();
        self.panels
            .show_context_menu(side, place_at, self.window_size, &facts);
    }

    fn run_context_menu_entry(&mut self) -> Task<Message> {
        let from_search = self.search_has_keys();
        let actions = self.panels.run_context_menu_entry(from_search);
        self.run_panel_actions(actions)
    }

    fn click_context_menu(&mut self, index: usize, in_submenu: bool) -> Task<Message> {
        let from_search = self.search_has_keys();
        let actions = self
            .panels
            .click_context_menu(index, in_submenu, from_search);
        self.run_panel_actions(actions)
    }
}
