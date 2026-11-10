//! Two ways to file that need no neighbours: into a new folder the user
//! picks, ordered the way they say, and file by file with the user naming the
//! folder of each.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use ncrs_filing::chat_work::{ChatWork, FolderReading, PatternLevel, RunItem, RunPlan, RunReport};
use ncrs_i18n::Msg;
use ncrs_index::Transfer;
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Deciding, NewLayout, ReadFor, State, Step};
use crate::texts::{fill, folder_name, number};
use crate::update::{cancel_chip, chip, chip_labelled, say, work};
use crate::{learn, tidy};

pub fn pick_new_target(state: &mut State) -> Option<Action> {
    state.step = Step::NewTarget;
    Some(Action::PickFolder)
}

pub fn new_target_picked(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    state.step = Step::Idle;
    learn::read_for(state, ctx, folder, ReadFor::NewTarget);
}

/// The new target was read: it becomes a filing location. A folder that
/// holds filed documents shows what it holds, any other asks for its order.
pub fn new_target_read(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
    let folder = reading.root.clone();
    if !state.session.learned.contains(&folder) {
        state.session.learned.push(folder.clone());
    }
    if reading.structure.areas.is_empty() {
        if !ctx.targets.contains(&folder) {
            state.queued_action = Some(Action::SetFolderRole {
                folder: folder.clone(),
                role: FolderRole::FilingLocation,
            });
        }
        state.step = Step::NewLayout;
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatNewOrderAsk,
                &[("folder", &folder_name(ctx, &folder))],
            ),
            layout_chips(ctx),
        );
        return;
    }
    state.session.resume_tidy = true;
    learn::summary(state, ctx, reading);
}

fn layout_chips(ctx: &Ctx) -> Vec<Chip> {
    vec![
        chip(
            ctx,
            Msg::HermesChatNewOrderYear,
            ChipReply::NewLayout(NewLayout::Year),
            false,
        ),
        chip(
            ctx,
            Msg::HermesChatNewOrderSender,
            ChipReply::NewLayout(NewLayout::Sender),
            false,
        ),
        chip(
            ctx,
            Msg::HermesChatNewOrderKind,
            ChipReply::NewLayout(NewLayout::Kind),
            false,
        ),
        chip(
            ctx,
            Msg::HermesChatNewOrderFlat,
            ChipReply::NewLayout(NewLayout::Flat),
            false,
        ),
    ]
}

pub fn layout_chosen(state: &mut State, ctx: &Ctx, layout: NewLayout) {
    state.session.layout = Some(match layout {
        NewLayout::Year => vec![PatternLevel::Year],
        NewLayout::Sender => vec![PatternLevel::Sender],
        NewLayout::Kind => vec![PatternLevel::Kind],
        NewLayout::Flat => Vec::new(),
    });
    tidy::resume(state, ctx);
}

/// The user wants to decide for every file; the mode is asked first.
pub fn per_file(state: &mut State, ctx: &Ctx) {
    let Some(source) = state
        .session
        .tidy
        .as_ref()
        .and_then(|tidy| state.session.readings.get(&tidy.source).cloned())
    else {
        return;
    };
    state.session.deciding = Some(Deciding {
        source,
        next: 0,
        picks: Vec::new(),
        runs: Vec::new(),
        done: 0,
        skipped: 0,
    });
    tidy::ask_mode(state, ctx);
}

pub fn start(state: &mut State, ctx: &Ctx, transfer: Transfer) {
    if let Some(tidy) = state.session.tidy.as_mut() {
        tidy.transfer = Some(transfer);
    }
    ask(state, ctx);
}

fn ask(state: &mut State, ctx: &Ctx) {
    let Some(deciding) = state.session.deciding.as_ref() else {
        return;
    };
    let total = deciding.source.files.len();
    let Some(file) = deciding.source.files.get(deciding.next) else {
        finish(state, ctx);
        return;
    };
    let name = file
        .relative_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let position = deciding.next + 1;
    let mut chips: Vec<Chip> = tidy::targets(state, ctx)
        .into_iter()
        .map(|path| {
            chip_labelled(
                folder_name(ctx, &path),
                ChipReply::DecideTarget(path),
                false,
            )
        })
        .collect();
    chips.push(chip(
        ctx,
        Msg::HermesChatDecideSkip,
        ChipReply::DecideSkip,
        false,
    ));
    chips.push(cancel_chip(ctx));
    state.step = Step::Decide;
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatDecideWhere,
            &[
                ("name", &name),
                ("position", &number(position)),
                ("total", &number(total)),
            ],
        ),
        chips,
    );
}

pub fn target_chosen(state: &mut State, ctx: &Ctx, target: PathBuf) {
    if let Some(deciding) = state.session.deciding.as_mut() {
        deciding.picks.push((deciding.next, target));
        deciding.next += 1;
    }
    ask(state, ctx);
}

pub fn skipped(state: &mut State, ctx: &Ctx) {
    if let Some(deciding) = state.session.deciding.as_mut() {
        deciding.skipped += 1;
        deciding.next += 1;
    }
    ask(state, ctx);
}

/// Every file is decided: one run per folder the files go to.
fn finish(state: &mut State, ctx: &Ctx) {
    let (Some(deciding), Some(tidy)) =
        (state.session.deciding.as_mut(), state.session.tidy.as_ref())
    else {
        return;
    };
    let Some(transfer) = tidy.transfer else {
        return;
    };
    let mut by_target: BTreeMap<PathBuf, Vec<RunItem>> = BTreeMap::new();
    for (position, target) in &deciding.picks {
        let Some(file) = deciding.source.files.get(*position) else {
            continue;
        };
        let Some(name) = file.relative_path.file_name() else {
            continue;
        };
        by_target.entry(target.clone()).or_default().push(RunItem {
            from: deciding.source.root.join(&file.relative_path),
            to: PathBuf::from(name),
            hash: file.hash,
        });
    }
    let folder = folder_name(ctx, &tidy.source);
    deciding.runs = by_target
        .into_iter()
        .rev()
        .map(|(target_root, items)| RunPlan {
            name: fill(ctx.lang, Msg::HermesChatRunName, &[("folder", &folder)]),
            transfer,
            source_root: tidy.source.clone(),
            target_root,
            inside: Vec::new(),
            items,
        })
        .collect();
    run_next(state, ctx);
}

fn run_next(state: &mut State, ctx: &Ctx) {
    let Some(run) = state
        .session
        .deciding
        .as_mut()
        .and_then(|deciding| deciding.runs.pop())
    else {
        summary(state, ctx);
        return;
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::File { run },
        Some(crate::texts::text(ctx.lang, Msg::HermesChatWorkingFiling)),
    );
}

pub fn run_done(state: &mut State, ctx: &Ctx, result: Result<RunReport, String>) {
    match result {
        Ok(report) => {
            if let Some(deciding) = state.session.deciding.as_mut() {
                deciding.done += report.done;
                deciding.skipped += report.skipped;
            }
            run_next(state, ctx);
        }
        Err(reason) => {
            let failure = fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]);
            let filed_before = state
                .session
                .deciding
                .as_ref()
                .is_some_and(|deciding| deciding.done > 0);
            if filed_before {
                say(state, failure, Vec::new());
                summary(state, ctx);
                return;
            }
            state.session.deciding = None;
            state.session.tidy = None;
            state.session.readings.clear();
            state.step = Step::Idle;
            say(state, failure, crate::update::fallback_chips(ctx));
        }
    }
}

fn summary(state: &mut State, ctx: &Ctx) {
    let (Some(deciding), Some(tidy)) = (state.session.deciding.take(), state.session.tidy.take())
    else {
        return;
    };
    state.session.readings.clear();
    if deciding.done == 0 {
        state.step = Step::Idle;
        say(
            state,
            crate::texts::text(ctx.lang, Msg::HermesChatDecideNothing),
            crate::update::fallback_chips(ctx),
        );
        return;
    }
    let total = deciding.source.files.len();
    state.step = Step::AfterRun;
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatFiled,
            &[
                ("done", &number(deciding.done)),
                ("stays", &number(total.saturating_sub(deciding.done))),
                ("folder", &folder_name(ctx, &tidy.source)),
            ],
        ),
        vec![chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false)],
    );
}
