//! The questions about the files that stay in the trial: one per group of
//! files from the same sender or of the same kind, then the single pieces,
//! then the files whose name is taken. An answer to a group becomes a rule.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use ncrs_filing::chat_work::{ChatWork, GroupKey, RuleSaveFailure, StayGroup, TrialOutcome};
use ncrs_i18n::Msg;
use ncrs_intake::is_windows_device_name;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::state::{Chip, ChipReply, Part, State, Step};
use crate::texts::{fill, number, rule_problem_text, text};
use crate::tidy::{card_of, filed_count, trial_chips, PAGE};
use crate::update::{cancel_chip, chip, chip_labelled, hermes, say, work};

/// Whether nothing is open but the single pieces, which the closing message
/// of the trial offers to list.
pub fn only_singles_open(state: &State) -> bool {
    let Some(trial) = state.session.trial.as_ref() else {
        return false;
    };
    let groups = &trial.plan.groups;
    groups.asked.is_empty() && (groups.name_taken.is_empty() || state.session.groups.names_done)
}

/// Whether the trial still has something to ask about the staying files.
pub fn has_open_questions(state: &State) -> bool {
    let Some(trial) = state.session.trial.as_ref() else {
        return false;
    };
    let flow = &state.session.groups;
    let groups = &trial.plan.groups;
    !groups.asked.is_empty()
        || (!groups.rest.is_empty() && !flow.singles_done)
        || (!groups.name_taken.is_empty() && !flow.names_done)
}

/// The name of a sender the model read together with the label printed in
/// front of it ("Account holder: Example GmbH"). Only the first short,
/// digit-free word group before a colon counts as a label, so a colon inside
/// the name stays. The stored value and the rule keep the raw text.
fn sender_without_label(sender: &str) -> &str {
    let Some((label, after_colon)) = sender.split_once(':') else {
        return sender;
    };
    let name = after_colon.trim();
    let label_like = !label.trim().is_empty()
        && label.split_whitespace().count() <= 2
        && !label.chars().any(|letter| letter.is_ascii_digit());
    if label_like && !name.is_empty() {
        name
    } else {
        sender
    }
}

fn group_question(ctx: &Ctx, group: &StayGroup) -> (String, Vec<Chip>) {
    let count = number(group.count());
    let examples = group.examples.join(", ");
    let sentence = match group.key {
        GroupKey::Sender(_) => fill(
            ctx.lang,
            Msg::HermesChatGroupWhere,
            &[
                ("count", &count),
                ("label", sender_without_label(&group.label)),
                ("examples", &examples),
            ],
        ),
        GroupKey::Kind(_) => fill(
            ctx.lang,
            Msg::HermesChatGroupWhereKind,
            &[
                ("count", &count),
                ("kind", &group.label),
                ("examples", &examples),
            ],
        ),
    };
    let mut chips: Vec<Chip> = group
        .areas
        .iter()
        .map(|area| chip_labelled(area.clone(), ChipReply::GroupArea(area.clone()), false))
        .collect();
    chips.push(chip(
        ctx,
        Msg::HermesChatNewFolder,
        ChipReply::GroupNewFolder,
        false,
    ));
    chips.push(chip(
        ctx,
        Msg::HermesChatLeaveThem,
        ChipReply::GroupLeave,
        false,
    ));
    (sentence, chips)
}

/// Asks the next open question, or offers to file when nothing is left.
pub fn ask_next(state: &mut State, ctx: &Ctx) {
    let Some(trial) = state.session.trial.as_ref() else {
        return;
    };
    let flow = &state.session.groups;
    let groups = &trial.plan.groups;
    state.step = Step::Trial;
    let question = if let Some(group) = groups.asked.first() {
        group_question(ctx, group)
    } else if !groups.rest.is_empty() && !flow.singles_done {
        (
            fill(
                ctx.lang,
                Msg::HermesChatSinglePieces,
                &[("count", &number(groups.rest.len()))],
            ),
            vec![
                chip(
                    ctx,
                    Msg::HermesChatLeaveThem,
                    ChipReply::SinglesLeave,
                    false,
                ),
                chip(
                    ctx,
                    Msg::HermesChatShowSingly,
                    ChipReply::SinglesShow,
                    false,
                ),
            ],
        )
    } else if let (Some(first), false) = (groups.name_taken.first(), flow.names_done) {
        let suggested = first
            .suggested
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        (
            fill(
                ctx.lang,
                Msg::HermesChatNameTaken,
                &[
                    ("count", &number(groups.name_taken.len())),
                    ("name", &first.name),
                    ("suggested", &suggested),
                ],
            ),
            vec![
                chip(
                    ctx,
                    Msg::HermesChatFileNumbered,
                    ChipReply::NamesNumbered,
                    true,
                ),
                chip(ctx, Msg::HermesChatLeaveThem, ChipReply::NamesLeave, false),
            ],
        )
    } else {
        let ready = filed_count(trial);
        if ready == 0 {
            crate::trial_end::without_filing(state, ctx, Vec::new());
            return;
        }
        let chips = trial_chips(ctx, trial);
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatGroupsDone,
                &[("count", &number(ready))],
            ),
            chips,
        );
        return;
    };
    say(state, question.0, question.1);
}

fn start_rule(state: &mut State, ctx: &Ctx, area: String) {
    let (Some(trial), Some(tidy)) = (state.session.trial.as_ref(), state.session.tidy.as_ref())
    else {
        return;
    };
    let (Some(group), Some(target)) = (
        trial.plan.groups.asked.first(),
        tidy.target
            .as_ref()
            .and_then(|target| state.session.readings.get(target).cloned()),
    ) else {
        crate::update::cancel(state, ctx);
        return;
    };
    let saving = ChatWork::SaveGroupRule {
        target,
        key: group.key.clone(),
        label: group.label.clone(),
        area,
    };
    state.step = Step::Trial;
    work(state, saving, None);
}

pub fn area_chosen(state: &mut State, ctx: &Ctx, area: String) {
    start_rule(state, ctx, area);
}

pub fn new_folder(state: &mut State, ctx: &Ctx) {
    state.step = Step::GroupFolderName;
    say(
        state,
        text(ctx.lang, Msg::HermesChatNewFolderName),
        vec![cancel_chip(ctx)],
    );
}

/// One plain folder name: not empty, no separators, nothing that climbs out,
/// no name Windows reads as a device.
fn plain_folder_name(typed: &str) -> Option<String> {
    let name = typed.trim();
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None)
            if !name.contains(['/', '\\']) && !is_windows_device_name(name) =>
        {
            Some(name.to_owned())
        }
        _ => None,
    }
}

/// The text typed after "New folder …": the name, not for the model.
pub fn folder_named(state: &mut State, ctx: &Ctx, typed: &str) -> Option<Action> {
    match plain_folder_name(typed) {
        Some(name) => start_rule(state, ctx, name),
        None => say(
            state,
            text(ctx.lang, Msg::HermesChatFolderNameBad),
            vec![cancel_chip(ctx)],
        ),
    }
    None
}

/// The rule is stored; the trial runs again so the counts show what it does.
pub fn rule_saved(state: &mut State, ctx: &Ctx, saved: Result<String, RuleSaveFailure>) {
    let name = match saved {
        Ok(name) => name,
        Err(failure) => {
            let sentence = match failure {
                RuleSaveFailure::Refused(problem) => fill(
                    ctx.lang,
                    Msg::HermesChatRuleRefused,
                    &[("reason", &rule_problem_text(ctx.lang, &problem))],
                ),
                RuleSaveFailure::Failed(reason) => {
                    fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)])
                }
            };
            say(state, sentence, Vec::new());
            ask_next(state, ctx);
            return;
        }
    };
    let flow = &mut state.session.groups;
    flow.fresh_rule = Some(name);
    flow.answered.questions += 1;
    rerun(state, ctx);
}

fn rerun(state: &mut State, ctx: &Ctx) {
    let (Some(trial), Some(tidy)) = (state.session.trial.as_ref(), state.session.tidy.as_ref())
    else {
        return;
    };
    let (Some(source), Some(target)) = (
        state.session.readings.get(&tidy.source).cloned(),
        tidy.target
            .as_ref()
            .and_then(|target| state.session.readings.get(target).cloned()),
    ) else {
        crate::update::cancel(state, ctx);
        return;
    };
    let carried: Vec<(PathBuf, PathBuf)> = trial
        .resolved
        .iter()
        .filter_map(|(position, folder)| {
            trial
                .plan
                .entries
                .get(*position)
                .map(|entry| (entry.from.clone(), folder.clone()))
        })
        .collect();
    let answered = state.session.groups.answered.clone();
    state.session.groups.carried = carried;
    state.step = Step::Idle;
    work(
        state,
        ChatWork::Trial {
            source,
            target,
            answered,
            layout: state.session.layout.clone(),
        },
        Some(text(ctx.lang, Msg::HermesChatWorkingTrial)),
    );
}

pub fn leave_group(state: &mut State, ctx: &Ctx) {
    let Some(trial) = state.session.trial.as_mut() else {
        return;
    };
    let groups = &mut trial.plan.groups;
    if groups.asked.is_empty() {
        return;
    }
    let left = groups.asked.remove(0);
    let flow = &mut state.session.groups;
    flow.answered.left.push(left.key);
    flow.answered.questions += 1;
    ask_next(state, ctx);
}

pub fn singles_leave(state: &mut State, ctx: &Ctx) {
    state.session.groups.singles_done = true;
    ask_next(state, ctx);
}

/// Lists the single pieces with the paging of the trial list.
pub fn singles_show(state: &mut State, ctx: &Ctx) {
    let Some(trial) = state.session.trial.as_ref() else {
        return;
    };
    let position_of: HashMap<&PathBuf, usize> = trial
        .plan
        .entries
        .iter()
        .enumerate()
        .map(|(position, entry)| (&entry.from, position))
        .collect();
    let positions: Vec<usize> = trial
        .plan
        .groups
        .rest
        .iter()
        .filter_map(|entry| position_of.get(&entry.from).copied())
        .collect();
    let from = state.session.groups.singles_shown;
    let to = positions.len().min(from + PAGE);
    let page: Vec<usize> = positions
        .get(from..to)
        .map(<[usize]>::to_vec)
        .unwrap_or_default();
    let left = positions.len().saturating_sub(to);
    let Some(card) = card_of(state, ctx, page, left) else {
        return;
    };
    state.session.groups.singles_shown = to;
    let mut chips = Vec::new();
    if left > 0 {
        chips.push(chip(
            ctx,
            Msg::HermesChatShowMore,
            ChipReply::SinglesShow,
            false,
        ));
    }
    chips.push(chip(
        ctx,
        Msg::HermesChatLeaveThem,
        ChipReply::SinglesLeave,
        false,
    ));
    hermes(
        state,
        vec![
            Part::Text(fill(
                ctx.lang,
                Msg::HermesChatTrialNext,
                &[("count", &number(to - from))],
            )),
            Part::Trial(card),
        ],
        chips,
    );
}

/// Files the files with a taken name under the free name, for this session
/// only: no rule is stored.
pub fn names_numbered(state: &mut State, ctx: &Ctx) {
    let Some(trial) = state.session.trial.as_mut() else {
        return;
    };
    let taken_names = std::mem::take(&mut trial.plan.groups.name_taken);
    for taken in taken_names {
        let Some(entry) = trial.plan.entries.get_mut(taken.entry) else {
            continue;
        };
        entry.outcome = TrialOutcome::Filed {
            to: taken.suggested.clone(),
        };
        trial.plan.taken.insert(taken.suggested);
        trial.plan.filed += 1;
        trial.plan.stays = trial.plan.stays.saturating_sub(1);
    }
    state.session.groups.names_done = true;
    ask_next(state, ctx);
}

pub fn names_leave(state: &mut State, ctx: &Ctx) {
    state.session.groups.names_done = true;
    ask_next(state, ctx);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{plain_folder_name, sender_without_label};

    #[test]
    fn a_label_in_front_of_the_sender_is_dropped() {
        assert_eq!(
            sender_without_label("Accountholder: Example GmbH"),
            "Example GmbH"
        );
        assert_eq!(
            sender_without_label("Kontoinhaber:  Muster AG "),
            "Muster AG"
        );
    }

    #[test]
    fn a_colon_later_in_the_name_survives() {
        assert_eq!(
            sender_without_label("Sender: Example: Branch GmbH"),
            "Example: Branch GmbH"
        );
        assert_eq!(
            sender_without_label("Example one two three: Branch"),
            "Example one two three: Branch"
        );
        assert_eq!(sender_without_label("Example GmbH:"), "Example GmbH:");
        assert_eq!(sender_without_label("Example GmbH"), "Example GmbH");
    }

    #[test]
    fn a_folder_name_is_one_plain_name() {
        assert_eq!(plain_folder_name("  Steuer ").as_deref(), Some("Steuer"));
        for refused in ["", "..", "a/b", "a\\b", "/abs"] {
            assert_eq!(plain_folder_name(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn a_windows_device_name_is_refused_like_a_climb() {
        for device in ["CON", "nul", "Aux.txt", "COM1", "lpt9"] {
            assert_eq!(plain_folder_name(device), None, "{device}");
        }
        assert_eq!(plain_folder_name("Console").as_deref(), Some("Console"));
    }
}
