//! A sentence that only refines the one before ("dann nur Bank"): what the
//! session keeps of the last request, and the answer when the model still
//! cannot place the sentence.

use ncrs_ai::ChatIntent;
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::State;
use crate::texts::text;
use crate::update::{cancel_chip, fallback_chips, say};

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
        | ChatIntent::OpenFolder { .. }
        | ChatIntent::SmallTalk => state.session.previous_message = None,
    }
}

/// Small talk or nothing right after a request is almost always a follow-up
/// the model could not place: Hermes asks again, with the areas as buttons.
pub fn answered_misread(state: &mut State, ctx: &Ctx, intent: &ChatIntent) -> bool {
    let answers_follow_up = match intent {
        ChatIntent::SmallTalk => state.session.previous_request.is_some(),
        ChatIntent::Other => {
            state.session.previous_request.is_some() || state.session.previous_message.is_some()
        }
        _ => false,
    };
    if !answers_follow_up {
        return false;
    }
    state.session.previous_message = None;
    let mut chips = fallback_chips(ctx);
    chips.push(cancel_chip(ctx));
    say(
        state,
        text(ctx.lang, Msg::HermesChatNotUnderstoodFollowUp),
        chips,
    );
    true
}
