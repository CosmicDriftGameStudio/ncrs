//! The Git tab: the branches, worktrees, commit graph and diffs of one repository.

use crate::app::update::git_repository_of;
use crate::app::*;
use ncrs_git_view::Action as Ask;

impl App {
    fn git_ctx(&self) -> ncrs_git_view::Ctx {
        ncrs_git_view::Ctx { lang: self.lang }
    }

    pub(in crate::app) fn git_area(&self) -> Container<'_, Message> {
        let ctx = ncrs_git_view::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            date_format: self.columns.date_format,
        };
        container(ncrs_git_view::view(&self.git_view, ctx).map(Message::GitView))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// "Open Git view": shows the tab for the repository the cursor or the
    /// folder of the active panel is in, and reads it.
    pub(in crate::app) fn open_git_view(&mut self) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let Some(root) = git_repository_of(self.active_panel()) else {
            self.set_status_error(self.lang.text(Msg::GitViewNotARepository));
            return Task::none();
        };
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let was_open = self.open_pages();
        let task = ncrs_git_view::open(&mut self.git_view, root, name).map(Message::GitView);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Git;
        task
    }

    pub(in crate::app) fn update_git_tab(
        &mut self,
        message: ncrs_git_view::Message,
    ) -> Task<Message> {
        let was_open = self.open_pages();
        let ctx = self.git_ctx();
        let (task, asked) = ncrs_git_view::update(&mut self.git_view, message, &ctx);
        let action_task = match asked {
            Some(action) => self.run_git_action(action),
            None => Task::none(),
        };
        self.follow_opened_pages(was_open);
        Task::batch([task.map(Message::GitView), action_task])
    }

    fn run_git_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::ShowWorktree(folder) => self.reveal_in_active_panel(&folder),
            Ask::ViewFile(file) => self.open_in_viewer(file),
        }
    }
}
