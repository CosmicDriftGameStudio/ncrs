//! The state changes of the command line. What reaches outside it comes back
//! as an action.

use iced::widget::operation;
use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;

use super::action::CommandLineAction;
use super::ctx::CommandLineCtx;
use super::view::COMMAND_LINE_ID;
use super::{parse, run, CommandLine, ShellOutcome, Submission};
use crate::message::CommandLineMessage;

type Outcome = (Task<CommandLineMessage>, Option<CommandLineAction>);

pub fn update(
    command_line: &mut CommandLine,
    message: CommandLineMessage,
    ctx: &CommandLineCtx<'_>,
) -> Outcome {
    match message {
        CommandLineMessage::Focus => {
            if ctx.blocked {
                return (Task::none(), None);
            }
            command_line.focus();
            (
                operation::focus(COMMAND_LINE_ID)
                    .chain(operation::move_cursor_to_end(COMMAND_LINE_ID)),
                Some(CommandLineAction::ClosePathField),
            )
        }
        CommandLineMessage::Leave => {
            command_line.leave();
            (release_focus(), None)
        }
        CommandLineMessage::Input(text) => {
            command_line.input(text);
            (Task::none(), Some(CommandLineAction::ClosePathField))
        }
        CommandLineMessage::HistoryPrevious => {
            walk_history(command_line, CommandLine::recall_previous)
        }
        CommandLineMessage::HistoryNext => walk_history(command_line, CommandLine::recall_next),
        CommandLineMessage::Submit => submit(command_line, ctx),
        CommandLineMessage::CdChecked {
            request_id,
            side,
            target,
            result,
        } => {
            if !command_line.is_current_cd_check(request_id) || !command_line.is_focused() {
                return (Task::none(), None);
            }
            match result {
                Ok(destination) => {
                    command_line.leave();
                    (
                        release_focus(),
                        Some(CommandLineAction::OpenDirectory { side, destination }),
                    )
                }
                Err(problem) => (
                    Task::none(),
                    Some(CommandLineAction::ShowPathProblem { problem, target }),
                ),
            }
        }
        CommandLineMessage::Finished { command, outcome } => {
            command_line.finished();
            (
                Task::none(),
                Some(CommandLineAction::ShellFinished {
                    report: shell_report(ctx, &command, outcome),
                }),
            )
        }
    }
}

fn walk_history(command_line: &mut CommandLine, step: fn(&mut CommandLine) -> bool) -> Outcome {
    if step(command_line) {
        (operation::move_cursor_to_end(COMMAND_LINE_ID), None)
    } else {
        (Task::none(), None)
    }
}

/// Enter in the command line: `cd` is checked off the UI thread and applied
/// to the active panel once the answer is in; anything else runs in the shell.
fn submit(command_line: &mut CommandLine, ctx: &CommandLineCtx<'_>) -> Outcome {
    let typed = command_line.typed().to_owned();
    match parse(&typed) {
        Submission::Empty => {
            command_line.leave();
            (release_focus(), None)
        }
        // Neither a `cd` nor a command means anything on a server: the
        // line runs on this computer.
        Submission::ChangeDirectory(_) | Submission::Shell(_) if ctx.remote => {
            let text = ctx.lang.text(Msg::StatusRemoteNoShell).to_owned();
            (Task::none(), Some(CommandLineAction::ShowError(text)))
        }
        Submission::ChangeDirectory(path) => {
            command_line.remember(&typed);
            let side = ctx.active_side;
            let target = ncrs_fs::resolve_typed_path(&ctx.active_path, ctx.home, path);
            let request_id = command_line.next_cd_check();
            (
                Task::perform(ncrs_fs::locate(target.clone()), move |result| {
                    CommandLineMessage::CdChecked {
                        request_id,
                        side,
                        target: target.clone(),
                        result,
                    }
                }),
                None,
            )
        }
        Submission::Shell(line) => {
            command_line.remember(line);
            command_line.started();
            command_line.leave();
            let directory = ctx.shell_directory.clone();
            let command = line.to_owned();
            (
                Task::batch([
                    release_focus(),
                    Task::perform(run(command.clone(), directory), move |outcome| {
                        CommandLineMessage::Finished {
                            command: command.clone(),
                            outcome,
                        }
                    }),
                ]),
                None,
            )
        }
    }
}

/// What the status bar says about a shell command that ended.
fn shell_report(
    ctx: &CommandLineCtx<'_>,
    command: &str,
    outcome: ShellOutcome,
) -> Result<String, String> {
    match outcome {
        ShellOutcome::Exited(code) => {
            let text = fill_template(
                ctx.lang.text(Msg::CommandLineExitStatus),
                &[("{command}", command), ("{code}", &code.to_string())],
            );
            if code == 0 {
                Ok(text)
            } else {
                Err(text)
            }
        }
        ShellOutcome::NoExitCode => Err(fill_template(
            ctx.lang.text(Msg::CommandLineNoExitStatus),
            &[("{command}", command)],
        )),
        ShellOutcome::NotStarted { reason } => Err(fill_template(
            ctx.lang.text(Msg::CommandLineNotStarted),
            &[("{command}", command), ("{reason}", &reason)],
        )),
    }
}
