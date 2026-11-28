//! The Platz answer: the chat asks the app to measure, shows the card, and
//! hands a run to the Platz tab, which confirms and does it.

use std::sync::Arc;

use iced::Task;
use ncrs_i18n::Msg;
use ncrs_space::{RunLog, Scan, Skip};
use ncrs_ui_kit::format;

use super::{chip, chip_labelled, drop_working, hermes, is_busy, say};
use crate::action::{Action, SpaceRunKind};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::space_card::{build, percent};
use crate::state::{Chip, ChipReply, Entry, Part, State};
use crate::texts::{fill, text};

/// Most lines about folders left standing after a run.
const LEFT_STANDING_LINES: usize = 3;

/// What a typed sentence about the space of the drive leads to: a measure,
/// then the card with the buttons that run something.
pub fn ask_measure(state: &mut State, ctx: &Ctx) -> Option<Action> {
    state.thread.push(Entry::Working(text(
        ctx.lang,
        Msg::HermesChatSpaceMeasuring,
    )));
    state.session.measuring_space = true;
    Some(Action::MeasureSpace)
}

pub fn measured(state: &mut State, ctx: &Ctx, scan: &Arc<Scan>) {
    if !std::mem::take(&mut state.session.measuring_space) {
        return;
    }
    drop_working(state);
    state.space_measured = Some(crate::state::SpaceMeasured {
        free: scan.volume.available,
        at: scan.measured_at,
    });
    let built = build(scan, ctx);
    let shown = |bytes: u64| format::size_shown(bytes, ctx.lang);
    let mut parts = vec![Part::Text(built.intro), Part::Space(built.card)];
    let mut chips = Vec::new();
    if built.safe_bytes > 0 {
        chips.push(chip_labelled(
            fill(
                ctx.lang,
                Msg::SpaceDeleteAllSafe,
                &[("size", &shown(built.safe_bytes))],
            ),
            ChipReply::SpaceDeleteSafe,
            true,
        ));
    } else {
        parts.push(Part::Text(text(ctx.lang, Msg::HermesChatSpaceNothingSafe)));
    }
    if !ctx.store_build && built.worktree_bytes > 0 {
        chips.push(chip_labelled(
            fill(
                ctx.lang,
                Msg::SpaceRemoveWorktrees,
                &[("size", &shown(built.worktree_bytes))],
            ),
            ChipReply::SpaceRemoveWorktrees,
            false,
        ));
    }
    chips.push(chip(
        ctx,
        Msg::SpaceOpenOverview,
        ChipReply::SpaceOverview,
        false,
    ));
    hermes(state, parts, chips);
    if ctx.model.is_some() && ctx.ai_on && !built.facts.is_empty() {
        state.describing = Some(built.facts);
        state.fresh_describe = true;
    }
}

pub fn measure_failed(state: &mut State, ctx: &Ctx, reason: &str) {
    if !std::mem::take(&mut state.session.measuring_space) {
        return;
    }
    drop_working(state);
    say(
        state,
        fill(ctx.lang, Msg::HermesChatSpaceFailed, &[("reason", reason)]),
        super::fallback_chips(ctx),
    );
}

/// The task that asks the model for one sentence per folder the user
/// decides about; the card is already on screen.
pub fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh_describe) {
        return Task::none();
    }
    let Some(facts) = state.describing.clone() else {
        return Task::none();
    };
    let Some(model) = ctx.model.clone() else {
        state.describing = None;
        return Task::none();
    };
    Task::perform(
        async move { tokio::task::spawn_blocking(move || model.describe_folders(&facts)).await },
        |joined| match joined {
            Ok(Ok(notes)) => Message::SpaceDescribed(Ok(notes)),
            Ok(Err(error)) => Message::SpaceDescribed(Err(error.to_string())),
            Err(error) => Message::SpaceDescribed(Err(error.to_string())),
        },
    )
}

/// The sentences go to the rows the user decides about, in the order they
/// were asked; the rows keep the reason of the code when there are none.
pub fn described(state: &mut State, ctx: &Ctx, result: Result<Vec<String>, String>) {
    let Some(asked) = state.describing.take() else {
        return;
    };
    let card = state.thread.iter_mut().rev().find_map(|entry| match entry {
        Entry::Hermes { parts, .. } => parts.iter_mut().find_map(|part| match part {
            Part::Space(card) => Some(card),
            _ => None,
        }),
        _ => None,
    });
    let Some(card) = card else {
        return;
    };
    match result {
        Ok(notes) if notes.len() == asked.len() => {
            let rows = card.rows.iter_mut().filter(|row| row.described);
            for (row, note) in rows.zip(notes) {
                row.hint = note;
            }
        }
        Ok(_) | Err(_) => {
            if let Some(Entry::Hermes { parts, .. }) = state.thread.last_mut() {
                parts.push(Part::Text(text(
                    ctx.lang,
                    Msg::HermesChatSpaceDescribeFailed,
                )));
            }
        }
    }
}

fn skip_text(ctx: &Ctx, skip: &Skip) -> Option<String> {
    let lang = ctx.lang;
    Some(match skip {
        Skip::Gone => return None,
        Skip::ToolRunning(holder) => {
            fill(lang, Msg::SpaceSkipToolRunning, &[("tool", &holder.name)])
        }
        Skip::FolderOpen(holder) => {
            fill(lang, Msg::SpaceSkipFolderOpen, &[("holder", &holder.name)])
        }
        Skip::UseUnknown => text(lang, Msg::SpaceSkipUseUnknown),
        Skip::Symlink => text(lang, Msg::SpaceSkipSymlink),
        Skip::OtherDrive => text(lang, Msg::SpaceSkipOtherDrive),
        Skip::Changed => text(lang, Msg::SpaceSkipChanged),
        Skip::NotGranted => text(lang, Msg::SpaceSkipNotGranted),
        Skip::Failed(reason) => fill(lang, Msg::SpaceSkipFailed, &[("reason", reason)]),
    })
}

/// What the run of the Platz tab did, in a few sentences; the overview opens
/// from the button.
pub fn run_done(state: &mut State, ctx: &Ctx, log: &RunLog) {
    let shown = |bytes: u64| format::size_shown(bytes, ctx.lang);
    let freed = log.freed_bytes();
    let mut parts = Vec::new();
    let free_now = ncrs_os_probe::volume_space(&ctx.home).ok();
    parts.push(Part::Text(match free_now {
        Some(volume) => fill(
            ctx.lang,
            Msg::HermesChatSpaceFreed,
            &[
                ("freed", &shown(freed)),
                ("free", &shown(volume.available)),
                (
                    "percent",
                    &percent(volume.total.saturating_sub(volume.available), volume.total)
                        .to_string(),
                ),
            ],
        ),
        None => fill(ctx.lang, Msg::SpaceResultFreed, &[("size", &shown(freed))]),
    }));
    let left: Vec<String> = log
        .rows
        .iter()
        .filter_map(|row| {
            let reason = skip_text(ctx, row.outcome.as_ref().err()?)?;
            let name = row.path.file_name()?.to_string_lossy().into_owned();
            Some(fill(
                ctx.lang,
                Msg::HermesChatSpaceLeftStanding,
                &[("name", &name), ("reason", &reason)],
            ))
        })
        .take(LEFT_STANDING_LINES)
        .collect();
    parts.extend(left.into_iter().map(Part::Text));
    let chips = vec![chip(
        ctx,
        Msg::SpaceOpenOverview,
        ChipReply::SpaceOverview,
        false,
    )];
    hermes(state, parts, chips);
}

/// The buttons of the card: the runs go through the Platz tab and its
/// confirm, never straight from the chat.
pub fn reply(ctx: &Ctx, reply: &ChipReply) -> Option<Action> {
    match reply {
        ChipReply::SpaceDeleteSafe => Some(Action::SpaceRun(SpaceRunKind::DeleteSafe)),
        ChipReply::SpaceRemoveWorktrees if !ctx.store_build => {
            Some(Action::SpaceRun(SpaceRunKind::RemoveWorktrees))
        }
        ChipReply::SpaceOverview => Some(Action::OpenSpace),
        _ => None,
    }
}

/// The message a key of the card stands for, when the button it stands for is
/// on screen and the chat is free.
pub fn card_key_message(state: &State, key: crate::keys::ChatKey) -> Option<Message> {
    use crate::keys::ChatKey;
    if is_busy(state) {
        return None;
    }
    let wanted = match key {
        ChatKey::SpaceDeleteSafe => ChipReply::SpaceDeleteSafe,
        ChatKey::SpaceRemoveWorktrees => ChipReply::SpaceRemoveWorktrees,
        ChatKey::SpaceOverview => ChipReply::SpaceOverview,
        _ => return None,
    };
    let shown: &Chip = state.chips().iter().find(|chip| chip.reply == wanted)?;
    Some(Message::ChipPressed {
        label: shown.label.clone(),
        reply: wanted,
    })
}
