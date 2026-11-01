use crate::app::*;

/// Reads a folder on a server and reports it as a panel's listing.
pub(in crate::app) fn remote_listing(
    pool: Arc<Pool>,
    side: PanelSide,
    request_id: u64,
    remote: RemotePath,
    sort: fs::SortKey,
    select: Option<String>,
) -> Task<Message> {
    Task::perform(
        async move { fs::remote::list::list(&pool, remote, sort).await },
        move |listing| {
            Message::Panel(PanelMessage::DirectoryLoaded {
                side,
                request_id,
                path: listing.path,
                entries: listing.entries,
                error: listing.error,
                sorted_by: listing.sorted_by,
                select: select.clone(),
                archive: listing.archive,
            })
        },
    )
}

impl App {
    /// Runs `step` on the connect feature. The state is taken out for it, since
    /// the context borrows the rest of the app; the overlay fact is read before,
    /// while the connect dialog still counts.
    pub(in crate::app) fn with_connect_feature<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_remote::Remote, &ncrs_remote::Ctx<'_>) -> R,
    ) -> R {
        let blocking_overlay_open = self.blocking_overlay_is_open();
        let mut remote_ui = std::mem::take(&mut self.remote_ui);
        let outcome = step(
            &mut remote_ui,
            &ncrs_remote::Ctx {
                lang: self.lang,
                pool: &self.remote,
                passwords: &self.passwords,
                blocking_overlay_open,
                active_side: self.panels.active,
                active_panel_server: self.active_panel().remote(),
                left_request_id: self.panel(PanelSide::Left).request_id,
                right_request_id: self.panel(PanelSide::Right).request_id,
            },
        );
        self.remote_ui = remote_ui;
        outcome
    }

    pub(in crate::app) fn run_remote_outcome(
        &mut self,
        (task, actions): ncrs_remote::Outcome,
    ) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_remote_action(action));
        }
        Task::batch(tasks)
    }

    fn run_remote_action(&mut self, action: ncrs_remote::Action) -> Task<Message> {
        use ncrs_remote::Action as Ask;
        match action {
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::ClearJobError => {
                self.job_error = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Ask::OpenServer(url) => {
                self.job_error = None;
                self.notice = None;
                let side = self.panels.active;
                self.load(side, url, None)
            }
            Ask::LoadFolder { side, path, select } => self.load(side, path, select),
            Ask::SaveServer { server, typed_path } => {
                self.save_server(&server, typed_path.as_deref())
            }
            Ask::StopWaiting { side, request_id } => {
                let panel = self.panel_mut(side);
                if panel.request_id == request_id {
                    panel.loading = false;
                }
                Task::none()
            }
            Ask::LeaveServer(server) => {
                let mut tasks = Vec::new();
                for side in [PanelSide::Left, PanelSide::Right] {
                    if self.panel(side).remote().as_ref() == Some(&server) {
                        let home = self.home.clone();
                        tasks.push(self.load(side, home, None));
                    }
                }
                Task::batch(tasks)
            }
            Ask::ListFolder {
                side,
                request_id,
                remote,
                select,
            } => {
                let sort = self.panel(side).sort;
                remote_listing(
                    Arc::clone(&self.remote),
                    side,
                    request_id,
                    remote,
                    sort,
                    select,
                )
            }
            Ask::ReportDisconnected {
                side,
                request_id,
                remote,
            } => {
                let server = remote.server().clone();
                Task::done(Message::Panel(PanelMessage::DirectoryLoaded {
                    side,
                    request_id,
                    path: remote.to_path_buf(),
                    entries: remote
                        .parent()
                        .map(|above| fs::FileEntry::parent(&above.to_path_buf()))
                        .into_iter()
                        .collect(),
                    error: Some(fs::ReadError::RemoteDisconnected { server }),
                    sorted_by: self.panel(side).sort,
                    select: None,
                    archive: None,
                }))
            }
        }
    }

    /// Puts the server into `[[servers]]`, or updates its start folder when it
    /// is there. Never the password.
    fn save_server(&mut self, server: &ServerId, typed_path: Option<&str>) -> Task<Message> {
        if self.config_path.is_none() {
            let text = self.lang.text(Msg::FilingSetupNoConfigPath).to_owned();
            self.set_status_error(&text);
            return Task::none();
        }
        let path = typed_path
            .filter(|text| text.starts_with('/'))
            .map(str::to_owned);
        let mut servers = self.saved_servers.clone();
        match servers
            .iter_mut()
            .find(|entry| entry.server_id() == *server)
        {
            Some(existing) => {
                if path.is_some() {
                    existing.path = path;
                }
            }
            None => servers.push(ServerEntry {
                name: None,
                host: server.host.clone(),
                port: server.port,
                user: server.user.clone(),
                path,
            }),
        }
        self.update_settings(SettingsMessage::Change(SettingChange::Servers(servers)))
    }
}
