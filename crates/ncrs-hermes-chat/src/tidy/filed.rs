//! What follows the filing: the report, the offers to trash duplicates, and the
//! renames inside the archive.

use std::path::PathBuf;

use ncrs_filing::chat_work::{ChatWork, RunPlan, RunReport, TrashItem, TrashReport, TrialOutcome};
use ncrs_filing::rules_work::ArchiveMove;
use ncrs_i18n::Msg;
use ncrs_index::Transfer;

use super::inside_items;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Offer, Part, State, Step, Tidy};
use crate::texts::{self, fill, folder_name, number, text};
use crate::update::{access, chip, hermes, say, work};

pub fn filed_done(state: &mut State, ctx: &Ctx, result: Result<RunReport, String>) {
    let chained = match (&result, state.session.tidy.as_ref()) {
        (Ok(_), Some(tidy)) if state.session.deciding.is_none() => {
            let left: Vec<ArchiveMove> = tidy
                .inside
                .iter()
                .filter(|found| Some(&found.root) != tidy.target.as_ref())
                .cloned()
                .collect();
            Some((tidy.source.clone(), left)).filter(|(_, moves)| !moves.is_empty())
        }
        _ => None,
    };
    let rule_prefix = state
        .session
        .tidy
        .as_ref()
        .and_then(|tidy| tidy.rule_now.clone());
    let went_well = result.is_ok();
    filed_report(state, ctx, result);
    if let Some((source, left)) = chained {
        archive_run(state, ctx, source, left);
        if let Some(tidy) = state.session.tidy.as_mut() {
            tidy.rule_now = rule_prefix;
        }
    } else if went_well && rule_prefix.is_some() {
        crate::rule_flow::trial::next_source(state, ctx, Vec::new());
    } else {
        state.session.rule_queue = None;
    }
}

/// The renames in the archive alone: one run for the archive the first of
/// the moves lies in. The moves of another archive follow once it is filed.
pub fn archive_run(state: &mut State, ctx: &Ctx, source: PathBuf, moves: Vec<ArchiveMove>) {
    let Some(target) = moves.first().map(|found| found.root.clone()) else {
        return;
    };
    let run = RunPlan {
        name: fill(
            ctx.lang,
            Msg::HermesChatRunName,
            &[("folder", &folder_name(ctx, &source))],
        ),
        transfer: Transfer::Move,
        source_root: source.clone(),
        inside: inside_items(&moves, &target),
        target_root: target.clone(),
        items: Vec::new(),
    };
    state.session.trial = None;
    state.session.tidy = Some(Tidy {
        rule_now: None,
        source,
        target: Some(target),
        transfer: Some(Transfer::Move),
        inside: moves,
    });
    state.step = Step::Idle;
    work(
        state,
        ChatWork::File { run },
        Some(text(ctx.lang, Msg::HermesChatWorkingFiling)),
    );
}

fn filed_report(state: &mut State, ctx: &Ctx, result: Result<RunReport, String>) {
    crate::follow_up::forget(state);
    if state.session.deciding.is_some() {
        crate::decide::run_done(state, ctx, result);
        return;
    }
    let (Some(tidy), trial) = (state.session.tidy.take(), state.session.trial.take()) else {
        return;
    };
    let report = match result {
        Ok(report) => report,
        Err(reason) => {
            state.step = Step::Idle;
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                crate::update::fallback_chips(ctx),
            );
            return;
        }
    };
    let folder = folder_name(ctx, &tidy.source);
    let total = state
        .session
        .readings
        .get(&tidy.source)
        .map_or(0, |reading| reading.files.len());
    state.session.readings.clear();
    let mut parts = Vec::new();
    if report.done > 0 || report.inside_done == 0 {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatFiled,
            &[
                ("done", &number(report.done)),
                ("stays", &number(total.saturating_sub(report.done))),
                ("folder", &folder),
            ],
        )));
    }
    if report.inside_done > 0 {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatFiledInside,
            &[("count", &number(report.inside_done))],
        )));
    }
    if report.skipped > 0 {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatFiledSkipped,
            &[("skipped", &number(report.skipped))],
        )));
    }
    let mut chips = Vec::new();
    let can_trash = access(ctx).is_some_and(|access| access.trash_can_restore());
    if let (Some(finished), Some(target), Some(Transfer::Move), true) =
        (trial, tidy.target.as_ref(), tidy.transfer, can_trash)
    {
        let items: Vec<TrashItem> = finished
            .plan
            .entries
            .iter()
            .filter_map(|entry| match &entry.outcome {
                TrialOutcome::AlreadyThere { at } => Some(TrashItem {
                    path: entry.from.clone(),
                    expected_hash: entry.hash,
                    keeper: target.join(at),
                }),
                _ => None,
            })
            .collect();
        if !items.is_empty() {
            parts.push(Part::Text(fill(
                ctx.lang,
                Msg::HermesChatAlreadyThereOffer,
                &[
                    ("count", &number(items.len())),
                    ("folder", &folder),
                    ("target", &folder_name(ctx, target)),
                ],
            )));
            state.session.offer = Some(Offer {
                name: fill(
                    ctx.lang,
                    Msg::HermesChatTrashRunName,
                    &[("folder", &folder)],
                ),
                root: tidy.source.clone(),
                items,
            });
            chips.push(chip(ctx, Msg::HermesChatToTrash, ChipReply::ToTrash, true));
            chips.push(chip(
                ctx,
                Msg::HermesChatLeaveThem,
                ChipReply::LeaveThem,
                false,
            ));
        }
    }
    if chips.is_empty() {
        if let Some((part, offered)) = archive_offer(state, ctx) {
            parts.push(part);
            chips = offered;
        }
    }
    chips.push(chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false));
    state.step = Step::AfterRun;
    hermes(state, parts, chips);
}

/// The duplicates of the folder learned first, offered once after the first
/// filing: the sentence and the buttons.
fn archive_offer(state: &mut State, ctx: &Ctx) -> Option<(Part, Vec<Chip>)> {
    let offer = state.session.archive_duplicates.take()?;
    state.session.archive_offered = true;
    let mut names: Vec<String> = Vec::new();
    for item in &offer.items {
        let top = item
            .path
            .strip_prefix(&offer.root)
            .ok()
            .and_then(|relative| relative.iter().next())
            .map(|segment| segment.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !top.is_empty() && !names.contains(&top) {
            names.push(top);
        }
    }
    let sentence = fill(
        ctx.lang,
        Msg::HermesChatDuplicatesOffer,
        &[
            ("names", &names.join(", ")),
            ("count", &number(offer.items.len())),
        ],
    );
    state.session.offer = Some(offer);
    Some((
        Part::Text(sentence),
        vec![
            chip(ctx, Msg::HermesChatToTrash, ChipReply::ToTrash, true),
            chip(ctx, Msg::HermesChatLeaveThem, ChipReply::LeaveThem, false),
        ],
    ))
}

pub fn trash_offer(state: &mut State, ctx: &Ctx, yes: bool) {
    let Some(offer) = state.session.offer.take() else {
        return;
    };
    if yes {
        work(
            state,
            ChatWork::TrashDuplicates {
                name: offer.name,
                root: offer.root,
                items: offer.items,
            },
            Some(text(ctx.lang, Msg::HermesChatWorkingTrash)),
        );
        return;
    }
    let mut parts = vec![Part::Text(text(ctx.lang, Msg::HermesChatLeft))];
    let mut chips = Vec::new();
    if let Some((part, offered)) = archive_offer(state, ctx) {
        parts.push(part);
        chips = offered;
    }
    if state.step == Step::AfterRun {
        chips.push(chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false));
    }
    hermes(state, parts, chips);
}

pub fn trashed_done(state: &mut State, ctx: &Ctx, result: Result<TrashReport, String>) {
    state.session.readings.clear();
    let report = match result {
        Ok(report) => report,
        Err(reason) => {
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                crate::update::fallback_chips(ctx),
            );
            return;
        }
    };
    let mut parts = vec![Part::Text(fill(
        ctx.lang,
        Msg::HermesChatTrashed,
        &[("count", &texts::number(report.trashed))],
    ))];
    if report.skipped > 0 {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatTrashedSkipped,
            &[("skipped", &number(report.skipped))],
        )));
    }
    let mut chips = Vec::new();
    if let Some((part, offered)) = archive_offer(state, ctx) {
        parts.push(part);
        chips = offered;
    }
    chips.push(chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false));
    hermes(state, parts, chips);
}
