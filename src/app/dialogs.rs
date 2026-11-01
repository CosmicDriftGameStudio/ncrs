use crate::app::*;

/// How long a confirmation stays in the status bar.
/// per file, all in one burst, so the burst becomes one transfer.
const NOTICE_DURATION: std::time::Duration = std::time::Duration::from_secs(3);

/// What a click on the "access blocked" error sends. Only macOS has a privacy
/// settings page to open, and only there does the message exist. In the
/// sandbox the click asks for access to the blocked folder instead, since Full
/// Disk Access does not reach into the sandbox.
pub(in crate::app) fn blocked_access_press(
    sandbox_active: bool,
    blocked: &Path,
) -> Option<Message> {
    if sandbox_active {
        return Some(Message::Grant(GrantMessage::OpenFor(blocked.to_path_buf())));
    }
    #[cfg(target_os = "macos")]
    return Some(Message::OpenPrivacySettings);
    #[cfg(not(target_os = "macos"))]
    None
}

impl App {
    /// What the menu bar lets the user run: nothing while a dialog or box is
    /// open, otherwise what works in the area the keys go to.
    #[cfg(any(target_os = "macos", test))]
    pub(in crate::app) fn menu_availability(&self) -> Availability {
        if self.overlay_is_open() {
            Availability::Blocked
        } else if self.search_has_keys() {
            Availability::Open(KeyScope::Search)
        } else {
            Availability::Open(KeyScope::Panel)
        }
    }

    /// The inquiries under the cursor or among the marked files of the active
    /// panel.
    pub(in crate::app) fn inquiry_paths(&self) -> Vec<PathBuf> {
        self.action_sources()
            .into_iter()
            .filter(|path| {
                self.hermes.hermes_column.state_of(path) == Some(HermesFileState::Inquiry)
            })
            .collect()
    }

    pub(in crate::app) fn update_command_line(
        &mut self,
        message: CommandLineMessage,
    ) -> Task<Message> {
        let ctx = CommandLineCtx {
            blocked: self.command_line_is_blocked(),
            lang: self.lang,
            remote: self.active_panel().remote().is_some(),
            active_side: self.panels.active,
            active_path: self.active_panel().path.clone(),
            home: &self.home,
            shell_directory: shell_directory(self.active_panel()),
        };
        let (inner, action) = command_line::update(&mut self.command_line, message, &ctx);
        let task = inner.map(Message::CommandLine);
        match action {
            None => task,
            Some(CommandLineAction::ClosePathField) => {
                self.panels.set_path_field(None);
                task
            }
            Some(CommandLineAction::ShowError(text)) => {
                self.set_status_error(&text);
                task
            }
            Some(CommandLineAction::OpenDirectory { side, destination }) => {
                self.job_error = None;
                self.notice = None;
                Task::batch([
                    self.load(side, destination.directory, destination.select),
                    task,
                ])
            }
            Some(CommandLineAction::ShowPathProblem { problem, target }) => {
                let reason = ncrs_files::path_field::problem_text(self.lang, &problem, &target);
                self.set_status_error(&reason);
                task
            }
            Some(CommandLineAction::ShellFinished { report }) => self.finish_shell_command(report),
        }
    }

    /// Settings and the filing setup replace the panel area; an overlay covers it.
    pub(in crate::app) fn command_line_is_blocked(&self) -> bool {
        self.overlay_is_open() || self.settings.page.is_some() || self.hermes.filing_setup.is_some()
    }

    /// A shell command ended: its report goes to the status bar and both panels
    /// are re-read, as the command may have changed what they show.
    fn finish_shell_command(&mut self, report: Result<String, String>) -> Task<Message> {
        let reloads = [PanelSide::Left, PanelSide::Right].map(|each| {
            let path = self.panel(each).path.clone();
            self.load(each, path, None)
        });
        let notice = match report {
            Ok(text) => {
                self.job_error = None;
                self.show_notice(text)
            }
            Err(text) => {
                self.set_status_error(&text);
                Task::none()
            }
        };
        Task::batch(reloads.into_iter().chain([notice]))
    }

    /// Shows `text` in the status bar for [`NOTICE_DURATION`]. The timer carries
    /// the generation, so it cannot clear a notice that came after it.
    /// F7 and Shift+F4: the name prompt for the active panel's folder.
    pub(in crate::app) fn open_prompt(&mut self, prompt: Prompt) -> Task<Message> {
        let side = self.panels.active;
        self.ops.open_prompt(prompt, side)
    }

    pub(in crate::app) fn show_notice(&mut self, text: String) -> Task<Message> {
        self.notice_generation = self.notice_generation.wrapping_add(1);
        self.notice = Some(text);
        let generation = self.notice_generation;
        Task::perform(
            async { tokio::time::sleep(NOTICE_DURATION).await },
            move |()| Message::NoticeExpired(generation),
        )
    }

    pub(in crate::app) fn dialog_button_count(&self) -> usize {
        if self.hermes.hermes_setup.is_some() {
            self.hermes.setup_focus_order().len()
        } else if self.about.is_some() {
            3
        } else if self.settings.model_card_has_one_button() {
            1
        } else if self.ops.delete_dialog.is_some()
            || self.start_over_dialog
            || self.settings.ocr_models.is_some()
            || self.settings.ai_card_is_shown()
            || self.grant_dialog
        {
            2
        } else if self.ops.conflict_is_open() {
            3
        } else {
            0
        }
    }

    /// Enter in a button dialog: whatever the focused button does.
    pub(in crate::app) fn activate_dialog_button(&mut self) -> Task<Message> {
        if self.hermes.hermes_setup.is_some() {
            return self.activate_hermes_focus();
        }
        if self.start_over_dialog {
            return if self.dialog_focus == hermes_overview_ui::START_OVER_CONFIRM_FOCUS {
                self.update_hermes_overview(HermesOverviewMessage::ConfirmStartOver)
            } else {
                self.start_over_dialog = false;
                Task::none()
            };
        }
        if self.ops.delete_dialog.is_some() {
            return if self.dialog_focus == delete_dialog_view::CONFIRM_FOCUS {
                self.confirm_delete()
            } else {
                self.ops.delete_dialog = None;
                Task::none()
            };
        }
        if self.about.is_some() {
            return match self.dialog_focus {
                about_ui::REPOSITORY_FOCUS => self.update_about(AboutMessage::OpenRepository),
                about_ui::LICENSES_FOCUS => self.update_about(AboutMessage::OpenLicenses),
                _ => self.update_about(AboutMessage::Close),
            };
        }
        if self.settings.model_card_has_one_button() {
            return if self.settings.ocr_models.is_some() {
                self.update_ocr_models(OcrModelsMessage::Close)
            } else {
                self.update_ai_model(AiModelMessage::Close)
            };
        }
        if self
            .settings
            .ai_model
            .as_ref()
            .is_some_and(AiModelCard::is_finished)
        {
            return if self.dialog_focus == ai_model_ui::DOWNLOAD_FOCUS {
                self.update_ai_model(AiModelMessage::ShowAiSetting)
            } else {
                self.update_ai_model(AiModelMessage::Close)
            };
        }
        if self.settings.ocr_models.is_some() {
            return if self.dialog_focus == ocr_models_ui::DOWNLOAD_FOCUS {
                self.update_ocr_models(OcrModelsMessage::Download)
            } else {
                self.update_ocr_models(OcrModelsMessage::Close)
            };
        }
        if self.settings.ai_card_is_shown() {
            return if self.dialog_focus == ai_model_ui::DOWNLOAD_FOCUS {
                self.press_ai_model_button()
            } else {
                self.update_ai_model(AiModelMessage::Close)
            };
        }
        if self.grant_dialog {
            return if self.dialog_focus == grant_ui::GRANT_FOCUS {
                self.update_grant(GrantMessage::Choose)
            } else {
                self.update_grant(GrantMessage::Later)
            };
        }
        if self.ops.conflict_is_open() {
            let choice = match self.dialog_focus {
                conflict::OVERWRITE_FOCUS => ConflictChoice::ThisOverwrite,
                conflict::KEEP_FOCUS => ConflictChoice::ThisKeep,
                _ => ConflictChoice::Cancel,
            };
            return self.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, choice, ctx));
        }
        Task::none()
    }

    /// A prompt, dialog or menu is on top of the panels.
    pub(in crate::app) fn overlay_is_open(&self) -> bool {
        self.blocking_overlay_is_open() || self.panels.context_menu.is_some()
    }

    /// Everything on top of the panels except the context menu, which a new
    /// right click may replace.
    pub(in crate::app) fn blocking_overlay_is_open(&self) -> bool {
        self.ablage_tab_is_open() || self.overlay_apart_from_overview_is_open()
    }

    /// The page of filing that stands in for the panels: the overview or the
    /// status.
    pub(in crate::app) fn ablage_tab_is_open(&self) -> bool {
        self.hermes.filing_status.page_open() || self.hermes.page_is_open()
    }

    /// Closes the page of filing without a word, for a page that replaces it.
    pub(in crate::app) fn close_ablage_tab(&mut self) -> Task<Message> {
        self.hermes.filing_status.close_page();
        let left_overview = self.leave_hermes_overview();
        self.hermes.close_pages();
        left_overview
    }

    /// What the overview itself may open on top of it: the model cards.
    pub(in crate::app) fn overlay_apart_from_overview_is_open(&self) -> bool {
        self.overlay_apart_from_overview_and_hermes_is_open()
            || self.hermes.quick_fix.is_some()
            || self.hermes.hermes_setup.is_some()
    }

    pub(in crate::app) fn overlay_apart_from_overview_and_hermes_is_open(&self) -> bool {
        self.ops.prompt.is_some()
            || self.ops.conflict_is_open()
            || self.ops.transfer_dialog.is_some()
            || self.ops.progress_mask_is_open()
            || self.ops.delete_dialog.is_some()
            || self.start_over_dialog
            || self.panels.volume_menu.is_some()
            || self.quick_open.is_some()
            || self.command_palette.is_some()
            || self.settings.key_bindings.is_some()
            || self.settings.theme_fetch.is_some()
            || self.about.is_some()
            || self.settings.ocr_models.is_some()
            || self.settings.ai_card_is_shown()
            || self.grant_dialog
            || self.remote_ui.dialog.is_some()
    }

    /// The create of the name prompt has reported: close the prompt or show
    /// why it failed, and reload the folder.
    pub(in crate::app) fn finish_prompt(
        &mut self,
        kind: PromptKind,
        request_id: u64,
        result: Result<(), fs::CreateDirError>,
    ) -> Task<Message> {
        if !self.ops.is_current_prompt_request(request_id) {
            return Task::none();
        }
        // The queued reload runs here, after the create has reported, never
        // beside it. On success it selects the new entry; on failure it still
        // re-reads, because the name may belong to something else that
        // appeared meanwhile.
        let pending = self.panels.take_pending_reload();
        let created = pending.as_ref().map(ReloadNow::created);
        let reload = pending.map(|due| self.load(due.side, due.path, Some(due.name)));
        let lang = self.lang;
        if let Some(path) = self.ops.finish_prompt(kind, result, created, lang) {
            self.launch_external(fs::OpenKind::Edit, path, None, CopyOf::Nothing);
        }
        reload.unwrap_or_else(Task::none)
    }

    /// Writes the session state here and now, then ends the app: the runtime
    /// stops right after, so no task would run.
    pub(in crate::app) fn quit(&mut self) -> Task<Message> {
        if let Some(path) = &self.state_path {
            write_session_state(
                &self.session_writes,
                WriteOrder::FINAL,
                path,
                &self.session_state(),
            );
        }
        if let Some(copies) = self.ops.view_copies.as_mut() {
            copies.close();
        }
        self.remote.close_all();
        iced::exit()
    }

    /// A click on the "access blocked" error: the Full Disk Access page of
    /// System Settings. A launch that fails says so in the status bar.
    #[cfg(target_os = "macos")]
    pub(in crate::app) fn open_privacy_settings(&mut self) {
        self.job_error = None;
        let command = fs::privacy_settings_command();
        if let Err(reason) = self.launcher.launch(&command) {
            let failure = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", self.lang.text(Msg::StatusSystemSettings))
                .replace("{reason}", &reason);
            self.set_status_error(&failure);
        }
    }

    pub(in crate::app) fn update_about(&mut self, message: AboutMessage) -> Task<Message> {
        match message {
            AboutMessage::Open => {
                if self.blocking_overlay_is_open() {
                    return Task::none();
                }
                self.panels.context_menu = None;
                self.dialog_focus = about_ui::CLOSE_FOCUS;
                self.about = Some(AboutCard::new());
            }
            AboutMessage::Close => self.about = None,
            AboutMessage::OpenLicenses => match (self.find_third_party_licenses)() {
                Some(path) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.launch_from_about(path.clone().into_os_string(), &name);
                }
                None => {
                    if let Some(card) = self.about.as_mut() {
                        card.report_licenses_missing();
                    }
                }
            },
            AboutMessage::OpenRepository => {
                self.launch_from_about(REPOSITORY_URL.into(), REPOSITORY_URL);
            }
        }
        Task::none()
    }

    /// A launch that fails closes the card, so the error in the status bar is
    /// not hidden behind it.
    fn launch_from_about(&mut self, target: std::ffi::OsString, name: &str) {
        let command = fs::system_open_command(target);
        if let Err(reason) = self.launcher.launch(&command) {
            self.about = None;
            let failure = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", name)
                .replace("{reason}", &reason);
            self.set_status_error(&failure);
        }
    }
}
