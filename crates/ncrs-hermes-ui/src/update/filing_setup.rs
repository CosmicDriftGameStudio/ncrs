use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_intake::LocationMode;

use crate::action::{Action, SetupCard};
use crate::ctx::Ctx;
use crate::filing_setup::{self, FilingSetup, SetupNote};
use crate::message::{FilingSetupMessage, Message};
use crate::state::Hermes;
use ncrs_config::config::FilingWriteError;

pub fn update(
    hermes: &mut Hermes,
    message: FilingSetupMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        FilingSetupMessage::Toggle => {
            if hermes.hermes_setup.take().is_some() {
                hermes.pending_hermes_start = None;
            } else if hermes.filing_setup.take().is_none() {
                if hermes.filing_config.locations.is_empty() {
                    super::lifecycle::open_hermes_setup(hermes, ctx, actions);
                } else {
                    hermes.filing_setup = Some(FilingSetup::new(&hermes.filing_config.locations));
                }
            }
            Task::none()
        }
        // The file is written even when the card was closed meanwhile, so the
        // module follows it all the same.
        FilingSetupMessage::Saved(Ok(filing)) => {
            let (applies, task) = super::lifecycle::apply_saved_filing(hermes, filing, ctx);
            hermes.filing_setup = None;
            actions.push(Action::ShowNotice(super::lifecycle::saved_filing_notice(
                applies, ctx,
            )));
            task
        }
        FilingSetupMessage::Saved(Err(error)) => {
            if let Some(setup) = hermes.filing_setup.as_mut() {
                setup.finish_saving(SetupNote::SaveFailed(error));
            }
            Task::none()
        }
        FilingSetupMessage::Close => {
            hermes.filing_setup = None;
            Task::none()
        }
        FilingSetupMessage::PickFolder(mode) => {
            let Some(setup) = hermes.filing_setup.as_mut() else {
                return Task::none();
            };
            if setup.start_adding() {
                actions.push(Action::PickFolder(SetupCard::Filing(mode)));
            }
            Task::none()
        }
        FilingSetupMessage::FolderPicked { mode, folder } => {
            let Some(setup) = hermes.filing_setup.as_mut() else {
                return Task::none();
            };
            match folder {
                Some(picked) => {
                    actions.push(Action::GrantFolder {
                        card: SetupCard::Filing(mode),
                        folder: picked.clone(),
                    });
                    resolve_setup_folder(ctx, mode, picked)
                }
                None => {
                    setup.cancel_adding();
                    Task::none()
                }
            }
        }
        FilingSetupMessage::AddPanelFolder(mode) => {
            let Some(setup) = hermes.filing_setup.as_mut() else {
                return Task::none();
            };
            if !setup.start_adding() {
                return Task::none();
            }
            resolve_setup_folder(ctx, mode, ctx.active_panel_path.to_path_buf())
        }
        FilingSetupMessage::Granted { folder, result } => {
            if let (Some(setup), Err(reason)) = (hermes.filing_setup.as_mut(), result) {
                setup.show_note(SetupNote::GrantFailed { folder, reason });
            }
            Task::none()
        }
        FilingSetupMessage::Added {
            mode,
            folder,
            resolved,
        } => {
            if let Some(setup) = hermes.filing_setup.as_mut() {
                setup.finish_adding(mode, folder, resolved);
            }
            Task::none()
        }
        FilingSetupMessage::SetTarget(index) => {
            if let Some(setup) = hermes.filing_setup.as_mut() {
                setup.set_target(index);
            }
            Task::none()
        }
        FilingSetupMessage::ToggleImages(index) => {
            if let Some(setup) = hermes.filing_setup.as_mut() {
                setup.toggle_images(index);
            }
            Task::none()
        }
        FilingSetupMessage::Remove(index) => {
            if let Some(setup) = hermes.filing_setup.as_mut() {
                setup.remove(index);
            }
            Task::none()
        }
        FilingSetupMessage::Save => save(hermes.filing_setup.as_mut(), ctx),
    }
}

fn save(open_setup: Option<&mut FilingSetup>, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(setup) = open_setup else {
        return Task::none();
    };
    let Some(config_path) = ctx.config_path.map(Path::to_path_buf) else {
        setup.refuse_saving(SetupNote::NoConfigPath);
        return Task::none();
    };
    let Some(chosen) = setup.start_saving() else {
        return Task::none();
    };
    let written_path = config_path.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || filing_setup::write_locations(&written_path, &chosen)),
        move |joined| {
            Message::FilingSetup(FilingSetupMessage::Saved(joined.unwrap_or_else(|join| {
                Err(FilingWriteError::Io {
                    path: config_path,
                    reason: join.to_string(),
                })
            })))
        },
    )
}

/// Looks up `folder` on a blocking thread; the answer is an `Added`.
fn resolve_setup_folder(ctx: &Ctx<'_>, mode: LocationMode, folder: PathBuf) -> Task<Message> {
    resolve_in_background(ctx, folder, move |looked_up, resolved| {
        Message::FilingSetup(FilingSetupMessage::Added {
            mode,
            folder: looked_up,
            resolved,
        })
    })
}

pub(in crate::update) fn resolve_in_background(
    ctx: &Ctx<'_>,
    folder: PathBuf,
    answer: impl FnOnce(PathBuf, Result<PathBuf, String>) -> Message + Send + 'static,
) -> Task<Message> {
    let resolve_folder = ctx.resolve_folder;
    Task::perform(
        tokio::task::spawn_blocking({
            let looked_up = folder.clone();
            move || resolve_folder(&looked_up)
        }),
        move |joined| answer(folder, joined.unwrap_or_else(|join| Err(join.to_string()))),
    )
}
