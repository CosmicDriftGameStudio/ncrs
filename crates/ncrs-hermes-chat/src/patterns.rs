//! "Is one of these your pattern?": the orders the folders already follow,
//! offered to be used below every area, with a preview before the yes.

use ncrs_filing::chat_work::{ChatWork, PatternChoice, PatternLevel, PatternPreview};
use ncrs_i18n::Msg;
use ncrs_intake::PlaceholderLabel;

use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, FileCard, Part, State, Step};
use crate::texts::{fill, pattern_levels, text};
use crate::update::{cancel_chip, chip, chip_labelled, hermes, say, work};

fn label_of(level: PatternLevel) -> PlaceholderLabel {
    match level {
        PatternLevel::Year => PlaceholderLabel::Year,
        PatternLevel::Month => PlaceholderLabel::Month,
        PatternLevel::Sender => PlaceholderLabel::Sender,
        PatternLevel::Kind => PlaceholderLabel::Kind,
    }
}

pub fn levels_words(ctx: &Ctx, levels: &[PatternLevel]) -> String {
    pattern_levels(
        ctx.lang,
        &levels.iter().copied().map(label_of).collect::<Vec<_>>(),
    )
}

fn levels_text(ctx: &Ctx, choice: &PatternChoice) -> String {
    levels_words(ctx, &choice.levels)
}

/// One button per pattern the folders follow, remembered for the answer.
pub fn chips(state: &mut State, ctx: &Ctx) -> Vec<Chip> {
    let choices = state
        .session
        .learning
        .as_ref()
        .map(|reading| reading.patterns.clone())
        .unwrap_or_default();
    let chips = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            chip_labelled(
                fill(
                    ctx.lang,
                    Msg::HermesChatAdoptPattern,
                    &[("pattern", &levels_text(ctx, choice))],
                ),
                ChipReply::AdoptPattern(index),
                false,
            )
        })
        .collect();
    state.session.patterns = choices;
    chips
}

pub fn adopt(state: &mut State, ctx: &Ctx, index: usize) {
    let (Some(reading), Some(choice)) = (
        state.session.learning.clone(),
        state.session.patterns.get(index).cloned(),
    ) else {
        return;
    };
    state.session.change_word = levels_text(ctx, &choice);
    work(
        state,
        ChatWork::PreviewPattern {
            reading,
            levels: choice.levels,
            name: None,
        },
        None,
    );
}

pub fn previewed(state: &mut State, ctx: &Ctx, computed: Option<PatternPreview>) {
    let Some(preview) = computed else {
        state.step = Step::AfterChange;
        say(
            state,
            text(ctx.lang, Msg::HermesChatPatternNothing),
            vec![chip(
                ctx,
                Msg::HermesChatFineAsIs,
                ChipReply::FineAsIs,
                true,
            )],
        );
        return;
    };
    let first_difference = preview
        .before
        .iter()
        .zip(&preview.after)
        .position(|(before, after)| before != after);
    let intro = if state.session.change_word.is_empty() {
        text(ctx.lang, Msg::HermesChatPatternPreviewNames)
    } else {
        fill(
            ctx.lang,
            Msg::HermesChatPatternPreview,
            &[("pattern", &state.session.change_word)],
        )
    };
    let names_unsaid = preview.change.name.is_some() && !state.session.change_word.is_empty();
    state.session.pending_change = None;
    state.session.pending_pattern = Some(preview.change);
    state.step = Step::ConfirmChange;
    let mut parts = vec![Part::Text(intro)];
    if names_unsaid {
        parts.push(Part::Text(text(
            ctx.lang,
            Msg::HermesChatChangePreviewWithNames,
        )));
    }
    parts.extend([
        Part::File(FileCard {
            segments: preview.before,
            highlighted: None,
        }),
        Part::File(FileCard {
            segments: preview.after,
            highlighted: first_difference,
        }),
    ]);
    hermes(
        state,
        parts,
        vec![
            chip(
                ctx,
                Msg::HermesChatConfirmYes,
                ChipReply::ConfirmChange,
                true,
            ),
            cancel_chip(ctx),
        ],
    );
}
