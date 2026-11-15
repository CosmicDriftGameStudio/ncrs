//! The model's folder picks for the read folders, made in the background
//! once the pipeline is idle, so the trial finds them and shows its full
//! count at once.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use ncrs_ai::ChatModel;
use ncrs_filing::chat_work::{remember_folder_picks, ChatAccess};

use crate::ctx::{Availability, Ctx};
use crate::message::Message;
use crate::state::State;
use crate::tidy;

/// What the background job needs, taken from the chat when it starts.
#[derive(Debug)]
pub struct PicksJob {
    access: ChatAccess,
    model: Arc<dyn ChatModel>,
    target: PathBuf,
    sources: Vec<PathBuf>,
}

/// Starts a job for `sequence` of the pipeline unless one runs, this
/// sequence was done already, or the chat has nothing to ask with.
pub fn begin(state: &mut State, ctx: &Ctx, sequence: usize) -> Option<PicksJob> {
    if state.picks_running || state.picks_settled_at == Some(sequence) || !ctx.ai_on {
        return None;
    }
    let (Availability::Ready(access), Some(model)) = (&ctx.availability, &ctx.model) else {
        return None;
    };
    let target = tidy::targets(state, ctx).into_iter().next()?;
    let sources: Vec<PathBuf> = ctx
        .sources
        .iter()
        .filter(|source| **source != target)
        .cloned()
        .collect();
    if sources.is_empty() {
        return None;
    }
    state.picks_running = true;
    state.picks_settled_at = Some(sequence);
    Some(PicksJob {
        access: (**access).clone(),
        model: Arc::clone(model),
        target,
        sources,
    })
}

pub fn launch(state: &mut State) -> Task<Message> {
    let Some(job) = state.picks_job.take() else {
        return Task::none();
    };
    Task::perform(
        async move {
            // A failed spawn only means the picks are made when the chat asks.
            let _ = tokio::task::spawn_blocking(move || {
                for source in &job.sources {
                    // A cache: the chat asks itself for what is missing.
                    let _ =
                        remember_folder_picks(&job.access, source, &job.target, job.model.as_ref());
                }
            })
            .await;
        },
        |()| Message::PicksRemembered,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::path::Path;
    use std::sync::Mutex;

    use chrono::Local;
    use ncrs_ai::{Compute, LocalModelStatus, ScriptedChatModel};
    use ncrs_fs::{FolderBackend, JournaledTrash};
    use ncrs_i18n::Language;
    use ncrs_index::Index;
    use ncrs_intake::{PlainText, TextChain};

    use super::*;

    fn resolve(folder: &Path) -> Result<PathBuf, String> {
        Ok(folder.to_path_buf())
    }

    fn ctx(dir: &Path, with_model: bool) -> Ctx {
        let index = Index::create_plain(&dir.join("index.db")).unwrap();
        let trash = JournaledTrash::new(dir.join("trash.toml"), FolderBackend(dir.join("bin")));
        let access = ChatAccess::new(
            Arc::new(Mutex::new(Some(index))),
            Arc::new(TextChain::new().with_source(PlainText)),
            Arc::new(trash),
        );
        let model: Arc<dyn ChatModel> = Arc::new(ScriptedChatModel::new(
            LocalModelStatus::Loaded {
                name: "test".to_owned(),
                memory_bytes: 0,
                compute: Compute::Cpu,
            },
            [],
        ));
        Ctx {
            lang: Language::German,
            availability: Availability::Ready(Box::new(access)),
            home: dir.to_path_buf(),
            targets: vec![dir.join("archive")],
            favourites: Vec::new(),
            sources: vec![dir.join("inbox")],
            now: Local::now(),
            resolve_folder: resolve,
            ai_on: true,
            ocr_models_present: true,
            model: with_model.then_some(model),
        }
    }

    #[test]
    fn a_job_starts_once_per_sequence_and_not_while_one_runs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), true);
        let mut state = State::default();

        assert!(begin(&mut state, &ctx, 5).is_some());
        assert!(state.picks_running);
        assert_eq!(state.picks_settled_at, Some(5));
        assert!(begin(&mut state, &ctx, 5).is_none());
        assert!(begin(&mut state, &ctx, 6).is_none());

        state.picks_running = false;
        assert!(begin(&mut state, &ctx, 5).is_none());
        assert!(begin(&mut state, &ctx, 6).is_some());
        assert_eq!(state.picks_settled_at, Some(6));
    }

    #[test]
    fn without_a_model_or_a_foreign_source_nothing_starts() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::default();
        assert!(begin(&mut state, &ctx(dir.path(), false), 5).is_none());
        assert_eq!(state.picks_settled_at, None);

        let second = tempfile::tempdir().unwrap();
        let mut only_target = ctx(second.path(), true);
        only_target.sources = only_target.targets.clone();
        assert!(begin(&mut state, &only_target, 5).is_none());
        assert!(!state.picks_running);

        let third = tempfile::tempdir().unwrap();
        let mut off = ctx(third.path(), true);
        off.ai_on = false;
        assert!(begin(&mut state, &off, 5).is_none());
    }
}
