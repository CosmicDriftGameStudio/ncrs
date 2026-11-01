//! The state changes of the command list. What reaches outside it comes back
//! as an action.

use iced::widget::operation;
use iced::{Size, Task};
use ncrs_config::keymap::KeyAction;
use ncrs_ui_kit::focus::release_focus;

use super::action::CommandPaletteAction;
use super::ctx::{CommandPaletteCtx, HermesCommands};
use super::view::{visible_rows, COMMAND_PALETTE_ID};
use super::{CommandPalette, Target};
use crate::message::CommandPaletteMessage;

type Outcome = (Task<CommandPaletteMessage>, Option<CommandPaletteAction>);

/// Applies a message to the open list, if any.
pub fn update(
    command_palette: &mut Option<CommandPalette>,
    message: CommandPaletteMessage,
    ctx: &CommandPaletteCtx<'_>,
) -> Outcome {
    match message {
        CommandPaletteMessage::Toggle => return open(command_palette, ctx),
        CommandPaletteMessage::Close => return (close(command_palette), None),
        _ => {}
    }
    let Some(palette) = command_palette.as_mut() else {
        return (Task::none(), None);
    };
    match message {
        CommandPaletteMessage::QueryChanged(text) => palette.set_query(text),
        CommandPaletteMessage::Move(delta) => palette.move_cursor(delta),
        CommandPaletteMessage::Select(index) => {
            palette.select(index);
            return run(command_palette);
        }
        CommandPaletteMessage::Run => return run(command_palette),
        CommandPaletteMessage::Toggle | CommandPaletteMessage::Close => {}
    }
    (Task::none(), None)
}

/// Cmd/Ctrl+Shift+P: lists the actions that work where the keys are now.
/// Pressed while the box is up, it only gives the field the focus again.
fn open(command_palette: &mut Option<CommandPalette>, ctx: &CommandPaletteCtx<'_>) -> Outcome {
    let focus = operation::focus(COMMAND_PALETTE_ID);
    if command_palette.is_some() {
        return (focus, None);
    }
    if ctx.blocked {
        return (Task::none(), None);
    }
    let mut palette = CommandPalette::new(ctx.scope, ctx.lang, ctx.keymap, ctx.recent);
    palette = match ctx.hermes {
        HermesCommands::None => palette.without_hermes_file_commands(),
        HermesCommands::RolesOnly => palette.without_hermes_location_commands(),
        HermesCommands::WithoutProcessAgain => {
            palette.without_command(KeyAction::HermesProcessAgain)
        }
        HermesCommands::All => palette,
    };
    palette = palette.with_settings(ctx.settings_targets.clone());
    palette.set_visible_rows(visible_rows(ctx.window));
    *command_palette = Some(palette);
    (focus, Some(CommandPaletteAction::CloseContextMenu))
}

/// Closes the list. Gives the focus up only when the list was open.
pub fn close(command_palette: &mut Option<CommandPalette>) -> Task<CommandPaletteMessage> {
    if command_palette.take().is_some() {
        release_focus()
    } else {
        Task::none()
    }
}

/// The window changed: the list shows as many rows as fit.
pub fn resize(command_palette: &mut Option<CommandPalette>, window: Size) {
    if let Some(palette) = command_palette.as_mut() {
        palette.set_visible_rows(visible_rows(window));
    }
}

/// Closes the list and hands the entry under the cursor to the app. The
/// closing task comes first: a prompt the entry opens takes the focus after
/// the list gave it up.
fn run(command_palette: &mut Option<CommandPalette>) -> Outcome {
    let Some(target) = command_palette
        .as_ref()
        .and_then(CommandPalette::selected_target)
    else {
        return (Task::none(), None);
    };
    let closed = close(command_palette);
    let action = match target {
        Target::Command(action) => CommandPaletteAction::Run(action),
        Target::Setting { feature, row } => CommandPaletteAction::ShowSetting { feature, row },
    };
    (closed, Some(action))
}
