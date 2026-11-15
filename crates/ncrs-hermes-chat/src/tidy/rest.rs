//! The files beyond the first page of the trial: the model looks at them in
//! the background, page by page, so the count rises while it runs; their
//! outcomes are mixed into the open trial.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ncrs_filing::chat_work::{ChatWork, ModelScope, TrialOutcome, TrialPlan, MODEL_FIRST_PAGE};
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Entry, Part, State, Step};
use crate::stay_groups;
use crate::texts::{fill, number};
use crate::tidy::{filed_count, trial_chips};
use crate::update::{clear_chips, hermes};

/// Queues the look at the next page of the files the first run left for the
/// model.
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
    let page: BTreeSet<_> = trial
        .plan
        .entries
        .iter()
        .filter(|entry| trial.plan.model_left.contains(&entry.from))
        .take(MODEL_FIRST_PAGE)
        .map(|entry| entry.from.clone())
        .collect();
    state.trial_rest = Some(ChatWork::Trial {
        source,
        target,
        answered: state.session.groups.answered.clone(),
        layout: state.session.layout.clone(),
        scope: ModelScope::Only(page.clone()),
    });
    state.trial_rest_page = page;
    state.trial_rest_run += 1;
    state.fresh_trial_rest = true;
}

pub fn done(state: &mut State, ctx: &Ctx, run: u64, result: Result<TrialPlan, String>) {
    if run != state.trial_rest_run {
        return;
    }
    let Ok(rest) = result else {
        if let Some(trial) = state.session.trial.as_mut() {
            trial.plan.model_left.clear();
        }
        state.trial_rest_page.clear();
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
    let page = std::mem::take(&mut state.trial_rest_page);
    for entry in &mut plan.entries {
        if let (true, Some(outcome)) = (
            page.contains(&entry.from),
            decided.get(entry.from.as_path()),
        ) {
            entry.outcome = (*outcome).clone();
        }
    }
    plan.model_left.retain(|path| !page.contains(path));
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
    let more_pages = !plan.model_left.is_empty();
    if more_pages {
        start(state);
    }
    if stay_groups::has_open_questions(state) {
        return;
    }
    let Some(open) = state.session.trial.as_ref() else {
        return;
    };
    let chips = trial_chips(ctx, open);
    if more_pages {
        if let Some(Entry::Hermes { chips: shown, .. }) = state.thread.last_mut() {
            *shown = chips;
        }
        return;
    }
    let line = fill(
        ctx.lang,
        Msg::HermesChatTrialRestDone,
        &[("count", &number(filed_count(open)))],
    );
    clear_chips(state);
    hermes(state, vec![Part::Text(line)], chips);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::Arc;

    use ncrs_filing::chat_work::{FolderReading, StayGroups, StayReason, TrialEntry};
    use ncrs_i18n::Language;
    use ncrs_index::{ContentHash, DocumentFields};

    use super::*;
    use crate::ctx::Availability;
    use crate::state::{Tidy, TrialSession};

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
        state.trial_rest_page = BTreeSet::from([waiting.from]);
        assert_eq!(filed_count(state.session.trial.as_ref().unwrap()), 0);

        done(&mut state, &ctx(), 0, Ok(rest));

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
            0,
            Ok(plan_of(vec![waiting], BTreeSet::new())),
        );

        assert_eq!(
            state.session.trial.as_ref().unwrap().plan.model_left.len(),
            1
        );
        assert!(state.thread.is_empty());
    }

    fn reading(root: &str) -> Arc<FolderReading> {
        Arc::new(FolderReading {
            ai: Default::default(),
            kinds: Default::default(),
            root: root.into(),
            files: Vec::new(),
            photos: 0,
            without_text: 0,
            structure: ncrs_intake::LocationStructure {
                areas: Vec::new(),
                unordered: Vec::new(),
            },
            duplicates: Vec::new(),
            hashes: Default::default(),
        })
    }

    fn state_with_twelve_waiting() -> State {
        let names: Vec<String> = (0..12).map(|number| format!("f{number:02}.pdf")).collect();
        let entries: Vec<TrialEntry> = names
            .iter()
            .map(|name| entry(name, TrialOutcome::Stays(StayReason::NoNeighbours)))
            .collect();
        let waiting = entries.iter().map(|entry| entry.from.clone()).collect();
        let mut state = State {
            step: Step::Trial,
            ..State::default()
        };
        state.session.trial = Some(TrialSession {
            plan: plan_of(entries, waiting),
            resolved: BTreeMap::new(),
            shown: 2,
        });
        state.session.tidy = Some(Tidy {
            rule_now: None,
            source: PathBuf::from("/inbox"),
            target: Some(PathBuf::from("/archive")),
            transfer: None,
            inside: Vec::new(),
        });
        state
            .session
            .readings
            .insert(PathBuf::from("/inbox"), reading("/inbox"));
        state
            .session
            .readings
            .insert(PathBuf::from("/archive"), reading("/archive"));
        let first = trial_chips(&ctx(), state.session.trial.as_ref().unwrap());
        state.thread.push(Entry::Hermes {
            parts: Vec::new(),
            chips: first,
        });
        start(&mut state);
        state
    }

    fn filed_outcome() -> TrialOutcome {
        TrialOutcome::Filed {
            to: PathBuf::from("Alfa"),
        }
    }

    #[test]
    fn a_page_raises_the_count_in_place_and_the_next_page_is_queued() {
        let mut state = state_with_twelve_waiting();
        assert_eq!(state.trial_rest_page.len(), MODEL_FIRST_PAGE);
        let page_entries = state
            .trial_rest_page
            .iter()
            .map(|from| {
                let name = from.file_name().unwrap().to_string_lossy().into_owned();
                entry(&name, filed_outcome())
            })
            .collect();
        let rest = plan_of(page_entries, BTreeSet::new());
        let before = state.chips()[0].label.clone();
        let thread_len = state.thread.len();

        let run = state.trial_rest_run;
        done(&mut state, &ctx(), run, Ok(rest));

        assert_eq!(state.thread.len(), thread_len);
        assert_ne!(state.chips()[0].label, before);
        assert!(state.chips()[0]
            .label
            .contains(&MODEL_FIRST_PAGE.to_string()));
        let trial = state.session.trial.as_ref().unwrap();
        assert_eq!(trial.plan.model_left.len(), 2);
        assert!(state.trial_rest.is_some());
        assert_eq!(state.trial_rest_page.len(), 2);
    }

    #[test]
    fn a_page_of_an_earlier_run_changes_nothing() {
        let mut state = state_with_twelve_waiting();
        let stale_run = state.trial_rest_run - 1;
        let page = state.trial_rest_page.clone();

        done(
            &mut state,
            &ctx(),
            stale_run,
            Ok(plan_of(
                vec![entry("f00.pdf", filed_outcome())],
                BTreeSet::new(),
            )),
        );

        let trial = state.session.trial.as_ref().unwrap();
        assert_eq!(trial.plan.model_left.len(), 12);
        assert_eq!(trial.plan.filed, 0);
        assert_eq!(state.trial_rest_page, page);
        assert_eq!(state.thread.len(), 1);
    }
}
