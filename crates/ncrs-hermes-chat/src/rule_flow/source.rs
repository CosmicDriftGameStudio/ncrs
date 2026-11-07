//! Where the documents of a proposed rule come from.

use std::path::{Path, PathBuf};

use ncrs_i18n::Msg;
use ncrs_index::{Combinator, Condition, ConditionKind, RuleSpec};
use ncrs_ui_kit::hermes_kit::FactRow;

use super::propose;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, State};
use crate::texts::{fill, folder_name, text};
use crate::update::chip_labelled;

/// The read folder a proposed rule is limited to.
fn limited_to(spec: &RuleSpec) -> Option<&Path> {
    spec.conditions
        .list
        .iter()
        .find(|condition| condition.kind == ConditionKind::ReadFolder && !condition.negated)
        .map(|condition| Path::new(condition.value.trim()))
}

/// The row of the proposal that says which read folders the rule looks at.
/// Without read folders there is no row.
pub fn from_fact(ctx: &Ctx, spec: &RuleSpec) -> Option<FactRow> {
    let value = match (limited_to(spec), ctx.sources.as_slice()) {
        (Some(folder), _) => folder_name(ctx, folder),
        (None, []) => return None,
        (None, [only]) => folder_name(ctx, only),
        (None, _) => text(ctx.lang, Msg::HermesChatProposalFromAll),
    };
    Some(FactRow {
        label: text(ctx.lang, Msg::HermesChatProposalFrom),
        value,
    })
}

/// A rule whose conditions are alternatives cannot be narrowed by one more.
fn can_be_limited(spec: &RuleSpec) -> bool {
    spec.conditions.combinator == Combinator::AllOf || spec.conditions.list.len() <= 1
}

/// With several read folders: one chip per read folder to limit the rule to
/// it, or the chip that lifts the limit.
pub fn source_chips(ctx: &Ctx, spec: &RuleSpec) -> Vec<Chip> {
    if ctx.sources.len() < 2 || !can_be_limited(spec) {
        return Vec::new();
    }
    if limited_to(spec).is_some() {
        return vec![chip_labelled(
            text(ctx.lang, Msg::HermesChatRuleFromAllSources),
            ChipReply::FromAllSources,
            false,
        )];
    }
    ctx.sources
        .iter()
        .map(|source| {
            chip_labelled(
                fill(
                    ctx.lang,
                    Msg::HermesChatRuleOnlyFrom,
                    &[("folder", &folder_name(ctx, source))],
                ),
                ChipReply::OnlyFrom(source.clone()),
                false,
            )
        })
        .collect()
}

/// The click on a source chip: the limit is set, replaced or removed, and
/// the proposal is tried again.
pub fn limit_to_source(state: &mut State, ctx: &Ctx, source: Option<PathBuf>) {
    let Some(mut spec) = state
        .session
        .rule
        .as_ref()
        .and_then(|flow| flow.proposed.clone())
    else {
        return;
    };
    spec.conditions
        .list
        .retain(|condition| condition.kind != ConditionKind::ReadFolder);
    if let Some(folder) = source {
        // The planner compares with the canonical root of the location, a
        // configured path may still be spelled through a link.
        let resolved = (ctx.resolve_folder)(&folder).unwrap_or(folder);
        spec.conditions.combinator = Combinator::AllOf;
        spec.conditions.list.push(Condition::new(
            ConditionKind::ReadFolder,
            resolved.to_string_lossy(),
        ));
    }
    propose(state, ctx, spec);
}
