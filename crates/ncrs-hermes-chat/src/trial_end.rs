//! The end of a trial that files nothing: one message with the ways on.

use ncrs_ai::Schedule;
use ncrs_filing::chat_work::{TrashItem, TrialOutcome};
use ncrs_i18n::Msg;
use ncrs_index::Transfer;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, GroupFlow, Offer, Part, Proposal, State, Step, TrialSession};
use crate::texts::{fill, folder_name, number, text};
use crate::tidy;
use crate::tidy::{card_of, PAGE};
use crate::update::{access, cancel_chip, chip, chip_labelled, fallback_chips, hermes, say};

/// The files of the trial that are in the filing folder already.
fn duplicates(trial: &TrialSession) -> usize {
    trial
        .plan
        .entries
        .iter()
        .filter(|entry| matches!(entry.outcome, TrialOutcome::AlreadyThere { .. }))
        .count()
}

/// How far the AI is with the documents of the source, as the end of the
/// sentence; empty when it understood all.
fn ai_note(state: &State, ctx: &Ctx) -> String {
    let Some(reading) = state
        .session
        .tidy
        .as_ref()
        .and_then(|tidy| state.session.readings.get(&tidy.source))
    else {
        return String::new();
    };
    if reading.ai.is_complete() {
        return String::new();
    }
    fill(
        ctx.lang,
        Msg::HermesChatTrialStuckAi,
        &[
            ("done", &number(reading.ai.understood)),
            ("all", &number(reading.ai.with_text)),
        ],
    )
}

fn can_trash_duplicates(state: &State, ctx: &Ctx) -> bool {
    state
        .session
        .tidy
        .as_ref()
        .is_some_and(|tidy| tidy.transfer == Some(Transfer::Move))
        && access(ctx).is_some_and(|access| access.trash_can_restore())
}

fn ways_on(state: &State, ctx: &Ctx, rest: usize, dupes: usize) -> Vec<Chip> {
    let mut chips = vec![chip(ctx, Msg::HermesChatLaterAgain, ChipReply::Later, true)];
    if rest > 0 && !state.session.groups.singles_done {
        chips.push(chip_labelled(
            fill(
                ctx.lang,
                Msg::HermesChatShowStaying,
                &[("count", &number(rest))],
            ),
            ChipReply::ShowStaying,
            false,
        ));
    }
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::TrialOtherTarget,
        false,
    ));
    if dupes > 0 && can_trash_duplicates(state, ctx) {
        chips.push(chip_labelled(
            fill(
                ctx.lang,
                Msg::HermesChatDeleteDupes,
                &[("count", &number(dupes))],
            ),
            ChipReply::TrashDuplicatesNow,
            false,
        ));
    }
    chips.push(cancel_chip(ctx));
    chips
}

/// One message instead of a line about the files, one about the missing
/// sender and a question.
pub fn without_filing(state: &mut State, ctx: &Ctx, mut parts: Vec<Part>) {
    let Some(trial) = state.session.trial.as_ref() else {
        return;
    };
    let total = trial.plan.entries.len();
    let dupes = duplicates(trial);
    let rest = total.saturating_sub(dupes);
    let note = ai_note(state, ctx);
    let sentence = if dupes > 0 {
        fill(
            ctx.lang,
            Msg::HermesChatTrialStuckSome,
            &[
                ("total", &number(total)),
                ("dupes", &number(dupes)),
                ("rest", &number(rest)),
                ("ai_note", &note),
            ],
        )
    } else {
        fill(
            ctx.lang,
            Msg::HermesChatTrialStuckNone,
            &[("rest", &number(rest)), ("ai_note", &note)],
        )
    };
    parts.push(Part::Text(sentence));
    state.step = Step::Trial;
    let chips = ways_on(state, ctx, rest, dupes);
    hermes(state, parts, chips);
}

/// The files that stay, one by one.
pub fn show_staying(state: &mut State, ctx: &Ctx) {
    let Some(trial) = state.session.trial.as_ref() else {
        return;
    };
    let dupes = duplicates(trial);
    let staying: Vec<usize> = trial
        .plan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !matches!(entry.outcome, TrialOutcome::AlreadyThere { .. }))
        .map(|(position, _)| position)
        .collect();
    let rest = staying.len();
    let listed: Vec<usize> = staying.into_iter().take(PAGE).collect();
    let left = rest.saturating_sub(listed.len());
    let count = listed.len();
    let Some(card) = card_of(state, ctx, listed, left) else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatFallback),
            fallback_chips(ctx),
        );
        return;
    };
    state.session.groups.singles_done = true;
    let chips = ways_on(state, ctx, rest, dupes);
    hermes(
        state,
        vec![
            Part::Text(fill(
                ctx.lang,
                Msg::HermesChatStayingIntro,
                &[("count", &number(count))],
            )),
            Part::Trial(card),
        ],
        chips,
    );
}

/// The files that are in the filing folder already go to the trash, as one
/// run that can be undone.
pub fn trash_duplicates(state: &mut State, ctx: &Ctx) {
    let configured_target = state
        .session
        .tidy
        .as_ref()
        .and_then(|tidy| tidy.target.clone());
    let ready = state.session.trial.is_some() && can_trash_duplicates(state, ctx);
    let (Some(target), true) = (configured_target, ready) else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatFallback),
            fallback_chips(ctx),
        );
        return;
    };
    let (Some(trial), Some(tidy)) = (state.session.trial.take(), state.session.tidy.take()) else {
        return;
    };
    let items: Vec<TrashItem> = trial
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
    state.session.groups = GroupFlow::default();
    state.step = Step::AfterRun;
    state.session.offer = Some(Offer {
        name: fill(
            ctx.lang,
            Msg::HermesChatTrashRunName,
            &[("folder", &folder_name(ctx, &tidy.source))],
        ),
        root: tidy.source,
        items,
    });
    tidy::trash_offer(state, ctx, true);
}

/// The same files against another folder: the picked one is read, and the
/// trial runs again with what was decided before.
pub fn other_target(state: &mut State) -> Option<Action> {
    let tidy = state.session.tidy.as_ref()?;
    state.session.proposal = Some(Proposal {
        source: tidy.source.clone(),
        target: None,
        schedule: Some(Schedule::Once),
        transfer: tidy.transfer,
    });
    state.session.trial = None;
    state.session.groups = GroupFlow::default();
    state.step = Step::Target;
    Some(Action::PickFolder)
}
