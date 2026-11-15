//! The state of the filing tab: one session, from the start of the reading to
//! the run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use ncrs_filing::chat_work::{apply_overrides, LiveFile, LivePlan, LiveRun, LiveState, Override};
use ncrs_i18n::{Language, Msg};

use crate::texts::say;

#[derive(Debug, Default)]
pub struct State {
    pub session: Option<Session>,
}

#[derive(Debug)]
pub struct Session {
    pub source: PathBuf,
    pub target: PathBuf,
    pub run: Arc<LiveRun>,
    /// The last plan the run published, without the user's changes.
    pub plan: Option<LivePlan>,
    /// The planning has returned.
    pub finished: Option<Result<(), String>>,
    /// By the path relative to the source.
    pub overrides: BTreeMap<PathBuf, Override>,
    /// Relative to the source.
    pub selected: Option<PathBuf>,
    /// The name typed for the selected file.
    pub renaming: Option<String>,
    pub only_new: bool,
    pub trash_dupes: bool,
    pub closed_folders: BTreeSet<PathBuf>,
    pub filing: bool,
    pub error: Option<String>,
    /// The names of the files that are in a folder of the write folder, by the
    /// folder below it.
    pub existing: BTreeMap<PathBuf, Vec<String>>,
    /// The folders of the write folder, read when the tree shows them all.
    pub target_folders: BTreeSet<PathBuf>,
}

impl Session {
    pub fn new(source: PathBuf, target: PathBuf, run: Arc<LiveRun>) -> Self {
        Self {
            source,
            target,
            run,
            plan: None,
            finished: None,
            overrides: BTreeMap::new(),
            selected: None,
            renaming: None,
            only_new: true,
            trash_dupes: false,
            closed_folders: BTreeSet::new(),
            filing: false,
            error: None,
            existing: BTreeMap::new(),
            target_folders: BTreeSet::new(),
        }
    }

    /// The plan the user sees: what the run published with the changes on top.
    pub fn shown_plan(&self) -> Option<LivePlan> {
        self.plan
            .as_ref()
            .map(|plan| apply_overrides(plan, &self.overrides))
    }

    pub fn plan_is_done(&self) -> bool {
        self.plan.as_ref().is_some_and(|plan| plan.done)
    }

    /// Takes over what the run has published since.
    pub fn take_snapshot(&mut self) {
        let published = self
            .run
            .plan
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if published.is_some() {
            self.plan = published;
        }
    }

    /// `(looked at, all)` files: while every file waits, the files the reading
    /// has taken in so far.
    pub fn read_count(&self) -> (usize, usize) {
        let Some(plan) = &self.plan else {
            return (self.run.progress.done(), self.run.progress.total());
        };
        let total = plan.files.len();
        let waiting = count_state(&plan.files, |state| matches!(state, LiveState::Waiting));
        if waiting == total {
            return (self.run.progress.done().min(total), total);
        }
        let looking = count_state(&plan.files, |state| matches!(state, LiveState::Looking));
        (total.saturating_sub(waiting + looking), total)
    }

    pub fn source_name(&self) -> String {
        self.source.file_name().map_or_else(
            || self.source.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    pub fn file(&self, from: &Path) -> Option<LiveFile> {
        self.shown_plan()?
            .files
            .into_iter()
            .find(|file| file.from == from)
    }
}

fn count_state(files: &[LiveFile], is: impl Fn(&LiveState) -> bool) -> usize {
    files.iter().filter(|file| is(&file.state)).count()
}

/// Planning runs: the app ticks while it does.
pub fn is_running(state: &State) -> bool {
    state
        .session
        .as_ref()
        .is_some_and(|session| session.finished.is_none())
}

/// Asks the planning to end; the app calls it on quit.
pub fn stop(state: &State) {
    if let Some(session) = &state.session {
        session.run.stop.store(true, Ordering::Relaxed);
    }
}

/// The title of the tab, `None` without a session.
pub fn tab_title(state: &State, lang: Language) -> Option<String> {
    let session = state.session.as_ref()?;
    let folder = session.source_name();
    if matches!(session.finished, Some(Err(_))) {
        return Some(say(lang, Msg::FilingViewTabFailed, &[("folder", &folder)]));
    }
    if session.plan_is_done() {
        return Some(say(lang, Msg::FilingViewTabReady, &[("folder", &folder)]));
    }
    let (done, total) = session.read_count();
    Some(say(
        lang,
        Msg::FilingViewTabRunning,
        &[
            ("folder", &folder),
            ("done", &done.to_string()),
            ("total", &total.to_string()),
        ],
    ))
}
