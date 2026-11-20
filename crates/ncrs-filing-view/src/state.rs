//! The state of the filing tab: one session, from the start of the reading to
//! the run.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};

use ncrs_filing::chat_work::{apply_overrides, LiveFile, LivePlan, LiveRun, LiveState, Override};
use ncrs_i18n::{Language, Msg};

use crate::texts::say;

/// Tells a session from an earlier one when a read it started comes back.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default)]
pub struct State {
    pub session: Option<Session>,
}

#[derive(Debug)]
pub struct Session {
    pub id: u64,
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
    /// The read folder is set to copy: its files are not moved out and its
    /// duplicates are not trashed.
    pub copy_only: bool,
    pub closed_folders: BTreeSet<PathBuf>,
    pub filing: bool,
    pub error: Option<String>,
    /// The names of the files that are in a folder of the write folder, by the
    /// folder below it.
    pub existing: BTreeMap<PathBuf, Vec<String>>,
    /// The folders of the write folder, read when the tree shows them all.
    pub target_folders: BTreeSet<PathBuf>,
    /// The folders whose files are being read right now.
    pub reading: BTreeSet<PathBuf>,
    /// The folders of the write folder are being listed.
    pub listing: bool,
    /// `plan` with `overrides` on top, formed when either changes.
    pub shown: Option<LivePlan>,
    /// The version of the run's plan that `plan` holds.
    pub seen_version: u64,
}

impl Session {
    pub fn new(source: PathBuf, target: PathBuf, run: Arc<LiveRun>) -> Self {
        Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
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
            copy_only: false,
            closed_folders: BTreeSet::new(),
            filing: false,
            error: None,
            existing: BTreeMap::new(),
            target_folders: BTreeSet::new(),
            reading: BTreeSet::new(),
            listing: false,
            shown: None,
            seen_version: 0,
        }
    }

    /// The plan the user sees: what the run published with the changes on top.
    pub fn shown_plan(&self) -> Option<LivePlan> {
        self.shown_ref().map(Cow::into_owned)
    }

    /// The same without a copy, as long as nothing changed since it was formed.
    pub fn shown_ref(&self) -> Option<Cow<'_, LivePlan>> {
        match (&self.shown, &self.plan) {
            (Some(shown), _) => Some(Cow::Borrowed(shown)),
            (None, Some(plan)) => Some(Cow::Owned(apply_overrides(plan, &self.overrides))),
            (None, None) => None,
        }
    }

    fn refresh_shown(&mut self) {
        self.shown = self
            .plan
            .as_ref()
            .map(|plan| apply_overrides(plan, &self.overrides));
    }

    pub fn set_override(&mut self, from: PathBuf, change: Override) {
        self.overrides.insert(from, change);
        self.refresh_shown();
    }

    pub fn plan_is_done(&self) -> bool {
        self.plan.as_ref().is_some_and(|plan| plan.done)
    }

    /// Takes over what the run has published since.
    pub fn take_snapshot(&mut self) {
        let version = self.run.version.load(Ordering::Acquire);
        if version == self.seen_version {
            return;
        }
        let published = self
            .run
            .plan
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if published.is_some() {
            self.plan = published;
            self.seen_version = version;
            self.refresh_shown();
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
        self.shown_ref()?
            .files
            .iter()
            .find(|file| file.from == from)
            .cloned()
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
    if session.filing {
        return Some(say(lang, Msg::FilingViewTabFiling, &[("folder", &folder)]));
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
