use iced::Task;

use super::filing_setup::resolve_in_background;
use super::lifecycle;
use crate::action::{Action, SetupCard};
use crate::ctx::Ctx;
use crate::filing_setup::SetupNote;
use crate::hermes_setup::{FolderSlot, SetupFocus};
use crate::message::{HermesSetupMessage, Message};
use crate::state::Hermes;

pub fn update(
    hermes: &mut Hermes,
    message: HermesSetupMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        HermesSetupMessage::Start => lifecycle::start_hermes_setup(hermes, ctx),
        HermesSetupMessage::Written(result) => match result {
            Ok(filing) => lifecycle::hermes_written(hermes, filing, ctx, actions),
            Err(error) => {
                hermes.pending_hermes_start = None;
                if let Some(setup) = hermes.hermes_setup.as_mut() {
                    setup.stop_writing(SetupNote::SaveFailed(error));
                }
                Task::none()
            }
        },
        HermesSetupMessage::Later => lifecycle::hermes_later(hermes, ctx, actions),
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

/// Enter on the element that has the focus.
pub fn activate_focus(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(element) = hermes.setup_focus_order().get(ctx.dialog_focus).copied() else {
        return Task::none();
    };
    update(
        hermes,
        match element {
            SetupFocus::ChooseInbox => HermesSetupMessage::PickFolder(FolderSlot::Inbox),
            SetupFocus::ChooseFilingLocation => {
                HermesSetupMessage::PickFolder(FolderSlot::FilingLocation)
            }
            SetupFocus::Encrypted => HermesSetupMessage::ChooseEncrypted(true),
            SetupFocus::Plain => HermesSetupMessage::ChooseEncrypted(false),
            SetupFocus::LoadModel => HermesSetupMessage::ToggleLoadModel,
            SetupFocus::Later => HermesSetupMessage::Later,
            SetupFocus::Start => HermesSetupMessage::Start,
        },
        ctx,
        actions,
    )
}
