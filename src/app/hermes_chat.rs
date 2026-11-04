use crate::app::*;
use iced::widget::{column, container, Container};

impl App {
    pub(in crate::app) fn update_hermes_chat(
        &mut self,
        message: HermesChatMessage,
    ) -> Task<Message> {
        let mut first_run = Task::none();
        if matches!(message, HermesChatMessage::Open) {
            if self.blocking_overlay_is_open() {
                return Task::none();
            }
            self.panels.context_menu = None;
            let filing = &self.hermes.filing_config;
            if filing.first_run && !filing.enabled && self.hermes.filing.is_none() {
                first_run = self.turn_on_hermes();
            }
        } else if !self.hermes_chat.open && !message.is_work_result() {
            return Task::none();
        }
        if let HermesChatMessage::FolderPicked(Some(folder)) = &message {
            if self.sandbox.grants_dir.is_some() {
                return self.grant_chosen_folder(folder, |granted, result| {
                    Message::HermesChat(HermesChatMessage::FolderGranted {
                        folder: granted,
                        result,
                    })
                });
            }
        }
        self.ensure_chat_model();
        let ctx = self.hermes_chat_ctx();
        let (task, asked) = ncrs_hermes_chat::update(&mut self.hermes_chat, message, &ctx);
        let mapped = task.map(Message::HermesChat);
        let focus = if self.hermes_chat.open {
            ncrs_hermes_chat::focus_input()
        } else {
            Task::none()
        };
        match asked {
            Some(action) => Task::batch([
                first_run,
                mapped,
                focus,
                self.run_hermes_chat_action(action),
            ]),
            None => Task::batch([first_run, mapped, focus]),
        }
    }

    /// The greeting or sentence that waited for Hermes' start follows once
    /// it is ready; a closed chat gets it when it is opened.
    pub(in crate::app) fn tell_chat_hermes_is_ready(&mut self) -> Task<Message> {
        if self.hermes_chat.open
            && self.hermes_chat.waiting_for_hermes
            && !matches!(
                self.chat_availability(),
                ncrs_hermes_chat::Availability::Starting
            )
        {
            return self.update_hermes_chat(HermesChatMessage::HermesReady);
        }
        Task::none()
    }

    /// The open chat has work or the model running, or waits for Hermes to
    /// start: its line moves.
    pub(in crate::app) fn hermes_chat_is_working(&self) -> bool {
        let chat = &self.hermes_chat;
        chat.open
            && (chat.pending_work().is_some()
                || chat.pending_turn().is_some()
                || chat.waits_for_hermes())
    }

    #[cfg(feature = "local-ai")]
    fn ensure_chat_model(&mut self) {
        if !self.chat_ai_on() {
            self.chat_model = None;
            return;
        }
        if self.chat_model.is_some() {
            return;
        }
        let Some(index_path) = self.hermes.index_path.as_deref() else {
            return;
        };
        let threads = match self.hermes.filing_config.ai {
            crate::config::AiSetting::Local { threads } => threads,
            _ => None,
        };
        self.chat_model = Some(crate::filing::shared_local_model(
            index_path,
            self.hermes.filing_config.load,
            threads,
        ));
    }

    #[cfg(not(feature = "local-ai"))]
    fn ensure_chat_model(&mut self) {}

    fn chat_ai_on(&self) -> bool {
        self.hermes.filing_config.ai != crate::config::AiSetting::Off
    }

    pub(in crate::app) fn chat_availability(&self) -> ncrs_hermes_chat::Availability {
        match self.hermes.filing.as_ref() {
            None if self.hermes.filing_config.enabled
                || self.hermes.is_restarting()
                || self.turning_hermes_on =>
            {
                ncrs_hermes_chat::Availability::Starting
            }
            None => ncrs_hermes_chat::Availability::Off,
            Some(module) => match module.chat_access(Arc::clone(&self.trash)) {
                Some(access) => ncrs_hermes_chat::Availability::Ready(Box::new(access)),
                None if self.hermes.is_restarting() || module.is_starting() => {
                    ncrs_hermes_chat::Availability::Starting
                }
                None => match module.phase() {
                    crate::filing::Phase::Failed(reason) => {
                        ncrs_hermes_chat::Availability::Failed(reason.clone())
                    }
                    _ => ncrs_hermes_chat::Availability::Off,
                },
            },
        }
    }

    pub(in crate::app) fn hermes_chat_ctx(&self) -> ncrs_hermes_chat::Ctx {
        let availability = self.chat_availability();
        let locations = &self.hermes.filing_config.locations;
        ncrs_hermes_chat::Ctx {
            lang: self.lang,
            availability,
            home: self.home.clone(),
            targets: locations
                .iter()
                .filter(|location| location.is_target)
                .map(|location| location.path.clone())
                .collect(),
            sources: locations
                .iter()
                .filter(|location| location.mode == LocationMode::Inbox)
                .map(|location| location.path.clone())
                .collect(),
            favourites: self.panels.favourites.clone(),
            now: chrono::Local::now(),
            resolve_folder: self.resolve_folder,
            ai_on: self.chat_ai_on(),
            ocr_models_present: self.ocr_models_present(),
            model: self.chat_ai_on().then(|| self.chat_model.clone()).flatten(),
        }
    }

    pub(in crate::app) fn run_hermes_chat_action(
        &mut self,
        action: ncrs_hermes_chat::Action,
    ) -> Task<Message> {
        match action {
            ncrs_hermes_chat::Action::Close => {
                self.hermes_chat.open = false;
                Task::none()
            }
            ncrs_hermes_chat::Action::PickFolder => {
                let start_in = if self.active_panel().remote().is_some() {
                    self.home.clone()
                } else {
                    self.active_panel().path.clone()
                };
                (self.pick_folder)(&start_in)
                    .map(|folder| Message::HermesChat(HermesChatMessage::FolderPicked(folder)))
            }
            ncrs_hermes_chat::Action::DownloadModel => {
                let card = self.open_and_download_ai_model();
                if self.chat_ai_on() {
                    card
                } else {
                    Task::batch([card, self.turn_on_ai()])
                }
            }
            ncrs_hermes_chat::Action::TurnOnAi => self.turn_on_ai(),
            ncrs_hermes_chat::Action::OpenSearch => {
                self.hermes_chat.open = false;
                self.open_search()
            }
            ncrs_hermes_chat::Action::SetFolderRole { folder, role } => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::set_folder_role(hermes, &folder, role, ctx)
                }),
            ncrs_hermes_chat::Action::OpenOcrModels => {
                self.update_ocr_models(OcrModelsMessage::Open)
            }
            ncrs_hermes_chat::Action::TurnOnHermes => self.turn_on_hermes(),
        }
    }

    /// Off by the setting: the setting is switched on. Stopped on a card the
    /// user left: the module is built again.
    fn turn_on_hermes(&mut self) -> Task<Message> {
        if self.hermes.filing_config.enabled {
            return self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::restart_hermes_module);
        }
        if self.turning_hermes_on {
            return Task::none();
        }
        if self.config_path.is_none() {
            let reason = self.lang.text(Msg::FilingSetupNoConfigPath).to_owned();
            return self.hermes_not_turned_on(reason);
        }
        self.turning_hermes_on = true;
        self.update_settings(SettingsMessage::Change(SettingChange::HermesEnabled(true)))
    }

    /// The setting could not be saved: the chat says why, or the notice does
    /// when the chat is closed.
    pub(in crate::app) fn hermes_not_turned_on(&mut self, reason: String) -> Task<Message> {
        self.turning_hermes_on = false;
        if self.hermes_chat.open {
            return self.update_hermes_chat(HermesChatMessage::TurnOnFailed(reason));
        }
        let notice = fill_template(
            self.lang.text(Msg::HermesChatTurnOnFailed),
            &[("{reason}", &reason)],
        );
        self.show_notice(notice)
    }

    fn turn_on_ai(&mut self) -> Task<Message> {
        self.update_settings(SettingsMessage::Change(SettingChange::AiEnabled(true)))
    }

    /// Shift+F2: the chat closes and the command line takes the keys.
    pub(in crate::app) fn hermes_chat_to_command_line(&mut self) -> Task<Message> {
        self.hermes_chat.open = false;
        self.update_command_line(CommandLineMessage::Focus)
    }

    pub(in crate::app) fn hermes_chat_area(&self) -> Container<'_, Message> {
        let chat = ncrs_hermes_chat::view(
            &self.hermes_chat,
            ncrs_hermes_chat::ViewCtx {
                lang: self.lang,
                palette: &self.palette,
            },
        )
        .map(Message::HermesChat);
        container(column![
            chat,
            fkeys::view_marked(&self.function_key_bar(), Some(1), &self.palette),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
    }
}
