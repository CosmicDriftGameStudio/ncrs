//! The column of Hermes in the file panels: what the index says about the
//! visible rows, and the files read again.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;

use iced::Task;
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::{fill_template, Msg};

use crate::action::Action;
use crate::ctx::{Ctx, PanelFacts};
use crate::hermes_column::{ColumnRequest, ColumnStage, FileDetail, HermesFileState};
use crate::message::{HermesColumnMessage, Message};
use crate::reading_again::{self, Ending};
use crate::state::Hermes;

const SPINNER_STEP: f32 = FRAC_PI_2;

/// Takes in the pipeline's latest snapshot: what became stale is asked for
/// again by [`request_hermes_column`] after this message.
pub fn refresh_hermes_column(hermes: &mut Hermes, ctx: &Ctx<'_>) {
    if !hermes.is_active() {
        return;
    }
    let epoch = hermes.hermes_column.epoch();
    hermes
        .hermes_column
        .set_snapshot(hermes.filing_status.snapshot());
    if hermes.hermes_column.epoch() != epoch {
        hermes.hermes_reads.clear();
        hermes.hermes_detail_reads.clear();
    }
    if visible_row_is_reading(hermes, ctx) {
        hermes.hermes_spinner_angle = (hermes.hermes_spinner_angle + SPINNER_STEP).rem_euclid(TAU);
    }
}

fn visible_row_is_reading(hermes: &Hermes, ctx: &Ctx<'_>) -> bool {
    ctx.panels.iter().any(|panel| {
        panel.visible.iter().any(|entry| {
            hermes.hermes_column.state_of(entry.path) == Some(HermesFileState::Reading)
        })
    })
}

/// Asks for what the visible rows lack: the listing and the direct counts
/// first, the deep counts after. Runs after every message, so a scroll, a
/// resize, a cursor move or a new directory all find their data here.
pub fn request_hermes_column(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    if !hermes.is_active() {
        return Task::none();
    }
    let mut tasks: Vec<Task<Message>> = ctx
        .panels
        .iter()
        .filter_map(|panel| request_panel_column(hermes, panel))
        .collect();
    tasks.extend(request_cursor_detail(hermes, ctx));
    Task::batch(tasks)
}

fn request_panel_column(hermes: &mut Hermes, panel: &PanelFacts<'_>) -> Option<Task<Message>> {
    if !panel.readable {
        return None;
    }
    let side = panel.side;
    let directory = panel.directory.to_path_buf();
    let generation = panel.request_id;
    let visible_folders: Vec<PathBuf> = panel
        .visible
        .iter()
        .filter(|entry| entry.is_dir && !entry.is_parent)
        .map(|entry| entry.path.to_path_buf())
        .collect();
    let listing_wanted = !hermes
        .hermes_column
        .wanted_listings(std::slice::from_ref(&directory))
        .is_empty();
    let direct_wanted = hermes
        .hermes_column
        .wanted_counts(&visible_folders, ColumnStage::Listing);
    let (stage, subfolders) = if listing_wanted || !direct_wanted.is_empty() {
        (ColumnStage::Listing, direct_wanted)
    } else {
        let deep_wanted = hermes
            .hermes_column
            .wanted_counts(&visible_folders, ColumnStage::Deep);
        if deep_wanted.is_empty() {
            return None;
        }
        (ColumnStage::Deep, deep_wanted)
    };
    if !hermes.hermes_reads.insert((directory.clone(), stage)) {
        return None;
    }
    let reader = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::column_reader)?;
    let request = ColumnRequest {
        generation,
        epoch: hermes.hermes_column.epoch(),
        directory: directory.clone(),
        subfolders,
        stage,
    };
    Some(Task::perform(
        tokio::task::spawn_blocking(move || reader.read(&request)),
        move |joined| {
            Message::HermesColumn(HermesColumnMessage::Read {
                side,
                directory: directory.clone(),
                stage,
                result: joined.unwrap_or_else(|err| Err(err.to_string())),
            })
        },
    ))
}

/// The review of the file under the cursor, once, when it is an inquiry.
fn request_cursor_detail(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Option<Task<Message>> {
    let path = ctx.cursor_entry?.to_path_buf();
    if !hermes.hermes_column.wants_detail(&path) || !hermes.hermes_detail_reads.insert(path.clone())
    {
        return None;
    }
    let access = hermes.filing.as_ref()?.review_access()?;
    let asked = path.clone();
    Some(Task::perform(
        tokio::task::spawn_blocking(move || access.suggestion_for(&asked)),
        move |joined| {
            let found = joined.unwrap_or_else(|err| Err(err.to_string()));
            Message::HermesColumn(HermesColumnMessage::Detail {
                path: path.clone(),
                result: found.map(|review| {
                    review.map(|(text_state, suggestion)| FileDetail {
                        text_state,
                        suggestion,
                    })
                }),
            })
        },
    ))
}

pub fn update_hermes_column(
    hermes: &mut Hermes,
    message: HermesColumnMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        HermesColumnMessage::Read {
            side,
            directory,
            stage,
            result,
        } => {
            let Ok(reading) = result else {
                // Stays in `hermes_reads`: not asked again until the next
                // invalidation, so a broken index is not hammered.
                return Task::none();
            };
            hermes.hermes_reads.remove(&(directory, stage));
            if let Some(panel) = ctx.panel(side) {
                if reading.generation == panel.request_id && reading.directory == panel.directory {
                    hermes.hermes_column.apply(reading);
                }
            }
            Task::none()
        }
        HermesColumnMessage::Detail { path, result } => {
            if let Ok(Some(detail)) = result {
                hermes.hermes_detail_reads.remove(&path);
                hermes.hermes_column.apply_detail(path, detail);
            }
            Task::none()
        }
        HermesColumnMessage::ReadAgainEnded { path, result } => {
            let text = reading_again::notice(ctx.lang, &path, &Ending::Finished, result.ok());
            actions.push(Action::ShowNotice(text));
            Task::none()
        }
        HermesColumnMessage::Excluded { paths, result } => {
            paths_excluded(hermes, &paths, result, ctx, actions);
            Task::none()
        }
    }
}

fn paths_excluded(
    hermes: &mut Hermes,
    paths: &[PathBuf],
    result: Result<(), String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    if let Err(detail) = result {
        actions.push(Action::ShowError(detail));
        return;
    }
    for path in paths {
        hermes.invalidate_hermes_path(path);
    }
    let text = fill_template(
        ctx.lang.text(Msg::StatusQuickFixExcluded),
        &[("{count}", &paths.len().to_string())],
    );
    actions.push(Action::ShowNotice(text));
}

/// Reports the files whose new reading has ended. Whether a finished file
/// waits for the user is in the index, so that is asked on the blocking pool
/// and the notice follows.
pub fn settle_reading_again(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if hermes.reading_again.is_empty() {
        return Task::none();
    }
    let ended = hermes.reading_again.settle(hermes.filing_status.snapshot());
    let tasks: Vec<Task<Message>> = ended
        .into_iter()
        .map(|(path, ending)| {
            let access = hermes
                .filing
                .as_ref()
                .and_then(FilingModule::review_access)
                .filter(|_| ending == Ending::Finished);
            match access {
                Some(review) => {
                    let asked = path.clone();
                    Task::perform(
                        tokio::task::spawn_blocking(move || {
                            review.suggestion_for(&asked).map(|found| found.is_some())
                        }),
                        move |joined| {
                            Message::HermesColumn(HermesColumnMessage::ReadAgainEnded {
                                path: path.clone(),
                                result: joined.unwrap_or_else(|err| Err(err.to_string())),
                            })
                        },
                    )
                }
                None => {
                    actions.push(Action::ShowNotice(reading_again::notice(
                        ctx.lang, &path, &ending, None,
                    )));
                    Task::none()
                }
            }
        })
        .collect();
    Task::batch(tasks)
}
