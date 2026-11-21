//! The Git tabs: the branches, worktrees, commit graph and diffs of one
//! repository each.

use crate::app::update::git_repository_of;
use crate::app::*;
use crate::messages::GitTabId;
use ncrs_git_view::Action as Ask;

impl App {
    fn git_ctx(&self) -> ncrs_git_view::Ctx {
        ncrs_git_view::Ctx { lang: self.lang }
    }

    /// The Git tab the area under the title bar shows, if one is.
    pub(in crate::app) fn shown_git_tab(&self) -> Option<GitTabId> {
        match self.shown_tab {
            ShownTab::Git(id) if self.git_tabs.get(id).is_some() => Some(id),
            _ => None,
        }
    }

    pub(in crate::app) fn git_area(&self) -> Container<'_, Message> {
        let ctx = ncrs_git_view::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            date_format: self.columns.date_format,
        };
        let content: Element<'_, Message> = match self
            .shown_git_tab()
            .and_then(|id| self.git_tabs.get(id).map(|state| (id, state)))
        {
            Some((id, state)) => {
                ncrs_git_view::view(state, ctx).map(move |message| Message::GitView(id, message))
            }
            None => Space::new().into(),
        };
        container(content).width(Length::Fill).height(Length::Fill)
    }

    /// "Open Git view": shows the tab for the repository the cursor or the
    /// folder of the active panel is in, and reads it. A repository that has
    /// a tab already is shown there; another one gets a tab of its own.
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
        let (id, task) = self.git_tabs.open(root, name);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Git(id);
        task.map(move |message| Message::GitView(id, message))
    }

    pub(in crate::app) fn update_git_tab(
        &mut self,
        id: GitTabId,
        message: ncrs_git_view::Message,
    ) -> Task<Message> {
        let ctx = self.git_ctx();
        let was_open = self.open_pages();
        let Some(state) = self.git_tabs.get_mut(id) else {
            return Task::none();
        };
        let (task, asked) = ncrs_git_view::update(state, message, &ctx);
        let action_task = match asked {
            Some(action) => self.run_git_action(action),
            None => Task::none(),
        };
        self.git_tabs.drop_closed();
        self.follow_opened_pages(was_open);
        Task::batch([
            task.map(move |inner| Message::GitView(id, inner)),
            action_task,
        ])
    }

    fn run_git_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::ShowWorktree(folder) => self.reveal_in_active_panel(&folder),
            Ask::ViewFile(file) => self.open_in_viewer(file),
        }
    }
}
