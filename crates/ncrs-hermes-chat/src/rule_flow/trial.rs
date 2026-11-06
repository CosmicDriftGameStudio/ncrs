//! The trial of a proposed rule, the click that stores it and "file now".

use ncrs_filing::chat_work::{ChatWork, RuleSaveFailure, RuleTrial, RULE_TRIAL_EXAMPLES};
use ncrs_filing::rules_work::TryOutcome;
use ncrs_i18n::Msg;
use ncrs_index::{Combinator, Condition, Kinds, RuleSpec};
use ncrs_intake::filing_pieces::FilingTarget;
use ncrs_ui_kit::hermes_kit::{FactRow, MoveRow, MovesBlock};
use ncrs_ui_kit::kit::condition_kind_name;

use std::path::PathBuf;

use super::change::{more_moves, move_row};
use super::{ask_what, is_fixed, RuleFlow};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{ChipReply, Part, ProposalCard, RuleMoves, State, Step};
use crate::texts::{fill, number, rule_problem_text, segments, text};
use crate::tidy::archive_run;
use crate::update::{
    cancel_chip, chip, chip_labelled, fallback_chips, folder_answer, hermes, say, source_chips,
    work,
};

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
    let facts = vec![
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

/// The fixed words of the folder levels, "Work / Salary"; the folder when
/// the filing has none.
fn rule_name(flow: &RuleFlow) -> String {
    let below = flow.prefix.split('/').filter(|name| !name.is_empty());
    let fixed = flow
        .levels
        .folders
        .iter()
        .filter(|level| is_fixed(level))
        .map(|level| level.text());
    let words: Vec<String> = below.map(str::to_owned).chain(fixed).collect();
    if words.is_empty() {
        flow.folder.clone()
    } else {
        words.join(" / ")
    }
}

pub fn stored(state: &mut State, ctx: &Ctx, stored: Result<(), RuleSaveFailure>) {
    match stored {
        Ok(()) => {
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
            let said = fill(ctx.lang, Msg::HermesChatRuleStored, &[("name", &name)]);
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

/// "File now": the usual tidy-up of the source folders, with the archive
/// documents the stored rule renames or moves in the same run. When nothing
/// comes from the inbox, the run holds only those.
pub fn file_now(state: &mut State, ctx: &Ctx) -> Option<Action> {
    state.step = Step::TidyFolder;
    let archive_only = state
        .session
        .rule_moves
        .as_ref()
        .is_some_and(|moves| moves.from_inbox == 0 && !moves.archive.is_empty());
    if archive_only {
        if let Some(source) = ctx.sources.first() {
            return file_archive_only(state, ctx, source.clone());
        }
    }
    state.session.rule_moves_wait = true;
    match ctx.sources.as_slice() {
        [only] => folder_answer(state, ctx, only.clone(), false),
        _ => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatAskTidyFolder),
                source_chips(state, ctx),
            );
            None
        }
    }
}

fn file_archive_only(state: &mut State, ctx: &Ctx, source: PathBuf) -> Option<Action> {
    let moves = state.session.rule_moves.take()?;
    state.step = Step::Idle;
    archive_run(state, ctx, source, moves.archive);
    None
}
