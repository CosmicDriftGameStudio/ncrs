//! `open` and `update`: the list, the cursor and marks, the scan, the learned
//! repos and the bulk actions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use iced::widget::operation;
use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::row_scroll::{row_into_view, FOCUSED_ROW_ID};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::cleanup::CleanupMessage;
use crate::message::{MenuEntry, Message};
use crate::rows::{is_hidden_worktree, RowKind};
use crate::run;
use crate::scan::ScanRequest;
use crate::state::State;
use crate::store::{Origin, ScanStamp, Store};
use crate::tasks;
use crate::texts::store_error_text;

/// The id of the scrollable around the list.
pub const LIST_ID: &str = "repos-list";
/// The id of the filter field.
pub const FILTER_ID: &str = "repos-filter";

/// How often the tab reads the repositories again while it is in view.
pub const REFRESH_EVERY: Duration = Duration::from_secs(60);

/// How far a Page key moves the cursor.
const PAGE_ROWS: isize = 10;

pub type Outcome = (Task<Message>, Vec<Action>);

fn quiet(task: Task<Message>) -> Outcome {
    (task, Vec::new())
}

fn scroll_to_cursor() -> Task<Message> {
    row_into_view(LIST_ID, FOCUSED_ROW_ID)
}

/// Reads the list from `repos.toml` and starts what the tab needs: the state
/// of every repository and the scan of the project folders. A file that cannot
/// be read is named once and then left alone: the list stays in memory.
pub fn open(ctx: &Ctx) -> (State, Task<Message>) {
    let mut state = match ctx.file.as_deref().map(Store::read).transpose() {
        Ok(store) => State::new(store.unwrap_or_default(), ctx.file.clone()),
        Err(error) => {
            let mut unsaved = State::new(Store::default(), None);
            unsaved.failure = Some(store_error_text(&error, ctx.lang));
            unsaved
        }
    };
    state.open_roots.clone_from(&ctx.open_roots);
    let reads = refresh(&mut state, ctx);
    let scan = start_scan(&mut state, ctx, false);
    (state, Task::batch([reads, scan]))
}

/// Reads every repository again, unless a run is going: its results are newer.
fn refresh(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if state.run.as_ref().is_some_and(|run| !run.is_finished()) {
        return Task::none();
    }
    state.refreshed_at = Some(ctx.now);
    let paths: Vec<PathBuf> = state
        .store
        .repos
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    for path in paths {
        enqueue_read(state, path);
    }
    pump_reads(state, ctx)
}

fn is_stale(state: &State, ctx: &Ctx) -> bool {
    state
        .refreshed_at
        .and_then(|last| ctx.now.duration_since(last).ok())
        .is_none_or(|age| age >= REFRESH_EVERY)
}

pub(crate) fn enqueue_read(state: &mut State, path: PathBuf) {
    if !state.read_queue.contains(&path) {
        state.read_queue.push_back(path);
    }
}

pub(crate) fn pump_reads(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let limit = usize::from(ctx.settings.concurrency).max(1);
    let mut started = Vec::new();
    while state.reads_running < limit {
        let Some(path) = state.read_queue.pop_front() else {
            break;
        };
        state.reads_running += 1;
        started.push(tasks::read(path, Arc::clone(&state.stop)));
    }
    Task::batch(started)
}

/// Starts the scan. Without `manual` it only looks at the project folders,
/// and at `~` once; with it, at everything the settings name.
fn start_scan(state: &mut State, ctx: &Ctx, manual: bool) -> Task<Message> {
    if state.scanning.is_some() {
        return Task::none();
    }
    let mut settings = ctx.settings.clone();
    if !manual {
        settings.scan_home = settings.scan_home && !state.store.home_scanned;
    }
    if !settings.has_roots() {
        return Task::none();
    }
    let stop = Arc::new(AtomicBool::new(false));
    let request = ScanRequest::from_settings(
        &settings,
        ctx.home.as_deref(),
        ctx.sandbox_roots.clone(),
        Arc::clone(&stop),
    );
    state.scanning = Some(stop);
    tasks::scan(request)
}

fn persist(state: &mut State, ctx: &Ctx) {
    let Some(file) = &state.file else {
        return;
    };
    state.failure = state
        .store
        .write(file)
        .err()
        .map(|error| store_error_text(&error, ctx.lang));
}

fn is_repo_row(state: &State, path: &Path) -> bool {
    state.store.contains(path) && !is_hidden_worktree(path, &state.store, &state.readings)
}

fn repo_rows(state: &State) -> Vec<PathBuf> {
    state
        .groups()
        .into_iter()
        .flat_map(|group| group.rows)
        .filter(|row| row.kind == RowKind::Repo)
        .map(|row| row.path)
        .collect()
}

fn move_cursor(state: &mut State, delta: isize) -> Outcome {
    state.filter_focused = false;
    let keys = state.visible();
    state.marks.move_by(delta, &keys);
    (
        Task::batch([release_focus(), scroll_to_cursor()]),
        Vec::new(),
    )
}

fn extend(state: &mut State, delta: isize) -> Outcome {
    let keys = state.visible();
    state.marks.extend_by(delta, &keys);
    quiet(scroll_to_cursor())
}

fn jump(state: &mut State, last: bool) -> Outcome {
    let keys = state.visible();
    let target = if last { keys.last() } else { keys.first() };
    if let Some(path) = target.cloned() {
        state.marks.move_to(path);
    }
    quiet(scroll_to_cursor())
}

fn toggle_mark(state: &mut State, path: &Path) {
    if !is_repo_row(state, path) {
        return;
    }
    let keys = state.visible();
    state.marks.move_to(path.to_path_buf());
    state.marks.toggle(&keys);
}

fn mark_all(state: &mut State, toggle: bool) {
    let rows = repo_rows(state);
    let all_marked = !rows.is_empty() && rows.iter().all(|path| state.is_marked(path));
    if toggle && all_marked {
        state.marks.clear();
    } else {
        state.marks.mark_all(&rows);
    }
}

fn toggle_favourite(state: &mut State, path: &Path, ctx: &Ctx) {
    if state.store.toggle_favourite(path).is_some() {
        persist(state, ctx);
    }
}

fn remove(state: &mut State, path: &Path, ctx: &Ctx) {
    if state.store.remove(path) {
        state.readings.remove(path);
        state.cleanable.remove(path);
        state.read_queue.retain(|queued| queued != path);
        persist(state, ctx);
    }
}

fn open_tab(path: &Path, state: &State) -> Vec<Action> {
    match state.readings.get(path) {
        Some(reading) if reading.is_unreachable() => Vec::new(),
        _ => vec![Action::OpenGitTab(path.to_path_buf())],
    }
}

fn open_in_tabs(state: &State) -> Vec<Action> {
    let mut paths = state.marked_repos();
    if paths.is_empty() {
        paths.extend(state.cursor());
    }
    paths.retain(|path| {
        state
            .readings
            .get(path)
            .is_none_or(|read| !read.is_unreachable())
    });
    if paths.is_empty() {
        Vec::new()
    } else {
        vec![Action::OpenGitTabs(paths)]
    }
}

pub fn learn(state: &mut State, path: PathBuf, origin: Origin, ctx: &Ctx) -> Task<Message> {
    if !state.store.add(path.clone(), origin, None) {
        return Task::none();
    }
    persist(state, ctx);
    enqueue_read(state, path);
    pump_reads(state, ctx)
}

fn scanned(state: &mut State, result: &crate::scan::ScanResult, ctx: &Ctx) -> Task<Message> {
    state.scanning = None;
    let known: Vec<PathBuf> = state
        .store
        .repos
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    state.store.merge_found(&result.found);
    if !result.stopped {
        state.store.last_scan = Some(ScanStamp {
            at: ctx
                .now
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
            repos: u64::try_from(result.found.len()).unwrap_or(u64::MAX),
            millis: result.millis,
        });
        if ctx.settings.scan_home && ctx.home.is_some() {
            state.store.home_scanned = true;
        }
    }
    persist(state, ctx);
    for (path, _) in &result.found {
        if !known.contains(path) {
            enqueue_read(state, path.clone());
        }
    }
    pump_reads(state, ctx)
}

fn escape(state: &mut State) -> Outcome {
    if let Some(draft) = &state.clone {
        if !draft.running {
            state.clone = None;
        }
        return quiet(Task::none());
    }
    if state.menu.take().is_some() {
        return quiet(Task::none());
    }
    if state.filter_focused {
        state.filter_focused = false;
        return quiet(release_focus());
    }
    if state
        .run
        .as_ref()
        .is_some_and(crate::state::Run::is_finished)
    {
        state.run = None;
        return quiet(Task::none());
    }
    if !state.filter.is_empty() {
        state.filter.clear();
    }
    quiet(Task::none())
}

fn menu_picked(state: &mut State, entry: MenuEntry, ctx: &Ctx) -> Outcome {
    let target = state.menu.take();
    let message = match (entry, target) {
        (MenuEntry::Open, Some(path)) => Message::RowOpened(path),
        (MenuEntry::Favourite, Some(path)) => Message::FavouriteClicked(path),
        (MenuEntry::Remove, Some(path)) => Message::RowRemoved(path),
        (MenuEntry::Bulk(action), _) => Message::Bulk(action),
        (MenuEntry::OpenInTabs, _) => Message::OpenInTabs,
        (MenuEntry::Cleanup, _) => Message::Cleanup(CleanupMessage::Open),
        (_, None) => return quiet(Task::none()),
    };
    update(state, message, ctx)
}

pub fn update(state: &mut State, incoming: Message, ctx: &Ctx) -> Outcome {
    state.open_roots.clone_from(&ctx.open_roots);
    let message = match crate::cleanup::update(state, incoming, ctx) {
        Ok(outcome) => return outcome,
        Err(other) => other,
    };
    let message = match crate::clone::update(state, message, ctx) {
        Ok(outcome) => return outcome,
        Err(other) => other,
    };
    match message {
        Message::Up =>move_cursor(state, -1),
        Message::Down => move_cursor(state, 1),
        Message::PageUp => move_cursor(state, -PAGE_ROWS),
        Message::PageDown => move_cursor(state, PAGE_ROWS),
        Message::First => jump(state, false),
        Message::Last => jump(state, true),
        Message::ExtendUp => extend(state, -1),
        Message::ExtendDown => extend(state, 1),
        Message::Mark => {
            if let Some(path) = state.cursor() {
                toggle_mark(state, &path);
            }
            quiet(Task::none())
        }
        Message::MarkAll => {
            mark_all(state, false);
            quiet(Task::none())
        }
        Message::AllToggled => {
            mark_all(state, true);
            quiet(Task::none())
        }
        Message::Enter => (
            Task::none(),
            state
                .cursor()
                .map(|path| open_tab(&path, state))
                .unwrap_or_default(),
        ),
        Message::ToggleFavouriteAtCursor => {
            if let Some(path) = state.cursor().filter(|path| is_repo_row(state, path)) {
                toggle_favourite(state, &path, ctx);
            }
            quiet(Task::none())
        }
        Message::RemoveAtCursor => {
            let gone = state.cursor().filter(|path| {
                state
                    .readings
                    .get(path)
                    .is_some_and(|read| read.is_unreachable())
            });
            if let Some(path) = gone {
                remove(state, &path, ctx);
            }
            quiet(Task::none())
        }
        Message::FocusFilter => {
            state.filter_focused = true;
            quiet(operation::focus(FILTER_ID))
        }
        Message::FilterChanged(text) => {
            state.filter = text;
            quiet(Task::none())
        }
        Message::FilterSubmitted => {
            state.filter_focused = false;
            quiet(release_focus())
        }
        Message::OnlyNeedingActionToggled => {
            state.only_needing_action = !state.only_needing_action;
            quiet(Task::none())
        }
        Message::Escape => escape(state),
        Message::OpenMenuKey => {
            state.menu = state.cursor();
            quiet(Task::none())
        }
        Message::OpenMenuAt(path) => {
            state.marks.move_to(path.clone());
            state.menu = Some(path);
            quiet(Task::none())
        }
        Message::CloseMenu => {
            state.menu = None;
            quiet(Task::none())
        }
        Message::MenuPicked(entry) => menu_picked(state, entry, ctx),
        Message::RowPressed(path) => {
            state.marks.move_to(path);
            quiet(Task::none())
        }
        Message::RowToggled(path) => {
            toggle_mark(state, &path);
            quiet(Task::none())
        }
        Message::FavouriteClicked(path) => {
            toggle_favourite(state, &path, ctx);
            quiet(Task::none())
        }
        Message::RowOpened(path) | Message::ShowInGitTab(path) => {
            (Task::none(), open_tab(&path, state))
        }
        Message::RowRemoved(path) => {
            remove(state, &path, ctx);
            quiet(Task::none())
        }
        Message::Hovered(path) => {
            state.hovered = path;
            quiet(Task::none())
        }
        Message::GroupToggled(key) => {
            if !state.folded.remove(&key) {
                state.folded.insert(key);
            }
            quiet(Task::none())
        }
        Message::ScanNow => {
            if let Some(flag) = &state.scanning {
                flag.store(true, Ordering::Relaxed);
                return quiet(Task::none());
            }
            quiet(start_scan(state, ctx, true))
        }
        Message::Scanned(result) => quiet(scanned(state, &result, ctx)),
        Message::AddRepo => (Task::none(), vec![Action::PickFolder]),
        Message::RepoPicked(folder) => {
            let Some(root) = ncrs_git::repository_root_of(&folder) else {
                state.failure = Some(fill_template(
                    ctx.lang.text(Msg::ReposNotARepo),
                    &[("{folder}", &folder.to_string_lossy())],
                ));
                return quiet(Task::none());
            };
            state.failure = None;
            quiet(learn(state, root, Origin::Manual, ctx))
        }
        Message::Learn(root) => {
            if !ctx.settings.learn_from_panel {
                return quiet(Task::none());
            }
            quiet(learn(state, root, Origin::Learned, ctx))
        }
        Message::ListScrolled(offset) => {
            state.list_scroll = offset;
            quiet(Task::none())
        }
        Message::CleanableRead(path, count) => {
            match count {
                Some(count) => state.cleanable.insert(path, count),
                None => state.cleanable.remove(&path),
            };
            quiet(Task::none())
        }
        Message::SummaryRead(path, reading) => {
            state.reads_running = state.reads_running.saturating_sub(1);
            if state.store.contains(&path) {
                state.readings.insert(path, *reading);
            }
            quiet(pump_reads(state, ctx))
        }
        Message::Tick => quiet(refresh(state, ctx)),
        Message::Shown if is_stale(state, ctx) => quiet(refresh(state, ctx)),
        Message::Shown => quiet(Task::none()),
        Message::Bulk(_) if state.cleanup.is_some() => quiet(Task::none()),
        Message::Bulk(action) => quiet(run::start(state, action, ctx)),
        Message::StepDone(path, executed) => {
            run::finish(state, ctx, path, *executed);
            quiet(run::pump(state, ctx))
        }
        Message::RetryFailed => quiet(run::retry_failed(state, ctx)),
        Message::OnlyProblemsToggled => {
            if let Some(run) = state.run.as_mut() {
                run.only_problems = !run.only_problems;
            }
            quiet(Task::none())
        }
        Message::CloseResult => {
            if state
                .run
                .as_ref()
                .is_some_and(crate::state::Run::is_finished)
            {
                state.run = None;
            }
            quiet(Task::none())
        }
        Message::OpenInTabs => (Task::none(), open_in_tabs(state)),
        Message::ShowDetails(path) => {
            if let Some(run) = state.run.as_mut() {
                run.details = Some(path);
            }
            quiet(Task::none())
        }
        Message::ClearDetails => {
            if let Some(run) = state.run.as_mut() {
                run.details = None;
            }
            quiet(Task::none())
        }
        // Taken by `cleanup::update` above, and only while the pane is open.
        Message::Left | Message::Right | Message::SwitchFocus | Message::Cleanup(_) => {
            quiet(Task::none())
        }
        // Taken by `clone::update` above.
        Message::CloneOpen
        | Message::CloneUrlChanged(_)
        | Message::ClonePickTarget
        | Message::CloneTargetPicked(_)
        | Message::CloneRun
        | Message::CloneDone(_)
        | Message::CloneClose => quiet(Task::none()),
    }
}
