use crate::app::*;
use iced::widget::{column, container, Container};

impl App {
    pub(in crate::app) fn update_hermes_chat(
        &mut self,
        message: HermesChatMessage,
    ) -> Task<Message> {
        if matches!(message, HermesChatMessage::Open) {
            if self.blocking_overlay_is_open() {
                return Task::none();
            }
            self.panels.context_menu = None;
        } else if !self.hermes_chat.open {
            return Task::none();
        }
        let ctx = self.hermes_chat_ctx();
        let (task, asked) = ncrs_hermes_chat::update(&mut self.hermes_chat, message, &ctx);
        let mapped = task.map(Message::HermesChat);
        let focus = if self.hermes_chat.open {
            ncrs_hermes_chat::focus_input()
        } else {
            Task::none()
        };
        match asked {
            Some(action) => Task::batch([mapped, focus, self.run_hermes_chat_action(action)]),
            None => Task::batch([mapped, focus]),
        }
    }

    fn hermes_chat_ctx(&self) -> ncrs_hermes_chat::Ctx {
        let availability = match self.hermes.filing.as_ref() {
            None => ncrs_hermes_chat::Availability::Starting,
            Some(module) => match module.chat_access(Arc::clone(&self.trash)) {
                Some(access) => ncrs_hermes_chat::Availability::Ready(access),
                None if self.hermes.is_restarting() => ncrs_hermes_chat::Availability::Starting,
                None => ncrs_hermes_chat::Availability::NeedsSetup,
            },
        };
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
            now: chrono::Local::now(),
        }
    }

    fn run_hermes_chat_action(&mut self, action: ncrs_hermes_chat::Action) -> Task<Message> {
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
            ncrs_hermes_chat::Action::OpenSearch => {
                self.hermes_chat.open = false;
                self.open_search()
            }
            ncrs_hermes_chat::Action::UseAsCommand(text) => {
                self.hermes_chat.open = false;
                Task::batch([
                    self.update_command_line(CommandLineMessage::Input(text)),
                    self.update_command_line(CommandLineMessage::Focus),
                ])
            }
            ncrs_hermes_chat::Action::SetFolderRole { folder, role } => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::set_folder_role(hermes, &folder, role, ctx)
                }),
            ncrs_hermes_chat::Action::OpenFilingSetup => {
                self.update_filing_setup(FilingSetupMessage::Toggle)
            }
        }
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
