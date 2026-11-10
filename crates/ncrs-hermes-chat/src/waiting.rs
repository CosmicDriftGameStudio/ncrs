//! The documents no rule could place: Hermes says how many wait, and walks
//! through them group by group, one question each.

use std::collections::VecDeque;
use std::path::PathBuf;

use ncrs_filing::chat_work::{ChatWork, WaitingGroup};
use ncrs_filing::review_work::AcceptSummary;
use ncrs_i18n::{Language, Msg};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Part, State, Step, Walk};
use crate::texts::{fill, number, segments, text};
use crate::update::{
    access, cancel_chip, chip, chip_labelled, fallback_chips, hermes, say, text_with, work,
};

/// The seconds between two looks for new waiting documents in an open chat.
const CHECK_EVERY_SECONDS: i64 = 30;
const NAMES_SHOWN: usize = 3;

pub fn has_review(ctx: &Ctx) -> bool {
    access(ctx).is_some_and(ncrs_filing::chat_work::ChatAccess::has_review)
}

pub fn count_before_greeting(state: &mut State) {
    state.session.greeting_after_count = true;
    work(state, ChatWork::WaitingCount, None);
}

pub fn opener_text(ctx: &Ctx, count: usize) -> String {
    if count == 1 {
        text(ctx.lang, Msg::HermesChatWaitingOne)
    } else {
        text_with(
            ctx,
            Msg::HermesChatWaitingMany,
            &[("count", &number(count))],
        )
    }
}

fn show_chip(ctx: &Ctx) -> Chip {
    chip(
        ctx,
        Msg::HermesChatWaitingShow,
        ChipReply::WaitingShow,
        true,
    )
}

/// The greeting, with the note about waiting documents when there are any.
pub fn greeting_entry(state: &mut State, ctx: &Ctx, count: usize) {
    let mut parts = vec![Part::Text(text(ctx.lang, Msg::HermesChatGreeting))];
    let mut chips = Vec::new();
    if count > 0 {
        parts.push(Part::Text(opener_text(ctx, count)));
        chips.push(show_chip(ctx));
        state.session.waiting_offered = count;
    }
    chips.extend(crate::greeting::chips(state, ctx));
    hermes(state, parts, chips);
}

fn is_idle(state: &State) -> bool {
    state.open
        && matches!(state.step, Step::Idle | Step::Greeting)
        && state.session.waiting.is_none()
        && !state.session.greeting_after_count
        && !state.waiting_for_hermes
        && state.pending.is_none()
        && state.thinking.is_none()
        && state.resolving.is_none()
        && state.naming.is_none()
        && state.chips_owner_is_hermes()
}

/// The app sends this now and then while the chat is open: new documents may
/// have arrived.
pub fn check(state: &mut State, ctx: &Ctx) {
    if !has_review(ctx) || !is_idle(state) {
        return;
    }
    let due = state.session.last_check.is_none_or(|last| {
        ctx.now.signed_duration_since(last).num_seconds() >= CHECK_EVERY_SECONDS
    });
    if due {
        state.session.last_check = Some(ctx.now);
        work(state, ChatWork::WaitingCount, None);
    }
}

pub fn counted(state: &mut State, ctx: &Ctx, result: Result<usize, String>) {
    let count = result.unwrap_or(0);
    if std::mem::take(&mut state.session.greeting_after_count) {
        greeting_entry(state, ctx, count);
        return;
    }
    state.session.waiting_offered = state.session.waiting_offered.min(count);
    if count > state.session.waiting_offered && is_idle(state) {
        state.session.waiting_offered = count;
        let mut chips = vec![show_chip(ctx)];
        chips.extend(if state.step == Step::Greeting {
            crate::greeting::chips(state, ctx)
        } else {
            fallback_chips(ctx)
        });
        hermes(state, vec![Part::Text(opener_text(ctx, count))], chips);
    }
}

pub fn show(state: &mut State) {
    work(state, ChatWork::WaitingLoad, None);
}

pub fn loaded(state: &mut State, ctx: &Ctx, result: Result<Vec<WaitingGroup>, String>) {
    let groups = match result {
        Ok(groups) if !groups.is_empty() => groups,
        Ok(_) => {
            state.session.waiting_offered = 0;
            say(
                state,
                text(ctx.lang, Msg::HermesChatWaitingNone),
                fallback_chips(ctx),
            );
            return;
        }
        Err(reason) => {
            say(
                state,
                text_with(ctx, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                fallback_chips(ctx),
            );
            return;
        }
    };
    let total = groups.iter().map(|group| group.items.len()).sum();
    state.session.waiting = Some(Walk {
        groups: VecDeque::from(groups),
        current: None,
        total,
        filed: 0,
    });
    next(state, ctx, None);
}

pub fn file_current(state: &mut State, ctx: &Ctx) {
    let Some(group) = state
        .session
        .waiting
        .as_mut()
        .and_then(|walk| walk.current.take())
    else {
        return;
    };
    work(
        state,
        ChatWork::WaitingFile { items: group.items },
        Some(text(ctx.lang, Msg::HermesChatWorkingWaiting)),
    );
}

pub fn other_folder(state: &mut State) -> Option<Action> {
    state.step = Step::WaitingFolder;
    Some(Action::PickFolder)
}

/// The folder dialog was closed: the document in question is asked about
/// again. `false` when no walk is running.
pub fn ask_again(state: &mut State, ctx: &Ctx) -> bool {
    let Some((parts, chips)) = state
        .session
        .waiting
        .as_ref()
        .and_then(|walk| walk.current.as_ref())
        .and_then(|group| question(ctx, group))
    else {
        return false;
    };
    state.step = Step::Idle;
    hermes(state, parts, chips);
    true
}

/// Files the document in question in `folder`: below the target when relative.
pub fn file_into(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    let Some(group) = state
        .session
        .waiting
        .as_mut()
        .and_then(|walk| walk.current.take())
    else {
        return;
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::WaitingFileInto {
            items: group.items,
            folder,
        },
        Some(text(ctx.lang, Msg::HermesChatWorkingWaiting)),
    );
}

pub fn skip(state: &mut State, ctx: &Ctx) {
    if let Some(walk) = state.session.waiting.as_mut() {
        walk.current = None;
    }
    next(state, ctx, None);
}

pub fn filed(state: &mut State, ctx: &Ctx, result: Result<AcceptSummary, String>) {
    let note = match result {
        Ok(summary) => {
            if let Some(walk) = state.session.waiting.as_mut() {
                walk.filed += summary.filed;
            }
            unfiled_note(ctx.lang, &summary)
        }
        Err(reason) => Some(text_with(
            ctx,
            Msg::HermesChatReadFailed,
            &[("reason", &reason)],
        )),
    };
    next(state, ctx, note);
}

fn next(state: &mut State, ctx: &Ctx, note: Option<String>) {
    let Some(walk) = state.session.waiting.as_mut() else {
        return;
    };
    let (group, (mut parts, chips)) = loop {
        let Some(group) = walk.groups.pop_front() else {
            let (filed, left) = (walk.filed, walk.total - walk.filed);
            state.session.waiting = None;
            state.session.waiting_offered = left;
            state.step = Step::Idle;
            let mut parts: Vec<Part> = note.into_iter().map(Part::Text).collect();
            parts.push(Part::Text(text_with(
                ctx,
                Msg::HermesChatWaitingDone,
                &[("filed", &number(filed)), ("left", &number(left))],
            )));
            hermes(state, parts, fallback_chips(ctx));
            return;
        };
        if let Some(asked) = question(ctx, &group) {
            break (group, asked);
        }
    };
    if let Some(failure) = note {
        parts.insert(0, Part::Text(failure));
    }
    walk.current = Some(group);
    hermes(state, parts, chips);
}

/// `None` for a group with nothing in it: there is nothing to ask.
fn question(ctx: &Ctx, group: &WaitingGroup) -> Option<(Vec<Part>, Vec<Chip>)> {
    let skip = chip(
        ctx,
        Msg::HermesChatWaitingSkip,
        ChipReply::WaitingSkip,
        false,
    );
    let first = group.items.first()?;
    Some(match (&group.folder, first) {
        (Some(folder), _) => {
            let names: Vec<String> = group
                .items
                .iter()
                .take(NAMES_SHOWN)
                .filter_map(|item| item.path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            let more = if group.items.len() > NAMES_SHOWN {
                " …"
            } else {
                ""
            };
            let sentence = fill(
                ctx.lang,
                Msg::HermesChatWaitingGroup,
                &[
                    ("count", &number(group.items.len())),
                    ("folder", &segments(folder).join(" / ")),
                    ("names", &format!("{}{more}", names.join(", "))),
                ],
            );
            (
                vec![Part::Text(sentence)],
                vec![
                    chip(
                        ctx,
                        Msg::HermesChatWaitingFile,
                        ChipReply::WaitingFile,
                        true,
                    ),
                    skip,
                    cancel_chip(ctx),
                ],
            )
        }
        (None, item) => {
            let name = item
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut parts = vec![Part::Text(fill(
                ctx.lang,
                Msg::HermesChatWaitingUnsure,
                &[("name", &name)],
            ))];
            let mut chips = Vec::new();
            if let Some(placement) = &item.placement {
                let folder = segments(&placement.folder).join(" / ");
                parts.push(Part::Text(fill(
                    ctx.lang,
                    Msg::HermesChatWaitingUnsureNear,
                    &[("folder", &folder)],
                )));
                chips.push(chip_labelled(
                    fill(
                        ctx.lang,
                        Msg::HermesChatWaitingFileInto,
                        &[("folder", &folder)],
                    ),
                    ChipReply::WaitingFileInto(placement.folder.clone()),
                    true,
                ));
            }
            chips.push(chip(
                ctx,
                Msg::HermesChatOtherFolder,
                ChipReply::WaitingOtherFolder,
                false,
            ));
            chips.push(skip);
            chips.push(cancel_chip(ctx));
            (parts, chips)
        }
    })
}

/// What Hermes says about the documents that were not filed: the failures with
/// their first reason, or else which of them found the place taken or were read
/// again. `None` when all were filed.
fn unfiled_note(lang: Language, summary: &AcceptSummary) -> Option<String> {
    if summary.failed > 0 {
        return Some(fill(
            lang,
            Msg::HermesChatWaitingFailed,
            &[
                ("count", &number(summary.failed)),
                (
                    "reason",
                    summary.first_failure.as_deref().unwrap_or_default(),
                ),
            ],
        ));
    }
    (summary.target_exists + summary.requeued > 0).then(|| {
        fill(
            lang,
            Msg::StatusQuickFixSummary,
            &[
                ("filed", &number(summary.filed)),
                ("taken", &number(summary.target_exists)),
                ("requeued", &number(summary.requeued)),
                ("failed", &number(summary.failed)),
            ],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_place_is_named_and_not_reported_as_a_failure_without_reason() {
        let summary = AcceptSummary {
            target_exists: 1,
            ..AcceptSummary::default()
        };
        assert_eq!(
            unfiled_note(Language::English, &summary).as_deref(),
            Some("0 filed, 1 place taken, 0 read again, 0 failed")
        );
    }

    #[test]
    fn failures_come_with_their_reason() {
        let summary = AcceptSummary {
            failed: 2,
            target_exists: 1,
            first_failure: Some("disk full".to_owned()),
            ..AcceptSummary::default()
        };
        assert_eq!(
            unfiled_note(Language::English, &summary).as_deref(),
            Some("2 could not be filed: disk full")
        );
    }

    #[test]
    fn nothing_is_said_when_all_were_filed() {
        let summary = AcceptSummary {
            filed: 3,
            ..AcceptSummary::default()
        };
        assert_eq!(unfiled_note(Language::English, &summary), None);
    }
}
