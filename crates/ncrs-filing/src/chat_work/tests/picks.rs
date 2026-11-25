use super::trial::*;
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex as StdMutex;

use ncrs_ai::{
    AiError, ChatIntent, ChatTurn, ContextDocument, FolderChoiceLane, Levels, LocalModelStatus,
    Mismatch, Revision,
};
use ncrs_index::{DocumentFields, Kinds, RuleSpec};

use super::*;
use crate::chat_work::picks::remember_from_readings;
use crate::chat_work::{classify, LiveState};

/// Records each folder choice it is asked and answers the first folder, or
/// fails, or picks no folder. With a `run`, it notes the plan that is
/// published while it is asked, and can stop that run.
#[derive(Debug, Default)]
pub struct RecordingModel {
    pub calls: StdMutex<Vec<(usize, FolderChoiceLane)>>,
    pub fails: bool,
    /// Fails like a model that cannot answer the question at all.
    pub unsupported: bool,
    pub declines: bool,
    /// Picks another folder for each document it is asked about.
    pub rotates: bool,
    /// Picks the folder of this name instead of the first one.
    pub picks_folder: Option<&'static str>,
    pub run: Option<Arc<LiveRun>>,
    pub stops_the_run: bool,
    pub seen: StdMutex<Vec<Option<LivePlan>>>,
}

impl RecordingModel {
    pub fn calls(&self) -> Vec<(usize, FolderChoiceLane)> {
        self.calls.lock().unwrap().clone()
    }
}

impl ChatModel for RecordingModel {
    fn understand(&self, _turn: &ChatTurn) -> Result<ChatIntent, AiError> {
        Err(AiError::Model("unused"))
    }

    fn generate_rule(
        &self,
        _description: &str,
        _documents: &[ContextDocument],
        _kinds: &Kinds,
    ) -> Result<RuleSpec, AiError> {
        Err(AiError::Model("unused"))
    }

    fn recognize_levels(
        &self,
        _files: &[(String, DocumentFields)],
        _kinds: &Kinds,
        _folder_depth: Option<usize>,
    ) -> Result<Levels, AiError> {
        Err(AiError::Model("unused"))
    }

    fn recognize_levels_again(
        &self,
        _files: &[(String, DocumentFields)],
        _kinds: &Kinds,
        _folder_depth: Option<usize>,
        _previous: &Levels,
        _misfits: &[Mismatch],
    ) -> Result<Levels, AiError> {
        Err(AiError::Model("unused"))
    }

    fn revise_levels(
        &self,
        _files: &[(String, DocumentFields)],
        _kinds: &Kinds,
        _previous: &Levels,
        _sentence: &str,
    ) -> Result<Revision, AiError> {
        Err(AiError::Model("unused"))
    }

    fn choose_folders(
        &self,
        folders: &[(String, Vec<String>)],
        documents: &[String],
        lane: FolderChoiceLane,
    ) -> Result<Vec<Option<usize>>, AiError> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((documents.len(), lane));
        if let Some(run) = &self.run {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(
                    run.plan
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone(),
                );
            if self.stops_the_run {
                run.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if self.unsupported {
            return Err(AiError::Unsupported);
        }
        if self.fails {
            return Err(AiError::Model("closed"));
        }
        let pick = match self.picks_folder {
            _ if self.declines => None,
            Some(wanted) => folders.iter().position(|(folder, _)| folder == wanted),
            None => Some(0),
        };
        if self.rotates {
            return Ok((0..documents.len())
                .map(|document| Some(document % folders.len().max(1)))
                .collect());
        }
        Ok(vec![pick; documents.len()])
    }

    fn status(&self) -> LocalModelStatus {
        LocalModelStatus::NotDownloaded
    }

    fn refresh(&self) {}
}

fn source_of(count: u8) -> FolderReading {
    reading(
        "/inbox",
        (0..count)
            .map(|seed| {
                plan_file(
                    &format!("unknown{seed:02}.pdf"),
                    100 + seed,
                    Some(&lone_sender(seed)),
                    date(2022, 7, 4),
                )
            })
            .collect(),
    )
}

fn plan_with(
    setup: &Setup,
    source: &FolderReading,
    model: &RecordingModel,
    scope: &ModelScope,
) -> TrialPlan {
    crate::chat_work::trial::trial(
        &setup.access,
        source,
        &target_reading(),
        &AnsweredGroups::default(),
        None,
        Some(model),
        scope,
    )
    .unwrap()
}

#[test]
fn remembering_asks_from_the_end_in_pages_and_the_chats_trial_finds_every_pick() {
    let setup = Setup::new();
    let model = RecordingModel::default();
    let source = source_of(25);

    remember_from_readings(
        &setup.access,
        &source,
        &target_reading(),
        &model,
        None,
        &AtomicUsize::new(0),
    )
    .unwrap();

    assert_eq!(
        model.calls(),
        vec![
            (10, FolderChoiceLane::Background),
            (10, FolderChoiceLane::Background),
            (5, FolderChoiceLane::Background),
        ]
    );
    let chat = plan_with(&setup, &source, &model, &ModelScope::FirstPage);
    assert_eq!(model.calls().len(), 3);
    assert!(chat.model_left.is_empty());
    assert_eq!(chat.unsure, 25);

    remember_from_readings(
        &setup.access,
        &source,
        &target_reading(),
        &model,
        None,
        &AtomicUsize::new(0),
    )
    .unwrap();
    assert_eq!(model.calls().len(), 3);
}

#[test]
fn a_failing_model_ends_the_remembering_with_an_error() {
    let setup = Setup::new();
    let model = RecordingModel {
        fails: true,
        ..RecordingModel::default()
    };

    let result = remember_from_readings(
        &setup.access,
        &source_of(25),
        &target_reading(),
        &model,
        None,
        &AtomicUsize::new(0),
    );

    assert!(result.is_err());
    assert_eq!(model.calls().len(), 1);
}

#[test]
fn the_last_page_asks_the_last_ten_files_and_leaves_the_first_ones() {
    let setup = Setup::new();
    let model = RecordingModel::default();

    let plan = plan_with(&setup, &source_of(25), &model, &ModelScope::LastPage);

    assert_eq!(model.calls(), vec![(10, FolderChoiceLane::Background)]);
    let left: Vec<PathBuf> = plan.model_left.iter().cloned().collect();
    let first_fifteen: Vec<PathBuf> = (0..15)
        .map(|seed| Path::new("/inbox").join(format!("unknown{seed:02}.pdf")))
        .collect();
    assert_eq!(left, first_fifteen);
}

#[test]
fn files_the_model_could_not_answer_for_do_not_count_as_staying() {
    let setup = Setup::new();
    let model = RecordingModel {
        unsupported: true,
        ..RecordingModel::default()
    };

    let plan = plan_with(&setup, &source_of(12), &model, &ModelScope::FirstPage);

    assert_eq!(plan.model_failure, Some(AiError::Unsupported));
    assert!(!plan.model_left.is_empty());
    let staying = plan
        .entries
        .iter()
        .filter(|entry| {
            matches!(entry.outcome, TrialOutcome::Stays(_))
                && !plan.model_left.contains(&entry.from)
        })
        .count();
    assert_eq!(plan.stays, staying);
}

fn alfa_new_file() -> FolderReading {
    let mut file = plan_file("new.pdf", 100, Some("Alfa AG"), date(2022, 7, 4));
    file.text = Some("Alfa AG invoice".to_owned());
    reading("/inbox", vec![file])
}

fn trial_with_model_pick(
    source: &FolderReading,
    target: &FolderReading,
    pick: Option<&'static str>,
) -> TrialPlan {
    let setup = Setup::new();
    let model = RecordingModel {
        picks_folder: pick,
        ..RecordingModel::default()
    };
    crate::chat_work::trial::trial(
        &setup.access,
        source,
        target,
        &AnsweredGroups::default(),
        None,
        Some(&model),
        &ModelScope::FirstPage,
    )
    .unwrap()
}

#[test]
fn a_model_that_picks_the_folder_the_neighbours_offered_first_makes_the_place_sure() {
    let plan = trial_with_model_pick(
        &alfa_new_file(),
        &super::groups::alfa_in_two_areas(),
        Some("Firma/Alfa"),
    );

    let entry = &plan.entries[0];
    assert_eq!(
        entry.outcome,
        TrialOutcome::Filed {
            to: PathBuf::from("Firma/Alfa/2022/2022-07-04 Alfa Rechnung.pdf")
        },
        "{:?}",
        entry.outcome
    );
    assert_eq!(entry.placed_by, Some(PlacedBy::ModelAndNeighbours));
    assert_eq!((plan.filed, plan.unsure), (1, 0));
    let live = classify(&plan, &alfa_new_file());
    assert!(
        matches!(live.files[0].state, LiveState::Filed { unsure: false, .. }),
        "{:?}",
        live.files[0].state
    );
}

#[test]
fn a_model_that_picks_another_folder_than_the_neighbours_first_leaves_the_place_unsure() {
    let plan = trial_with_model_pick(
        &alfa_new_file(),
        &super::groups::alfa_in_two_areas(),
        Some("Haus/Alfa"),
    );

    let TrialOutcome::Unsure { choices, .. } = &plan.entries[0].outcome else {
        panic!("{:?}", plan.entries[0].outcome);
    };
    assert_eq!(choices[0], PathBuf::from("Haus/Alfa/2022"));
    assert_eq!(plan.entries[0].placed_by, Some(PlacedBy::Model));
    assert_eq!((plan.filed, plan.unsure), (0, 1));
    let live = classify(&plan, &alfa_new_file());
    assert!(
        matches!(live.files[0].state, LiveState::Filed { unsure: true, .. }),
        "{:?}",
        live.files[0].state
    );
}

#[test]
fn a_model_pick_without_any_neighbour_choice_is_unsure() {
    let plan = trial_with_model_pick(&source_of(1), &target_reading(), Some("Alfa"));

    assert!(
        matches!(plan.entries[0].outcome, TrialOutcome::Unsure { .. }),
        "{:?}",
        plan.entries[0].outcome
    );
    assert_eq!(plan.entries[0].placed_by, Some(PlacedBy::Model));
}

fn first_choice(plan: &TrialPlan, file: &str) -> PathBuf {
    let entry = plan
        .entries
        .iter()
        .find(|entry| entry.name == file)
        .unwrap();
    match &entry.outcome {
        TrialOutcome::Unsure { choices, .. } => choices[0].clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn alike_files_of_one_sender_written_differently_get_one_question_and_one_folder() {
    let setup = Setup::new();
    let model = RecordingModel {
        rotates: true,
        ..RecordingModel::default()
    };
    let source = reading(
        "/inbox",
        [
            "Gamma Bank AG",
            "Gamma Bank Aktiengesellschaft",
            "Ihre Gamma Bank",
        ]
        .into_iter()
        .zip(0..)
        .map(|(sender, seed)| {
            plan_file(
                &format!("statement{seed}.pdf"),
                100 + seed,
                Some(sender),
                date(2022, 7, 4),
            )
        })
        .collect(),
    );

    let plan = plan_with(&setup, &source, &model, &ModelScope::FirstPage);

    assert_eq!(model.calls(), vec![(1, FolderChoiceLane::Waiting)]);
    let first = first_choice(&plan, "statement0.pdf");
    assert_eq!(first_choice(&plan, "statement1.pdf"), first);
    assert_eq!(first_choice(&plan, "statement2.pdf"), first);
    assert!(plan.model_left.is_empty());

    plan_with(&setup, &source, &model, &ModelScope::FirstPage);
    assert_eq!(model.calls().len(), 1, "the picks of the group are stored");
}

#[test]
fn files_with_the_same_account_number_get_one_folder_whatever_their_senders() {
    let setup = Setup::new();
    let model = RecordingModel {
        rotates: true,
        ..RecordingModel::default()
    };
    let source = reading(
        "/inbox",
        ["Gamma Bank", "Delta Depot"]
            .into_iter()
            .zip(0..)
            .map(|(sender, seed)| {
                let mut file = plan_file(
                    &format!("statement{seed}.pdf"),
                    100 + seed,
                    Some(sender),
                    date(2022, 7, 4),
                );
                file.text = Some("Stamm-Nr.: 3117713 Portfolio: 1".to_owned());
                file
            })
            .collect(),
    );

    let plan = plan_with(&setup, &source, &model, &ModelScope::FirstPage);

    assert_eq!(model.calls().len(), 1);
    assert_eq!(
        first_choice(&plan, "statement0.pdf"),
        first_choice(&plan, "statement1.pdf")
    );
}

/// Three statements of Delta Bank in `Bank/Delta`, named as they came, so the
/// neighbours know the folder but no name pattern; `Haus` and
/// `Bank/Delta/Alt` hold other senders.
fn delta_without_a_name_pattern() -> FolderReading {
    let mut files: Vec<PlanFile> = ["auszug.pdf", "scan 7.pdf", "kontoinfo.pdf"]
        .into_iter()
        .zip(0..)
        .map(|(name, seed)| {
            plan_file(
                &format!("Bank/Delta/{name}"),
                10 + seed,
                Some("Delta Bank"),
                date(2021, 3, 1 + u32::from(seed)),
            )
        })
        .collect();
    files.push(plan_file(
        "Haus/brief.pdf",
        30,
        Some("Omega KG"),
        date(2021, 4, 1),
    ));
    files.push(plan_file(
        "Bank/Delta/Alt/x.pdf",
        31,
        Some("Sigma AG"),
        date(2021, 5, 1),
    ));
    reading("/archive", files)
}

fn delta_new_file() -> FolderReading {
    let mut file = plan_file("new.pdf", 100, Some("Delta Bank"), date(2022, 7, 4));
    file.text = Some("Delta Bank statement".to_owned());
    reading("/inbox", vec![file])
}

#[test]
fn a_model_pick_outside_the_folder_the_neighbours_are_sure_of_does_not_win() {
    let plan = trial_with_model_pick(
        &delta_new_file(),
        &delta_without_a_name_pattern(),
        Some("Haus"),
    );

    let entry = &plan.entries[0];
    assert_eq!(
        entry.outcome,
        TrialOutcome::Filed {
            to: PathBuf::from("Bank/Delta/new.pdf")
        },
        "{:?}",
        entry.outcome
    );
    assert_eq!(entry.placed_by, Some(PlacedBy::Neighbours));
    let live = classify(&plan, &delta_new_file());
    assert!(
        matches!(live.files[0].state, LiveState::Filed { unsure: false, .. }),
        "{:?}",
        live.files[0].state
    );
}

#[test]
fn a_model_pick_inside_the_folder_the_neighbours_are_sure_of_is_offered_first_but_unsure() {
    let plan = trial_with_model_pick(
        &delta_new_file(),
        &delta_without_a_name_pattern(),
        Some("Bank/Delta/Alt"),
    );

    let entry = &plan.entries[0];
    let TrialOutcome::Unsure { choices, .. } = &entry.outcome else {
        panic!("{:?}", entry.outcome);
    };
    assert_eq!(
        choices,
        &[PathBuf::from("Bank/Delta/Alt"), PathBuf::from("Bank/Delta")]
    );
    assert_eq!(entry.placed_by, Some(PlacedBy::Model));
}

#[test]
fn a_model_pick_of_the_folder_the_neighbours_are_sure_of_keeps_the_name() {
    let plan = trial_with_model_pick(
        &delta_new_file(),
        &delta_without_a_name_pattern(),
        Some("Bank/Delta"),
    );

    let entry = &plan.entries[0];
    assert_eq!(
        entry.outcome,
        TrialOutcome::Filed {
            to: PathBuf::from("Bank/Delta/new.pdf")
        },
        "{:?}",
        entry.outcome
    );
    assert_eq!(entry.placed_by, Some(PlacedBy::ModelAndNeighbours));
}

#[test]
fn a_folder_without_a_name_pattern_keeps_the_incoming_name_in_the_live_plan() {
    let source = delta_new_file();
    let sure = trial_with_model_pick(&source, &delta_without_a_name_pattern(), Some("Bank/Delta"));
    let deeper = trial_with_model_pick(
        &source,
        &delta_without_a_name_pattern(),
        Some("Bank/Delta/Alt"),
    );

    assert_eq!(
        classify(&sure, &source).files[0].state,
        LiveState::Filed {
            folder: PathBuf::from("Bank/Delta"),
            name: "new.pdf".to_owned(),
            unsure: false,
        }
    );
    assert_eq!(
        classify(&deeper, &source).files[0].state,
        LiveState::Filed {
            folder: PathBuf::from("Bank/Delta/Alt"),
            name: "new.pdf".to_owned(),
            unsure: true,
        }
    );
}

/// Three statements of Delta Bank in `Bank/DB/2023` and three in
/// `Bank/DB/2024/Konto`: the years are laid out differently.
fn delta_with_years_laid_out_differently() -> FolderReading {
    let files = [
        ("Bank/DB/2023/a1.pdf", 10, 2023),
        ("Bank/DB/2023/a2.pdf", 11, 2023),
        ("Bank/DB/2023/a3.pdf", 12, 2023),
        ("Bank/DB/2024/Konto/b1.pdf", 13, 2024),
        ("Bank/DB/2024/Konto/b2.pdf", 14, 2024),
        ("Bank/DB/2024/Konto/b3.pdf", 15, 2024),
    ]
    .into_iter()
    .map(|(path, seed, year)| plan_file(path, seed, Some("Delta Bank"), date(year, 3, 1)))
    .collect();
    reading("/archive", files)
}

#[test]
fn a_model_pick_of_the_year_folder_the_neighbours_of_that_year_lie_in_is_sure() {
    let mut file = plan_file("new.pdf", 100, Some("Delta Bank"), date(2024, 7, 4));
    file.text = Some("Delta Bank statement".to_owned());
    let source = reading("/inbox", vec![file]);

    let plan = trial_with_model_pick(
        &source,
        &delta_with_years_laid_out_differently(),
        Some("Bank/DB/Konto"),
    );

    let entry = &plan.entries[0];
    assert_eq!(
        entry.outcome,
        TrialOutcome::Filed {
            to: PathBuf::from("Bank/DB/2024/Konto/new.pdf")
        },
        "{:?}",
        entry.outcome
    );
    assert_eq!(entry.placed_by, Some(PlacedBy::ModelAndNeighbours));
    assert_eq!(
        classify(&plan, &source).files[0].state,
        LiveState::Filed {
            folder: PathBuf::from("Bank/DB/2024/Konto"),
            name: "new.pdf".to_owned(),
            unsure: false,
        }
    );
}
