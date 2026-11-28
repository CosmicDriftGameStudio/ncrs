use crate::app::*;
use ncrs_space_view::{Action as Ask, ConfirmKind};

/// A measure the Hermes chat asked for is answered from the last one up to this old.
const FRESH_SCAN: std::time::Duration = std::time::Duration::from_secs(10 * 60);

impl App {
    fn space_log_dir(&self) -> Option<PathBuf> {
        self.state_path
            .as_deref()
            .and_then(Path::parent)
            .map(|folder| folder.join("space-logs"))
    }

    /// The repositories the Repos tab knows: where worktrees and `target` folders are looked for.
    fn space_repos(&self) -> Vec<PathBuf> {
        let paths = |store: &ncrs_repos::Store| {
            store
                .repos
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>()
        };
        match &self.repos {
            Some(state) => paths(state.store()),
            None => self
                .repos_file()
                .and_then(|file| ncrs_repos::Store::read(&file).ok())
                .map(|store| paths(&store))
                .unwrap_or_default(),
        }
    }

    pub(in crate::app) fn space_ctx(&self) -> ncrs_space_view::Ctx {
        ncrs_space_view::Ctx {
            lang: self.lang,
            home: self.home.clone(),
            repos: self.space_repos(),
            settings: self.space_settings.clone(),
            processes_known: self.space_processes_known,
            store_build: cfg!(feature = "sandbox"),
            log_dir: self.space_log_dir(),
            trash_can_purge: self.trash.can_purge(),
            window: self.window_size,
            now: std::time::SystemTime::now(),
        }
    }

    pub(in crate::app) fn space_area(&self) -> Container<'_, Message> {
        let content = match &self.space {
            Some(state) => {
                let ctx = ncrs_space_view::ViewCtx {
                    disk_warning: self.disk_warning_percent(),
                    lang: self.lang,
                    palette: &self.palette,
                    window: self.window_size,
                    keys: &self.space_keys,
                    home: &self.home,
                    now: std::time::SystemTime::now(),
                    store_build: cfg!(feature = "sandbox"),
                    processes_known: self.space_processes_known,
                    trash_can_purge: self.trash.can_purge(),
                    warn_from_percent: self.space_settings.warn_from_percent,
                    open_key: Some(self.monitor.space.key.as_str()).filter(|key| !key.is_empty()),
                };
                ncrs_space_view::view(state, ctx).map(Message::Space)
            }
            None => Space::new().into(),
        };
        if !self.hermes_chat.open {
            return container(content).width(Length::Fill).height(Length::Fill);
        }
        let chat = ncrs_hermes_chat::view(
            &self.hermes_chat,
            ncrs_hermes_chat::ViewCtx {
                disk_warning: self.disk_warning_percent(),
                lang: self.lang,
                palette: &self.palette,
                now: chrono::Local::now(),
            },
        )
        .map(Message::HermesChat);
        container(row![
            container(content).width(Length::Fill).height(Length::Fill),
            container(chat)
                .width(ncrs_ui_kit::theme::size::HERMES_DOCK)
                .height(Length::Fill),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
    }

    /// Shows the tab, measuring the drive when it was not open.
    pub(in crate::app) fn open_space(&mut self) -> Task<Message> {
        self.space_hint = None;
        if self.space.is_some() {
            self.shown_tab = ShownTab::Space;
            return Task::none();
        }
        let was_open = self.open_pages();
        let (state, task) = ncrs_space_view::open(&self.space_ctx());
        self.space = Some(state);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Space;
        task.map(Message::Space)
    }

    pub(in crate::app) fn close_space(&mut self) {
        let was_open = self.open_pages();
        if let Some(state) = &self.space {
            state.stop_all();
        }
        self.space = None;
        self.space_run_ask = None;
        self.space_chat_confirm = false;
        self.space_progress = None;
        self.follow_opened_pages(was_open);
    }

    pub(in crate::app) fn update_space(
        &mut self,
        message: ncrs_space_view::Message,
    ) -> Task<Message> {
        if matches!(
            message,
            ncrs_space_view::Message::ConfirmCancel | ncrs_space_view::Message::Escape
        ) {
            self.space_chat_confirm = false;
        }
        let ctx = self.space_ctx();
        let Some(state) = self.space.as_mut() else {
            return Task::none();
        };
        let (task, actions) = ncrs_space_view::update(state, message, &ctx);
        let asked = actions
            .into_iter()
            .map(|action| self.run_space_action(action));
        Task::batch(std::iter::once(task.map(Message::Space)).chain(asked))
    }

    /// Opens the confirm of a run the chat asked for; with nothing to delete no confirm
    /// opens and the run is not the chat's to answer.
    fn confirm_run_for_chat(&mut self, kind: ConfirmKind) -> Task<Message> {
        self.space_chat_confirm = true;
        let task = self.update_space(message_of_run(kind));
        self.space_chat_confirm = self
            .space
            .as_ref()
            .is_some_and(|state| state.confirm().is_some());
        task
    }

    pub(in crate::app) fn run_space_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::ShowInPanel(folder) => {
                self.shown_tab = ShownTab::Panels;
                self.show_folder_in_active_panel(folder, None)
            }
            Ask::AskHermes => self.ask_hermes_about_space(),
            Ask::GrantHome => self.update_grant(GrantMessage::Open),
            Ask::Measured(scan) => {
                self.space_last_scan = Some(scan);
                match self.space_run_ask.take() {
                    Some(kind) => self.confirm_run_for_chat(kind),
                    None => Task::none(),
                }
            }
            Ask::RunDone(log) => {
                self.space_progress = None;
                self.space_chat_confirm = false;
                if std::mem::take(&mut self.space_chat_run) {
                    self.update_hermes_chat(HermesChatMessage::SpaceRunDone(log))
                } else {
                    Task::none()
                }
            }
            Ask::SetJobProgress(tick) => {
                if std::mem::take(&mut self.space_chat_confirm) {
                    self.space_chat_run = true;
                }
                self.space_progress = Some(tick);
                Task::none()
            }
            Ask::ClearJobProgress => {
                self.space_progress = None;
                Task::none()
            }
            Ask::TrashOwn {
                root,
                items,
                empty_after,
            } => self.trash_own_files(root, items, empty_after),
            Ask::UndoTrash => self.undo_own_trash(),
            Ask::Closed => {
                self.close_space();
                Task::none()
            }
        }
    }

    /// "Why is my drive so full?" from the tab: the chat opens and answers it.
    fn ask_hermes_about_space(&mut self) -> Task<Message> {
        let opened = self.update_hermes_chat(HermesChatMessage::Open);
        let asked = self.update_hermes_chat(HermesChatMessage::SpaceAsked);
        Task::batch([opened, asked])
    }

    fn trash_own_files(
        &mut self,
        root: PathBuf,
        items: Vec<ncrs_filing::execute::OwnTrashItem>,
        empty_after: bool,
    ) -> Task<Message> {
        let trash = Arc::clone(&self.trash);
        let access = self
            .hermes
            .filing
            .as_ref()
            .and_then(|module| module.sorting_access(Arc::clone(&self.trash)));
        let changed_reason = self.lang.text(Msg::SpaceDecideChanged).to_owned();
        let run_name = self.lang.text(Msg::SpaceDecideTrashRunName).to_owned();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let run = match access {
                        Some(sorting) => sorting.trash_own(&run_name, &root, &items, empty_after),
                        None => ncrs_filing::execute::trash_own_items(
                            None,
                            &run_name,
                            &root,
                            &items,
                            trash.as_ref(),
                            empty_after,
                        ),
                    };
                    let failed = run.refused.first().map(|first| {
                        let reason = match first {
                            ncrs_filing::execute::SkipReason::Failed(detail) => detail.clone(),
                            ncrs_filing::execute::SkipReason::SourceChanged
                            | ncrs_filing::execute::SkipReason::SourceMissing => {
                                changed_reason.clone()
                            }
                            other => other.text(),
                        };
                        (run.refused.len(), reason)
                    });
                    (run.gone, failed, run.batch)
                })
                .await
                .unwrap_or_else(|failure| (Vec::new(), Some((1, failure.to_string())), None))
            },
            move |(paths, failed, batch)| Message::SpaceTrashed {
                paths,
                failed,
                batch,
                emptied: empty_after,
            },
        )
    }

    pub(in crate::app) fn space_trashed(
        &mut self,
        paths: Vec<PathBuf>,
        failed: Option<(usize, String)>,
        batch: Option<ncrs_index::BatchId>,
        emptied: bool,
    ) -> Task<Message> {
        if let Some((count, reason)) = failed {
            let shown = ncrs_i18n::fill_template(
                self.lang.text(Msg::SpaceDecideTrashFailed),
                &[("{count}", &count.to_string()), ("{reason}", &reason)],
            );
            self.set_status_error(&shown);
        }
        if paths.is_empty() || emptied {
            self.last_own_trash.clear();
            self.last_own_batch = None;
        } else {
            self.last_own_trash.clone_from(&paths);
            self.last_own_batch = batch;
        }
        self.update_space(ncrs_space_view::Message::Decide(
            ncrs_space_view::DecideMessage::Trashed(paths),
        ))
    }

    fn undo_own_trash(&mut self) -> Task<Message> {
        let trash = Arc::clone(&self.trash);
        let wanted = std::mem::take(&mut self.last_own_trash);
        let journaled = self.last_own_batch.take().zip(
            self.hermes
                .filing
                .as_ref()
                .and_then(|module| module.sorting_access(Arc::clone(&self.trash))),
        );
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || match journaled {
                    Some((batch, access)) => access.undo(batch).map(|report| report.undone),
                    None => ncrs_filing::execute::restore_own_items(trash.as_ref(), &wanted),
                })
                .await
                .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            |result| match result {
                Ok(count) => Message::Space(ncrs_space_view::Message::Decide(
                    ncrs_space_view::DecideMessage::Undone(count),
                )),
                Err(reason) => Message::SpaceTrashFailed(reason),
            },
        )
    }

    /// The name of the drive the home folder lies on, for the chat's card.
    pub(in crate::app) fn space_drive_name(&self) -> String {
        self.monitor
            .drive_rows
            .iter()
            .flatten()
            .filter(|drive| self.home.starts_with(&drive.mount))
            .max_by_key(|drive| drive.mount.components().count())
            .map_or_else(|| "/".to_owned(), |drive| drive.name.clone())
    }

    /// The chat asks for the numbers: the last measure answers when it is fresh.
    pub(in crate::app) fn measure_space_for_chat(&mut self) -> Task<Message> {
        let fresh = self.space_last_scan.clone().filter(|scan| {
            std::time::SystemTime::now()
                .duration_since(scan.measured_at)
                .is_ok_and(|age| age < FRESH_SCAN)
        });
        match fresh {
            Some(scan) => self.update_hermes_chat(HermesChatMessage::SpaceMeasured(scan)),
            None => ncrs_space_view::scan_once(&self.space_ctx()).map(Message::SpaceHermesMeasured),
        }
    }

    pub(in crate::app) fn space_measured_for_chat(
        &mut self,
        scan: Option<Arc<ncrs_space::Scan>>,
    ) -> Task<Message> {
        match scan {
            Some(measured) => {
                self.space_last_scan = Some(Arc::clone(&measured));
                self.update_hermes_chat(HermesChatMessage::SpaceMeasured(measured))
            }
            None => {
                let reason = ncrs_os_probe::volume_space(&self.home)
                    .err()
                    .map(|err| err.to_string())
                    .unwrap_or_default();
                self.update_hermes_chat(HermesChatMessage::SpaceMeasureFailed(reason))
            }
        }
    }

    /// A button of the chat's card: the tab opens on the confirm of this run.
    pub(in crate::app) fn run_space_for_chat(
        &mut self,
        kind: ncrs_hermes_chat::SpaceRunKind,
    ) -> Task<Message> {
        let asked = match kind {
            ncrs_hermes_chat::SpaceRunKind::DeleteSafe => ConfirmKind::Safe,
            ncrs_hermes_chat::SpaceRunKind::RemoveWorktrees => ConfirmKind::Worktrees,
        };
        self.space_run_ask = Some(asked);
        let was_open = self.space.is_some();
        let opened = self.open_space();
        let ready = self
            .space
            .as_ref()
            .is_some_and(|state| state.scan().is_some() && !state.is_measuring());
        let confirm = if was_open && ready {
            self.space_run_ask = None;
            self.confirm_run_for_chat(asked)
        } else {
            Task::none()
        };
        Task::batch([opened, confirm])
    }

    /// The Platz tab with the decide pane of this folder open, once the tab has its scan.
    pub(in crate::app) fn show_space_decide(
        &mut self,
        target: ncrs_hermes_chat::SpaceDecideTarget,
    ) -> Task<Message> {
        let opened = self.open_space();
        let folder = match target {
            ncrs_hermes_chat::SpaceDecideTarget::Downloads => {
                ncrs_space_view::DecideTarget::Downloads
            }
            ncrs_hermes_chat::SpaceDecideTarget::SessionLogs => {
                ncrs_space_view::DecideTarget::SessionLogs
            }
        };
        Task::batch([
            opened,
            self.update_space(ncrs_space_view::Message::OpenDecide(folder)),
        ])
    }

    /// The key of a card button of the chat, or the key back to the input.
    pub(in crate::app) fn hermes_chat_key(
        &mut self,
        key: ncrs_hermes_chat::ChatKey,
    ) -> Task<Message> {
        if key == ncrs_hermes_chat::ChatKey::FocusInput {
            return ncrs_hermes_chat::focus_input();
        }
        match ncrs_hermes_chat::card_key_message(&self.hermes_chat, key) {
            Some(message) => self.update_hermes_chat(message),
            None => Task::none(),
        }
    }

    pub(in crate::app) fn apply_space_keys(&mut self, overrides: &ncrs_config::keymap::TabKeys) {
        match crate::space_keys::resolve(overrides, &self.keymap, cfg!(target_os = "macos")) {
            Ok(keys) => self.space_keys = Arc::new(keys),
            Err(problem) => {
                self.space_keys = Arc::default();
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

    /// What the System page needs of the Platz settings: the threshold, the home folder
    /// and the key that opens the tab.
    pub(in crate::app) fn refresh_space_facts(&mut self) {
        let key = self
            .keymap
            .shortcut_label_of(KeyAction::Space)
            .unwrap_or_default();
        self.monitor.set_space_facts(ncrs_monitor::SpaceFacts {
            warn_from_percent: self.space_settings.warn_from_percent,
            home: Some(self.home.clone()),
            key,
        });
    }

    /// The hourly look at the drive: measured off the UI thread.
    pub(in crate::app) fn measure_space_hint(&self) -> Task<Message> {
        let home = self.home.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || ncrs_monitor::measure_home(&home))
                    .await
                    .ok()
                    .flatten()
            },
            Message::SpaceHintMeasured,
        )
    }

    pub(in crate::app) fn space_hint_measured(
        &mut self,
        volume: Option<ncrs_os_probe::VolumeSpace>,
    ) {
        self.home_volume = volume;
        let drive_name = self.space_drive_name();
        let measured_hint = volume.and_then(|measured| {
            ncrs_monitor::home_disk_hint(measured, &self.space_settings, &drive_name)
        });
        let Some(hint) = measured_hint else {
            self.space_hint = None;
            return;
        };
        if self.space_hint.is_some() {
            self.space_hint = Some(hint);
            return;
        }
        let Some(stamp_file) = self
            .state_path
            .as_deref()
            .map(ncrs_monitor::stamp_file_beside)
        else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        if ncrs_monitor::load_stamp(&stamp_file).is_due(now) && self.space.is_none() {
            self.space_hint = Some(hint);
            ncrs_monitor::save_stamp(&stamp_file, now);
        }
    }

    pub(in crate::app) fn space_free_text(&self) -> Option<String> {
        let volume = self.home_volume?;
        Some(fill_template(
            self.lang.text(Msg::StatusSpaceFree),
            &[
                (
                    "{free}",
                    &ncrs_ui_kit::format::size_shown(volume.available, self.lang),
                ),
                (
                    "{total}",
                    &ncrs_ui_kit::format::size_shown(volume.total, self.lang),
                ),
            ],
        ))
    }

    /// The used share of the home drive while it is past the warn level, for the bar of every page.
    pub(in crate::app) fn disk_warning_percent(&self) -> Option<u8> {
        self.space_hint.map(|hint| hint.used_percent)
    }

    pub(in crate::app) fn space_hint_badge(&self) -> Option<statusbar::SpaceHintBadge<Message>> {
        let hint = self.space_hint?;
        let percent = hint.used_percent.to_string();
        let key = self.monitor.space.key.as_str();
        let text = if key.is_empty() {
            fill_template(
                self.lang.text(Msg::StatusDiskHint),
                &[("{percent}", &percent)],
            )
        } else {
            fill_template(
                self.lang.text(Msg::StatusDiskHintKey),
                &[("{percent}", &percent), ("{key}", key)],
            )
        };
        Some(statusbar::SpaceHintBadge {
            text,
            tooltip: self.lang.text(Msg::StatusDiskHintTooltip).to_owned(),
            on_press: Message::HermesChat(HermesChatMessage::SpaceAsked),
        })
    }
}

fn message_of_run(kind: ConfirmKind) -> ncrs_space_view::Message {
    match kind {
        ConfirmKind::Worktrees => ncrs_space_view::Message::RemoveWorktrees,
        ConfirmKind::Safe | ConfirmKind::One => ncrs_space_view::Message::DeleteSafe,
    }
}

/// Whether the running processes can be listed here; looked up once per process.
pub(in crate::app) fn processes_known() -> bool {
    static KNOWN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *KNOWN.get_or_init(|| ncrs_os_probe::running_processes().is_some())
}
