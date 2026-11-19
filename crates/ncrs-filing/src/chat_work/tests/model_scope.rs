use std::sync::atomic::{AtomicUsize, Ordering};

use ncrs_ai::{
    AiError, ChatIntent, ChatTurn, ContextDocument, FolderChoiceLane, Levels, LocalModelStatus,
    Mismatch, Revision,
};
use ncrs_index::{DocumentFields, Kinds, RuleSpec};

use super::*;

/// Picks the first folder for every document and counts what it was asked.
#[derive(Debug, Default)]
struct CountingModel {
    asked: AtomicUsize,
}

impl ChatModel for CountingModel {
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
        _lane: FolderChoiceLane,
    ) -> Result<Vec<Option<usize>>, AiError> {
        self.asked.fetch_add(documents.len(), Ordering::SeqCst);
        Ok(vec![Some(0); documents.len()])
    }

    fn status(&self) -> LocalModelStatus {
        LocalModelStatus::NotDownloaded
    }

    fn refresh(&self) {}
}

fn unknown_files() -> Vec<PlanFile> {
    (0..15)
        .map(|seed: u8| {
            plan_file(
                &format!("unknown{seed:02}.pdf"),
                100 + seed,
                Some("Unknown KG"),
                date(2022, 7, 4),
            )
        })
        .collect()
}

/// The directory of the index has to outlive the access.
fn access_with(model: &Arc<CountingModel>) -> (TempDir, ChatAccess) {
    let Setup {
        dir,
        access: plain_access,
    } = Setup::new();
    let access = plain_access.with_model(Some(Arc::clone(model) as Arc<dyn ChatModel>));
    (dir, access)
}

fn trial_on(access: &ChatAccess, target: FolderReading, scope: ModelScope) -> TrialPlan {
    match access.run(ChatWork::Trial {
        source: Arc::new(reading("/inbox", unknown_files())),
        target: Arc::new(target),
        answered: AnsweredGroups::default(),
        layout: None,
        scope,
    }) {
        ChatResult::Trial(plan) => *plan.unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_first_page_asks_the_model_about_ten_files_and_the_rest_follows_on_its_own() {
    let model = Arc::new(CountingModel::default());
    let (_setup, access) = access_with(&model);
    let first = trial_on(&access, target_reading(), ModelScope::FirstPage);
    assert_eq!(model.asked.load(Ordering::SeqCst), 10);
    assert_eq!(first.model_left.len(), 5);
    assert_eq!(first.unsure, 10);

    let rest = trial_on(&access, target_reading(), ModelScope::FirstPage);
    assert_eq!(model.asked.load(Ordering::SeqCst), 15);
    assert!(rest.model_left.is_empty());
    for left in &first.model_left {
        let entry = rest
            .entries
            .iter()
            .find(|entry| entry.from == *left)
            .unwrap();
        assert!(matches!(entry.outcome, TrialOutcome::Unsure { .. }));
    }
}

#[test]
fn a_second_trial_over_the_same_files_does_not_ask_the_model_again() {
    let model = Arc::new(CountingModel::default());
    let (_setup, access) = access_with(&model);
    trial_on(&access, target_reading(), ModelScope::FirstPage);
    let first = trial_on(&access, target_reading(), ModelScope::FirstPage);
    assert_eq!(model.asked.load(Ordering::SeqCst), 15);

    let second = trial_on(&access, target_reading(), ModelScope::FirstPage);

    assert_eq!(model.asked.load(Ordering::SeqCst), 15);
    assert_eq!(second.unsure, first.unsure);
    assert_eq!(second.entries, first.entries);
}

#[test]
fn a_stored_pick_whose_folder_is_gone_is_asked_again() {
    let model = Arc::new(CountingModel::default());
    let (_setup, access) = access_with(&model);
    trial_on(&access, target_reading(), ModelScope::FirstPage);
    trial_on(&access, target_reading(), ModelScope::FirstPage);
    assert_eq!(model.asked.load(Ordering::SeqCst), 15);

    let without_alfa = reading(
        "/archive",
        [(20, "Privat/Beta"), (21, "Privat/Beta"), (22, "Firma/Beta")]
            .into_iter()
            .map(|(seed, folder)| {
                plan_file(
                    &format!("{folder}/{seed}.pdf"),
                    seed,
                    Some("Beta GmbH"),
                    date(2022, 3, 3),
                )
            })
            .collect(),
    );
    trial_on(&access, without_alfa.clone(), ModelScope::FirstPage);
    trial_on(&access, without_alfa, ModelScope::FirstPage);

    assert_eq!(model.asked.load(Ordering::SeqCst), 30);
}
