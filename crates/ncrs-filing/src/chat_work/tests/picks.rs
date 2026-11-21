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
        _folders: &[(String, Vec<String>)],
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
        let pick = if self.declines { None } else { Some(0) };
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
                    Some("Unknown KG"),
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
