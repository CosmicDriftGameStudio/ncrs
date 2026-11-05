use crate::app::*;
use ncrs_monitor::Action as Ask;

impl App {
    fn monitor_ctx(&self) -> ncrs_monitor::Ctx {
        ncrs_monitor::Ctx {
            window_size: self.window_size,
            pointer: self.input.lock().pointer,
        }
    }

    pub(in crate::app) fn monitor_view_ctx(&self) -> ncrs_monitor::ViewCtx<'_> {
        ncrs_monitor::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            window_size: self.window_size,
        }
    }

    pub(in crate::app) fn monitor_badge(&self) -> Option<statusbar::MonitorBadge<Message>> {
        ncrs_monitor::status_text(&self.monitor).map(|text| statusbar::MonitorBadge {
            text,
            on_press: Message::Monitor(MonitorMessage::Toggle),
        })
    }

    pub(in crate::app) fn monitor_area(&self) -> Container<'_, Message> {
        container(ncrs_monitor::view(&self.monitor, self.monitor_view_ctx()).map(Message::Monitor))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    pub(in crate::app) fn update_monitor(&mut self, message: MonitorMessage) -> Task<Message> {
        let opening = matches!(message, MonitorMessage::Toggle) && !self.monitor.is_open();
        if opening && self.dialog_is_open() {
            return Task::none();
        }
        let left = if opening {
            self.leave_pages_for_monitor()
        } else {
            Task::none()
        };
        let ctx = self.monitor_ctx();
        let (task, asked) = ncrs_monitor::update(&mut self.monitor, message, &ctx);
        let action_task = match asked {
            Some(action) => self.run_monitor_action(action),
            None => Task::none(),
        };
        Task::batch([left, task.map(Message::Monitor), action_task])
    }

    /// A click that no widget took may have taken the focus from the filter.
    pub(in crate::app) fn monitor_clicked_away(&mut self) -> Task<Message> {
        if self.monitor.is_open() {
            self.update_monitor(MonitorMessage::ClickedAway)
        } else {
            Task::none()
        }
    }

    /// The System page stands in for the panels, like the settings tab, and
    /// replaces whichever of them is up.
    fn leave_pages_for_monitor(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if self.settings.page.is_some() {
            tasks.push(self.leave_settings_page());
        }
        if self.hermes_chat.open {
            tasks.push(self.update_hermes_chat(HermesChatMessage::Close));
        }
        self.panels.context_menu = None;
        self.panels.set_path_field(None);
        self.notice = None;
        Task::batch(tasks)
    }

    /// A page opened over the System page, from a menu or a key: it wins.
    pub(in crate::app) fn monitor_yields_to_other_pages(&mut self) -> Task<Message> {
        let other_page_is_open = self.settings.page.is_some() || self.hermes_chat.open;
        if !(self.monitor.is_open() && other_page_is_open) {
            return Task::none();
        }
        let ctx = self.monitor_ctx();
        let (task, _) = ncrs_monitor::update(&mut self.monitor, MonitorMessage::Close, &ctx);
        task.map(Message::Monitor)
    }

    fn run_monitor_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Opened | Ask::Closed => Task::none(),
            Ask::CopyToClipboard(text) => {
                let notice = self.lang.text(Msg::MonitorNoticeCopied).to_owned();
                Task::batch([iced::clipboard::write(text), self.show_notice(notice)])
            }
            Ask::GoToFolder { folder, file } => self.show_folder_in_active_panel(folder, file),
        }
    }

    /// The folder in the active panel, as the search shows a hit: whatever
    /// covered the panel is closed, and a panel in an archive or on a server
    /// reads the local folder instead.
    fn show_folder_in_active_panel(
        &mut self,
        folder: std::path::PathBuf,
        file: Option<String>,
    ) -> Task<Message> {
        let side = self.panels.active;
        let closed_search = if self.search.open && self.search.side == side {
            self.close_search()
        } else {
            Task::none()
        };
        if self.ops.trash_view.is_some() && self.ops.trash_side == side {
            self.ops.close_trash();
        }
        self.panels.panel_mut(side).drop_hit_list();
        Task::batch([closed_search, self.load(side, folder, file)])
    }
}
