//! The trial of a proposed rule, the click that stores it and "file now".

use ncrs_filing::chat_work::{ChatWork, RuleSaveFailure, RuleTrial};
use ncrs_filing::rules_work::TryOutcome;
use ncrs_i18n::Msg;
use ncrs_index::{Combinator, Condition, Kinds, RuleSpec};
use ncrs_intake::filing_pieces::FilingTarget;
use ncrs_ui_kit::kit::condition_kind_name;

use super::ask_what;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{ChipReply, Part, State, Step};
use crate::texts::{fill, number, rule_problem_text, segments, text};
use crate::update::{
    cancel_chip, chip, fallback_chips, folder_answer, hermes, say, source_chips, work,
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

fn example_line(ctx: &Ctx, document: &ncrs_filing::rules_work::TriedDocument) -> String {
    let after = match &document.after {
        TryOutcome::Filed(path) | TryOutcome::Lies(path) => segments(path).join("/"),
        TryOutcome::Review(_) => text(ctx.lang, Msg::HermesChatRuleExampleReview),
    };
    fill(
        ctx.lang,
        Msg::HermesChatRuleExample,
        &[
            ("from", &segments(&document.path).join("/")),
            ("to", &after),
        ],
    )
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
        ask_what(
            state,
            ctx,
            Some(text(ctx.lang, Msg::HermesChatRuleNoneMatch)),
        );
        return;
    }
    let kinds = flow.kinds.clone();
    let moves = trial.matches.saturating_sub(trial.in_archive);
    flow.trial_moves = moves;
    let sentence = fill(
        ctx.lang,
        Msg::HermesChatRuleProposal,
        &[
            ("count", &number(trial.matches)),
            ("conditions", &condition_words(ctx, &spec, &kinds)),
            (
                "pattern",
                &FilingTarget::parse(&flow.pattern()).in_words(ctx.lang),
            ),
        ],
    );
    let mut parts = vec![Part::Text(sentence)];
    let mut lines: Vec<String> = trial
        .examples
        .iter()
        .map(|document| example_line(ctx, document))
        .collect();
    if moves > trial.examples.len() {
        lines.push(fill(
            ctx.lang,
            Msg::HermesChatRuleExamplesMore,
            &[("count", &number(moves - trial.examples.len()))],
        ));
    }
    if trial.in_archive > 0 {
        lines.push(fill(
            ctx.lang,
            Msg::HermesChatRuleInArchive,
            &[
                ("in_archive", &number(trial.in_archive)),
                ("moves", &number(moves)),
            ],
        ));
    }
    if !lines.is_empty() {
        parts.push(Part::Text(lines.join("\n")));
    }
    let change = if flow.candidate.is_some() {
        ChipReply::LevelsChange
    } else {
        ChipReply::RuleEdit
    };
    let chips = vec![
        chip(ctx, Msg::HermesChatRuleCreate, ChipReply::RuleCreate, true),
        chip(ctx, Msg::HermesChatLevelsChange, change, false),
        cancel_chip(ctx),
    ];
    state.step = Step::RuleWhat;
    hermes(state, parts, chips);
}

/// The click on "Create rule".
pub fn create(state: &mut State) {
    let Some(spec) = state
        .session
        .rule
        .as_ref()
        .and_then(|flow| flow.proposed.clone())
    else {
        return;
    };
    work(state, ChatWork::SaveRule { spec }, None);
}

pub fn stored(state: &mut State, ctx: &Ctx, stored: Result<(), RuleSaveFailure>) {
    match stored {
        Ok(()) => {
            let something_moves = state
                .session
                .rule
                .as_ref()
                .is_some_and(|flow| flow.trial_moves > 0);
            state.session.rule = None;
            state.step = Step::Idle;
            let mut chips = Vec::new();
            if something_moves && !ctx.sources.is_empty() {
                chips.push(chip(
                    ctx,
                    Msg::HermesChatRuleFileNow,
                    ChipReply::RuleNow,
                    true,
                ));
            }
            chips.extend(fallback_chips(ctx));
            say(state, text(ctx.lang, Msg::HermesChatRuleStored), chips);
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

/// "File now": the usual tidy-up of the source folders.
pub fn file_now(state: &mut State, ctx: &Ctx) -> Option<Action> {
    state.step = Step::TidyFolder;
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
