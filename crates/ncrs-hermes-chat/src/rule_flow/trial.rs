//! The trial of a proposed rule, the click that stores it and "file now".

use ncrs_filing::chat_work::{ChatWork, RuleSaveFailure, RuleTrial, RULE_TRIAL_EXAMPLES};
use ncrs_filing::rules_work::TryOutcome;
use ncrs_i18n::Msg;
use ncrs_index::{Combinator, Condition, Kinds, RuleSaved, RuleSpec};
use ncrs_intake::filing_pieces::FilingTarget;
use ncrs_ui_kit::hermes_kit::{FactRow, MoveRow, MovesBlock};
use ncrs_ui_kit::kit::condition_kind_name;

use std::path::{Path, PathBuf};

use ncrs_filing::chat_work::TrialOutcome;

use super::change::{more_moves, move_row};
use super::{ask_what, RuleFlow};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::ReadFor;
use crate::state::{ChipReply, Part, ProposalCard, RuleMoves, RuleQueue, State, Step, Tidy};
use crate::texts::{fill, number, rule_problem_text, segments, text};
use crate::tidy::{archive_run, file_all};
use crate::update::{cancel_chip, chip, chip_labelled, fallback_chips, hermes, say, work};
use crate::{learn, trial_end};
use ncrs_index::Transfer;

fn condition_words(ctx: &Ctx, spec: &RuleSpec, kinds: &Kinds) -> String {
    let items: Vec<String> = spec
        .conditions
        .list
        .iter()
        .map(|condition: &Condition| {
            let value = match condition.kind {
                ncrs_index::ConditionKind::Kind => kinds.kind_word(&condition.value).into_owned(),
                _ => condition.value.clone(),
            };
            let item = format!(
                "{}: {value}",
                text(ctx.lang, condition_kind_name(condition.kind))
            );
            if condition.negated {
                fill(
                    ctx.lang,
                    Msg::HermesChatRuleConditionNot,
                    &[("condition", &item)],
                )
            } else {
                item
            }
        })
        .collect();
    if items.len() < 2 {
        return items.join("");
    }
    let msg = match spec.conditions.combinator {
        Combinator::AllOf => Msg::HermesChatRuleConditionAll,
        Combinator::AnyOf => Msg::HermesChatRuleConditionAny,
    };
    fill(ctx.lang, msg, &[("list", &items.join(", "))])
}

fn fact(ctx: &Ctx, label: Msg, value: String) -> FactRow {
    FactRow {
        label: text(ctx.lang, label),
        value,
    }
}

fn example_row(ctx: &Ctx, document: &ncrs_filing::rules_work::TriedDocument) -> MoveRow {
    let after = match &document.after {
        TryOutcome::Filed(path) | TryOutcome::Lies(path) => segments(path).join("/"),
        TryOutcome::Review(_) => text(ctx.lang, Msg::HermesChatRuleExampleReview),
    };
    move_row(ctx, &segments(&document.path).join("/"), after)
}

pub fn trial_done(state: &mut State, ctx: &Ctx, tried: Result<RuleTrial, String>) {
    let trial = match tried {
        Ok(trial) => trial,
        Err(reason) => {
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                fallback_chips(ctx),
            );
            return;
        }
    };
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    let Some(spec) = flow.proposed.clone() else {
        return;
    };
    if trial.matches == 0 {
        if flow.from_levels {
            let total = flow.files.len();
            let known =
                (trial.not_indexed > 0).then(|| (total.saturating_sub(trial.not_indexed), total));
            super::change::no_match(state, ctx, known);
            return;
        }
        ask_what(
            state,
            ctx,
            Some(text(ctx.lang, Msg::HermesChatRuleNoneMatch)),
        );
        return;
    }
    let kinds = flow.kinds.clone();
    let moved = trial.from_inbox + trial.archive_moves.len();
    flow.moves = RuleMoves {
        prefix: PathBuf::from(&flow.prefix),
        target: flow
            .target
            .clone()
            .or_else(|| ctx.targets.first().cloned())
            .unwrap_or_default(),
        from_inbox: trial.from_inbox,
        archive: trial.archive_moves.clone(),
    };
    let pattern = FilingTarget::parse(&flow.pattern());
    let condition = condition_words(ctx, &spec, &kinds);
    let applies_to = match flow.shared {
        Some(shared) if shared.count == shared.total => fill(
            ctx.lang,
            Msg::HermesChatProposalAppliesSharedAll,
            &[("condition", &condition), ("count", &number(shared.count))],
        ),
        Some(shared) => fill(
            ctx.lang,
            Msg::HermesChatProposalAppliesSharedSome,
            &[
                ("condition", &condition),
                ("count", &number(shared.count)),
                ("total", &number(shared.total)),
            ],
        ),
        None => condition,
    };
    let mut facts = vec![
        fact(ctx, Msg::HermesChatProposalAppliesTo, applies_to),
        fact(
            ctx,
            Msg::HermesChatProposalFiling,
            pattern.folders_in_words(ctx.lang),
        ),
        fact(
            ctx,
            Msg::HermesChatProposalFileName,
            pattern.file_name_in_words(ctx.lang),
        ),
        fact(
            ctx,
            Msg::HermesChatProposalMatchesLabel,
            fill(
                ctx.lang,
                Msg::HermesChatProposalMatches,
                &[("count", &number(trial.matches))],
            ),
        ),
    ];
    if let Some(from) = super::source::from_fact(ctx) {
        facts.insert(1, from);
    }
    let rows: Vec<MoveRow> = trial
        .examples
        .iter()
        .map(|document| example_row(ctx, document))
        .chain(trial.archive_moves.iter().map(|found| {
            move_row(
                ctx,
                &segments(&found.from).join("/"),
                segments(&found.to).join("/"),
            )
        }))
        .take(RULE_TRIAL_EXAMPLES)
        .collect();
    let moves = (!rows.is_empty()).then(|| MovesBlock {
        header: text(ctx.lang, Msg::HermesChatProposalExamples),
        more: more_moves(ctx, moved.saturating_sub(rows.len())),
        rows,
    });
    let counts = (trial.right > 0 || !trial.archive_moves.is_empty()).then(|| {
        fill(
            ctx.lang,
            Msg::HermesChatRuleTrialCounts,
            &[
                ("right", &number(trial.right)),
                ("moves", &number(moved)),
                ("inbox", &number(trial.from_inbox)),
            ],
        )
    });
    let parts = vec![Part::Proposal(ProposalCard {
        title: text(ctx.lang, Msg::HermesChatProposalTitle),
        facts,
        moves,
        counts,
    })];
    let mut chips = vec![
        chip(ctx, Msg::HermesChatRuleCreate, ChipReply::RuleCreate, true),
        chip(
            ctx,
            Msg::HermesChatRuleChangeCondition,
            ChipReply::RuleCondition,
            false,
        ),
    ];
    chips.push(chip(
        ctx,
        Msg::HermesChatLevelsChange,
        ChipReply::LevelsChange,
        false,
    ));
    chips.push(cancel_chip(ctx));
    state.step = Step::RuleWhat;
    hermes(state, parts, chips);
}

/// The click on "Create rule".
pub fn create(state: &mut State) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    let name = rule_name(flow);
    let Some(proposed) = flow.proposed.as_mut() else {
        return;
    };
    proposed.name = Some(name);
    let spec = proposed.clone();
    work(state, ChatWork::SaveRule { spec }, None);
}

/// The path of the opened folder below its target, "Archive / Work"; the
/// folder itself at the target root. The rule belongs to the folder, so
/// changing a level word keeps the name and two folders stay apart. With
/// several targets the target's name leads; a rule with nothing to name it
/// by is named after its filing pattern, which is never empty.
///
pub fn rule_name(flow: &RuleFlow) -> String {
    let words: Vec<&str> = flow
        .prefix
        .split('/')
        .filter(|name| !name.is_empty())
        .collect();
    let name = match (&flow.target_name, words.is_empty()) {
        (Some(target), true) => target.clone(),
        (Some(target), false) => format!("{target} / {}", words.join(" / ")),
        (None, true) => flow.folder.clone(),
        (None, false) => words.join(" / "),
    };
    if name.is_empty() {
        flow.pattern()
    } else {
        name
    }
}

pub fn stored(state: &mut State, ctx: &Ctx, stored: Result<RuleSaved, RuleSaveFailure>) {
    match stored {
        Ok(saved) => {
            let finished = state.session.rule.take();
            let name = finished
                .as_ref()
                .and_then(|open| open.proposed.as_ref())
                .and_then(|proposed| proposed.name.clone())
                .unwrap_or_default();
            let moves = finished.map(|open| open.moves).unwrap_or_default();
            let count = moves.total();
            state.session.rule_moves = Some(moves);
            state.step = Step::Idle;
            let message = match saved {
                RuleSaved::Added => Msg::HermesChatRuleStored,
                RuleSaved::Changed => Msg::HermesChatRuleUpdated,
            };
            let said = fill(ctx.lang, message, &[("name", &name)]);
            if count == 0 || ctx.sources.is_empty() {
                say(state, said, fallback_chips(ctx));
                return;
            }
            say(state, said, Vec::new());
            let question = if count == 1 {
                Msg::HermesChatRuleMoveQuestionOne
            } else {
                Msg::HermesChatRuleMoveQuestionOther
            };
            let chips = vec![
                chip_labelled(
                    fill(
                        ctx.lang,
                        Msg::HermesChatRuleRearrangeNow,
                        &[("count", &number(count))],
                    ),
                    ChipReply::RuleNow,
                    true,
                ),
                chip(
                    ctx,
                    Msg::HermesChatRuleOnlyNew,
                    ChipReply::RuleOnlyNew,
                    false,
                ),
            ];
            say(
                state,
                fill(ctx.lang, question, &[("count", &number(count))]),
                chips,
            );
        }
        Err(RuleSaveFailure::Refused(problem)) => {
            let reason = fill(
                ctx.lang,
                Msg::HermesChatRuleRefused,
                &[("reason", &rule_problem_text(ctx.lang, &problem))],
            );
            ask_what(state, ctx, Some(reason));
        }
        Err(RuleSaveFailure::Failed(reason)) => say(
            state,
            fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
            fallback_chips(ctx),
        ),
    }
}

/// "No, only new ones": nothing moves.
pub fn only_new(state: &mut State, ctx: &Ctx) {
    state.session.rule_moves = None;
    state.step = Step::Idle;
    say(
        state,
        text(ctx.lang, Msg::HermesChatRuleOnlyNewDone),
        fallback_chips(ctx),
    );
}

/// "Rearrange now" is the confirmation: the documents of the inbox go into the
/// rule's target at once, moved, once, with the archive documents the stored
/// rule renames in the same run. With several sources one run follows the
/// other; when nothing comes from the inbox, the run holds only the archive.
pub fn file_now(state: &mut State, ctx: &Ctx) -> Option<Action> {
    state.session.rule_queue = None;
    let archive_only = state
        .session
        .rule_moves
        .as_ref()
        .is_some_and(|moves| moves.from_inbox == 0 && !moves.archive.is_empty());
    if archive_only {
        let moves = state.session.rule_moves.take()?;
        state.step = Step::Idle;
        let source = ctx
            .sources
            .first()
            .cloned()
            .unwrap_or_else(|| moves.target.clone());
        archive_run(state, ctx, source, moves.archive);
        return None;
    }
    let mut sources = ctx.sources.iter().cloned();
    let first = sources.next()?;
    let moves = state.session.rule_moves.take()?;
    state.step = Step::Idle;
    state.session.rule_queue = Some(RuleQueue {
        target: moves.target.clone(),
        prefix: moves.prefix.clone(),
        sources: sources.collect(),
    });
    start_source(
        state,
        ctx,
        first,
        (moves.target, moves.prefix),
        moves.archive,
    );
    None
}

fn start_source(
    state: &mut State,
    ctx: &Ctx,
    source: PathBuf,
    (target, prefix): (PathBuf, PathBuf),
    inside: Vec<ncrs_filing::rules_work::ArchiveMove>,
) {
    state.session.tidy = Some(Tidy {
        rule_now: Some(prefix),
        source: source.clone(),
        target: Some(target),
        transfer: Some(Transfer::Move),
        inside,
    });
    state.session.layout = None;
    state.session.deciding = None;
    state.step = Step::Idle;
    learn::read_for(state, ctx, source, ReadFor::Source);
}

/// The next source of "Rearrange now"; `false` when none is left.
pub fn next_source(
    state: &mut State,
    ctx: &Ctx,
    inside: Vec<ncrs_filing::rules_work::ArchiveMove>,
) -> bool {
    let Some(queue) = state.session.rule_queue.as_mut() else {
        return false;
    };
    if queue.sources.is_empty() {
        state.session.rule_queue = None;
        return false;
    }
    let source = queue.sources.remove(0);
    let at = (queue.target.clone(), queue.prefix.clone());
    start_source(state, ctx, source, at, inside);
    true
}

/// A source of "Rearrange now" holds no files: its archive moves go on with
/// the next source, or run alone when none is left. `false` when there is
/// nothing to do for this source.
pub fn empty_source(state: &mut State, ctx: &Ctx) -> bool {
    let Some(tidy) = state
        .session
        .tidy
        .as_ref()
        .filter(|tidy| tidy.rule_now.is_some())
    else {
        return false;
    };
    let (source, inside) = (tidy.source.clone(), tidy.inside.clone());
    if next_source(state, ctx, inside.clone()) {
        return true;
    }
    if inside.is_empty() {
        return false;
    }
    archive_run(state, ctx, source, inside);
    true
}

/// Whether "Rearrange now" files this entry of the tidy trial: only what
/// lands below the rule's own folder; unsure ones stay. Without a prefix (a
/// normal tidy-up) everything goes through.
/// ponytail: the trial knows no rule names, so another rule filing below the
/// same prefix would ride along. A rule made in the target root itself has an
/// empty prefix, which keeps everything the trial files.
pub fn rule_keeps(prefix: Option<&Path>, outcome: &TrialOutcome) -> bool {
    match (prefix, outcome) {
        (None, _) => true,
        (Some(folder), TrialOutcome::Filed { to }) => to.starts_with(folder),
        (Some(_), _) => false,
    }
}

/// The tidy trial of a "Rearrange now" source is back: file what it can
/// file, no second look. What is unsure or stays, stays where it is.
pub fn file_rule_trial(state: &mut State, ctx: &Ctx) {
    let prefix = state
        .session
        .tidy
        .as_ref()
        .and_then(|tidy| tidy.rule_now.as_deref());
    let nothing_to_file = state.session.trial.as_ref().is_none_or(|trial| {
        !trial.plan.entries.iter().any(|entry| {
            matches!(entry.outcome, TrialOutcome::Filed { .. })
                && rule_keeps(prefix, &entry.outcome)
        })
    }) && state
        .session
        .tidy
        .as_ref()
        .is_none_or(|tidy| tidy.inside.is_empty());
    if nothing_to_file {
        if next_source(state, ctx, Vec::new()) {
            return;
        }
        trial_end::without_filing(state, ctx, Vec::new());
        return;
    }
    file_all(state, ctx);
}
