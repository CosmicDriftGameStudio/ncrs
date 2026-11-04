use iced::Task;
use ncrs_filing::filing::FilingModule;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{HermesOverviewMessage, Message};
use crate::state::Hermes;
use crate::ui::hermes_overview::START_OVER_CANCEL_FOCUS;

/// What the buttons for the protection of the index ask for.
pub fn update(
    hermes: &mut Hermes,
    message: HermesOverviewMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        HermesOverviewMessage::SetLoad(load) => actions.push(Action::SetLoad(load)),
        HermesOverviewMessage::EnterRecoveryKey => {}
        HermesOverviewMessage::EncryptNow => {
            actions.push(Action::ChangeProtection(FilingModule::begin_encrypting));
        }
        HermesOverviewMessage::AskStartOver => {
            let can_ask = hermes
                .filing
                .as_ref()
                .is_some_and(FilingModule::can_start_over)
                && !ctx.jobs_busy
                && !ctx.hermes_is_restarting;
            if can_ask {
                actions.push(Action::OpenStartOverDialog(START_OVER_CANCEL_FOCUS));
            }
        }
        HermesOverviewMessage::CancelStartOver => actions.push(Action::CloseStartOverDialog),
        HermesOverviewMessage::ConfirmStartOver => {
            if ctx.start_over_dialog_open {
                actions.push(Action::CloseStartOverDialog);
                actions.push(Action::ChangeProtection(FilingModule::begin_starting_over));
            }
        }
        HermesOverviewMessage::TryAgain => actions.push(Action::RestartHermesModule),
    }
    Task::none()
}
