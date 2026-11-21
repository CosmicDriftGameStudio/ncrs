//! The model's folder picks for the read folders, made in the background
//! once the pipeline is idle, so the trial finds them and shows its full
//! count at once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use iced::Task;
use ncrs_ai::ChatModel;
use ncrs_filing::chat_work::{remember_folder_picks, ChatAccess};
use ncrs_i18n::{fill_template, Language, Msg};

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
    stop: Arc<AtomicBool>,
    /// Sources finished so far; the status bar reads it.
    done: Arc<AtomicUsize>,
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
    state.picks_stop = Arc::default();
    state.picks_done = Arc::default();
    state.picks_sources.clone_from(&sources);
    Some(PicksJob {
        access: (**access).clone(),
        model: Arc::clone(model),
        target,
        sources,
        stop: Arc::clone(&state.picks_stop),
        done: Arc::clone(&state.picks_done),
    })
}

/// What the background job is doing, for the status bar: the folder being
/// prepared and how many of the read folders are through.
pub fn picks_line(state: &State, lang: Language) -> Option<String> {
    if !state.picks_running {
        return None;
    }
    let total = state.picks_sources.len();
    let done = state
        .picks_done
        .load(Ordering::Relaxed)
        .min(total.saturating_sub(1));
    let source = state.picks_sources.get(done)?;
    let name = source
        .file_name()
        .unwrap_or(source.as_os_str())
        .to_string_lossy();
    Some(fill_template(
        lang.text(Msg::StatusHermesPreparingFolder),
        &[
            ("{name}", &name),
            ("{done}", &(done + 1).to_string()),
            ("{total}", &total.to_string()),
        ],
    ))
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
                    if job.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    // A cache: the chat asks itself for what is missing.
                    let _ = remember_folder_picks(
                        &job.access,
                        source,
                        &job.target,
                        job.model.as_ref(),
                        &job.stop,
                    );
                    job.done.fetch_add(1, Ordering::Relaxed);
                }
            })
            .await;
        },
        |()| Message::PicksRemembered,
    )
}

#[cfg(test)]
mod tests {

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
    fn the_status_line_names_the_folder_and_counts_up() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path(), true);
        ctx.sources = vec![dir.path().join("inbox"), dir.path().join("scans")];
        let mut state = State::default();
        assert_eq!(picks_line(&state, Language::English), None);

        assert!(begin(&mut state, &ctx, 1).is_some());
        assert_eq!(
            picks_line(&state, Language::English).as_deref(),
            Some("Hermes is preparing folder suggestions for inbox (1/2)")
        );
        state.picks_done.store(1, Ordering::Relaxed);
        assert_eq!(
            picks_line(&state, Language::German).as_deref(),
            Some("Hermes bereitet Zielordner-Vorschläge für scans vor (2/2)")
        );
        state.picks_running = false;
        assert_eq!(picks_line(&state, Language::English), None);
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

    #[test]
    fn a_new_chat_keeps_the_job_that_runs_and_its_stop_flag() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), true);
        let mut state = State::default();
        assert!(begin(&mut state, &ctx, 5).is_some());
        let stop = Arc::clone(&state.picks_stop);

        let _ = crate::update::update(&mut state, crate::Message::NewChat, &ctx);

        assert!(state.picks_running);
        assert!(Arc::ptr_eq(&state.picks_stop, &stop));
        assert_eq!(state.picks_settled_at, Some(5));
    }

    #[test]
    fn switching_the_ai_off_stops_the_job_that_runs() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path(), true);
        let mut state = State::default();
        assert!(begin(&mut state, &ctx, 5).is_some());
        assert!(!state.picks_stop.load(Ordering::Relaxed));

        ctx.ai_on = false;
        let _ = crate::update::update(&mut state, crate::Message::NewChat, &ctx);

        assert!(state.picks_stop.load(Ordering::Relaxed));
    }
}
