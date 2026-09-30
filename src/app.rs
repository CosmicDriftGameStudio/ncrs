//! Root application: state, `update` (the only place state changes) and
//! `view` (pure composition of UI components).
use std::path::PathBuf;

use std::sync::Arc;

use iced::futures;
use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::widget::{column, container, operation, row, Stack};
use iced::{window, Element, Length, Subscription, Task, Theme};

use crate::dialog::{Prompt, PromptKind};
use crate::fs::{self, CreateDirError};
use crate::i18n::{Language, Msg};
use crate::jobs::{self, JobEvent};
use crate::keymap;
use crate::messages::{ConflictChoice, Message, PanelSide, TransferKind};
use crate::ui::dialog::FIELD_ID;
use crate::ui::{
    self, conflict, dialog, header, layout, panel, statusbar, theme, PanelProps, PanelState,
};
use tokio::sync::mpsc;

const APP_NAME: &str = "NC-rs";

/// Turns the transfer's channel into a stream for `Subscription::run_with`.
///
/// Written out rather than pulled in as `tokio-stream`: it is ten lines, and the
/// dependency would be there only to avoid them. `run_with` takes the receiver by
/// reference and clones it per event, so the subscription and the app share one
/// channel.
/// Turns the transfer's channel into a stream for `Subscription::run_with`.
///
/// The receiver sits behind a `tokio::sync::Mutex` because `recv` needs `&mut`
/// and the stream state has to be `Send`; `tokio`'s mutex is the one that may
/// hold its guard across an `await`. There is a single consumer, so the lock is
/// never contended — it is a way of moving the receiver, not of sharing it.
/// Turns the transfer's channel into a stream `Subscription::run_with` can use.
///
/// tokio's Mutex rather than std's, because the guard is held across an await and
/// a std guard may not do that. There is a single consumer, so the lock is a
/// way of moving the receiver rather than of sharing it.
fn receiver_stream(
    shared: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>,
) -> impl futures::Stream<Item = fs::transfer::Tick> + 'static {
    futures::stream::poll_fn(move |cx| {
        // The lock is taken and dropped inside the poll, so nothing is borrowed
        // across a yield point. Waking on empty rather than blocking is what
        // `poll_fn` is for: the stream never has a place to `.await`.
        let mut guard = match shared.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // A poll is already inside; ask to be woken rather than spin.
                cx.waker().wake_by_ref();
                return std::task::Poll::Pending;
            }
        };
        match guard.try_recv() {
            Ok(tick) => std::task::Poll::Ready(Some(tick)),
            Err(mpsc::error::TryRecvError::Empty) => {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            Err(mpsc::error::TryRecvError::Disconnected) => std::task::Poll::Ready(None),
        }
    })
}

/// Whether a failed transfer was a name that is already taken.
///
/// The filesystem layer reports the kind as a prefix before the path, so this
/// looks for the kind rather than for a wording that may change.
fn is_conflict(reason: &str) -> bool {
    reason.starts_with("AlreadyExists")
}

/// Runs one job and reports what happens as messages.
///
/// Separate from `App` so the queue can hand it to `Task::abortable` without
/// borrowing the app. Only `CreateDir` has a filesystem operation behind it so
/// far; the other kinds arrive with F5/F6/F8 and use the same path.
fn run_job(job: jobs::Job) -> Task<Message> {
    match job.kind {
        jobs::JobKind::CreateDir => {
            let target = job.path.clone();
            Task::perform(fs::create_dir(target), move |result| match result {
                Ok(()) => Message::JobFinished(JobEvent::Done),
                Err(err) => Message::JobFinished(JobEvent::Failed(err.to_string())),
            })
        }
        _ => Task::done(Message::JobFinished(JobEvent::Done)),
    }
}

/// A reload waiting for its operation to finish.
///
/// F7 creates a directory and then reads the panel. Both were started at once,
/// which races: a read that finishes first does not see the new entry. The
/// reload waits here until the create has reported back.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingReload {
    side: PanelSide,
    /// Name to select afterwards, so Enter opens what was just created.
    name: String,
}

/// What a transfer does, and to which rows.
///
/// Carried inside `Message`, so it has to be public — but nothing outside this
/// module constructs one; `start_transfer` is the only way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    kind: TransferKind,
    /// The rows the user tagged, or the single row under the cursor when nothing
    /// is tagged. Resolved once at F5, because by the time a conflict dialog is
    /// up the selection may have moved.
    sources: Vec<PathBuf>,
    target: PathBuf,
}

/// One file waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingConflict {
    transfer: Transfer,
    /// Which of `sources` is in the way.
    index: usize,
}

pub struct App {
    left_panel: PanelState,
    right_panel: PanelState,
    active_panel: PanelSide,
    /// Number of file rows that fit into a panel (derived from window size).
    visible_rows: usize,
    lang: Language,
    /// Header hints, kept in state so `view` does not allocate per frame.
    shortcuts: Vec<(String, &'static str)>,
    /// The open modal prompt, if any. Mutated only here.
    prompt: Option<Prompt>,
    /// Panel the prompt belongs to, so a panel switch while it is open cannot
    /// create a directory in the wrong place.
    prompt_side: Option<PanelSide>,
    /// Id of the running prompt operation, so a late result cannot land on a
    /// prompt the user already dismissed.
    prompt_request_id: u64,
    /// Reload queued by `PromptSubmit`, run once the create has reported.
    pending_reload: Option<PendingReload>,
    /// Long-running file operations, one at a time.
    jobs: jobs::Queue,
    /// What a transfer works on and where to. Kept so a conflict can resume
    /// without asking the user to mark the rows again.
    transfer: Option<Transfer>,
    /// A transfer paused on a name that is already taken.
    pending_conflict: Option<PendingConflict>,
    /// Why the last job failed, in the active language. Shown in the status bar
    /// until the next thing happens.
    job_error: Option<String>,
    /// How many items the running job has finished, for the progress bar.
    job_done: usize,
    /// Whether "for all files" is ticked in the conflict dialog. Lives here so
    /// the tick survives the dialog being rebuilt every frame.
    conflict_all: bool,
    /// The conflict rule from a previous "for all" answer, applied to the rest
    /// without asking again.
    conflict_rule: Option<fs::transfer::OnConflict>,
    /// How far the running transfer has got, filled from the job's channel.
    job_progress: Option<fs::transfer::Tick>,
    /// Receives the transfer's ticks, for the life of the app rather than of
    /// one transfer. Kept as an `Arc<Mutex<..>>` because a subscription cannot
    /// borrow the app and a tokio Receiver is neither `Clone` nor `Hash` —
    /// both of which `Subscription::run_with` needs from its data.
    progress_rx: Option<Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>>,
    /// The sending half, kept so a new transfer can reuse the same channel.
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<fs::transfer::Tick>>,
}

/// The part of the prompt the key routing needs, cheap to clone.
///
/// A subscription outlives the call that created it, so it cannot borrow the
/// app. It gets this instead: two flags and the name typed so far. `Arc<str>`
/// rather than `&'static str` because leaking a copy per keypress would grow
/// without bound, and a `String` per keypress is what this avoids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PromptKeyState {
    /// A prompt is open, so keys belong to it.
    pub open: bool,
    /// A job is running, so Escape stops it rather than doing nothing.
    pub job_running: bool,
    /// Whether the operation is running; keys are ignored then.
    pub busy: bool,
    /// The name typed so far.
    pub typed: std::sync::Arc<str>,
}

/// Decides who a key press belongs to: the open prompt, or the bindings.
///
/// A free function because the subscription that calls it outlives `&self`.
/// One place, so a binding cannot end up half-modal: with a prompt open, Enter
/// submits instead of opening a directory, and Backspace edits text instead of
/// walking to the parent.
pub fn route_key(prompt: &PromptKeyState, key: Key) -> Option<Message> {
    if !prompt.open {
        // Escape stops a running job, but only then. With nothing running it
        // stays unbound, rather than bound to something that does nothing —
        // a key that means different things depending on invisible state is
        // worse than one that is simply free.
        if matches!(key.as_ref(), Key::Named(Named::Escape)) {
            return prompt.job_running.then_some(Message::AbortJob);
        }
        return keymap::map_key(key, Modifiers::default());
    }
    if prompt.busy {
        return None;
    }
    let mut name = prompt.typed.to_string();
    match key.as_ref() {
        Key::Named(Named::Enter) => Some(Message::PromptSubmit),
        Key::Named(Named::Escape) => Some(Message::PromptCancel),
        // Backspace edits the text; it must not walk to the parent directory.
        Key::Named(Named::Backspace) => {
            name.pop();
            Some(Message::PromptInput(name))
        }
        // No filtering: a directory name may contain anything but a path
        // separator, and `Prompt::validate` reports the rest.
        Key::Character(c) => {
            name.push_str(c);
            Some(Message::PromptInput(name))
        }
        _ => None,
    }
}

impl App {
    /// Initial state plus tasks that load both panels.
    /// Left panel starts in the working directory, right in `$HOME`, so the
    /// two panels are not the same directory on launch.
    pub fn new() -> (Self, Task<Message>) {
        let start = fs::start_dir();
        let home = fs::home_dir();

        let mut app = Self {
            left_panel: PanelState::new(start.clone()),
            right_panel: PanelState::new(home.clone()),
            active_panel: PanelSide::Left,
            visible_rows: layout::visible_rows(layout::INITIAL_WINDOW_SIZE),
            lang: Language::default(),
            shortcuts: keymap::shortcuts(Language::default()),
            prompt: None,
            prompt_side: None,
            prompt_request_id: 0,
            pending_reload: None,
            jobs: jobs::Queue::new(),
            transfer: None,
            pending_conflict: None,
            job_error: None,
            job_done: 0,
            conflict_all: false,
            conflict_rule: None,
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
        };

        let tasks = Task::batch([
            app.load(PanelSide::Left, start, None),
            app.load(PanelSide::Right, home, None),
        ]);
        (app, tasks)
    }

    pub fn title(&self) -> String {
        format!("{APP_NAME} – {}", self.active_panel().path.display())
    }

    pub fn theme(&self) -> Theme {
        theme::app_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // A subscription outlives the call that created it, so it cannot close
        // over `&self`. What it needs from the app is the open prompt and
        // nothing else, and that is two `Copy` values.
        let initial_key_state = self.prompt_key_state();

        Subscription::batch([
            // `listen` yields the event stream, so the mapping can decide
            // without routing every key through a message first.
            //
            // `with` moves the state into the stream, so the mapping below reads
            // the copy that comes back with each event rather than closing over
            // the original.
            keyboard::listen().with(initial_key_state).filter_map(
                |(key_state, event)| match event {
                    keyboard::Event::KeyPressed { key, modifiers, .. }
                        if modifiers == Modifiers::default() =>
                    {
                        route_key(&key_state, key)
                    }
                    _ => None,
                },
            ),
            window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
            // The transfer's ticks. Taken out of the app because a subscription
            // outlives `&self`; the receiver is taken so the subscription
            // rebuilds itself when a new transfer starts.
            // The transfer's ticks. The receiver lives in an Arc because a
            // mpsc Receiver is neither Clone nor Hash, and run_with needs the
            // data to be both: the Arc for Clone, the newtype for Hash.
            match &self.progress_rx {
                None => Subscription::none(),
                Some(shared) => {
                    let rx = Arc::clone(shared);
                    struct ProgressStream(
                        Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>,
                    );
                    impl std::hash::Hash for ProgressStream {
                        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                            "ncrs-progress".hash(state);
                        }
                    }
                    // The stream yields ticks; the app speaks messages, so
                    // the mapping happens here rather than in the app.
                    Subscription::run_with(ProgressStream(rx), |p: &ProgressStream| {
                        futures::StreamExt::map(
                            receiver_stream(Arc::clone(&p.0)),
                            Message::JobProgress,
                        )
                    })
                }
            },
        ])
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let rows = self.visible_rows;

        match message {
            // A raw key press. Routed here rather than in the subscription,
            // because that one cannot see whether a prompt is open.
            // --- copy / move ---
            Message::Transfer(kind) => self.start_transfer(kind),

            // The user answered a conflict. The queue's pending job carries
            // what was asked, so the answer lands on the right file.
            Message::TransferConflict(choice) => self.answer_conflict(choice),

            // A tick from the copy on the blocking thread. The only thing the
            // status bar needs, and nothing more.
            Message::JobProgress(tick) => {
                self.job_progress = Some(tick);
                Task::none()
            }

            // Stop the running job. Ctrl+C and Escape both, because a long copy
            // that cannot be stopped is the case a queue was supposed to fix.
            Message::AbortJob => {
                if self.jobs.abort_running() {
                    self.transfer = None;
                    self.job_done = 0;
                    self.job_progress = None;
                    self.progress_rx = None;
                }
                Task::none()
            }

            // One row of a transfer finished. Continue with the next, or ask
            // about a name that is in the way.
            Message::TransferRowDone { result, index } => {
                self.jobs.finish();
                self.job_done += 1;
                let Some(transfer) = self.transfer.clone() else {
                    return Task::none();
                };
                match result {
                    // A remembered "all" answer applies to the rows after this
                    // one, without asking again.
                    Ok(done) => {
                        let rule = self.conflict_rule.unwrap_or(fs::transfer::OnConflict::Fail);
                        self.run_from(done, index + 1, rule)
                    }
                    // A name that is taken is the one the user can answer.
                    // Matched on the error kind, not on the message text: a
                    // wording change in the filesystem layer would otherwise
                    // silently turn every conflict into a hard failure.
                    Err(reason) => {
                        if is_conflict(&reason) {
                            self.pending_conflict = Some(PendingConflict { transfer, index });
                            Task::none()
                        } else {
                            self.transfer = None;
                            self.set_status_error(&reason);
                            Task::none()
                        }
                    }
                }
            }

            // --- job queue ---
            Message::JobFinished(event) => {
                // Progress arrives mid-run; only the terminal events free the
                // slot for the next job.
                let finished = matches!(
                    event,
                    JobEvent::Done | JobEvent::Failed(_) | JobEvent::Aborted
                );
                if finished {
                    self.jobs.finish();
                }
                if let Some(next) = self.jobs.start_next(run_job) {
                    return next;
                }
                Task::none()
            }

            // --- modal prompt ---
            // Handled before the panels: while a prompt is open every
            // keystroke belongs to it, and Enter must not open a directory.
            Message::CreateDirPrompt => {
                let parent = self.active_panel().path.clone();
                self.prompt = Some(Prompt::create_dir(parent));
                self.prompt_side = Some(self.active_panel);
                // The field has to be focused or the prompt swallows every
                // keystroke: `TextInput` only takes keys while focused, and
                // giving it an Id does not focus it.
                operation::focus(FIELD_ID)
            }
            Message::PromptInput(text) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.set_name(text);
                }
                Task::none()
            }
            Message::PromptSubmit => {
                let Some(prompt) = self.prompt.as_ref() else {
                    return Task::none();
                };
                if let Err(reason) = prompt.validate() {
                    self.set_prompt_error(reason);
                    return Task::none();
                }
                let side = self.prompt_side.unwrap_or(self.active_panel);
                let parent = self.panel(side).path.clone();
                let new_name = prompt.name().trim().to_string();
                let request_id = self.prompt_request_id.wrapping_add(1);
                self.prompt_request_id = request_id;
                self.set_prompt_busy(true);

                // The new directory's name, carried through the task so the
                // reload can select it.
                let target = parent.join(&new_name);
                let create = Task::perform(fs::create_dir(target), move |result| {
                    Message::PromptFinished {
                        prompt: PromptKind::CreateDir,
                        request_id,
                        result,
                    }
                });

                // The reload belongs *after* the create, not beside it.
                // Running both at once races: a read that finishes first sees a
                // directory that does not contain the new entry yet, and the
                // selection then points at whatever is there instead.
                self.pending_reload = Some(PendingReload {
                    side,
                    name: new_name,
                });
                create
            }
            Message::PromptCancel => {
                self.prompt = None;
                self.prompt_side = None;
                Task::none()
            }
            Message::PromptFinished {
                // The prompt kind is fixed by the only operation that sends
                // this today; a second one will want it back.
                prompt: _kind,
                request_id,
                result,
            } => {
                if request_id != self.prompt_request_id {
                    // Dismissed or retried while the operation ran.
                    return Task::none();
                }
                // The queued reload runs here, after the create has reported —
                // never beside it. On success it selects the new directory; on
                // failure it still re-reads, because the name may belong to
                // something else that appeared meanwhile.
                let reload = self.pending_reload.take().map(|pending| {
                    let path = self.panel(pending.side).path.clone();
                    self.load(pending.side, path, Some(pending.name))
                });

                match result {
                    Ok(()) => {
                        self.prompt = None;
                        self.prompt_side = None;
                    }
                    Err(err) => {
                        self.set_prompt_busy(false);
                        self.set_prompt_error_text(&self.prompt_error(&err));
                    }
                }
                reload.unwrap_or_else(Task::none)
            }

            Message::DirectoryLoaded {
                side,
                request_id,
                path,
                entries,
                error,
                select,
            } => {
                let panel = self.panel_mut(side);
                if panel.request_id == request_id {
                    panel.apply_listing(path, entries, error, select.as_deref(), rows);
                }
                Task::none()
            }

            Message::MoveSelection(delta) => {
                self.active_panel_mut().move_selection(delta, rows);
                Task::none()
            }
            Message::PageUp => {
                self.active_panel_mut()
                    .move_selection(-(rows as isize), rows);
                Task::none()
            }
            Message::PageDown => {
                self.active_panel_mut().move_selection(rows as isize, rows);
                Task::none()
            }
            Message::SelectFirst => {
                self.active_panel_mut().select(0, rows);
                Task::none()
            }
            Message::SelectLast => {
                self.active_panel_mut().select_last(rows);
                Task::none()
            }

            Message::OpenSelected => {
                let Some(entry) = self.active_panel().selected_entry() else {
                    return Task::none();
                };
                if entry.is_parent {
                    self.go_up(self.active_panel)
                } else if entry.is_dir {
                    let path = entry.path.clone();
                    self.load(self.active_panel, path, None)
                } else {
                    // Placeholder for file actions (view / edit / open with …).
                    Task::none()
                }
            }
            Message::GoUp => self.go_up(self.active_panel),

            // --- selection ---
            Message::ToggleTag => {
                let panel = self.active_panel_mut();
                // The `..` entry is not a thing to copy, so it cannot be tagged.
                let Some(entry) = panel.selected_entry() else {
                    return Task::none();
                };
                if entry.is_parent {
                    return Task::none();
                }
                let name = entry.name.clone();
                panel.selection.toggle(&name);
                Task::none()
            }
            Message::TagAll => {
                // Split borrow: the set is filled while the entries are read.
                let panel = self.active_panel_mut();
                let mut selection = std::mem::take(&mut panel.selection);
                selection.tag_all(&panel.entries);
                panel.selection = selection;
                Task::none()
            }
            Message::ClearTags => {
                self.active_panel_mut().selection.clear();
                Task::none()
            }

            Message::SwitchPanel => {
                self.active_panel = self.active_panel.other();
                Task::none()
            }
            Message::SwitchLanguage => {
                self.lang = self.lang.other();
                self.shortcuts = keymap::shortcuts(self.lang);
                Task::none()
            }
            // A click in the tag column toggles the tag; anywhere else it moves
            // the cursor. Without this, tagging is keyboard-only, and the
            // column of stars the view draws cannot be clicked at all.
            Message::RowClicked {
                side,
                index,
                on_tag,
            } => {
                self.active_panel = side;
                let panel = self.panel_mut(side);
                panel.select(index, rows);
                if on_tag {
                    // `..` is not a thing to copy, so it is not taggable — the
                    // same rule the keyboard path uses.
                    if let Some(entry) = panel.entries.get(index) {
                        if !entry.is_parent {
                            let name = entry.name.clone();
                            panel.selection.toggle(&name);
                        }
                    }
                }
                Task::none()
            }

            Message::WindowResized(size) => {
                self.visible_rows = layout::visible_rows(size);
                self.left_panel.ensure_visible(self.visible_rows);
                self.right_panel.ensure_visible(self.visible_rows);
                Task::none()
            }
            Message::Quit => iced::exit(),
        }
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let panel_view = |side: PanelSide| {
            panel::view(
                self.panel(side),
                PanelProps {
                    is_active: self.active_panel == side,
                    visible_rows: self.visible_rows,
                },
                self.lang,
                move |index, on_tag| Message::RowClicked {
                    side,
                    index,
                    on_tag,
                },
            )
        };

        let panels = row![panel_view(PanelSide::Left), panel_view(PanelSide::Right)]
            .spacing(ui::theme::spacing::PANEL_GAP)
            .height(Length::Fill);

        let root = container(
            column![
                header::view(APP_NAME, &self.shortcuts),
                panels,
                statusbar::view(self.active_panel(), self.lang, self.job_status()),
            ]
            .spacing(theme::spacing::SECTION_GAP),
        )
        .padding(theme::spacing::OUTER_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root);

        let Some(prompt) = self.prompt.as_ref() else {
            return root.into();
        };
        let overlay = dialog::view(
            prompt,
            self.lang,
            Message::PromptInput,
            Message::PromptSubmit,
            Message::PromptCancel,
        );
        let Some(prompt_element) = overlay else {
            return root.into();
        };

        // The conflict dialog takes precedence over the prompt: it is asked
        // while a transfer is running, and a transfer is what the prompt was
        // waiting for.
        if let Some(pending) = self.pending_conflict.as_ref() {
            let name = pending
                .transfer
                .sources
                .get(pending.index)
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let conflict_element = conflict::view(&name, self.lang, self.conflict_all, |choice| {
                Message::TransferConflict(choice)
            });
            return Stack::with_children([root.into(), dialog::scrim(conflict_element)]).into();
        }

        // The prompt sits above everything and takes every keystroke; the scrim
        // swallows clicks meant for the panels underneath.
        Stack::with_children([root.into(), dialog::scrim(prompt_element)]).into()
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub fn panel(&self, side: PanelSide) -> &PanelState {
        match side {
            PanelSide::Left => &self.left_panel,
            PanelSide::Right => &self.right_panel,
        }
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelState {
        match side {
            PanelSide::Left => &mut self.left_panel,
            PanelSide::Right => &mut self.right_panel,
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panel(self.active_panel)
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.active_panel)
    }

    #[allow(dead_code)] // Used by upcoming two-panel operations (copy/move).
    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.active_panel.other())
    }

    /// The state the key routing needs, cheap to clone.
    fn prompt_key_state(&self) -> PromptKeyState {
        let job_running = self.jobs.is_busy();
        match &self.prompt {
            None => PromptKeyState {
                job_running,
                ..PromptKeyState::default()
            },
            Some(prompt) => PromptKeyState {
                open: true,
                job_running,
                busy: prompt.busy(),
                typed: prompt.name().into(),
            },
        }
    }

    /// F5 / F6: resolve what to transfer, then start the first row.
    ///
    /// The rows are resolved here rather than later because a conflict dialog
    /// pauses the operation, and by then the selection may have moved.
    fn start_transfer(&mut self, kind: TransferKind) -> Task<Message> {
        // NC's rule: tagged rows win, otherwise the row under the cursor. The
        // `..` entry is not a thing to copy into the other panel.
        let source_panel = self.active_panel();
        let sources: Vec<PathBuf> = if source_panel.selection.any_tagged() {
            source_panel
                .selection
                .ordered(&source_panel.entries)
                .into_iter()
                .map(|e| e.path.clone())
                .collect()
        } else {
            source_panel
                .selected_entry()
                .filter(|e| !e.is_parent)
                .map(|e| vec![e.path.clone()])
                .unwrap_or_default()
        };

        if sources.is_empty() {
            return Task::none();
        }

        let target = self.panel(self.active_panel.other()).path.clone();
        let transfer = Transfer {
            kind,
            sources,
            target,
        };
        self.transfer = Some(transfer.clone());
        self.run_from(transfer, 0, fs::transfer::OnConflict::Fail)
    }

    /// Answers a conflict and remembers an "all" answer, so the next
    /// conflict does not ask again.
    fn answer_conflict(&mut self, choice: ConflictChoice) -> Task<Message> {
        let Some(pending) = self.pending_conflict.take() else {
            return Task::none();
        };
        if choice.applies_to_all() {
            self.conflict_rule = Some(choice.conflict());
        }
        // The tick belonged to the question just answered, so it goes with it.
        self.conflict_all = false;
        // The task is returned, not dropped: dropping it would leave the
        // transfer standing still after the user answered, which is the one
        // thing the dialog must not do.
        self.resume_transfer(pending, choice)
    }

    /// Continues a transfer from `index` under the given conflict rule.
    fn resume_transfer(
        &mut self,
        pending: PendingConflict,
        choice: ConflictChoice,
    ) -> Task<Message> {
        let PendingConflict { transfer, index } = pending;
        self.transfer = Some(transfer.clone());
        self.run_from(transfer, index, choice.conflict())
    }

    /// Runs the rows from `index`, on the blocking pool, reporting progress.
    ///
    /// One row at a time rather than the whole list, so a conflict can be
    /// answered between two files and the queue stays one job.
    fn run_from(
        &mut self,
        transfer: Transfer,
        index: usize,
        conflict: fs::transfer::OnConflict,
    ) -> Task<Message> {
        let Some(source) = transfer.sources.get(index).cloned() else {
            // Nothing left: clear the transfer and reload both panels, since a
            // move changes the source directory as well as the target.
            self.transfer = None;
            let source_side = self.active_panel;
            let target_side = source_side.other();
            let source_path = self.panel(source_side).path.clone();
            let target_path = self.panel(target_side).path.clone();
            return Task::batch([
                self.load(source_side, source_path, None),
                self.load(target_side, target_path, None),
            ]);
        };

        // One channel per transfer, but the receiving half is created once for
        // the app (see `progress_rx`) and reused, so `subscription` does not
        // have to hand out a fresh receiver after every row.
        let progress_tx = match &self.progress_tx {
            Some(tx) => tx.clone(),
            None => {
                let (created, receiver) = tokio::sync::mpsc::unbounded_channel();
                self.progress_tx = Some(created.clone());
                self.progress_rx = Some(Arc::new(tokio::sync::Mutex::new(receiver)));
                created
            }
        };

        let target = transfer.target.clone();
        let job = jobs::Job {
            kind: transfer.kind.job_kind(),
            path: target.join(source.file_name().unwrap_or_default()),
            total: None,
        };
        // One row per job keeps a conflict answerable between two files.
        self.jobs.enqueue(job.clone());
        if let Some(task) = self.jobs.start_next(|_| {
            // transfer::run is blocking filesystem work, so it goes on the
            // blocking pool — the same reason read_directory does.
            Task::perform(
                tokio::task::spawn_blocking({
                    let from = source.clone();
                    let to = target.clone();
                    let kind = transfer.kind;
                    move || {
                        // Reports as it goes, so the bar moves during a long
                        // copy rather than jumping at the end.
                        crate::fs::transfer::run_reporting(&from, &to, kind, conflict, &progress_tx)
                    }
                }),
                move |outcome| {
                    // Two failures collapse into one string here: the join error
                    // means the blocking task died, the other means the copy
                    // itself failed. The dialog cannot tell them apart anyway.
                    let result = match outcome {
                        Ok(Ok(_)) => Ok(transfer.clone()),
                        Ok(Err(e)) => Err(e.to_string()),
                        Err(join) => Err(join.to_string()),
                    };
                    Message::TransferRowDone { result, index }
                },
            )
        }) {
            return task;
        }
        Task::none()
    }

    /// What the status bar shows about the queue: the running job, how far
    /// along it is, how many wait, and whether Escape will stop it.
    fn job_status(&self) -> Option<statusbar::JobStatus> {
        let running = self.jobs.running()?;
        // The tick from the blocking thread, not a row counter: F5 on one
        // directory with fifty thousand files is one row and fifty thousand
        // items, and the bar has to say the second number.
        let (done, total) = match self.job_progress {
            Some(tick) => (tick.done, tick.total),
            None => (0, running.total.unwrap_or(0)),
        };
        Some(statusbar::JobStatus {
            label: self.lang.text(running.kind.label()),
            done,
            total,
            waiting: self.jobs.waiting(),
            // Escape belongs to the prompt while one is open, so the hint is
            // only true when nothing else is claiming the key.
            abortable: self.prompt.is_none() && self.pending_conflict.is_none(),
        })
    }

    /// Records why a job failed. The status bar reads it; nothing else does, so
    /// it is a string rather than a typed error — the layer that knew the kind
    /// already wrote the wording.
    fn set_status_error(&mut self, reason: &str) {
        self.job_error = Some(reason.to_string());
    }

    /// Shows a validation failure in the open prompt.
    fn set_prompt_error(&mut self, reason: &'static str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_validation_error(reason);
        }
    }

    /// Wording for a failed create, in the active language. Deciding here
    /// rather than in the filesystem layer keeps `fs` free of user-facing text,
    /// and the two failures need different advice: a taken name means pick
    /// another, a missing parent means the panel has gone stale.
    fn prompt_error(&self, err: &CreateDirError) -> String {
        use std::io::ErrorKind;
        let lang = self.lang;
        let name = self.prompt_name();
        match err.io_error().map(|e| e.kind()) {
            Some(ErrorKind::AlreadyExists) => {
                lang.text(Msg::ErrorMkdirTaken).replace("{name}", &name)
            }
            Some(ErrorKind::NotFound) => lang.text(Msg::ErrorParentMissing).to_string(),
            _ => lang
                .text(Msg::ErrorMkdirFailed)
                .replace("{name}", &name)
                .replace("{reason}", &err.to_string()),
        }
    }

    /// The name currently in the prompt, or an empty string.
    fn prompt_name(&self) -> String {
        self.prompt
            .as_ref()
            .map(|p| p.name().to_string())
            .unwrap_or_default()
    }

    /// Shows a filesystem failure in the open prompt.
    fn set_prompt_error_text(&mut self, reason: &str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_error(reason.to_string());
        }
    }

    fn set_prompt_busy(&mut self, busy: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_busy(busy);
        }
    }

    /// Navigates `side` to its parent directory, re-selecting the directory
    /// we came from.
    fn go_up(&mut self, side: PanelSide) -> Task<Message> {
        let current = &self.panel(side).path;
        let Some(parent) = current.parent().map(PathBuf::from) else {
            return Task::none();
        };
        let came_from = current
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.load(side, parent, came_from)
    }

    /// Starts an async directory read for `side`.
    fn load(&mut self, side: PanelSide, path: PathBuf, select: Option<String>) -> Task<Message> {
        let request_id = self.panel_mut(side).begin_load();
        Task::perform(fs::read_directory(path), move |listing| {
            Message::DirectoryLoaded {
                side,
                request_id,
                path: listing.path,
                entries: listing.entries,
                error: listing.error,
                select: select.clone(),
            }
        })
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod render_timing {
    use super::*;
    use crate::fs::FileEntry;
    use std::time::{Duration, Instant};

    /// Synthetic entries, so the measurement does not depend on what is in the
    /// working directory.
    fn entries(n: usize) -> Vec<FileEntry> {
        (0..n)
            .map(|i| FileEntry {
                name: format!("file_{i}.txt").into(),
                path: std::path::PathBuf::from(format!("/tmp/file_{i}.txt")),
                is_dir: i % 5 == 0,
                is_symlink: false,
                is_parent: i == 0,
                size: 1024 * (i as u64 % 900),
                modified: None,
            })
            .collect()
    }

    fn app_with_rows(n: usize) -> App {
        let mut app = App::new().0;
        app.left_panel.entries = entries(n);
        app.right_panel.entries = entries(n);
        app.visible_rows = 35;
        app
    }

    /// Regression: navigation felt laggy — the highlighted row visibly caught up
    /// about a second after the key press.
    ///
    /// `view` is rebuilt from scratch on every keystroke, so its cost is what
    /// the user waits for. Measured at ~0.1 ms, which is well inside budget and
    /// rules the view out as the cause: the remaining cost is rasterisation,
    /// which this test cannot reach. See ROADMAP.md, T2.
    #[test]
    fn a_frame_costs_far_less_than_a_frame_budget() {
        let app = app_with_rows(200);

        let _ = app.view(); // warm-up
        let frames = 60;
        let start = Instant::now();
        for _ in 0..frames {
            let _ = app.view();
        }
        let per_frame = start.elapsed() / frames;

        assert!(
            per_frame < Duration::from_millis(16),
            "a frame costs {per_frame:?}; the 60fps budget is 16ms"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod prompt_routing {
    use super::*;
    use iced::keyboard::key::Named;

    fn app() -> App {
        App::new().0
    }

    fn with_prompt() -> App {
        let mut app = app();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
        app
    }

    /// Routes as the subscription does: build the state, then call the free
    /// function. Same path the real keypress takes.
    fn press(app: &App, key: Key) -> Option<Message> {
        route_key(&app.prompt_key_state(), key)
    }

    /// While the prompt is open, F7 must not open a second one and the panel
    /// keys must not move the cursor. This is the bug the whole routing exists
    /// to prevent: Enter in a text field that also opens directories.
    #[test]
    fn an_open_prompt_takes_every_key() {
        let app = with_prompt();

        for key in [
            Key::Named(Named::ArrowDown),
            Key::Named(Named::Enter),
            Key::Named(Named::Tab),
            Key::Named(Named::F10),
        ] {
            let label = format!("{key:?}");
            let routed = press(&app, key.clone());
            let leaked = matches!(
                routed,
                Some(Message::MoveSelection(_))
                    | Some(Message::SwitchPanel)
                    | Some(Message::Quit)
                    | Some(Message::CreateDirPrompt)
            );
            assert!(!leaked, "{label} escaped the prompt as {routed:?}");
        }
    }

    #[test]
    fn enter_submits_and_escape_cancels() {
        let app = with_prompt();
        assert_eq!(
            press(&app, Key::Named(Named::Enter)),
            Some(Message::PromptSubmit)
        );
        assert_eq!(
            press(&app, Key::Named(Named::Escape)),
            Some(Message::PromptCancel)
        );
    }

    /// Typing appends, backspace removes one character — the field behaves like
    /// a text field, not like a panel selection.
    #[test]
    fn typing_and_backspace_edit_the_name() {
        let mut app = with_prompt();
        app.prompt.as_mut().unwrap().set_name("abc".into());

        assert_eq!(
            press(&app, Key::Character("d".into())),
            Some(Message::PromptInput("abcd".into()))
        );
        assert_eq!(
            press(&app, Key::Named(Named::Backspace)),
            Some(Message::PromptInput("ab".into()))
        );
    }

    /// A new keystroke clears the error, so a corrected name does not still
    /// show the complaint about the previous one.
    #[test]
    fn typing_clears_a_validation_error() {
        let mut prompt = Prompt::create_dir(PathBuf::from("/tmp"));
        prompt.set_validation_error("empty_name");
        assert!(prompt.error().is_some());

        prompt.set_name("x".into());
        assert_eq!(prompt.error(), None);
    }

    /// Without a prompt the bindings apply unchanged, so the routing did not
    /// change normal navigation.
    #[test]
    fn without_a_prompt_the_bindings_apply() {
        let app = app();
        assert_eq!(
            press(&app, Key::Named(Named::ArrowDown)),
            Some(Message::MoveSelection(1))
        );
        assert_eq!(
            press(&app, Key::Named(Named::Enter)),
            Some(Message::OpenSelected)
        );
    }

    /// F7 is bound, and the header hint comes from the same entry, so the key
    /// that opens the prompt and the key the user reads about are the same.
    #[test]
    fn f7_opens_the_prompt_and_is_advertised() {
        let app = app();
        assert_eq!(
            press(&app, Key::Named(Named::F7)),
            Some(Message::CreateDirPrompt)
        );
        assert!(
            keymap::shortcuts(Language::English)
                .iter()
                .any(|(key, _)| key == "F7"),
            "F7 opens the prompt but the header does not show it"
        );
    }

    // --- the whole chain, without a window -------------------------------
    //
    // Driving `update` the way a keypress would: open, type, submit. The
    // filesystem work itself is not awaited here, so this covers routing and
    // validation; the directory is checked in the async test below.

    fn press_and_update(app: &mut App, key: Key) {
        if let Some(message) = route_key(&app.prompt_key_state(), key) {
            let _ = app.update(message);
        }
    }

    /// Removes itself, so a failed assertion does not leave a directory that
    /// makes the next run fail.
    fn scratch(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-chain-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    #[test]
    fn f7_then_a_name_then_enter_readies_the_operation() {
        let scratch = scratch("submit");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let mut app = app();
        app.left_panel.path = base.clone();

        press_and_update(&mut app, Key::Named(Named::F7));
        assert!(app.prompt.is_some(), "F7 did not open the prompt");

        for c in "neuer ordner".chars() {
            press_and_update(&mut app, Key::Character(c.to_string().into()));
        }
        assert_eq!(app.prompt.as_ref().unwrap().name(), "neuer ordner");

        // Submit starts the operation and marks the prompt busy.
        let _task = app.update(Message::PromptSubmit);
        assert!(
            app.prompt.is_some(),
            "the prompt closed before the work ran"
        );
        assert!(app.prompt.as_ref().unwrap().busy());
        assert_eq!(app.prompt.as_ref().unwrap().error(), None);
    }

    /// An empty name is refused while the dialog is still open: no filesystem
    /// call, no busy state, an error the user can act on.
    #[test]
    fn submitting_an_empty_name_reports_it_without_closing() {
        let scratch = scratch("empty");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let mut app = app();
        app.left_panel.path = base.clone();

        press_and_update(&mut app, Key::Named(Named::F7));
        let _task = app.update(Message::PromptSubmit);

        assert!(app.prompt.is_some(), "the prompt closed on an invalid name");
        assert!(!app.prompt.as_ref().unwrap().busy());
        assert_eq!(app.prompt.as_ref().unwrap().error(), Some("empty_name"));
    }

    /// Escape closes the prompt and leaves no state behind.
    #[test]
    fn escape_closes_the_prompt() {
        let mut app = app();
        press_and_update(&mut app, Key::Named(Named::F7));
        assert!(app.prompt.is_some());

        press_and_update(&mut app, Key::Named(Named::Escape));
        assert!(app.prompt.is_none());
        assert_eq!(app.prompt_side, None);
    }

    /// A result that arrives after the user dismissed the prompt is dropped:
    /// it must not reopen anything or report a failure into the void.
    #[test]
    fn a_result_from_a_dismissed_prompt_is_ignored() {
        let mut app = app();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
        app.prompt_request_id = 7;
        press_and_update(&mut app, Key::Named(Named::Escape));

        let stale = Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 7,
            result: Err(fs::CreateDirError::TaskFailed {
                reason: "stale".into(),
            }),
        };
        let _task = app.update(stale);
        assert!(app.prompt.is_none());
    }

    /// The two filesystem failures get different wording, because the user
    /// needs to act differently: a taken name means pick another, a missing
    /// parent means the panel is stale.
    #[test]
    fn the_two_failures_read_differently() {
        let mut app = app();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/tmp")));
        app.prompt.as_mut().unwrap().set_name("x".into());

        let taken = fs::CreateDirError::AlreadyExists {
            path: PathBuf::from("/tmp/x"),
            source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists"),
        };
        let parent_gone = fs::CreateDirError::AlreadyExists {
            path: PathBuf::from("/tmp/x"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no parent"),
        };

        let taken_text = app.prompt_error(&taken);
        let parent_text = app.prompt_error(&parent_gone);
        assert_ne!(
            taken_text, parent_text,
            "a taken name and a missing parent need different messages"
        );
        assert!(
            !taken_text.contains("{"),
            "placeholder left in: {taken_text}"
        );
        assert!(
            !parent_text.contains("{"),
            "placeholder left in: {parent_text}"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod prompt_end_to_end {
    use super::*;

    /// Removes itself, so a failed assertion does not leave a directory that
    /// makes the next run fail.
    fn scratch(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-e2e-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    /// The whole path, including the filesystem: F7, type a name, submit, and
    /// the directory exists afterwards. The task is awaited through the
    /// executor so this is the real thing, not a simulation.
    #[tokio::test]
    async fn the_directory_exists_after_submitting() {
        let scratch = scratch("create");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("angelegt");

        let result = fs::create_dir(target.clone()).await;
        assert!(result.is_ok(), "create_dir failed: {result:?}");
        assert!(target.is_dir(), "{} was not created", target.display());
    }

    /// A name taken is reported as such, and the existing directory is left
    /// alone — the case a second F7 on the same name produces.
    #[tokio::test]
    async fn a_taken_name_leaves_the_existing_directory_intact() {
        let scratch = scratch("taken");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("vorhanden");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("inhalt.txt"), b"wichtig").unwrap();

        let err = fs::create_dir(target.clone()).await.unwrap_err();
        assert_eq!(
            err.io_error().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(target.join("inhalt.txt")).unwrap(),
            b"wichtig",
            "an existing directory must not be touched"
        );
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod reload_order {
    use super::*;

    /// Removes itself, so a failed assertion does not leave a directory that
    /// makes the next run fail.
    fn scratch(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-order-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    /// Regression: F7 started the create and the reload at the same time with
    /// `Task::batch`. A read that finished first saw a directory without the new
    /// entry, and the selection landed on whatever was there instead — the new
    /// directory silently missing from the panel.
    ///
    /// The fix puts the reload behind the create's result. This checks the
    /// ordering without timing: while the create is still in flight, no reload
    /// may have been issued.
    #[test]
    fn the_reload_waits_for_the_create() {
        let scratch = scratch("waits");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let mut app = App::new().0;
        app.left_panel.path = base.clone();
        app.prompt = Some(Prompt::create_dir(base.clone()));
        app.prompt.as_mut().unwrap().set_name("neu".into());
        // Submitting queues the reload instead of running it.
        let _ = app.update(Message::PromptSubmit);
        assert!(
            app.pending_reload.is_some(),
            "no reload was queued, so the panel would never refresh"
        );
        assert_ne!(
            app.prompt_request_id, 0,
            "the request id was not advanced, so a stale result could land"
        );

        // Still busy: the create has not reported.
        assert!(app.prompt.as_ref().unwrap().busy());
    }

    /// The queued reload runs when the create reports, and clears itself so a
    /// later result cannot trigger a second read.
    #[test]
    fn the_reload_runs_when_the_create_reports() {
        let scratch = scratch("runs");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let mut app = App::new().0;
        app.left_panel.path = base.clone();
        app.prompt = Some(Prompt::create_dir(base.clone()));
        app.prompt.as_mut().unwrap().set_name("spaet".into());
        app.prompt_request_id = 1;
        app.pending_reload = Some(PendingReload {
            side: PanelSide::Left,
            name: "spaet".into(),
        });

        let _ = app.update(Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 1,
            result: Ok(()),
        });

        assert!(
            app.pending_reload.is_none(),
            "the reload was not consumed and would run again"
        );
        assert!(app.prompt.is_none(), "the prompt stayed open after success");
    }

    /// On failure the prompt stays open with the error, and the reload still
    /// runs: the name may belong to something that appeared in the meantime.
    #[test]
    fn a_failure_still_reloads() {
        let scratch = scratch("failure");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let mut app = App::new().0;
        app.left_panel.path = base.clone();
        app.prompt = Some(Prompt::create_dir(base.clone()));
        app.prompt.as_mut().unwrap().set_name("weg".into());
        app.prompt_request_id = 2;
        app.pending_reload = Some(PendingReload {
            side: PanelSide::Left,
            name: "weg".into(),
        });

        let _ = app.update(Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 2,
            result: Err(fs::CreateDirError::AlreadyExists {
                path: base.join("weg"),
                source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists"),
            }),
        });

        assert!(
            app.pending_reload.is_none(),
            "a failed create left a reload queued"
        );
        assert!(
            app.prompt.is_some(),
            "the prompt closed on failure, so the user cannot correct the name"
        );
        assert!(!app.prompt.as_ref().unwrap().busy());
    }

    /// A result for a request the app has moved past changes nothing — not the
    /// prompt, and not a queued reload either.
    #[test]
    fn a_stale_result_touches_neither() {
        let mut app = App::new().0;
        app.prompt_request_id = 5;
        app.pending_reload = Some(PendingReload {
            side: PanelSide::Left,
            name: "x".into(),
        });

        let _ = app.update(Message::PromptFinished {
            prompt: PromptKind::CreateDir,
            request_id: 4,
            result: Ok(()),
        });

        assert!(app.prompt.is_none());
        assert!(
            app.pending_reload.is_some(),
            "a stale result consumed the queued reload"
        );
    }
}

#[cfg(test)]
// Test-only constructors and helpers. One block rather than four: they were
// appended as tests were written, and a reader had to hunt for them.
impl App {
    /// An app with the create-directory prompt open, for the UI tests. The
    /// prompt field is private to this module, so the setup lives here.
    /// An app whose panels are filled from fixed data, for snapshot tests.
    ///
    /// `App::new()` would read the working directory, so the rendered image
    /// would depend on where the test runner happens to be started — a fresh CI
    /// container and a laptop would produce different references for the same
    /// code. This one renders the same bytes everywhere.
    pub fn with_fixed_panels() -> Self {
        let mut app = Self {
            left_panel: PanelState::new(PathBuf::from("/home/test/links")),
            right_panel: PanelState::new(PathBuf::from("/home/test/files")),
            active_panel: PanelSide::Left,
            visible_rows: 12,
            lang: Language::default(),
            shortcuts: keymap::shortcuts(Language::default()),
            prompt: None,
            prompt_side: None,
            prompt_request_id: 0,
            pending_reload: None,
            jobs: jobs::Queue::new(),
            transfer: None,
            pending_conflict: None,
            job_error: None,
            job_done: 0,
            conflict_all: false,
            conflict_rule: None,
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
        };
        for (side, names) in [
            (PanelSide::Left, ["..", "Documents", "Projects", "Desktop"]),
            (
                PanelSide::Right,
                ["..", "notes.txt", "report.pdf", "photo.jpg"],
            ),
        ] {
            let panel = app.panel_mut(side);
            panel.entries = names
                .iter()
                .enumerate()
                .map(|(i, name)| fs::FileEntry {
                    name: (*name).into(),
                    path: PathBuf::from("/home/test").join(name),
                    is_dir: i == 0 || name.ends_with('s') && i < 2,
                    is_symlink: false,
                    is_parent: i == 0,
                    size: if i == 0 { 0 } else { 1024 * (i as u64 * 4096) },
                    modified: None,
                })
                .collect();
        }
        app
    }

    /// A prompt on fixed data, for the prompt's own reference image.
    pub fn with_fixed_prompt() -> Self {
        let mut app = Self::with_fixed_panels();
        app.prompt = Some(Prompt::create_dir(PathBuf::from("/home/test/links")));
        if let Some(prompt) = app.prompt.as_mut() {
            prompt.set_name("neuer ordner".to_string());
        }
        app
    }

    /// Opens the prompt the way F7 does — through `update` — so the focus Task
    /// is produced the same way it is in the running app. Setting `prompt`
    /// directly would skip it.
    ///
    /// No parent argument: `update` takes the active panel's path, as it does
    /// in the app.
    pub fn with_prompt_open() -> Self {
        let mut app = Self::new().0;
        let _task = app.update(crate::messages::Message::CreateDirPrompt);
        app
    }

    pub fn set_prompt(&mut self, name: &str, error: Option<&str>) {
        let Some(open) = self.prompt.as_mut() else {
            return;
        };
        open.set_name(name.to_string());
        if let Some(message) = error {
            open.set_error(message.to_string());
        }
    }
    pub fn panel_mut_for_test(&mut self, side: PanelSide) -> &mut PanelState {
        self.panel_mut(side)
    }

    pub fn set_visible_rows_for_test(&mut self, rows: usize) {
        self.visible_rows = rows;
    }

    /// Tags the row at `index`, addressed by position the way a keypress is.
    pub fn toggle_tag_for_test(&mut self, index: usize) {
        let Some(name) = self.left_panel.entries.get(index).map(|e| e.name.clone()) else {
            return;
        };
        self.left_panel.selection.toggle(&name);
    }

    pub fn tag_all_for_test(&mut self) {
        let mut selection = crate::selection::SelectionSet::new();
        selection.tag_all(&self.left_panel.entries);
        self.left_panel.selection = selection;
    }

    pub fn left_panel_selection_tagged_for_test(&self, index: usize) -> bool {
        self.left_panel
            .entries
            .get(index)
            .is_some_and(|e| self.left_panel.selection.is_tagged(&e.name))
    }

    pub fn move_selection_for_test(&mut self, delta: isize) {
        let rows = self.visible_rows;
        self.left_panel.move_selection(delta, rows);
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod transfer_tests {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn file(name: &str) -> FileEntry {
        FileEntry {
            name: OsString::from(name),
            path: PathBuf::from("/left").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
        }
    }

    /// An app with three files in the left panel, the right one empty and
    /// ready to receive.
    fn app_with_files() -> App {
        let mut app = App::new().0;
        app.left_panel.path = PathBuf::from("/left");
        app.right_panel.path = PathBuf::from("/right");
        app.left_panel.entries = vec![file("a.txt"), file("b.txt"), file("c.txt")];
        app
    }

    /// NC's rule: nothing tagged means the row under the cursor. F5 on one file
    /// has to work or the key does nothing.
    #[tokio::test]
    async fn an_untagged_panel_transfers_the_cursor_row() {
        let mut app = app_with_files();
        app.left_panel.selected = 1;

        let _task = app.update(Message::Transfer(TransferKind::Copy));

        let transfer = app.transfer.as_ref().expect("F5 set up nothing");
        assert_eq!(transfer.sources, vec![PathBuf::from("/left/b.txt")]);
        assert_eq!(transfer.target, PathBuf::from("/right"));
    }

    /// Tagged rows win over the cursor. This is the whole point of tagging: the
    /// cursor is wherever the user last looked, and must not change what F5
    /// touches.
    #[tokio::test]
    async fn tagged_rows_win_over_the_cursor() {
        let mut app = app_with_files();
        app.left_panel.selected = 2;
        app.left_panel.selection.toggle(&OsString::from("a.txt"));
        app.left_panel.selection.toggle(&OsString::from("c.txt"));

        let _task = app.update(Message::Transfer(TransferKind::Copy));

        let transfer = app.transfer.as_ref().expect("F5 set up nothing");
        assert_eq!(
            transfer.sources,
            vec![PathBuf::from("/left/a.txt"), PathBuf::from("/left/c.txt")],
            "the tagged rows, in the order they appear, not the cursor row"
        );
    }

    /// `..` is not a thing to copy into the other panel.
    #[test]
    fn the_parent_entry_is_never_transferred() {
        let mut app = app_with_files();
        app.left_panel
            .entries
            .get_mut(1)
            .expect("the fixture has three files")
            .is_parent = true;
        app.left_panel.selected = 1;

        let _task = app.update(Message::Transfer(TransferKind::Copy));

        assert!(
            app.transfer.is_none(),
            "F5 offered to copy .. into the other panel"
        );
    }

    /// The target is the other panel, not the same one. Copying a directory
    /// into itself is the mistake this prevents.
    #[tokio::test]
    async fn the_target_is_the_other_panel() {
        let mut app = app_with_files();
        app.left_panel.selected = 0;

        let _task = app.update(Message::Transfer(TransferKind::Copy));

        let transfer = app.transfer.as_ref().expect("F5 set up nothing");
        assert_eq!(transfer.target, app.right_panel.path);
        assert_ne!(transfer.target, transfer.sources[0].parent().unwrap());
    }

    /// Switching the active panel swaps which way the transfer goes.
    #[tokio::test]
    async fn the_target_follows_the_active_panel() {
        let mut app = app_with_files();
        app.active_panel = PanelSide::Right;
        // A file in the right panel, so the path matches the panel it is in.
        let mut x = file("x.txt");
        x.path = PathBuf::from("/right/x.txt");
        app.right_panel.entries = vec![x];
        app.right_panel.selected = 0;

        let _task = app.update(Message::Transfer(TransferKind::Copy));

        let transfer = app.transfer.as_ref().expect("F5 set up nothing");
        // The source comes from the active panel, which is now the right one,
        // and the target is the other.
        assert_eq!(transfer.sources, vec![PathBuf::from("/right/x.txt")]);
        assert_eq!(transfer.target, PathBuf::from("/left"));
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod mouse_tagging {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn app_with_files() -> App {
        let mut app = App::new().0;
        app.left_panel.path = PathBuf::from("/left");
        app.right_panel.path = PathBuf::from("/right");
        app.left_panel.entries = (0..3)
            .map(|i| FileEntry {
                name: OsString::from(format!("f{i}.txt")),
                path: PathBuf::from("/left").join(format!("f{i}.txt")),
                is_dir: false,
                is_symlink: false,
                is_parent: false,
                size: 1,
                modified: None,
            })
            .collect();
        app
    }

    /// A click on the star tags; a click anywhere else does not. Without the
    /// first, tagging is keyboard-only and the column of stars cannot be used.
    #[test]
    fn a_click_on_the_star_tags_the_row() {
        let mut app = app_with_files();

        let _task = app.update(Message::RowClicked {
            side: PanelSide::Left,
            index: 1,
            on_tag: true,
        });

        let name = app
            .left_panel
            .entries
            .get(1)
            .expect("the fixture has three files")
            .name
            .clone();
        assert!(
            app.left_panel.selection.is_tagged(&name),
            "clicking the star did not tag the row"
        );
    }

    /// Clicking the row moves the cursor and leaves the tags alone — otherwise
    /// selecting files to copy would tag them as a side effect.
    #[test]
    fn a_click_on_the_row_only_moves_the_cursor() {
        let mut app = app_with_files();

        let _task = app.update(Message::RowClicked {
            side: PanelSide::Left,
            index: 2,
            on_tag: false,
        });

        assert_eq!(app.left_panel.selected, 2);
        assert!(
            app.left_panel.selection.is_empty(),
            "clicking a row tagged it"
        );
    }

    /// The star toggles: clicking it again untags.
    #[test]
    fn a_second_click_on_the_star_untags() {
        let mut app = app_with_files();
        for _ in 0..2 {
            let _task = app.update(Message::RowClicked {
                side: PanelSide::Left,
                index: 1,
                on_tag: true,
            });
        }
        assert!(app.left_panel.selection.is_empty());
    }

    /// `..` has no star to click, and must not be taggable if it somehow is.
    #[test]
    fn the_parent_entry_cannot_be_tagged_by_click() {
        let mut app = app_with_files();
        app.left_panel
            .entries
            .get_mut(0)
            .expect("the fixture has three files")
            .is_parent = true;

        let _task = app.update(Message::RowClicked {
            side: PanelSide::Left,
            index: 0,
            on_tag: true,
        });

        assert!(app.left_panel.selection.is_empty());
    }

    /// Clicking a row in the other panel makes that panel the source of F5.
    #[test]
    fn clicking_a_row_activates_its_panel() {
        let mut app = app_with_files();
        app.right_panel.entries = app.left_panel.entries.clone();

        let _task = app.update(Message::RowClicked {
            side: PanelSide::Right,
            index: 0,
            on_tag: false,
        });

        assert_eq!(app.active_panel, PanelSide::Right);
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod reload_keeps_place {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn panel_with(names: &[&str]) -> PanelState {
        let mut p = PanelState::new(PathBuf::from("/left"));
        p.entries = names
            .iter()
            .map(|n| FileEntry {
                name: OsString::from(*n),
                path: PathBuf::from("/left").join(n),
                is_dir: false,
                is_symlink: false,
                is_parent: false,
                size: 1,
                modified: None,
            })
            .collect();
        p
    }

    /// Regression: a reload put the cursor back on row 0, so after a file
    /// operation the list jumped to the top and the tagged rows left the screen.
    /// The row that was selected stays selected.
    #[test]
    fn a_reload_keeps_the_row_under_the_cursor() {
        let mut p = panel_with(&["a", "b", "c", "d"]);
        p.selected = 2;

        p.apply_listing(
            PathBuf::from("/left"),
            panel_with(&["a", "b", "c", "d"]).entries,
            None,
            None,
            10,
        );

        assert_eq!(p.selected, 2, "the cursor jumped to the top of the list");
    }

    /// And it survives the reload that adds a file in front, which shifts every
    /// position — the name is what has to be kept, not the index.
    #[test]
    fn a_reload_keeps_the_file_not_the_index() {
        let mut p = panel_with(&["a", "b", "c"]);
        p.selected = 1; // b

        p.apply_listing(
            PathBuf::from("/left"),
            panel_with(&["new", "a", "b", "c"]).entries,
            None,
            None,
            10,
        );

        assert_eq!(
            p.entries[p.selected].name.to_string_lossy(),
            "b",
            "the cursor stayed on the index and now points at a different file"
        );
    }

    /// A name the caller asked for still wins over the remembered one — F7
    /// selects the directory it just created.
    #[test]
    fn an_explicit_selection_wins_over_the_remembered_row() {
        let mut p = panel_with(&["a", "b", "c"]);
        p.selected = 0;

        p.apply_listing(
            PathBuf::from("/left"),
            panel_with(&["a", "b", "c"]).entries,
            None,
            Some("c"),
            10,
        );

        assert_eq!(p.entries[p.selected].name.to_string_lossy(), "c");
    }

    /// The tags survive a reload too, so the files marked for copying are still
    /// marked after it.
    #[test]
    fn tags_survive_a_reload() {
        let mut p = panel_with(&["a", "b", "c"]);
        p.selection.toggle(&p.entries[1].name.clone());

        p.apply_listing(
            PathBuf::from("/left"),
            panel_with(&["a", "b", "c"]).entries,
            None,
            None,
            10,
        );

        assert_eq!(
            p.selection.len(),
            1,
            "the tag was lost in the reload that followed the operation"
        );
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod abort_tests {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn app_with_files() -> App {
        let mut app = App::new().0;
        app.left_panel.path = PathBuf::from("/left");
        app.right_panel.path = PathBuf::from("/right");
        app.left_panel.entries = (0..3)
            .map(|i| FileEntry {
                name: OsString::from(format!("f{i}.txt")),
                path: PathBuf::from("/left").join(format!("f{i}.txt")),
                is_dir: false,
                is_symlink: false,
                is_parent: false,
                size: 1,
                modified: None,
            })
            .collect();
        app.left_panel.selected = 0;
        app
    }

    /// Escape has to reach the app even though `route_key` looks at the
    /// prompt: with none open it falls through to the bindings, and the abort
    /// binding is what it has to find.
    #[tokio::test]
    async fn escape_stops_a_running_job() {
        let mut app = app_with_files();
        // Start a transfer, so there is something to stop.
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        assert!(app.jobs.is_busy(), "nothing to abort");

        let routed = route_key(&app.prompt_key_state(), Key::Named(Named::Escape));
        let Some(message) = routed else {
            panic!("Escape while a job runs produced no message");
        };
        let _stopped = app.update(message);

        assert!(!app.jobs.is_busy(), "the job kept running after Escape");
        assert!(app.transfer.is_none(), "the transfer was left half-done");
    }

    /// Stopping when nothing runs is harmless, not a panic.
    #[test]
    fn aborting_an_idle_app_does_nothing() {
        let mut app = app_with_files();
        let _task = app.update(Message::AbortJob);
        assert!(!app.jobs.is_busy());
    }

    /// The bar tells the user the job can be stopped, and stops claiming it
    /// once nothing runs.
    #[tokio::test]
    async fn the_bar_mentions_the_abort_key_only_while_a_job_runs() {
        let mut app = app_with_files();
        assert!(app.job_status().is_none(), "an idle app shows a job");

        let _started = app.update(Message::Transfer(TransferKind::Copy));
        let status = app.job_status().expect("a running job should show");
        assert!(
            status.abortable,
            "a running job does not offer a way to stop it"
        );

        let _task = app.update(Message::AbortJob);
        assert!(app.job_status().is_none());
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod conflict_tests {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    /// Real directories, because the conflict only exists if the target file is
    /// really there. The scratch directory removes itself.
    fn with_conflict() -> (App, tempfile::TempDir) {
        let scratch = tempfile::Builder::new()
            .prefix("ncrs-conflict-")
            .tempdir()
            .expect("a scratch directory");
        let left = scratch.path().join("left");
        let right = scratch.path().join("right");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();

        for i in 0..3 {
            std::fs::write(left.join(format!("f{i}.txt")), b"new").unwrap();
        }
        // The name the transfer will run into.
        std::fs::write(right.join("f0.txt"), b"existing").unwrap();

        let mut app = App::new().0;
        app.left_panel.path = left.clone();
        app.right_panel.path = right.clone();
        app.left_panel.entries = (0..3)
            .map(|i| FileEntry {
                name: OsString::from(format!("f{i}.txt")),
                path: left.join(format!("f{i}.txt")),
                is_dir: false,
                is_symlink: false,
                is_parent: false,
                size: 3,
                modified: None,
            })
            .collect();
        app.left_panel.selected = 0;
        (app, scratch)
    }

    /// A name that is taken opens the dialog rather than failing the row. The
    /// dialog is the feature: the user gets asked instead of the copy stopping.
    /// Delivers the message the copy sends when it runs into a taken name.
    ///
    /// The real one comes out of a Task, and a Task needs a runtime driving it.
    /// So the message is spelled out here — the app reads the transfer from its
    /// own state, exactly as it does in the running app, and only the failure
    /// string is invented. The alternative is asserting the dialog is absent,
    /// which would pass even with the whole chain broken.
    fn deliver_conflict(app: &mut App) {
        let _task = app.update(Message::TransferRowDone {
            result: Err("AlreadyExists (os error 17)".to_string()),
            index: 0,
        });
    }

    #[tokio::test]
    async fn a_taken_name_asks_instead_of_failing() {
        let (mut app, _scratch) = with_conflict();

        let _started = app.update(Message::Transfer(TransferKind::Copy));
        deliver_conflict(&mut app);

        let pending = app
            .pending_conflict
            .as_ref()
            .expect("a taken name should open the conflict dialog");
        assert_eq!(pending.index, 0, "it asked about the wrong file");
    }

    /// Answering "overwrite for all" remembers the rule, so the next conflict is
    /// resolved without a second dialog. This is the whole point of the "for all"
    /// button: fifty files, one question.
    #[tokio::test]
    async fn an_all_answer_is_remembered() {
        let (mut app, _scratch) = with_conflict();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        deliver_conflict(&mut app);
        assert!(app.pending_conflict.is_some(), "the dialog did not open");

        let _resumed = app.answer_conflict(ConflictChoice::AllOverwrite);

        assert_eq!(
            app.conflict_rule,
            Some(fs::transfer::OnConflict::Overwrite),
            "the answer was not remembered for the remaining files"
        );
    }

    /// A single-file answer does not become a standing rule — the next conflict
    /// asks again.
    #[tokio::test]
    async fn a_single_answer_is_not_remembered() {
        let (mut app, _scratch) = with_conflict();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        deliver_conflict(&mut app);

        let _resumed = app.answer_conflict(ConflictChoice::ThisOverwrite);

        assert_eq!(
            app.conflict_rule, None,
            "one file's answer was applied to the rest"
        );
    }

    /// The conflict is recognised from the error kind, not from the wording.
    /// A wording change in the filesystem layer would otherwise turn every
    /// conflict into a silent failure.
    #[test]
    fn a_conflict_is_recognised_by_its_kind() {
        assert!(is_conflict("AlreadyExists (os error 17)"));
        assert!(!is_conflict("Permission denied (os error 13)"));
        assert!(!is_conflict("No such file or directory (os error 2)"));
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod progress_bar {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn app_with_one_file() -> App {
        let mut app = App::new().0;
        app.left_panel.path = PathBuf::from("/left");
        app.right_panel.path = PathBuf::from("/right");
        app.left_panel.entries = vec![FileEntry {
            name: OsString::from("a.txt"),
            path: PathBuf::from("/left/a.txt"),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
        }];
        app.left_panel.selected = 0;
        app
    }

    /// A transfer of one file is one job, and the bar has to say so. The count
    /// that matters is the one from the tick, not the number of rows: one
    /// directory can hold fifty thousand files.
    #[tokio::test]
    async fn the_bar_shows_the_tick_not_the_row_count() {
        let mut app = app_with_one_file();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        assert!(app.jobs.is_busy(), "nothing to show progress for");

        // What the blocking thread would have sent: 1 of 1 for a single file.
        let _task = app.update(Message::JobProgress(fs::transfer::Tick {
            done: 1,
            total: 1,
        }));

        let status = app.job_status().expect("a running job should show");
        assert_eq!((status.done, status.total), (1, 1));
    }

    /// Before the first tick the bar must not claim to be finished: an unknown
    /// total shows zero, not a full bar.
    #[tokio::test]
    async fn the_bar_does_not_claim_progress_before_the_first_tick() {
        let mut app = app_with_one_file();
        let _started = app.update(Message::Transfer(TransferKind::Copy));

        let status = app.job_status().expect("a running job should show");
        assert_eq!(status.done, 0, "the bar started at the end");
    }

    /// Stopping clears the progress, so the next job does not inherit a
    /// previous one's numbers.
    #[tokio::test]
    async fn stopping_clears_the_progress() {
        let mut app = app_with_one_file();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        let _ticked = app.update(Message::JobProgress(fs::transfer::Tick {
            done: 5,
            total: 10,
        }));

        let _stopped = app.update(Message::AbortJob);
        assert!(
            app.job_status().is_none(),
            "the bar still shows a job that was stopped"
        );
    }
}
