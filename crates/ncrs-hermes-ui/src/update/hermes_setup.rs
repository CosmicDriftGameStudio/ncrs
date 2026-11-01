use iced::Task;

use super::filing_setup::resolve_in_background;
use crate::action::{Action, SetupCard};
use crate::ctx::Ctx;
use crate::filing_setup::SetupNote;
use crate::message::{HermesSetupMessage, Message};
use crate::state::Hermes;

/// Everything the card sends except `Start`, `Written` and `Later`, which
/// write the config and start or stop the filing module: the app runs those
/// itself.
pub fn update(
    hermes: &mut Hermes,
    message: HermesSetupMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        HermesSetupMessage::Start | HermesSetupMessage::Written(_) | HermesSetupMessage::Later => {
            Task::none()
        }
        HermesSetupMessage::PickFolder(slot) => {
            let Some(setup) = hermes.hermes_setup.as_mut() else {
                return Task::none();
            };
            if setup.start_picking(slot) {
                actions.push(Action::PickFolder(SetupCard::Hermes(slot)));
            }
            Task::none()
        }
        HermesSetupMessage::FolderPicked { slot, folder } => match folder {
            Some(picked) => {
                actions.push(Action::GrantFolder {
                    card: SetupCard::Hermes(slot),
                    folder: picked.clone(),
                });
                resolve_in_background(ctx, picked, move |looked_up, resolved| {
                    Message::HermesSetup(HermesSetupMessage::Resolved {
                        slot,
                        folder: looked_up,
                        resolved,
                    })
                })
            }
            None => {
                if let Some(setup) = hermes.hermes_setup.as_mut() {
                    setup.cancel_picking();
                }
                Task::none()
            }
        },
        HermesSetupMessage::Granted { folder, result } => {
            if let (Some(setup), Err(reason)) = (hermes.hermes_setup.as_mut(), result) {
                setup.show_note(SetupNote::GrantFailed { folder, reason });
            }
            Task::none()
        }
        HermesSetupMessage::Resolved {
            slot,
            folder,
            resolved,
        } => {
            if let Some(setup) = hermes.hermes_setup.as_mut() {
                setup.finish_picking(slot, folder, resolved);
            }
            Task::none()
        }
        HermesSetupMessage::ChooseEncrypted(encrypted) => {
            if let Some(setup) = hermes.hermes_setup.as_mut() {
                setup.choose_encrypted(encrypted);
            }
            Task::none()
        }
        HermesSetupMessage::ToggleLoadModel => {
            if let Some(setup) = hermes.hermes_setup.as_mut() {
                setup.toggle_load_model();
            }
            Task::none()
        }
    }
}
