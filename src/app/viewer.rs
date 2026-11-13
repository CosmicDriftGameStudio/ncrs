use crate::app::*;
use ncrs_viewer::Action as Ask;

impl App {
    fn viewer_ctx(&self) -> ncrs_viewer::Ctx {
        ncrs_viewer::Ctx {
            lang: self.lang,
            window_size: self.window_size,
        }
    }

    pub(in crate::app) fn viewer_area(&self) -> Container<'_, Message> {
        let ctx = ncrs_viewer::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            home: &self.home,
        };
        container(ncrs_viewer::view(&self.viewer, ctx).map(Message::Viewer))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// F3 on a file the rules send to the viewer tab; the tab shows when the
    /// file is read.
    pub(in crate::app) fn open_in_viewer(&mut self, path: PathBuf) -> Task<Message> {
        self.job_error = None;
        self.notice = None;
        ncrs_viewer::open(&mut self.viewer, path).map(Message::Viewer)
    }

    pub(in crate::app) fn update_viewer(&mut self, message: ncrs_viewer::Message) -> Task<Message> {
        let was_open = self.open_pages();
        let ctx = self.viewer_ctx();
        let (task, asked) = ncrs_viewer::update(&mut self.viewer, message, &ctx);
        let action_task = match asked {
            Some(action) => self.run_viewer_action(action),
            None => Task::none(),
        };
        self.follow_opened_pages(was_open);
        Task::batch([task.map(Message::Viewer), action_task])
    }

    fn run_viewer_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::OpenUrl(url) => {
                self.open_release_page(&url);
                Task::none()
            }
            Ask::Reveal(file) => self.reveal_in_active_panel(&file),
            Ask::OpenExternal(path) => {
                self.open_with_system(&path);
                Task::none()
            }
            Ask::Edit(path) => {
                self.launch_external(fs::OpenKind::Edit, path, None, CopyOf::Nothing, false);
                Task::none()
            }
            Ask::OpenSystemInstead { path, notice } => {
                self.open_with_system(&path);
                self.show_notice(notice)
            }
            Ask::Error(reason) => {
                self.set_status_error(&reason);
                Task::none()
            }
        }
    }
}
