//! The hand-over to the "Filing" tab and what the chat says when the tab is done.

use std::path::PathBuf;

use ncrs_filing::chat_work::RunSummary;
use ncrs_i18n::Msg;
use ncrs_index::{BatchId, Transfer};

use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, State, Step};
use crate::texts::{fill, folder_name, number, text};
use crate::update::{chip, say};

/// A run the filing tab finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledInTab {
    pub source: PathBuf,
    pub transfer: Transfer,
    pub filed: usize,
    /// The files that stay in the source.
    pub left: usize,
    pub batch: BatchId,
}

/// The chat says the plan is in the tab.
pub fn hand_over(state: &mut State, ctx: &Ctx) {
    state.step = Step::Idle;
    say(state, text(ctx.lang, Msg::HermesChatHandover), Vec::new());
}

/// Posts the report of a run done in the tab, with the chip that puts it back.
/// The chat stays closed or open as it was.
pub fn filed_in_tab(state: &mut State, ctx: &Ctx, done: FiledInTab) {
    let folder = folder_name(ctx, &done.source);
    let msg = match done.transfer {
        Transfer::Copy => Msg::HermesChatFiledInTabCopy,
        Transfer::Move | Transfer::Trash => Msg::HermesChatFiledInTabMove,
    };
    let sentence = fill(
        ctx.lang,
        msg,
        &[
            ("filed", &number(done.filed)),
            ("left", &number(done.left)),
            ("folder", &folder),
        ],
    );
    state.session.last_run = Some(RunSummary {
        batch: done.batch,
        name: fill(ctx.lang, Msg::HermesChatRunName, &[("folder", &folder)]),
        transfer: done.transfer,
        items: done.filed,
        finished_at: ctx.now.timestamp(),
        source_folder: Some(done.source),
    });
    state.step = Step::UndoAsk;
    let chips: Vec<Chip> = vec![chip(ctx, Msg::HermesChatUndo, ChipReply::PutBack, false)];
    say(state, sentence, chips);
}
