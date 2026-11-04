//! A sentence that only refines the one before ("dann nur Bank"): what the
//! session keeps of the last request, and the answer when the model still
//! cannot place the sentence.

use ncrs_ai::ChatIntent;
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, State};
use crate::texts::text;
use crate::update::{cancel_chip, chip_labelled, fallback_chips, say};

const AREA_CHIPS: usize = 4;

pub fn forget(state: &mut State) {
    state.session.previous_request = None;
    state.session.previous_message = None;
}

/// A tidy or a change is the request the next sentence may refine; a sentence
/// Hermes could not use is kept as typed.
pub fn remember(state: &mut State, intent: &ChatIntent, typed: &str) {
    match intent {
        ChatIntent::TidyFolder { .. } | ChatIntent::ChangeStructure { .. } => {
            state.session.previous_request = Some(intent.clone());
            state.session.previous_message = None;
        }
        ChatIntent::Undo | ChatIntent::LeaveAsIs => forget(state),
        ChatIntent::Answer { .. } => {}
        ChatIntent::BulkMove { .. } | ChatIntent::Other => {
            state.session.previous_message = Some(typed.to_owned());
        }
        ChatIntent::Search(_)
        | ChatIntent::AskContent { .. }
        | ChatIntent::Explain { .. }
        | ChatIntent::SmallTalk => state.session.previous_message = None,
    }
}

/// Small talk or nothing right after a request is almost always a follow-up
/// the model could not place: Hermes asks again, with the areas as buttons.
pub fn answered_misread(state: &mut State, ctx: &Ctx, intent: &ChatIntent) -> bool {
    let after_request =
        state.session.previous_request.is_some() || state.session.previous_message.is_some();
    if !after_request || !matches!(intent, ChatIntent::SmallTalk | ChatIntent::Other) {
        return false;
    }
    state.session.previous_message = None;
    let mut chips = area_chips(state);
    if chips.is_empty() {
        chips = fallback_chips(ctx);
    }
    chips.push(cancel_chip(ctx));
    say(
        state,
        text(ctx.lang, Msg::HermesChatNotUnderstoodFollowUp),
        chips,
    );
    true
}

fn area_chips(state: &State) -> Vec<Chip> {
    // The button only re-runs a change; for any other request it would end in
    // the plain fallback.
    if !matches!(
        state.session.previous_request,
        Some(ChatIntent::ChangeStructure { .. })
    ) {
        return Vec::new();
    }
    let Some(reading) = state.session.learning.as_ref() else {
        return Vec::new();
    };
    let mut areas: Vec<_> = reading
        .structure
        .areas
        .iter()
        .filter(|area| !area.folder.is_empty())
        .collect();
    areas.sort_by_key(|area| std::cmp::Reverse(area.documents));
    areas
        .into_iter()
        .take(AREA_CHIPS)
        .map(|area| {
            chip_labelled(
                area.folder.clone(),
                ChipReply::FollowUpArea(area.folder.clone()),
                false,
            )
        })
        .collect()
}
