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

/// The walk through exactly these documents: those that would go to the same
/// folder are one question, the others without a folder are one question per
/// sender, and a document with neither is asked about alone.
pub fn ask_about(state: &mut State, ctx: &Ctx, items: Vec<ncrs_filing::review_work::ReviewItem>) {
    state.step = Step::Idle;
    let total = items.len();
    state.session.waiting = Some(Walk {
        groups: grouped_for_questions(items).into(),
        current: None,
        total,
        filed: 0,
    });
    next(state, ctx, None);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SameThing {
    Folder(PathBuf),
    Sender(String),
}

fn same_thing_of(item: &ncrs_filing::review_work::ReviewItem) -> Option<SameThing> {
    match (&item.placement, &item.fields.sender) {
        (Some(placement), _) => Some(SameThing::Folder(placement.folder.clone())),
        (None, Some(sender)) => Some(SameThing::Sender(sender.clone())),
        (None, None) => None,
    }
}

fn grouped_for_questions(items: Vec<ncrs_filing::review_work::ReviewItem>) -> Vec<WaitingGroup> {
    let mut grouped: Vec<(Option<SameThing>, Vec<ncrs_filing::review_work::ReviewItem>)> =
        Vec::new();
    for item in items {
        let same = same_thing_of(&item);
        match grouped
            .iter_mut()
            .find(|(known, _)| same.is_some() && *known == same)
        {
            Some((_, members)) => members.push(item),
            None => grouped.push((same, vec![item])),
        }
    }
    grouped
        .into_iter()
        .map(|(_, items)| WaitingGroup {
            folder: None,
            items,
        })
        .collect()
}

/// The folder all documents of an unsure group would go to, when they agree.
fn shared_placement(group: &WaitingGroup) -> Option<&std::path::Path> {
    let first = group.items.first()?.placement.as_ref()?;
    group
        .items
        .iter()
        .all(|item| {
            item.placement
                .as_ref()
                .is_some_and(|placement| placement.folder == first.folder)
        })
        .then_some(first.folder.as_path())
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

/// The first file names of the group, with an ellipsis when there are more.
fn file_names(group: &WaitingGroup) -> String {
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
    format!("{}{more}", names.join(", "))
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
            let sentence = fill(
                ctx.lang,
                Msg::HermesChatWaitingGroup,
                &[
                    ("count", &number(group.items.len())),
                    ("folder", &segments(folder).join(" / ")),
                    ("names", &file_names(group)),
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
        (None, _) => {
            let many = group.items.len() > 1;
            let names = file_names(group);
            let mut parts = vec![Part::Text(if many {
                fill(
                    ctx.lang,
                    Msg::HermesChatWaitingUnsureMany,
                    &[("count", &number(group.items.len())), ("names", &names)],
                )
            } else {
                fill(ctx.lang, Msg::HermesChatWaitingUnsure, &[("name", &names)])
            })];
            let mut chips = Vec::new();
            if let Some(placed) = shared_placement(group) {
                let folder = segments(placed).join(" / ");
                parts.push(Part::Text(fill(
                    ctx.lang,
                    Msg::HermesChatWaitingUnsureNear,
                    &[("folder", &folder)],
                )));
                let label = if many {
                    Msg::HermesChatWaitingFileIntoMany
                } else {
                    Msg::HermesChatWaitingFileInto
                };
                chips.push(chip_labelled(
                    fill(ctx.lang, label, &[("folder", &folder)]),
                    ChipReply::WaitingFileInto(placed.to_path_buf()),
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
                    &summary
                        .first_failure
                        .as_ref()
                        .map(|failure| failure.text(lang))
                        .unwrap_or_default(),
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
            first_failure: Some(ncrs_filing::review_work::NotMoved::other("disk full")),
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

#[cfg(test)]
mod group_tests {
    use chrono::{TimeZone as _, Utc};
    use ncrs_filing::review_work::{ReviewItem, TextState};
    use ncrs_index::{
        ContentHash, DocumentFields, DocumentSource, FieldOrigins, Index, NewDocument, ReviewReason,
    };
    use ncrs_intake::{Placement, Signal};

    use super::*;

    fn items_of(shapes: &[(Option<&str>, Option<&str>)]) -> Vec<ReviewItem> {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        (1..)
            .zip(shapes)
            .map(|(seed, (sender, folder))| {
                let document = index
                    .insert_document(&NewDocument {
                        hash: ContentHash([seed; 32]),
                        path: dir.path().join(format!("scan-{seed}.pdf")),
                        source: DocumentSource::Inbox,
                        received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                        stamp: None,
                    })
                    .unwrap();
                let reason = ReviewReason::NoMatchingRule;
                ReviewItem {
                    entry: index.add_review(document, reason).unwrap(),
                    document_id: document,
                    path: PathBuf::from(format!("inbox/scan-{seed}.pdf")),
                    reason,
                    text_state: TextState::Read,
                    fields: DocumentFields {
                        sender: sender.map(str::to_owned),
                        ..DocumentFields::default()
                    },
                    proposed_pattern: None,
                    origins: FieldOrigins::default(),
                    passages: Vec::new(),
                    placement: folder.map(|folder| Placement {
                        folder: PathBuf::from(folder),
                        signal: Signal::SameSender,
                        agreeing: 3,
                        similar: 3,
                        pattern: None,
                        missing: None,
                        runner_up: None,
                        candidates: Vec::new(),
                    }),
                }
            })
            .collect()
    }

    #[test]
    fn documents_with_one_folder_or_one_sender_are_asked_about_together() {
        let mut shapes = vec![(Some("Company A"), Some("Insurance")); 5];
        shapes.extend([(Some("Company B"), None); 3]);
        shapes.push((None, None));

        let groups = grouped_for_questions(items_of(&shapes));

        let sizes: Vec<usize> = groups.iter().map(|group| group.items.len()).collect();
        assert_eq!(sizes, [5, 3, 1]);
        assert_eq!(
            shared_placement(&groups[0]),
            Some(std::path::Path::new("Insurance"))
        );
        assert_eq!(shared_placement(&groups[1]), None);
    }
}
