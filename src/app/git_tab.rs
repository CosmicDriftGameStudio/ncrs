//! The Git tabs: the branches, worktrees, commit graph and diffs of one
//! repository each.

use crate::app::update::git_repository_of;
use crate::app::*;
use crate::messages::{GitFileView, GitTabId};
use ncrs_git_view::Action as Ask;
use ncrs_ui_kit::git_rows::CommitColumns;
use std::path::{Path, PathBuf};

impl App {
    pub(in crate::app) fn git_ctx(&self, root: Option<&Path>) -> ncrs_git_view::Ctx {
        let settings = &self.git_view_settings;
        ncrs_git_view::Ctx {
            lang: self.lang,
            folded: root
                .map(|folder| settings.folded_in(&folds_key(folder)).to_vec())
                .unwrap_or_default(),
            columns: self.git_columns(),
            panes: self.git_panes(),
            repositories: self.git_known_repositories(),
            pull_default: self.git_pull_default(),
        }
    }

    /// The way the Pull button joins what it fetched, from `[git] pull`.
    fn git_pull_default(&self) -> ncrs_git::PullStrategy {
        use ncrs_config::git::PullDefault;
        match self.git_settings.pull {
            PullDefault::Merge => ncrs_git::PullStrategy::Merge,
            PullDefault::Rebase => ncrs_git::PullStrategy::Rebase,
            PullDefault::FastForwardOnly => ncrs_git::PullStrategy::FastForwardOnly,
        }
    }

    /// The repositories of the open Git tabs, for the repository list of the header.
    fn git_known_repositories(&self) -> Vec<ncrs_git_view::RepoEntry> {
        self.git_tabs
            .iter()
            .filter_map(|(_, state)| {
                Some(ncrs_git_view::RepoEntry {
                    name: state.name.clone(),
                    path: state.root.clone()?,
                })
            })
            .collect()
    }

    /// The panes the user dragged to; the standard widths where none were.
    fn git_panes(&self) -> ncrs_git_view::PaneWidths {
        let settings = &self.git_view_settings;
        ncrs_git_view::PaneWidths::saved(settings.sidebar_width, settings.detail_width)
    }

    /// The commit columns the user dragged to; the standard width where none was.
    fn git_columns(&self) -> CommitColumns {
        let settings = &self.git_view_settings;
        let standard = CommitColumns::STANDARD;
        CommitColumns {
            author: settings.author_width.unwrap_or(standard.author),
            date: settings.date_width.unwrap_or(standard.date),
            sha: settings.sha_width.unwrap_or(standard.sha),
        }
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
            window: self.window_size,
            columns: self.git_columns(),
            panes: self.git_panes(),
            home: &self.home,
            keys: &self.git_keys,
            keymap: &self.keymap,
            pull_default: self.git_pull_default(),
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
        self.open_git_tab_at(root)
    }

    /// The repository and the path in it of the file under the cursor of the
    /// active panel.
    pub(in crate::app) fn git_file_under_cursor(&self) -> Option<(PathBuf, String)> {
        let file = self.file_under_cursor()?;
        let root = ncrs_git::repository_root_of(file.parent()?)?;
        let path = ncrs_git::path_in_repository(&root, &file)?;
        Some((root, path))
    }

    /// Shows the history or the blame of the file under the cursor in the Git
    /// tab of its repository.
    pub(in crate::app) fn open_git_file(&mut self, view: GitFileView) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let Some((root, path)) = self.git_file_under_cursor() else {
            self.set_status_error(self.lang.text(Msg::GitViewNotARepositoryFile));
            return Task::none();
        };
        let opened = self.open_git_tab_at(root);
        let Some(id) = self.shown_git_tab() else {
            return opened;
        };
        let message = match view {
            GitFileView::History => ncrs_git_view::Message::OpenHistory(path),
            GitFileView::Blame => ncrs_git_view::Message::OpenBlame(path, None),
        };
        Task::batch([opened, self.update_git_tab(id, message)])
    }

    /// Shows the Git tab of the repository at `root`, opening one if it has
    /// none, and reads the repository.
    pub(in crate::app) fn open_git_tab_at(&mut self, root: PathBuf) -> Task<Message> {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let ctx = self.git_ctx(Some(&root));
        let was_open = self.open_pages();
        let (id, task) = self.git_tabs.open(root, name, &ctx);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Git(id);
        task.map(move |message| Message::GitView(id, message))
    }

    pub(in crate::app) fn update_git_tab(
        &mut self,
        id: GitTabId,
        message: ncrs_git_view::Message,
    ) -> Task<Message> {
        let ctx = self.git_ctx(None);
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

    /// Hands a login question to the tab whose job asked; no tab left, no answer.
    pub(in crate::app) fn route_git_request(
        &mut self,
        request: ncrs_git::askpass::AskRequest,
    ) -> Task<Message> {
        let asking = self
            .git_tabs
            .iter()
            .find_map(|(id, state)| (state.job_token.0 == request.job).then_some(id));
        match asking {
            Some(id) => self.update_git_tab(id, ncrs_git_view::Message::CredentialAsked(request)),
            None => {
                ncrs_git::askpass::answer(request.id, None);
                Task::none()
            }
        }
    }

    /// The timer of the quiet fetch: every open tab looks at its own remote.
    pub(in crate::app) fn fetch_git_tabs_in_background(&mut self) -> Task<Message> {
        let tasks: Vec<_> = self
            .git_tabs
            .ids()
            .into_iter()
            .map(|id| self.update_git_tab(id, ncrs_git_view::Message::BackgroundFetch))
            .collect();
        Task::batch(tasks)
    }

    fn run_git_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::OpenWorktreeTab(root) => self.open_git_tab_at(root),
            Ask::ViewFile(file) => self.open_in_viewer(file),
            Ask::OpenInEditor(file) => {
                self.launch_external(
                    ncrs_fs::OpenKind::Edit,
                    file,
                    None,
                    ncrs_files::open_plan::CopyOf::Nothing,
                    false,
                );
                Task::none()
            }
            Ask::FoldsChanged { root, folded } => {
                let changed = self
                    .git_view_settings
                    .with_folded(&folds_key(&root), folded);
                self.save_git_view(changed)
            }
            Ask::PanesResized(widths) => {
                let changed = ncrs_config::git_view::GitViewSettings {
                    sidebar_width: Some(widths.sidebar),
                    detail_width: Some(widths.detail),
                    ..self.git_view_settings.clone()
                };
                self.save_git_view(changed)
            }
            Ask::OpenRepository(root) => self.open_git_tab_at(root),
            Ask::WorktreesRemoved(folders) => {
                for folder in &folders {
                    self.worktree_removed(folder);
                }
                Task::none()
            }
            Ask::ColumnsResized(columns) => {
                let changed = ncrs_config::git_view::GitViewSettings {
                    author_width: Some(columns.author),
                    date_width: Some(columns.date),
                    sha_width: Some(columns.sha),
                    ..self.git_view_settings.clone()
                };
                self.save_git_view(changed)
            }
        }
    }

    /// Takes `settings` at once and writes them to the config, one write after
    /// the other with the other settings.
    fn save_git_view(&mut self, settings: ncrs_config::git_view::GitViewSettings) -> Task<Message> {
        if settings == self.git_view_settings {
            return Task::none();
        }
        self.git_view_settings = settings.clone();
        if self.config_path.is_none() {
            return Task::none();
        }
        self.update_settings(ncrs_settings::message::SettingsMessage::Change(
            ncrs_settings::message::SettingChange::GitView(settings),
        ))
    }
}

impl App {
    /// Takes the keys of `[keys.git]`. Keys that do not fit leave the
    /// built-in ones and say why in the status line.
    pub(in crate::app) fn apply_git_keys(&mut self, overrides: &ncrs_config::keymap::TabKeys) {
        match crate::git_keys::resolve(overrides, &self.keymap, cfg!(target_os = "macos")) {
            Ok(keys) => self.git_keys = Arc::new(keys),
            Err(problem) => {
                self.git_keys = Arc::default();
                self.report(ConfigProblem::Invalid(
                    ncrs_config::config::ConfigError::Invalid {
                        path: self.config_path.clone().unwrap_or_default(),
                        line: problem.line,
                        reason: problem.reason,
                    },
                ));
            }
        }
    }
}

/// The worktrees of one repository fold alike: their groups are remembered
/// under the folder of the repository.
fn folds_key(root: &Path) -> String {
    ncrs_git::main_worktree_of(root)
        .unwrap_or_else(|| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}
