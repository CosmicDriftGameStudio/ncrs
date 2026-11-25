//! The activity page: reads the latest runs that moved files.

use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;

use super::status::open_waiting;
use crate::action::Action;
use crate::activity_page::{ActivityPage, MAX_BATCHES};
use crate::message::{ActivityMessage, HermesPage, Message};
use crate::state::Hermes;

pub fn open(hermes: &mut Hermes, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.activity.is_some() {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.activity_generation = hermes.activity_generation.wrapping_add(1);
    let generation = hermes.activity_generation;
    hermes.activity = Some(ActivityPage::new());
    let Some(reader) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::activity_reader)
    else {
        if let Some(page) = hermes.activity.as_mut() {
            page.failed(INDEX_NOT_OPEN.to_owned());
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || reader.read(MAX_BATCHES)),
        move |joined| {
            Message::Activity(ActivityMessage::Loaded {
                generation,
                result: joined.unwrap_or_else(|err| Err(err.to_string())),
            })
        },
    )
}

pub fn update(
    hermes: &mut Hermes,
    message: ActivityMessage,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        ActivityMessage::Loaded { generation, result } => {
            if generation == hermes.activity_generation {
                if let Some(page) = hermes.activity.as_mut() {
                    match result {
                        Ok(batches) => page.loaded(batches),
                        Err(detail) => page.failed(detail),
                    }
                }
            }
        }
        ActivityMessage::ShowFile(path) => actions.push(Action::RevealFile(path)),
        ActivityMessage::OpenWaiting => open_waiting(hermes, actions),
        ActivityMessage::OpenReadErrors => actions.push(Action::ShowPage(HermesPage::Inspection)),
    }
    Task::none()
}
