//! The answers that only point to what Hermes can do, and the chips for them.

use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, State};
use crate::texts::text;
use crate::update::{chip, say};

pub fn search_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatSearchNotYet),
        vec![chip(
            ctx,
            Msg::HermesChatOpenSearch,
            ChipReply::OpenSearch,
            true,
        )],
    );
}

pub fn explain_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatExplainNotYet),
        fallback_chips(ctx),
    );
}

pub fn batch_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatBatchNotYet),
        fallback_chips(ctx),
    );
}

pub fn fallback(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatFallback),
        fallback_chips(ctx),
    );
}

pub fn fallback_chips(ctx: &Ctx) -> Vec<Chip> {
    vec![
        chip(
            ctx,
            Msg::HermesChatIntentSearch,
            ChipReply::IntentSearch,
            false,
        ),
        chip(ctx, Msg::HermesChatIntentTidy, ChipReply::IntentTidy, false),
        chip(
            ctx,
            Msg::HermesChatIntentStructure,
            ChipReply::IntentStructure,
            false,
        ),
        chip(ctx, Msg::HermesChatIntentUndo, ChipReply::IntentUndo, false),
    ]
}

/// The same, with the button to read the folder again when there is one.
pub fn lost_folder_retry_chips(state: &State, ctx: &Ctx) -> Vec<Chip> {
    let mut chips = lost_folder_chips(ctx);
    if state.session.retry.is_some() {
        chips.insert(1, chip(ctx, Msg::HermesChatRetry, ChipReply::Retry, false));
    }
    chips
}

/// A folder that cannot be read: another one, or one of the ways on.
pub fn lost_folder_chips(ctx: &Ctx) -> Vec<Chip> {
    let mut chips = vec![chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        true,
    )];
    chips.extend(fallback_chips(ctx));
    chips
}
