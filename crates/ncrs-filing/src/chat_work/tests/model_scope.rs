use std::sync::atomic::{AtomicUsize, Ordering};

use ncrs_ai::{
    AiError, ChatIntent, ChatTurn, ContextDocument, Levels, LocalModelStatus, Mismatch, Revision,
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
    ) -> Result<Vec<Option<usize>>, AiError> {
        self.asked.fetch_add(documents.len(), Ordering::SeqCst);
        Ok(vec![Some(0); documents.len()])
    }

    fn status(&self) -> LocalModelStatus {
        LocalModelStatus::NotDownloaded
    }

    fn refresh(&self) {}
}

fn trial_with(model: &Arc<CountingModel>, scope: ModelScope) -> TrialPlan {
    let files = (0..15_u8)
        .map(|seed| {
            plan_file(
                &format!("unknown{seed:02}.pdf"),
                100 + seed,
                Some("Unknown KG"),
                date(2022, 7, 4),
            )
        })
        .collect();
    let access = Setup::new()
        .access
        .with_model(Some(Arc::clone(model) as Arc<dyn ChatModel>));
    match access.run(ChatWork::Trial {
        source: Arc::new(reading("/inbox", files)),
        target: Arc::new(target_reading()),
        answered: AnsweredGroups::default(),
        layout: None,
        scope,
    }) {
        ChatResult::Trial(plan) => plan.unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_first_page_asks_the_model_about_ten_files_and_the_rest_follows_on_its_own() {
    let model = Arc::new(CountingModel::default());
    let first = trial_with(&model, ModelScope::FirstPage);
    assert_eq!(model.asked.load(Ordering::SeqCst), 10);
    assert_eq!(first.model_left.len(), 5);
    assert_eq!(first.unsure, 10);

    let rest = trial_with(&model, ModelScope::Only(first.model_left.clone()));
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
