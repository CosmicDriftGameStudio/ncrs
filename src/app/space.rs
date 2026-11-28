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
        self.space_progress = None;
        self.follow_opened_pages(was_open);
    }

    pub(in crate::app) fn update_space(
        &mut self,
        message: ncrs_space_view::Message,
    ) -> Task<Message> {
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
                match self.space_run_ask {
                    Some(kind) => self.update_space(message_of_run(kind)),
                    None => Task::none(),
                }
            }
            Ask::RunDone(log) => {
                self.space_progress = None;
                if self.space_run_ask.take().is_some() {
                    self.update_hermes_chat(HermesChatMessage::SpaceRunDone(log))
                } else {
                    Task::none()
                }
            }
            Ask::SetJobProgress(tick) => {
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
        _root: PathBuf,
        items: Vec<ncrs_filing::execute::OwnTrashItem>,
        empty_after: bool,
    ) -> Task<Message> {
        let trash = Arc::clone(&self.trash);
        let changed_reason = self.lang.text(Msg::SpaceDecideChanged).to_owned();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let (unchanged, changed): (Vec<_>, Vec<_>) = items
                        .into_iter()
                        .partition(|item| !changed_since_measure(item));
                    let (gone, trash_failed) = trash_items(trash.as_ref(), &unchanged, empty_after);
                    let failed = match (trash_failed, changed.len()) {
                        (failed, 0) => failed,
                        (None, count) => Some((count, changed_reason)),
                        (Some((count, reason)), changed_count) => {
                            Some((count + changed_count, reason))
                        }
                    };
                    (gone, failed)
                })
                .await
                .unwrap_or_else(|failure| (Vec::new(), Some((1, failure.to_string()))))
            },
            |(paths, failed)| Message::SpaceTrashed { paths, failed },
        )
    }

    pub(in crate::app) fn space_trashed(
        &mut self,
        paths: Vec<PathBuf>,
        failed: Option<(usize, String)>,
    ) -> Task<Message> {
        if let Some((count, reason)) = failed {
            let shown = ncrs_i18n::fill_template(
                self.lang.text(Msg::SpaceDecideTrashFailed),
                &[("{count}", &count.to_string()), ("{reason}", &reason)],
            );
            self.set_status_error(&shown);
        }
        self.last_own_trash.clone_from(&paths);
        self.update_space(ncrs_space_view::Message::Decide(
            ncrs_space_view::DecideMessage::Trashed(paths),
        ))
    }

    fn undo_own_trash(&mut self) -> Task<Message> {
        let trash = Arc::clone(&self.trash);
        let wanted = std::mem::take(&mut self.last_own_trash);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || restore_items(trash.as_ref(), &wanted))
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
            self.update_space(message_of_run(asked))
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
        let downloads = self.home.join("Downloads");
        let row = self.space_last_scan.as_ref().and_then(|scan| {
            scan.entries
                .iter()
                .find(|entry| {
                    entry.group == ncrs_space::Group::Decide
                        && match target {
                            ncrs_hermes_chat::SpaceDecideTarget::Downloads => {
                                entry.path == downloads
                            }
                            ncrs_hermes_chat::SpaceDecideTarget::SessionLogs => matches!(
                                entry.kind,
                                ncrs_space::EntryKind::Decide(
                                    ncrs_space::decide::DecideKind::SessionLogs
                                )
                            ),
                        }
                })
                .map(|entry| entry.path.clone())
        });
        let ready = self
            .space
            .as_ref()
            .is_some_and(|state| state.scan().is_some() && !state.is_measuring());
        match row {
            Some(path) if ready => Task::batch([
                opened,
                self.update_space(ncrs_space_view::Message::RowOpened(path)),
            ]),
            _ => opened,
        }
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
        let measured_hint = volume.and_then(|measured| {
            ncrs_monitor::disk_hint(measured, self.space_settings.warn_from_percent)
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

/// What went to the trash, and the first reason (with the count) of those that did not.
/// A file whose modification time differs from the one measured is not the file the user
/// saw; same rule as the checks of `ncrs-space`.
pub(in crate::app) fn changed_since_measure(item: &ncrs_filing::execute::OwnTrashItem) -> bool {
    item.modified.is_some()
        && std::fs::metadata(&item.path)
            .and_then(|meta| meta.modified())
            .ok()
            != item.modified
}

pub(in crate::app) fn trash_items(
    trash: &dyn fs::Trash,
    items: &[ncrs_filing::execute::OwnTrashItem],
    empty_after: bool,
) -> (Vec<PathBuf>, Option<(usize, String)>) {
    let mut gone = Vec::new();
    let mut failures = Vec::new();
    for item in items {
        match trash.trash(&item.path) {
            Ok(()) => gone.push(item.path.clone()),
            Err(reason) => failures.push(reason),
        }
    }
    if empty_after && trash.can_purge() {
        if let Ok(listed) = trash.list() {
            let entries: Vec<_> = listed
                .into_iter()
                .filter(|entry| gone.contains(&entry.original))
                .collect();
            drop(trash.purge(&entries));
        }
    }
    let failed = failures
        .first()
        .map(|first| (failures.len(), first.clone()));
    (gone, failed)
}

pub(in crate::app) fn restore_items(
    trash: &dyn fs::Trash,
    wanted: &[PathBuf],
) -> Result<usize, String> {
    let listed = trash.list()?;
    Ok(listed
        .iter()
        .filter(|entry| wanted.contains(&entry.original))
        .filter(|entry| trash.restore(entry).is_ok())
        .count())
}
