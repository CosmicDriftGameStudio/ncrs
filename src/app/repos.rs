use crate::app::*;
use ncrs_repos::Action as Ask;

impl App {
    pub(in crate::app) fn repos_file(&self) -> Option<PathBuf> {
        self.config_path
            .as_deref()
            .and_then(Path::parent)
            .map(|folder| folder.join("repos.toml"))
    }

    fn repos_ctx(&self) -> ncrs_repos::Ctx {
        ncrs_repos::Ctx {
            lang: self.lang,
            settings: self.repos_settings.clone(),
            home: Some(self.home.clone()),
            file: self.repos_file(),
            now: std::time::SystemTime::now(),
            actions_available: !self.repos_read_only && ncrs_git::write_actions_available(),
            sandbox_roots: self
                .sandbox
                .grants_dir
                .is_some()
                .then(|| self.repos_settings.sources.clone()),
            open_roots: self
                .git_tabs
                .iter()
                .filter_map(|(_, state)| state.root.clone())
                .collect(),
        }
    }

    pub(in crate::app) fn repos_area(&self) -> Container<'_, Message> {
        let content = match &self.repos {
            Some(state) => {
                let ctx = ncrs_repos::ViewCtx {
                    lang: self.lang,
                    palette: &self.palette,
                    window: self.window_size,
                    keys: &self.repos_keys,
                    keymap: &self.keymap,
                    home: Some(&self.home),
                    now: std::time::SystemTime::now(),
                    actions_available: !self.repos_read_only && ncrs_git::write_actions_available(),
                    settings: &self.repos_settings,
                };
                ncrs_repos::view(state, ctx).map(Message::Repos)
            }
            None => Space::new().into(),
        };
        container(content).width(Length::Fill).height(Length::Fill)
    }

    /// The command list's "Repositories": shows the tab, reading the list
    /// when it was not open.
    pub(in crate::app) fn open_repos(&mut self) -> Task<Message> {
        if self.repos.is_some() {
            self.shown_tab = ShownTab::Repos;
            return Task::none();
        }
        let was_open = self.open_pages();
        let (state, task) = ncrs_repos::open(&self.repos_ctx());
        self.repos = Some(state);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Repos;
        task.map(Message::Repos)
    }

    pub(in crate::app) fn close_repos(&mut self) {
        let was_open = self.open_pages();
        if let Some(tab) = &self.repos {
            ncrs_git::askpass::cancel_job(tab.job);
        }
        self.repos = None;
        self.follow_opened_pages(was_open);
    }

    /// The tab came to the front or the window did: an old state is read again.
    pub(in crate::app) fn repos_came_into_view(&mut self) -> Task<Message> {
        if self.repos_shown() {
            self.update_repos(ncrs_repos::Message::Shown)
        } else {
            Task::none()
        }
    }

    fn repos_in_view(&self) -> bool {
        self.repos_shown() && !self.input.lock().window_unfocused
    }

    pub(in crate::app) fn update_repos(&mut self, message: ncrs_repos::Message) -> Task<Message> {
        if matches!(message, ncrs_repos::Message::Tick) && !self.repos_in_view() {
            return Task::none();
        }
        let ctx = self.repos_ctx();
        let Some(state) = self.repos.as_mut() else {
            return Task::none();
        };
        let (task, actions) = ncrs_repos::update(state, message, &ctx);
        let asked = actions
            .into_iter()
            .map(|action| self.run_repos_action(action));
        Task::batch(std::iter::once(task.map(Message::Repos)).chain(asked))
    }

    fn run_repos_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::OpenGitTab(root) => self.open_git_tab_at(root),
            Ask::OpenGitTabs(roots) => {
                Task::batch(roots.into_iter().map(|root| self.open_git_tab_at(root)))
            }
            Ask::PickFolder => (self.pick_folder)(&self.home).then(|picked| {
                picked.map_or_else(Task::none, |folder| {
                    Task::done(Message::Repos(ncrs_repos::Message::RepoPicked(folder)))
                })
            }),
            Ask::PickCloneTarget => (self.pick_folder)(&self.home).then(|picked| {
                picked.map_or_else(Task::none, |folder| {
                    Task::done(Message::Repos(ncrs_repos::Message::CloneTargetPicked(
                        folder,
                    )))
                })
            }),
            Ask::ShowInFiles(folder) => {
                self.shown_tab = ShownTab::Panels;
                self.show_folder_in_active_panel(folder, None)
            }
            Ask::ShowWorktreesChanged(shown) => {
                let changed = ncrs_config::repos::ReposSettings {
                    show_worktrees: shown,
                    ..self.repos_settings.clone()
                };
                self.save_repos_settings(changed)
            }
            Ask::ColumnsResized(columns) => {
                let changed = ncrs_config::repos::ReposSettings {
                    columns,
                    ..self.repos_settings.clone()
                };
                self.save_repos_settings(changed)
            }
            Ask::OpenTerminal(folder) => self.open_terminal_in(&folder),
            Ask::CopyPath(path) => self.copy_path_of(&path),
            Ask::WorktreeRemoved(folder) => {
                self.worktree_removed(&folder);
                Task::none()
            }
            Ask::RepoChanged(root) => {
                let Some(id) = self.git_tabs.id_of(&root) else {
                    return Task::none();
                };
                let name = root
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                let ctx = self.git_ctx(Some(&root));
                let (_, task) = self.git_tabs.open(root, name, &ctx);
                task.map(move |message| Message::GitView(id, message))
            }
        }
    }

    /// Takes `settings` at once and writes them to the config, one write after
    /// the other with the other settings.
    fn save_repos_settings(
        &mut self,
        settings: ncrs_config::repos::ReposSettings,
    ) -> Task<Message> {
        if settings == self.repos_settings {
            return Task::none();
        }
        self.repos_settings = settings.clone();
        if self.config_path.is_none() {
            return Task::none();
        }
        self.update_settings(ncrs_settings::message::SettingsMessage::Change(
            ncrs_settings::message::SettingChange::Repos(settings),
        ))
    }

    /// A worktree folder is gone: the Git tab that showed it goes too.
    pub(in crate::app) fn worktree_removed(&mut self, folder: &std::path::Path) {
        let was_open = self.open_pages();
        if let Some(state) = self
            .git_tabs
            .id_of(folder)
            .and_then(|id| self.git_tabs.get_mut(id))
        {
            state.close();
        }
        self.git_tabs.drop_closed();
        self.follow_opened_pages(was_open);
    }

    /// The search of the Settings → Git page: reads the folders again.
    pub(in crate::app) fn scan_repos(&mut self) -> Task<Message> {
        let opened = self.open_repos();
        let scanned = self.update_repos(ncrs_repos::Message::Rescan);
        Task::batch([opened, scanned])
    }

    /// A panel that now shows a repository teaches the list its root.
    pub(in crate::app) fn learn_repo_of_panel(&mut self, root: Option<PathBuf>) -> Task<Message> {
        match root {
            Some(known) if self.repos_settings.learn_from_panel => {
                self.update_repos_when_open_or_stored(known)
            }
            _ => Task::none(),
        }
    }

    fn update_repos_when_open_or_stored(&mut self, root: PathBuf) -> Task<Message> {
        if self.repos.is_some() {
            return self.update_repos(ncrs_repos::Message::Learn(root));
        }
        let Some(file) = self.repos_file() else {
            return Task::none();
        };
        // A list that cannot be read stays as it is; the tab says so when it opens.
        if let Ok(mut store) = ncrs_repos::Store::read(&file) {
            if store.add(root, ncrs_repos::Origin::Learned, None) {
                let _ = store.write(&file);
            }
        }
        Task::none()
    }

    /// Takes the keys of `[keys.repos]`. Keys that do not fit leave the
    /// built-in ones and say why in the status line.
    pub(in crate::app) fn apply_repos_keys(&mut self, overrides: &ncrs_config::keymap::TabKeys) {
        match crate::repos_keys::resolve(overrides, &self.keymap, cfg!(target_os = "macos")) {
            Ok(keys) => self.repos_keys = Arc::new(keys),
            Err(problem) => {
                self.repos_keys = Arc::default();
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

    /// What the Settings → Git page says about the last search.
    pub(in crate::app) fn repos_scan_facts(&self) -> ncrs_settings::facts::ReposScanFacts {
        let latest = match &self.repos {
            Some(state) => state.store().last_scan,
            None => self
                .repos_file()
                .and_then(|file| ncrs_repos::Store::read(&file).ok())
                .and_then(|store| store.last_scan),
        };
        ncrs_settings::facts::ReposScanFacts {
            now: std::time::SystemTime::now(),
            last: latest.map(|scan| ncrs_settings::facts::LastRepoScan {
                at: scan.time(),
                repos: scan.repos,
                millis: scan.millis,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{event, window, Event};

    fn app_with_repos() -> App {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenRepos));
        app
    }

    /// Forgets the last refresh, handles `message`, says whether it refreshed.
    fn refreshes(app: &mut App, message: Message) -> bool {
        app.repos.as_mut().unwrap().refreshed_at = None;
        drop(app.update(message));
        app.repos.as_ref().unwrap().refreshed_at.is_some()
    }

    fn tick() -> Message {
        Message::Repos(ncrs_repos::Message::Tick)
    }

    fn window_event(app: &App, sent: window::Event) {
        app.input
            .lock()
            .handle(&Event::Window(sent), event::Status::Ignored);
    }

    #[tokio::test]
    async fn a_hidden_tab_is_not_read_again_until_it_is_shown() {
        let mut app = app_with_repos();
        assert!(refreshes(&mut app, tick()));
        drop(app.update(Message::ShowTab(ShownTab::Panels)));
        assert!(!refreshes(&mut app, tick()));
        assert!(refreshes(&mut app, Message::ShowTab(ShownTab::Repos)));
    }

    #[tokio::test]
    async fn a_window_without_focus_is_not_read_again_until_it_is_back() {
        let mut app = app_with_repos();
        window_event(&app, window::Event::Unfocused);
        assert!(!refreshes(&mut app, tick()));
        window_event(&app, window::Event::Focused);
        assert!(refreshes(&mut app, Message::WindowFocused));
        assert!(refreshes(&mut app, tick()));
    }
}
