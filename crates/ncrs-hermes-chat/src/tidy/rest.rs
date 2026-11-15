//! The files beyond the first page of the trial: the model looks at them page
//! by page while the chat shows how far it is; their outcomes are mixed into
//! the open trial, which is posted once when the last page is in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ncrs_filing::chat_work::{ChatWork, ModelScope, TrialOutcome, TrialPlan, MODEL_FIRST_PAGE};
use ncrs_i18n::Msg;

use crate::ctx::Ctx;
use crate::state::{Entry, State, Step};
use crate::texts::{fill, number};
use crate::tidy;
use crate::update::drop_working;

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
        .model_left
        .iter()
        .take(MODEL_FIRST_PAGE)
        .cloned()
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

/// The line shown while the model looks at the files: how many it is through.
pub fn progress_line(ctx: &Ctx, plan: &TrialPlan) -> String {
    let total = plan.entries.len();
    let done = total.saturating_sub(plan.model_left.len());
    fill(
        ctx.lang,
        Msg::HermesChatWorkingTrialProgress,
        &[("done", &number(done)), ("total", &number(total))],
    )
}

/// Takes the progress line out and posts the trial with what is decided.
fn finish(state: &mut State, ctx: &Ctx) {
    state.trial_rest_page.clear();
    drop_working(state);
    tidy::post_trial(state, ctx);
}

/// Swaps the progress line for one with the new count.
fn show_progress(state: &mut State, line: String) {
    if let Some(Entry::Working(shown)) = state
        .thread
        .iter_mut()
        .rev()
        .find(|entry| matches!(entry, Entry::Working(_)))
    {
        *shown = line;
    }
}

pub fn done(state: &mut State, ctx: &Ctx, run: u64, result: Result<TrialPlan, String>) {
    if run != state.trial_rest_run {
        return;
    }
    let trial_is_open = state.step == Step::Trial && state.session.trial.is_some();
    let rest = match result {
        Ok(rest) if trial_is_open => rest,
        Err(_) if trial_is_open => {
            if let Some(trial) = state.session.trial.as_mut() {
                trial.plan.model_left.clear();
            }
            finish(state, ctx);
            return;
        }
        _ => {
            state.trial_rest_page.clear();
            drop_working(state);
            return;
        }
    };
    let Some(trial) = state.session.trial.as_mut() else {
        return;
    };
    let plan = &mut trial.plan;
    if plan.source_root != rest.source_root || plan.model_left.is_empty() {
        state.trial_rest_page.clear();
        drop_working(state);
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
    if plan.model_left.is_empty() {
        finish(state, ctx);
        return;
    }
    let line = progress_line(ctx, plan);
    start(state);
    show_progress(state, line);
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
    use crate::state::{Part, Tidy, TrialSession};

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
        let entries: Vec<TrialEntry> = (0..12)
            .map(|number| {
                entry(
                    &format!("f{number:02}.pdf"),
                    TrialOutcome::Stays(StayReason::NoNeighbours),
                )
            })
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
            transfer: Some(ncrs_index::Transfer::Move),
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
        state
    }

    /// The trial as the first run returns it: `waiting` of the twelve files
    /// still wait for the model.
    fn first_trial(waiting: usize) -> TrialPlan {
        let entries: Vec<TrialEntry> = (0..12)
            .map(|number| {
                let outcome = if number < 12 - waiting {
                    filed_outcome()
                } else {
                    TrialOutcome::Stays(StayReason::NoNeighbours)
                };
                entry(&format!("f{number:02}.pdf"), outcome)
            })
            .collect();
        let left = entries
            .iter()
            .skip(12 - waiting)
            .map(|entry| entry.from.clone())
            .collect();
        let mut plan = plan_of(entries, left);
        plan.filed = 12 - waiting;
        plan.stays = waiting;
        plan
    }

    fn state_after_first_trial(waiting: usize) -> State {
        let mut state = state_with_twelve_waiting();
        state.session.trial = None;
        tidy::trial_done(&mut state, &ctx(), Ok(first_trial(waiting)));
        state
    }

    fn filed_outcome() -> TrialOutcome {
        TrialOutcome::Filed {
            to: PathBuf::from("Alfa"),
        }
    }

    fn finish_page(state: &mut State) {
        let page_entries = state
            .trial_rest_page
            .iter()
            .map(|from| {
                let name = from.file_name().unwrap().to_string_lossy().into_owned();
                entry(&name, filed_outcome())
            })
            .collect();
        let run = state.trial_rest_run;
        done(
            state,
            &ctx(),
            run,
            Ok(plan_of(page_entries, BTreeSet::new())),
        );
    }

    fn working_lines(state: &State) -> Vec<&str> {
        state
            .thread
            .iter()
            .filter_map(|entry| match entry {
                Entry::Working(line) => Some(line.as_str()),
                _ => None,
            })
            .collect()
    }

    fn trial_cards(state: &State) -> usize {
        state
            .thread
            .iter()
            .filter(|entry| match entry {
                Entry::Hermes { parts, .. } => {
                    parts.iter().any(|part| matches!(part, Part::Trial(_)))
                }
                _ => false,
            })
            .count()
    }

    #[test]
    fn while_the_model_looks_only_a_progress_line_is_shown() {
        let state = state_after_first_trial(12);

        assert_eq!(working_lines(&state), ["Looking at the files: 0 of 12"]);
        assert_eq!(state.thread.len(), 1);
        assert_eq!(trial_cards(&state), 0);
        assert!(state.looks_at_files());
        assert_eq!(state.trial_rest_page.len(), MODEL_FIRST_PAGE);
    }

    #[test]
    fn a_page_moves_the_count_and_the_last_page_posts_the_trial_once() {
        let mut state = state_after_first_trial(12);

        finish_page(&mut state);
        assert_eq!(working_lines(&state), ["Looking at the files: 10 of 12"]);
        assert_eq!(state.thread.len(), 1);
        assert_eq!(trial_cards(&state), 0);

        finish_page(&mut state);
        assert!(working_lines(&state).is_empty());
        assert!(!state.looks_at_files());
        assert_eq!(trial_cards(&state), 1);
        assert_eq!(state.thread.len(), 1);
        let chips = state.chips();
        assert!(chips[0].label.contains("12"), "{}", chips[0].label);
    }

    #[test]
    fn picks_that_cover_everything_post_the_trial_at_once() {
        let state = state_after_first_trial(0);

        assert!(working_lines(&state).is_empty());
        assert_eq!(trial_cards(&state), 1);
        assert!(!state.looks_at_files());
    }

    #[test]
    fn a_failed_page_posts_the_trial_with_what_is_decided() {
        let mut state = state_after_first_trial(12);
        finish_page(&mut state);
        let run = state.trial_rest_run;

        done(&mut state, &ctx(), run, Err("model".to_owned()));

        assert!(working_lines(&state).is_empty());
        assert!(!state.looks_at_files());
        assert_eq!(trial_cards(&state), 1);
        assert!(state.chips()[0].label.contains("10"));
    }

    #[test]
    fn a_page_of_an_earlier_run_changes_nothing() {
        let mut state = state_after_first_trial(12);
        let stale_run = state.trial_rest_run - 1;
        let page = state.trial_rest_page.clone();

        done(
            &mut state,
            &ctx(),
            stale_run,
            Ok(plan_of(
                vec![entry("f10.pdf", filed_outcome())],
                BTreeSet::new(),
            )),
        );

        let trial = state.session.trial.as_ref().unwrap();
        assert_eq!(trial.plan.model_left.len(), 12);
        assert_eq!(state.trial_rest_page, page);
        assert_eq!(working_lines(&state).len(), 1);
    }
}
