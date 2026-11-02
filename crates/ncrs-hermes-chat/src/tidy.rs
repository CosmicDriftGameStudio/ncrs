//! Conversation 2: tidying a source folder into a target, with a trial first.

use std::path::PathBuf;
use std::sync::Arc;

use ncrs_filing::chat_work::{
    ChatWork, FolderReading, RunItem, RunPlan, RunReport, StayReason, TrashItem, TrashReport,
    TrialOutcome, TrialPlan,
};
use ncrs_i18n::Msg;
use ncrs_index::Transfer;

use crate::ctx::Ctx;
use crate::state::{
    Chip, ChipReply, Entry, Offer, Part, ReadFor, RowOutcome, State, Step, Tidy, TrialCard,
    TrialRow, TrialSession,
};
use crate::texts::{field_label, fill, folder_name, number, segments, text};
use crate::update::{access, ask_chips, cancel_chip, chip, chip_labelled, hermes, say, work};
use crate::{learn, texts};

const PAGE: usize = 10;

fn targets(state: &State, ctx: &Ctx) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for path in ctx.targets.iter().chain(&state.session.learned) {
        if !found.contains(path) {
            found.push(path.clone());
        }
    }
    found
}

pub fn begin(state: &mut State, ctx: &Ctx, source: PathBuf) {
    let is_target = targets(state, ctx)
        .iter()
        .any(|target| source.starts_with(target));
    if is_target {
        state.step = Step::Idle;
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatSourceIsTarget,
                &[("folder", &folder_name(ctx, &source))],
            ),
            Vec::new(),
        );
        return;
    }
    state.session.tidy = Some(Tidy {
        source: source.clone(),
        target: None,
        transfer: None,
    });
    state.step = Step::Idle;
    learn::read_for(state, ctx, source, ReadFor::Source);
}

pub fn source_read(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
    let folder = folder_name(ctx, &reading.root);
    if reading.files.is_empty() {
        state.session.tidy = None;
        state.step = Step::Idle;
        say(
            state,
            fill(ctx.lang, Msg::HermesChatReadEmpty, &[("folder", &folder)]),
            Vec::new(),
        );
        return;
    }
    let candidates = targets(state, ctx);
    if candidates.is_empty() {
        state.session.resume_tidy = true;
        state.session.read_for = Some(ReadFor::Learn);
        state.step = Step::ReadFolder;
        say(
            state,
            text(ctx.lang, Msg::HermesChatNoTarget),
            ask_chips(ctx),
        );
        return;
    }
    if let [only] = candidates.as_slice() {
        let target = only.clone();
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatSourceSummaryOneTarget,
                &[
                    ("folder", &folder),
                    ("count", &number(reading.files.len())),
                    ("target", &folder_name(ctx, &target)),
                ],
            ),
            Vec::new(),
        );
        choose_target(state, ctx, target);
        return;
    }
    state.step = Step::Target;
    let chips = candidates
        .into_iter()
        .map(|path| chip_labelled(folder_name(ctx, &path), ChipReply::Folder(path), false))
        .collect();
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatSourceSummary,
            &[("folder", &folder), ("count", &number(reading.files.len()))],
        ),
        chips,
    );
}

/// The target was just learned: go on with the tidy-up it interrupted.
pub fn resume(state: &mut State, ctx: &Ctx) {
    state.session.resume_tidy = false;
    let Some(target) = state.session.learned.last().cloned() else {
        return;
    };
    choose_target(state, ctx, target);
}

pub fn choose_target(state: &mut State, ctx: &Ctx, target: PathBuf) {
    let Some(tidy) = state.session.tidy.as_mut() else {
        return;
    };
    tidy.target = Some(target.clone());
    state.step = Step::Idle;
    learn::read_for(state, ctx, target, ReadFor::Target);
}

pub fn target_read(state: &mut State, ctx: &Ctx) {
    let Some(tidy) = state.session.tidy.as_ref() else {
        return;
    };
    let folder = folder_name(ctx, &tidy.source);
    state.step = Step::Once;
    say(
        state,
        fill(ctx.lang, Msg::HermesChatAskOnce, &[("folder", &folder)]),
        vec![
            chip(ctx, Msg::HermesChatOnce, ChipReply::Once, true),
            chip(ctx, Msg::HermesChatAlways, ChipReply::Always, false),
        ],
    );
}

pub fn once(state: &mut State, ctx: &Ctx, always: bool) {
    let Some(tidy) = state.session.tidy.as_ref() else {
        return;
    };
    let folder = folder_name(ctx, &tidy.source);
    let mut parts = Vec::new();
    if always {
        parts.push(Part::Text(text(ctx.lang, Msg::HermesChatAlwaysNotYet)));
    }
    parts.push(Part::Text(fill(
        ctx.lang,
        Msg::HermesChatAskMode,
        &[("folder", &folder)],
    )));
    state.step = Step::Mode;
    hermes(
        state,
        parts,
        vec![
            chip(ctx, Msg::HermesChatCopy, ChipReply::ModeCopy, false),
            chip(ctx, Msg::HermesChatMove, ChipReply::ModeMove, false),
        ],
    );
}

pub fn mode(state: &mut State, ctx: &Ctx, transfer: Transfer) {
    let Some(tidy) = state.session.tidy.as_mut() else {
        return;
    };
    tidy.transfer = Some(transfer);
    let (Some(source), Some(target)) = (
        state.session.readings.get(&tidy.source).cloned(),
        tidy.target
            .as_ref()
            .and_then(|target| state.session.readings.get(target).cloned()),
    ) else {
        crate::update::cancel(state, ctx);
        return;
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::Trial {
            source,
            target,
            changes: state.session.changes.clone(),
        },
        Some(text(ctx.lang, Msg::HermesChatWorkingTrial)),
    );
}

fn filed_count(trial: &TrialSession) -> usize {
    trial.plan.filed + trial.resolved.len()
}

fn trial_chips(ctx: &Ctx, trial: &TrialSession) -> Vec<Chip> {
    vec![
        chip_labelled(
            fill(
                ctx.lang,
                Msg::HermesChatFileAll,
                &[("count", &number(filed_count(trial)))],
            ),
            ChipReply::FileAll,
            true,
        ),
        chip(ctx, Msg::HermesChatShowMore, ChipReply::ShowMore, false),
        cancel_chip(ctx),
    ]
}

fn row_of(ctx: &Ctx, trial: &TrialSession, position: usize) -> Option<TrialRow> {
    let entry = trial.plan.entries.get(position)?;
    let outcome = match &entry.outcome {
        TrialOutcome::Filed { to } => RowOutcome::Filed {
            segments: segments(to),
        },
        TrialOutcome::Unsure {
            choices,
            kind_label,
        } => {
            let kind = kind_label
                .clone()
                .unwrap_or_else(|| text(ctx.lang, Msg::HermesChatTrialFile));
            let shown: Vec<String> = choices
                .iter()
                .map(|choice| choice.to_string_lossy().into_owned())
                .collect();
            let question = match shown.as_slice() {
                [first, second, ..] => fill(
                    ctx.lang,
                    Msg::HermesChatTrialUnsure,
                    &[("kind", &kind), ("first", first), ("second", second)],
                ),
                [first] => fill(
                    ctx.lang,
                    Msg::HermesChatTrialUnsureOne,
                    &[("kind", &kind), ("first", first)],
                ),
                [] => kind,
            };
            RowOutcome::Unsure {
                question,
                choices: shown
                    .into_iter()
                    .enumerate()
                    .map(|(choice, label)| {
                        chip_labelled(
                            label,
                            ChipReply::UnsureChoice {
                                entry: position,
                                choice,
                            },
                            false,
                        )
                    })
                    .collect(),
                chosen: trial
                    .resolved
                    .get(&position)
                    .and_then(|folder| choices.iter().position(|choice| choice == folder)),
            }
        }
        TrialOutcome::AlreadyThere { at } => RowOutcome::AlreadyThere(fill(
            ctx.lang,
            Msg::HermesChatTrialAlreadyThere,
            &[("path", &at.to_string_lossy())],
        )),
        TrialOutcome::Stays(reason) => {
            let described = match reason {
                StayReason::NoNeighbours => text(ctx.lang, Msg::HermesChatStayNoNeighbours),
                StayReason::MissingField(field) => fill(
                    ctx.lang,
                    Msg::HermesChatStayMissingField,
                    &[("field", field_label(ctx.lang, *field))],
                ),
                StayReason::NoPattern => text(ctx.lang, Msg::HermesChatStayNoPattern),
                StayReason::NameTaken => text(ctx.lang, Msg::HermesChatStayNameTaken),
            };
            RowOutcome::Stays(fill(
                ctx.lang,
                Msg::HermesChatTrialStays,
                &[("reason", &described)],
            ))
        }
    };
    Some(TrialRow {
        entry: position,
        name: entry.name.clone(),
        outcome,
    })
}

fn page_card(state: &State, ctx: &Ctx, from: usize, to: usize) -> Option<TrialCard> {
    let trial = state.session.trial.as_ref()?;
    let transfer = state.session.tidy.as_ref()?.transfer?;
    let rows: Vec<TrialRow> = (from..to).filter_map(|at| row_of(ctx, trial, at)).collect();
    let left = trial.plan.entries.len().saturating_sub(to);
    Some(TrialCard {
        header: text(
            ctx.lang,
            if transfer == Transfer::Copy {
                Msg::HermesChatTrialHeaderCopy
            } else {
                Msg::HermesChatTrialHeaderMove
            },
        ),
        rows,
        more: (left > 0).then(|| {
            fill(
                ctx.lang,
                Msg::HermesChatTrialMore,
                &[("count", &number(left))],
            )
        }),
    })
}

pub fn trial_done(state: &mut State, ctx: &Ctx, result: Result<TrialPlan, String>) {
    let plan = match result {
        Ok(plan) => plan,
        Err(reason) => {
            state.step = Step::Idle;
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                Vec::new(),
            );
            return;
        }
    };
    let total = plan.entries.len();
    let shown = total.min(PAGE);
    state.session.trial = Some(TrialSession {
        plan,
        resolved: Default::default(),
        shown,
    });
    state.step = Step::Trial;
    let Some(trial) = state.session.trial.as_ref() else {
        return;
    };
    if filed_count(trial) == 0 && trial.plan.unsure == 0 {
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatNothingToFile,
                &[("stays", &number(total))],
            ),
            vec![cancel_chip(ctx)],
        );
        return;
    }
    let chips = trial_chips(ctx, trial);
    let Some(card) = page_card(state, ctx, 0, shown) else {
        return;
    };
    hermes(
        state,
        vec![
            Part::Text(fill(
                ctx.lang,
                Msg::HermesChatTrialIntro,
                &[("count", &number(shown))],
            )),
            Part::Trial(card),
        ],
        chips,
    );
}

pub fn show_more(state: &mut State, ctx: &Ctx) {
    let Some(trial) = state.session.trial.as_ref() else {
        return;
    };
    let (shown, total) = (trial.shown, trial.plan.entries.len());
    let chips = trial_chips(ctx, trial);
    if shown >= total {
        say(state, text(ctx.lang, Msg::HermesChatTrialEnd), chips);
        return;
    }
    let to = total.min(shown + PAGE);
    let Some(card) = page_card(state, ctx, shown, to) else {
        return;
    };
    if let Some(running) = state.session.trial.as_mut() {
        running.shown = to;
    }
    hermes(
        state,
        vec![
            Part::Text(fill(
                ctx.lang,
                Msg::HermesChatTrialNext,
                &[("count", &number(to - shown))],
            )),
            Part::Trial(card),
        ],
        chips,
    );
}

pub fn unsure(state: &mut State, ctx: &Ctx, entry: usize, choice: usize) {
    let Some(trial) = state.session.trial.as_mut() else {
        return;
    };
    let Some(found) = trial.plan.entries.get(entry) else {
        return;
    };
    let TrialOutcome::Unsure { choices, .. } = &found.outcome else {
        return;
    };
    let Some(folder) = choices.get(choice).cloned() else {
        return;
    };
    let name = found.name.clone();
    trial.resolved.insert(entry, folder.clone());
    for thread_entry in &mut state.thread {
        let Entry::Hermes { parts, .. } = thread_entry else {
            continue;
        };
        for part in parts {
            let Part::Trial(card) = part else {
                continue;
            };
            for row in &mut card.rows {
                if let (true, RowOutcome::Unsure { chosen, .. }) =
                    (row.entry == entry, &mut row.outcome)
                {
                    *chosen = Some(choice);
                }
            }
        }
    }
    let chips = state
        .session
        .trial
        .as_ref()
        .map(|current| trial_chips(ctx, current))
        .unwrap_or_default();
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatUnsureChosen,
            &[("name", &name), ("path", &folder.to_string_lossy())],
        ),
        chips,
    );
}

pub fn file_all(state: &mut State, ctx: &Ctx) {
    let (Some(trial), Some(tidy)) = (state.session.trial.as_mut(), state.session.tidy.as_ref())
    else {
        return;
    };
    let (Some(transfer), Some(target)) = (tidy.transfer, tidy.target.clone()) else {
        return;
    };
    let mut items = Vec::new();
    let entries = trial.plan.entries.clone();
    for (position, entry) in entries.iter().enumerate() {
        let to = match &entry.outcome {
            TrialOutcome::Filed { to } => to.clone(),
            TrialOutcome::Unsure { .. } => {
                let Some(folder) = trial.resolved.get(&position) else {
                    continue;
                };
                let to = folder.join(&entry.name);
                if !trial.plan.claim(&to) {
                    continue;
                }
                to
            }
            _ => continue,
        };
        items.push(RunItem {
            from: entry.from.clone(),
            to,
            hash: entry.hash,
        });
    }
    let folder = folder_name(ctx, &tidy.source);
    let run = RunPlan {
        name: fill(ctx.lang, Msg::HermesChatRunName, &[("folder", &folder)]),
        transfer,
        source_root: tidy.source.clone(),
        target_root: target,
        items,
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::File { run },
        Some(text(ctx.lang, Msg::HermesChatWorkingFiling)),
    );
}

pub fn filed_done(state: &mut State, ctx: &Ctx, result: Result<RunReport, String>) {
    let (Some(tidy), trial) = (state.session.tidy.take(), state.session.trial.take()) else {
        return;
    };
    let report = match result {
        Ok(report) => report,
        Err(reason) => {
            state.step = Step::Idle;
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                Vec::new(),
            );
            return;
        }
    };
    let folder = folder_name(ctx, &tidy.source);
    let total = state
        .session
        .readings
        .get(&tidy.source)
        .map_or(0, |reading| reading.files.len());
    state.session.readings.clear();
    let mut parts = vec![Part::Text(fill(
        ctx.lang,
        Msg::HermesChatFiled,
        &[
            ("done", &number(report.done)),
            ("stays", &number(total.saturating_sub(report.done))),
            ("folder", &folder),
        ],
    ))];
    if report.skipped > 0 {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatFiledSkipped,
            &[("skipped", &number(report.skipped))],
        )));
    }
    let mut chips = Vec::new();
    let can_trash = access(ctx).is_some_and(|access| access.trash_can_restore());
    if let (Some(finished), Some(target), Some(Transfer::Move), true) =
        (trial, tidy.target.as_ref(), tidy.transfer, can_trash)
    {
        let items: Vec<TrashItem> = finished
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
        if !items.is_empty() {
            parts.push(Part::Text(fill(
                ctx.lang,
                Msg::HermesChatAlreadyThereOffer,
                &[
                    ("count", &number(items.len())),
                    ("folder", &folder),
                    ("target", &folder_name(ctx, target)),
                ],
            )));
            state.session.offer = Some(Offer {
                name: fill(
                    ctx.lang,
                    Msg::HermesChatTrashRunName,
                    &[("folder", &folder)],
                ),
                root: tidy.source.clone(),
                items,
            });
            chips.push(chip(ctx, Msg::HermesChatToTrash, ChipReply::ToTrash, true));
            chips.push(chip(
                ctx,
                Msg::HermesChatLeaveThem,
                ChipReply::LeaveThem,
                false,
            ));
        }
    }
    chips.push(chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false));
    state.step = Step::AfterRun;
    hermes(state, parts, chips);
}

pub fn trash_offer(state: &mut State, ctx: &Ctx, yes: bool) {
    let Some(offer) = state.session.offer.take() else {
        return;
    };
    if yes {
        work(
            state,
            ChatWork::TrashDuplicates {
                name: offer.name,
                root: offer.root,
                items: offer.items,
            },
            Some(text(ctx.lang, Msg::HermesChatWorkingTrash)),
        );
        return;
    }
    let chips = if state.step == Step::AfterRun {
        vec![chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false)]
    } else {
        Vec::new()
    };
    say(state, text(ctx.lang, Msg::HermesChatLeft), chips);
}

pub fn trashed_done(state: &mut State, ctx: &Ctx, result: Result<TrashReport, String>) {
    state.session.readings.clear();
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
    let mut parts = vec![Part::Text(fill(
        ctx.lang,
        Msg::HermesChatTrashed,
        &[("count", &texts::number(report.trashed))],
    ))];
    if report.skipped > 0 {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatTrashedSkipped,
            &[("skipped", &number(report.skipped))],
        )));
    }
    if state.step == Step::Duplicates {
        hermes(state, parts, Vec::new());
        learn::after_duplicates(state, ctx);
    } else {
        hermes(
            state,
            parts,
            vec![chip(ctx, Msg::HermesChatUndo, ChipReply::Undo, false)],
        );
    }
}
