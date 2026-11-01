use crate::app::*;
use iced::widget::column;

/// How often the bar of a model download reads the byte counter.
pub(in crate::app) const DOWNLOAD_BAR_EVERY: std::time::Duration =
    std::time::Duration::from_millis(250);

impl App {
    pub(in crate::app) fn settings_view<'a>(
        &'a self,
        state: &'a settings_ui::SettingsState,
    ) -> settings_ui::SettingsView<'a> {
        settings_ui::SettingsView {
            state,
            preset: self.theme_preset,
            vscode_theme: self.vscode_theme.as_deref(),
            filing: &self.hermes.filing_config,
            servers: &self.saved_servers,
            home: &self.home,
            keys_changed: KeyAction::ALL
                .iter()
                .filter(|&&action| !self.keymap.has_built_in_keys(action))
                .count(),
            protection: Some(self.protection_panel(&self.overview_facts(state.ocr_models_present))),
            lang: self.lang,
            palette: &self.palette,
        }
    }

    /// The protection of the index as the settings page shows it.
    fn protection_panel(&self, facts: &OverviewFacts) -> settings_ui::ProtectionPanel {
        let text = hermes_overview_ui::protection_text(facts, self.lang);
        settings_ui::ProtectionPanel {
            state: text.state.unwrap_or_default().to_owned(),
            good: text.good,
            sentence: text.sentence.to_owned(),
            failure: text.failure,
            buttons: hermes_overview_ui::protection_actions(facts)
                .into_iter()
                .map(|action| settings_ui::ProtectionButton {
                    label: action.label,
                    enabled: action.enabled,
                    primary: action.primary,
                })
                .collect(),
        }
    }

    pub(in crate::app) fn settings_ctx(&self) -> ncrs_settings::Ctx<'_> {
        ncrs_settings::Ctx {
            lang: self.lang,
            keymap: &self.keymap,
            config_path: self.config_path.as_deref(),
            home: &self.home,
            sandbox_active: self.sandbox.active,
            blocking_overlay_open: self.blocking_overlay_is_open(),
            overlay_apart_from_overview_open: self.overlay_apart_from_overview_is_open(),
            overview_open: self.hermes.overview.is_some(),
            ocr_models_dir: self
                .hermes
                .index_path
                .as_deref()
                .and_then(crate::filing::ocr_models_dir),
            ai_models_dir: self
                .hermes
                .index_path
                .as_deref()
                .and_then(crate::filing::ai_models_dir),
            file_list_tags: &self.file_list_tags,
            ai_is_on: self.hermes.filing_config.ai != crate::config::AiSetting::Off,
            services: ncrs_settings::Services {
                fetch_extension: self.fetch_extension,
                install_theme: self.install_theme,
                download_ocr_models: self.download_ocr_models,
                download_ai_model: self.download_ai_model,
            },
        }
    }

    /// Runs `step` on the settings. The state is taken out for it, since the
    /// context borrows the rest of the app; the overlay facts are read before,
    /// while the settings' own dialogs still count.
    pub(in crate::app) fn with_settings<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_settings::Settings, &ncrs_settings::Ctx<'_>) -> R,
    ) -> R {
        let blocking_overlay_open = self.blocking_overlay_is_open();
        let overlay_apart_from_overview_open = self.overlay_apart_from_overview_is_open();
        let mut settings = std::mem::take(&mut self.settings);
        let outcome = step(
            &mut settings,
            &ncrs_settings::Ctx {
                blocking_overlay_open,
                overlay_apart_from_overview_open,
                ..self.settings_ctx()
            },
        );
        self.settings = settings;
        outcome
    }

    fn run_settings(&mut self, message: ncrs_settings::Message) -> Task<Message> {
        let (task, actions) =
            self.with_settings(|settings, ctx| ncrs_settings::update(settings, message, ctx));
        self.run_settings_actions(task, actions)
    }

    pub(in crate::app) fn run_settings_actions(
        &mut self,
        task: Task<ncrs_settings::Message>,
        actions: Vec<ncrs_settings::Action>,
    ) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from_settings)];
        for action in actions {
            tasks.push(self.run_settings_action(action));
        }
        Task::batch(tasks)
    }

    fn run_settings_action(&mut self, action: ncrs_settings::Action) -> Task<Message> {
        use ncrs_settings::Action as Ask;
        match action {
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::SetDialogFocus(focus) => {
                self.dialog_focus = focus;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::LeaveAblagePage => {
                let left_ablage = self.close_ablage_tab();
                self.panels.context_menu = None;
                self.panels.set_path_field(None);
                self.notice = None;
                left_ablage
            }
            Ask::PickProgram { kind, apps_only } => (self.pick_file)(FilePick::Program {
                apps_only,
            })
            .map(move |path| Message::Settings(SettingsMessage::ProgramPicked { kind, path })),
            Ask::PickThemeFile => (self.pick_file)(FilePick::ThemeFile)
                .map(|path| Message::Settings(SettingsMessage::ThemeFilePicked(path))),
            Ask::ShowTagsPage => self.show_tags_page(),
            Ask::OpenServerAdd => self.update(Message::Remote(RemoteMessage::OpenAdd)),
            Ask::OpenServer(url) => self.update(Message::Remote(RemoteMessage::OpenServer(url))),
            Ask::OpenFilingStatus => self.update(Message::FilingStatus(FilingStatusMessage::Open)),
            Ask::Protection(position) => {
                let facts = self.overview_facts(
                    self.settings
                        .page
                        .as_ref()
                        .is_some_and(|state| state.ocr_models_present),
                );
                match hermes_overview_ui::protection_actions(&facts)
                    .into_iter()
                    .nth(position)
                {
                    Some(button) => self.update(Message::HermesOverview(button.message)),
                    None => Task::none(),
                }
            }
            Ask::Apply { change, config } => self.apply_setting(change, *config),
            Ask::ApplyLook(config) => {
                self.apply_look(*config);
                Task::none()
            }
            Ask::KeymapChanged(keymap) => {
                self.keymap = keymap;
                self.refresh_function_keys();
                Task::none()
            }
            Ask::OpenOverview => self.open_hermes_overview(),
            Ask::OcrModelsInstalled => {
                if let Some(overview) = self.hermes.overview.as_mut() {
                    overview.set_ocr_models_present(true);
                }
                Task::none()
            }
            Ask::RefreshAiModel => {
                if let Some(module) = self.hermes.filing.as_ref() {
                    module.refresh_ai_model();
                }
                Task::none()
            }
        }
    }

    pub(in crate::app) fn update_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::Settings(message))
    }

    pub(in crate::app) fn update_key_bindings(
        &mut self,
        message: KeyBindingsMessage,
    ) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::KeyBindings(message))
    }

    pub(in crate::app) fn update_theme_fetch(
        &mut self,
        message: ThemeFetchMessage,
    ) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::ThemeFetch(message))
    }

    pub(in crate::app) fn update_ocr_models(&mut self, message: OcrModelsMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::OcrModels(message))
    }

    pub(in crate::app) fn update_ai_model(&mut self, message: AiModelMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::AiModel(message))
    }

    pub(in crate::app) fn press_ai_model_button(&mut self) -> Task<Message> {
        let (task, actions) = self.with_settings(ncrs_settings::press_ai_model_button);
        self.run_settings_actions(task, actions)
    }

    pub(in crate::app) fn ocr_models_present(&self) -> bool {
        ncrs_settings::ocr_models_present(self.settings_ctx().ocr_models_dir.as_deref())
    }

    pub(in crate::app) fn close_settings(&mut self) -> Task<Message> {
        ncrs_settings::close_settings(&mut self.settings).map(Message::from_settings)
    }

    /// Writes the values typed on the page and closes it.
    pub(in crate::app) fn leave_settings_page(&mut self) -> Task<Message> {
        self.update_settings(SettingsMessage::Close)
    }

    pub(in crate::app) fn settings_area<'a>(
        &'a self,
        state: &'a settings_ui::SettingsState,
    ) -> Container<'a, Message> {
        let page = settings_ui::view(&self.settings_view(state)).map(Message::Settings);
        let bar = settings_ui::status_bar(&settings_ui::StatusBar {
            changed: state.changed,
            failure: self.settings.failure.as_deref(),
            notice: self.notice.as_deref(),
            lang: self.lang,
            palette: &self.palette,
        })
        .map(Message::Settings);
        container(column![page, bar])
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// Takes over what the file now says about `change`.
    fn apply_setting(&mut self, change: SettingChange, config: Config) -> Task<Message> {
        match change {
            SettingChange::Preset(_)
            | SettingChange::VscodeTheme(_)
            | SettingChange::Color { .. } => {
                self.apply_look(config);
                Task::none()
            }
            SettingChange::Program { .. } => {
                self.open_programs = config.open;
                Task::none()
            }
            SettingChange::Language(lang) => {
                self.set_language(lang);
                Task::none()
            }
            SettingChange::Load(load) => {
                self.hermes.filing_config.load = load;
                self.hermes.set_pipeline_load(load, self.wall_clock.now());
                Task::none()
            }
            SettingChange::Servers(_) => self.take_over_servers(config.servers),
            SettingChange::FileListTags(_) => {
                self.file_list_tags = config.file_list_tags;
                Task::none()
            }
            SettingChange::AiEnabled(_) | SettingChange::AiProvider(_) => {
                let (applies, task) = self.apply_ai_change(config.filing);
                let outcome = if matches!(applies, SavedFilingApplies::Now) {
                    InstallOutcome::TurnedOn
                } else {
                    InstallOutcome::TurnedOnAtNextStart
                };
                if let Some(card) = self.settings.ai_model.as_mut() {
                    card.set_outcome(outcome);
                }
                if matches!(applies, SavedFilingApplies::AtNextStart) {
                    let notice = self.lang.text(Msg::SettingsSavedNextStart).to_owned();
                    Task::batch([task, self.show_notice(notice)])
                } else {
                    task
                }
            }
            SettingChange::OwnDetails { .. } => {
                let (applies, task) = self.apply_ai_change(config.filing);
                if matches!(applies, SavedFilingApplies::AtNextStart) {
                    let notice = self.lang.text(Msg::SettingsSavedNextStart).to_owned();
                    Task::batch([task, self.show_notice(notice)])
                } else {
                    task
                }
            }
            SettingChange::HermesEnabled(_) => {
                let (applies, task) = self.apply_saved_filing(config.filing);
                if matches!(applies, SavedFilingApplies::AtNextStart) {
                    let notice = self.lang.text(Msg::SettingsSavedNextStart).to_owned();
                    Task::batch([task, self.show_notice(notice)])
                } else {
                    task
                }
            }
        }
    }

    /// The list of saved servers is what the file now says. A server that left
    /// it takes its password in the keychain along: nothing is kept for a
    /// server the user removed.
    fn take_over_servers(&mut self, servers: Vec<ServerEntry>) -> Task<Message> {
        let removed: Vec<ServerId> = self
            .saved_servers
            .iter()
            .map(ServerEntry::server_id)
            .filter(|gone| !servers.iter().any(|kept| kept.server_id() == *gone))
            .collect();
        self.saved_servers = servers;
        let forgets = removed.into_iter().map(|server| {
            let passwords = Arc::clone(&self.passwords);
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || passwords.forget(&server))
                        .await
                        .unwrap_or_else(|failure| Err(failure.to_string()))
                },
                |result| Message::Remote(RemoteMessage::PasswordStored(result)),
            )
        });
        Task::batch(forgets)
    }

    /// Takes over the look `config` describes; the warnings about the look
    /// before it no longer apply.
    pub(in crate::app) fn apply_look(&mut self, config: Config) {
        self.theme_preset = config.theme_preset();
        self.vscode_theme = config.vscode_theme_path().map(str::to_owned);
        self.palette = config.palette;
        self.kinds = Kinds::new(config.kinds, self.lang.kind_language());
        let catalog = Arc::clone(&self.kinds.catalog);
        if let Some(module) = &mut self.hermes.filing {
            module.set_kinds(catalog);
        }
        if matches!(
            self.config_problem,
            Some(
                ConfigProblem::LowContrast(_)
                    | ConfigProblem::VscodeTheme(_)
                    | ConfigProblem::UnknownLanguage(_)
                    | ConfigProblem::Kinds(_)
            )
        ) {
            self.config_problem = None;
            self.config_error = None;
        }
        self.report_look_warnings(config.contrast_warning, config.vscode_warning);
        self.report_language_warning(config.language_warning);
        self.report_kind_warning(config.kind_warning);
    }

    pub(in crate::app) fn update_grant(&mut self, message: GrantMessage) -> Task<Message> {
        match message {
            GrantMessage::Open => {
                if !self.sandbox.active {
                    let text = self.lang.text(Msg::GrantNotNeeded).to_owned();
                    return self.show_notice(text);
                }
                if self.blocking_overlay_is_open() {
                    return Task::none();
                }
                self.panels.context_menu = None;
                self.pick_grant_folder()
            }
            GrantMessage::OpenFor(blocked) => {
                if self.blocking_overlay_is_open() {
                    return Task::none();
                }
                self.panels.context_menu = None;
                (self.pick_folder)(&fs::grant_start_dir_for(&blocked))
                    .map(|folder| Message::Grant(GrantMessage::Picked(folder)))
            }
            GrantMessage::Choose => self.pick_grant_folder(),
            GrantMessage::Later => {
                self.grant_dialog = false;
                Task::none()
            }
            GrantMessage::Picked(None) => Task::none(),
            GrantMessage::Picked(Some(folder)) => self.grant_picked_folder(folder),
            GrantMessage::Saved { folder, result } => self.grant_saved(&folder, result),
        }
    }

    /// Opens the system folder dialog in the real home, where the user is most
    /// likely to look. Called from `update`, like every use of `pick_folder`.
    fn pick_grant_folder(&mut self) -> Task<Message> {
        (self.pick_folder)(&fs::grant_start_dir())
            .map(|folder| Message::Grant(GrantMessage::Picked(folder)))
    }

    /// Keeps the access the user gave by choosing a folder in a setup card
    /// for the next start, like the grant dialog does, but without its dialog.
    /// Nothing when the app is not sandboxed. `message` gets the folder and
    /// whether saving worked.
    pub(in crate::app) fn grant_chosen_folder(
        &self,
        folder: &Path,
        message: impl Fn(PathBuf, Result<(), String>) -> Message + Send + 'static,
    ) -> Task<Message> {
        let Some(dir) = self.sandbox.grants_dir.clone() else {
            return Task::none();
        };
        let grant = self.sandbox.grant;
        let saved = folder.to_path_buf();
        let chosen = saved.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || grant(&dir, &saved))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            move |result| message(chosen.clone(), result),
        )
    }

    /// Saves the access the user just gave, while the open panel's access is
    /// fresh, and shows both panels again: the folder may be one they could not
    /// read.
    fn grant_picked_folder(&mut self, folder: PathBuf) -> Task<Message> {
        // The dialog goes either way: behind it the status bar is dimmed, and
        // a failure is read there. "Grant folder access…" tries again.
        self.grant_dialog = false;
        let Some(dir) = self.sandbox.grants_dir.clone() else {
            return self.grant_saved(
                &folder,
                Err("there is no data folder to keep it in".to_owned()),
            );
        };
        // Right away, while the access the panel gave is fresh, but not on the
        // thread that draws: making a bookmark and writing its file can take a
        // moment on a slow volume.
        let grant = self.sandbox.grant;
        let saved = folder.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || grant(&dir, &saved))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            move |result| {
                Message::Grant(GrantMessage::Saved {
                    folder: folder.clone(),
                    result,
                })
            },
        )
    }

    /// The grant of `folder` is saved, or not: both panels load again when it
    /// is, since the folder may be one they could not read.
    fn grant_saved(&mut self, folder: &Path, saved: Result<(), String>) -> Task<Message> {
        let name = folder.to_string_lossy();
        let report = match saved {
            Ok(()) => Ok(fill_template(
                self.lang.text(Msg::GrantAdded),
                &[("{folder}", &name)],
            )),
            Err(reason) => Err(fill_template(
                self.lang.text(Msg::GrantFailed),
                &[("{folder}", &name), ("{reason}", &reason)],
            )),
        };
        match report {
            Ok(text) => {
                self.job_error = None;
                let reloads = [PanelSide::Left, PanelSide::Right].map(|each| {
                    let path = self.panel(each).path.clone();
                    self.load(each, path, None)
                });
                let notice = self.show_notice(text);
                Task::batch(reloads.into_iter().chain([notice]))
            }
            Err(text) => {
                self.set_status_error(&text);
                Task::none()
            }
        }
    }

    /// Asks for the home folder when the sandbox is on and home cannot be
    /// listed. Asked again at every start while that stays so.
    pub(in crate::app) fn open_grant_dialog_if_needed(&mut self) {
        if self.sandbox.needs_home_grant() {
            self.dialog_focus = grant_ui::GRANT_FOCUS;
            self.grant_dialog = true;
        }
    }
}
