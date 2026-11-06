//! Changing the levels in words: the model gets the levels and the sentence,
//! the card shows what changed against the levels found, and a way out is
//! always on screen.

use ncrs_ai::{pattern_mismatches, Revision};
use ncrs_i18n::Msg;

use super::{
    ask_model, levels_card, levels_kept, sample, show_levels_checked, with_extension_of, RuleAsk,
    RuleFlow, MISFITS_SHOWN, MODEL_FILES,
};
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Part, State, Step};
use crate::texts::{fill, number, text};
use crate::update::{chip, hermes};

fn is_changed(flow: &RuleFlow) -> bool {
    flow.levels != flow.found
}

/// The user wants to change the levels: they are shown with the way out, and
/// the next words are the change.
pub fn levels_change(state: &mut State, ctx: &Ctx) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    flow.changing = true;
    show_change(state, ctx, true);
}

/// One more change of the levels as they stand.
pub fn levels_more(state: &mut State, ctx: &Ctx) {
    show_change(state, ctx, true);
}

/// The changes are thrown away: the levels found are shown again.
pub fn levels_discard(state: &mut State, ctx: &Ctx) {
    if let Some(flow) = state.session.rule.as_mut() {
        flow.levels = flow.found.clone();
        flow.changing = false;
    }
    show_levels_checked(state, ctx, true);
}

/// The model gets the levels as they are and the sentence about them.
pub fn revise(state: &mut State, ctx: &Ctx, sentence: &str) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    flow.changing = true;
    let ask = RuleAsk::Revise {
        files: sample(&flow.files, MODEL_FILES),
        kinds: flow.kinds.clone(),
        previous: flow.levels.clone(),
        sentence: sentence.to_owned(),
    };
    ask_model(state, ctx, ask);
}

/// The model's answer to a sentence: the new levels are shown, or taken at
/// once when the user said they are done. The small model sets the done flag
/// on a plain change too, so it counts only when the levels stay as they are:
/// a sentence that changes them is shown first.
pub fn revised(state: &mut State, ctx: &Ctx, revision: Revision) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    let unchanged = flow.levels == revision.levels;
    flow.levels = revision.levels;
    if revision.done && unchanged {
        levels_kept(state, ctx);
    } else {
        show_change(state, ctx, false);
    }
}

/// The card with the changed levels marked, what applying them would do to
/// the files, and the buttons: done is always there.
pub fn show_change(state: &mut State, ctx: &Ctx, hint: bool) {
    let Some(flow) = state.session.rule.as_ref() else {
        return;
    };
    let kinds = flow.kinds.clone();
    let changed = is_changed(flow);
    // The hint follows a card that is still on screen.
    let mut parts = Vec::new();
    if !hint {
        parts.push(Part::Levels(levels_card(
            ctx,
            flow,
            &kinds,
            None,
            Some(&flow.found),
        )));
        if changed {
            parts.push(Part::Text(preview(ctx, flow)));
        }
    }
    if hint {
        parts.push(Part::Text(text(ctx.lang, Msg::HermesChatLevelsChangeHint)));
    }
    let done = chip(ctx, Msg::HermesChatLevelsDone, ChipReply::LevelsDone, false);
    let chips: Vec<Chip> = if changed {
        vec![
            chip(
                ctx,
                Msg::HermesChatLevelsApply,
                ChipReply::LevelsApply,
                true,
            ),
            chip(ctx, Msg::HermesChatLevelsMore, ChipReply::LevelsMore, false),
            chip(
                ctx,
                Msg::HermesChatLevelsDiscard,
                ChipReply::LevelsDiscard,
                false,
            ),
            done,
        ]
    } else {
        vec![done]
    };
    state.step = Step::RuleLevels;
    hermes(state, parts, chips);
}

/// What applying the levels does to the files the folder holds now: how many
/// are renamed or moved, and a few of them as they were and as they would be.
fn preview(ctx: &Ctx, flow: &RuleFlow) -> String {
    let moving: Vec<_> = pattern_mismatches(&flow.pattern(), &flow.written_files(), &flow.kinds)
        .into_iter()
        .filter_map(|misfit| Some((misfit.file, misfit.written?)))
        .collect();
    if moving.is_empty() {
        return text(ctx.lang, Msg::HermesChatLevelsPreviewNone);
    }
    let mut lines = vec![fill(
        ctx.lang,
        Msg::HermesChatLevelsPreview,
        &[("count", &number(moving.len()))],
    )];
    lines.extend(moving.iter().take(MISFITS_SHOWN).map(|(file, written)| {
        fill(
            ctx.lang,
            Msg::HermesChatRuleExample,
            &[("from", file), ("to", &with_extension_of(written, file))],
        )
    }));
    if moving.len() > MISFITS_SHOWN {
        lines.push(fill(
            ctx.lang,
            Msg::HermesChatRuleExamplesMore,
            &[("count", &number(moving.len() - MISFITS_SHOWN))],
        ));
    }
    lines.join("\n")
}
