//! The files beyond the first page of the trial: the model looks at them in
//! the background and their outcomes are mixed into the open trial.

use std::collections::BTreeMap;
use std::path::Path;

use ncrs_filing::chat_work::{ChatWork, ModelScope, TrialOutcome, TrialPlan};
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Part, State, Step};
use crate::stay_groups;
use crate::texts::{fill, number};
use crate::tidy::{filed_count, trial_chips};
use crate::update::{clear_chips, hermes};

/// Queues the look at the files the first run left for the model.
pub fn start(state: &mut State) {
    let (Some(trial), Some(tidy)) = (state.session.trial.as_ref(), state.session.tidy.as_ref())
    else {
        return;
    };
    if trial.plan.model_left.is_empty() {
        return;
    }
    let (Some(source), Some(target)) = (
        state.session.readings.get(&tidy.source).cloned(),
        tidy.target
            .as_ref()
            .and_then(|target| state.session.readings.get(target).cloned()),
    ) else {
        return;
    };
    state.trial_rest = Some(ChatWork::Trial {
        source,
        target,
        answered: state.session.groups.answered.clone(),
        layout: state.session.layout.clone(),
        scope: ModelScope::Only(trial.plan.model_left.clone()),
    });
    state.fresh_trial_rest = true;
}

pub fn done(state: &mut State, ctx: &Ctx, result: Result<TrialPlan, String>) {
    let Ok(rest) = result else {
        return;
    };
    if state.step != Step::Trial {
        return;
    }
    let Some(trial) = state.session.trial.as_mut() else {
        return;
    };
    let plan = &mut trial.plan;
    if plan.source_root != rest.source_root || plan.model_left.is_empty() {
        return;
    }
    let decided: BTreeMap<&Path, &TrialOutcome> = rest
        .entries
        .iter()
        .map(|entry| (entry.from.as_path(), &entry.outcome))
        .collect();
    let left = std::mem::take(&mut plan.model_left);
    for entry in &mut plan.entries {
        if let (true, Some(outcome)) = (
            left.contains(&entry.from),
            decided.get(entry.from.as_path()),
        ) {
            entry.outcome = (*outcome).clone();
        }
    }
    let count = |matches_outcome: fn(&TrialOutcome) -> bool| {
        plan.entries
            .iter()
            .filter(|entry| matches_outcome(&entry.outcome))
            .count()
    };
    plan.filed = count(|outcome| matches!(outcome, TrialOutcome::Filed { .. }));
    plan.unsure = count(|outcome| matches!(outcome, TrialOutcome::Unsure { .. }));
    plan.already_there = count(|outcome| matches!(outcome, TrialOutcome::AlreadyThere { .. }));
    plan.stays = count(|outcome| matches!(outcome, TrialOutcome::Stays(_)));
    if stay_groups::has_open_questions(state) {
        return;
    }
    let Some(open) = state.session.trial.as_ref() else {
        return;
    };
    let line = fill(
        ctx.lang,
        Msg::HermesChatTrialRestDone,
        &[("count", &number(filed_count(open)))],
    );
    let chips = trial_chips(ctx, open);
    clear_chips(state);
    hermes(state, vec![Part::Text(line)], chips);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use ncrs_filing::chat_work::{StayGroups, StayReason, TrialEntry};
    use ncrs_i18n::Language;
    use ncrs_index::{ContentHash, DocumentFields};

    use super::*;
    use crate::ctx::Availability;
    use crate::state::{Entry, TrialSession};

    fn entry(name: &str, outcome: TrialOutcome) -> TrialEntry {
        TrialEntry {
            from: PathBuf::from("/inbox").join(name),
            name: name.to_owned(),
            hash: ContentHash([1; 32]),
            kind_word: None,
            fields: DocumentFields::default(),
            outcome,
            neighbour: None,
            wanted: None,
        }
    }

    fn plan_of(entries: Vec<TrialEntry>, model_left: BTreeSet<PathBuf>) -> TrialPlan {
        TrialPlan {
            source_root: PathBuf::from("/inbox"),
            target_root: PathBuf::from("/archive"),
            stays: entries.len(),
            entries,
            filed: 0,
            unsure: 0,
            already_there: 0,
            taken: BTreeSet::new(),
            groups: StayGroups::default(),
            model_left,
        }
    }

    fn ctx() -> Ctx {
        Ctx {
            lang: Language::English,
            availability: Availability::Off,
            home: PathBuf::new(),
            targets: Vec::new(),
            favourites: Vec::new(),
            sources: Vec::new(),
            now: chrono::Local::now(),
            resolve_folder: |path| Ok(path.to_path_buf()),
            ai_on: true,
            ocr_models_present: true,
            model: None,
        }
    }

    #[test]
    fn the_rest_is_mixed_into_the_open_trial_and_the_count_on_the_chip_rises() {
        let stays = || TrialOutcome::Stays(StayReason::NoNeighbours);
        let unsure = TrialOutcome::Unsure {
            choices: vec![PathBuf::from("Alfa")],
            file_names: vec!["b.pdf".to_owned()],
            kind_label: None,
        };
        let waiting = entry("b.pdf", stays());
        let open = plan_of(
            vec![entry("a.pdf", stays()), waiting.clone()],
            BTreeSet::from([waiting.from.clone()]),
        );
        let rest = plan_of(vec![entry("b.pdf", unsure)], BTreeSet::new());
        let mut state = State {
            step: Step::Trial,
            ..State::default()
        };
        state.session.trial = Some(TrialSession {
            plan: open,
            resolved: BTreeMap::new(),
            shown: 2,
        });
        assert_eq!(filed_count(state.session.trial.as_ref().unwrap()), 0);

        done(&mut state, &ctx(), Ok(rest));

        let trial = state.session.trial.as_ref().unwrap();
        assert_eq!(filed_count(trial), 1);
        assert_eq!((trial.plan.unsure, trial.plan.stays), (1, 1));
        assert!(trial.plan.model_left.is_empty());
        assert!(
            matches!(state.thread.last(), Some(Entry::Hermes { chips, .. }) if !chips.is_empty())
        );
    }

    #[test]
    fn a_rest_that_comes_after_the_trial_ended_is_dropped() {
        let waiting = entry("b.pdf", TrialOutcome::Stays(StayReason::NoNeighbours));
        let mut state = State::default();
        state.session.trial = Some(TrialSession {
            plan: plan_of(
                vec![waiting.clone()],
                BTreeSet::from([waiting.from.clone()]),
            ),
            resolved: BTreeMap::new(),
            shown: 1,
        });
        state.step = Step::Idle;

        done(
            &mut state,
            &ctx(),
            Ok(plan_of(vec![waiting], BTreeSet::new())),
        );

        assert_eq!(
            state.session.trial.as_ref().unwrap().plan.model_left.len(),
            1
        );
        assert!(state.thread.is_empty());
    }
}
