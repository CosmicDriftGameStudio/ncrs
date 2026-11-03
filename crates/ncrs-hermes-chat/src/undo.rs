//! Taking the last run back.

use ncrs_filing::chat_work::{ChatWork, RunSummary, UndoReport};
use ncrs_i18n::Msg;
use ncrs_index::Transfer;

use crate::ctx::Ctx;
use crate::state::{ChipReply, State, Step};
use crate::texts::{fill, folder_name, number, text, when_text};
use crate::update::{cancel_chip, chip, say, work};

pub fn ask(state: &mut State, _ctx: &Ctx) {
    work(state, ChatWork::LastRun, None);
}

pub fn last_run_done(state: &mut State, ctx: &Ctx, result: Result<Option<RunSummary>, String>) {
    let summary = match result {
        Ok(Some(summary)) => summary,
        Ok(None) => {
            let chips = crate::update::fallback_chips(ctx)
                .into_iter()
                .filter(|chip| chip.reply != ChipReply::IntentUndo)
                .collect();
            say(state, text(ctx.lang, Msg::HermesChatUndoNone), chips);
            return;
        }
        Err(reason) => {
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                Vec::new(),
            );
            return;
        }
    };
    let msg = match summary.transfer {
        Transfer::Move => Msg::HermesChatUndoAskMove,
        Transfer::Copy => Msg::HermesChatUndoAskCopy,
        Transfer::Trash => Msg::HermesChatUndoAskTrash,
    };
    let folder = summary
        .source_folder
        .as_ref()
        .map(|folder| folder_name(ctx, folder))
        .unwrap_or_default();
    let sentence = fill(
        ctx.lang,
        msg,
        &[
            ("name", &summary.name),
            ("count", &number(summary.items)),
            ("when", &when_text(ctx, summary.finished_at)),
            ("folder", &folder),
        ],
    );
    state.session.last_run = Some(summary);
    state.step = Step::UndoAsk;
    say(
        state,
        sentence,
        vec![
            chip(ctx, Msg::HermesChatPutBack, ChipReply::PutBack, true),
            cancel_chip(ctx),
        ],
    );
}

pub fn put_back(state: &mut State, ctx: &Ctx) {
    let Some(summary) = state.session.last_run.take() else {
        ask(state, ctx);
        return;
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::Undo {
            batch: summary.batch,
        },
        Some(text(ctx.lang, Msg::HermesChatWorkingUndo)),
    );
}

pub fn undone_done(state: &mut State, ctx: &Ctx, result: Result<UndoReport, String>) {
    state.session.readings.clear();
    state.step = Step::Idle;
    let report = match result {
        Ok(report) => report,
        Err(reason) => {
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                Vec::new(),
            );
            return;
        }
    };
    let mut parts = vec![crate::state::Part::Text(fill(
        ctx.lang,
        Msg::HermesChatUndone,
        &[("count", &number(report.undone))],
    ))];
    if report.left > 0 {
        parts.push(crate::state::Part::Text(fill(
            ctx.lang,
            Msg::HermesChatUndoneLeft,
            &[("left", &number(report.left))],
        )));
    }
    crate::update::hermes(state, parts, Vec::new());
}
