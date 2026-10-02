//! Root application: state, `update` (the only place state changes) and
//! `view` (pure composition of UI components).
use std::path::{Path, PathBuf};

use std::sync::{Arc, Mutex, OnceLock};

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
use crate::ui::delete as delete_dialog_view;
use crate::ui::dialog::FIELD_ID;
use crate::ui::volumes as volumes_view;
use crate::ui::{
    self, conflict, dialog, fkeys, header, layout, panel, statusbar, theme, PanelProps, PanelState,
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

/// The routing state, shared with the keyboard subscription.
///
/// A newtype rather than a bare `Arc<Mutex<..>>`, because a subscription's
/// identity is its hash, and `Arc` hashes through to what it points at. The
/// value inside here *changes* on every message, so hashing the `Arc` directly
/// would change the subscription's identity on every message — the exact bug
/// this type exists to avoid.
///
/// Hashing nothing makes the identity fixed for the life of the app: one cell,
/// one subscription, and it reads whatever the current value is when a key
/// arrives.
struct KeyStateCell(Mutex<PromptKeyState>);

impl std::hash::Hash for KeyStateCell {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Deliberately content-free. See the type's own comment.
        "ncrs-key-state".hash(state);
    }
}

/// Why one row of a transfer failed.
///
/// The failure kind is carried across the task boundary rather than formatted
/// into the message. It used to be a plain `String`, and the app told "the name
/// is taken" from a real error by looking for `AlreadyExists` at the start of
/// that string — which never matches, because `io::Error`'s `Display` is the
/// *message*, not the kind's name. Every conflict therefore fell through as a
/// hard error and the overwrite dialog was unreachable.
///
/// A typed failure is the fix that cannot rot: a wording change in the
/// filesystem layer or on the operating system leaves this matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowFailure {
    /// The target already exists, and the user can say what to do about it.
    Conflict(String),
    /// Anything else, shown as it came.
    Other(String),
}

impl RowFailure {
    pub fn from_io(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        if err.kind() == ErrorKind::AlreadyExists {
            RowFailure::Conflict(err.to_string())
        } else {
            RowFailure::Other(err.to_string())
        }
    }
}

/// Runs one job and reports what happens as messages.
///
/// Separate from `App` so the queue can hand it to `Task::abortable` without
/// borrowing the app. The real work of F5/F6/F8 runs per row in `App`; a job
/// that reaches the queue's `start_next` here has nothing left to do.
fn run_job(_job: jobs::Job) -> Task<Message> {
    Task::done(Message::JobFinished(JobEvent::Done))
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

/// What a delete works on, resolved once when F8 is pressed: by the time the
/// user has answered the dialog the cursor may have moved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Deletion {
    sources: Vec<PathBuf>,
    /// Shift+F8: no trash.
    permanent: bool,
    /// The panel the entries came from, which is cleared and reloaded at the
    /// end even if the user switches panels meanwhile.
    side: PanelSide,
}

/// Which keys the open delete dialog answers to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DeleteKeys {
    #[default]
    Closed,
    /// Enter confirms.
    Trash,
    /// Enter cancels; only Shift+F8 pressed again confirms.
    Permanent,
}

/// One file waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingConflict {
    transfer: Transfer,
    /// Which of `sources` is in the way.
    index: usize,
}

/// The open drive menu. The list is read when the menu opens, so a drive
/// plugged in a moment ago is there and one removed since is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VolumeMenu {
    /// The panel that changes drive.
    side: PanelSide,
    volumes: Vec<fs::Volume>,
    /// The highlighted row, the one Enter goes to.
    selected: usize,
}

impl VolumeMenu {
    /// Opens on the drive `current` lives on: the volume with the longest path
    /// that `current` starts with.
    fn new(side: PanelSide, volumes: Vec<fs::Volume>, current: &Path) -> Self {
        let selected = volumes
            .iter()
            .enumerate()
            .filter(|(_, volume)| current.starts_with(&volume.path))
            .max_by_key(|(index, volume)| (volume.path.components().count(), usize::MAX - index))
            .map_or(0, |(index, _)| index);
        Self {
            side,
            volumes,
            selected,
        }
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.volumes.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }
}

pub struct App {
    left_panel: PanelState,
    right_panel: PanelState,
    active_panel: PanelSide,
    /// Number of file rows that fit into a panel (derived from window size).
    visible_rows: usize,
    lang: Language,
    /// Function key bar labels, kept in state so `view` does not look them up
    /// per frame.
    function_keys: [Option<&'static str>; keymap::FUNCTION_KEY_COUNT],
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
    /// The delete confirmation dialog, while it is up.
    delete_dialog: Option<Deletion>,
    /// The delete that is running.
    deleting: Option<Deletion>,
    /// The button the open dialog has focused, counted from the left.
    dialog_focus: usize,
    /// Modifiers held right now, for Cmd/Ctrl-click on a row.
    modifiers: Modifiers,
    /// Where F8 sends entries. Injected so tests never touch the real trash.
    trash: Arc<dyn fs::Trash>,
    /// Starts the external program of F3 and F4. Injected so tests never
    /// launch a real viewer.
    launcher: Arc<dyn fs::Launcher>,
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
    /// Which transfer is current. A row's result carries the number it started
    /// under, and one from an earlier transfer — stopped, then replaced — is
    /// ignored instead of steering the new one.
    generation: u64,
    /// How far the running transfer has got, filled from the job's channel.
    job_progress: Option<fs::transfer::Tick>,
    /// Receives the transfer's ticks, for the life of the app rather than of
    /// one transfer. Kept as an `Arc<Mutex<..>>` because a subscription cannot
    /// borrow the app and a tokio Receiver is neither `Clone` nor `Hash` —
    /// both of which `Subscription::run_with` needs from its data.
    progress_rx: Option<Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>>,
    /// The sending half, kept so a new transfer can reuse the same channel.
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<fs::transfer::Tick>>,
    /// The routing state the keyboard subscription reads on every key.
    ///
    /// `None` only before the first `subscription()` call. The subscription
    /// cannot close over `&self`, so this is how the two are connected: the
    /// subscription puts its cell here, and `update` keeps the value in it
    /// current after every message.
    key_state: OnceLock<Arc<KeyStateCell>>,
    /// The drive menu, while it is up.
    volume_menu: Option<VolumeMenu>,
    /// Where the drive menu gets its list. Injected so tests offer their own
    /// directories instead of the machine's disks.
    list_volumes: Arc<dyn Fn() -> Vec<fs::Volume> + Send + Sync>,
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
    /// The conflict dialog is up, so keys answer it rather than the panels.
    pub conflict_open: bool,
    /// The delete dialog is up, so keys answer it.
    pub delete_dialog: DeleteKeys,
    /// The drive menu is up, so keys move its highlight.
    pub volume_menu: bool,
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
/// Keys while no prompt is open: the conflict dialog first, then the running
/// job, then the bindings.
///
/// The order matters and is not arbitrary. A conflict is asked *while* a job
/// runs, so if the job branch came first, Escape would stop the copy instead of
/// answering the question the app is waiting on.
fn keys_without_prompt(
    prompt: &PromptKeyState,
    key: Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    // The menu takes the arrows and Enter that would otherwise move the panel
    // behind it; any other key does nothing until it is closed.
    if prompt.volume_menu {
        return match key.as_ref() {
            Key::Named(Named::Escape) => Some(Message::VolumeMenuClose),
            Key::Named(Named::Enter) => Some(Message::VolumeMenuActivate),
            Key::Named(Named::ArrowUp) => Some(Message::VolumeMenuMove(-1)),
            Key::Named(Named::ArrowDown) => Some(Message::VolumeMenuMove(1)),
            Key::Named(Named::Home) => Some(Message::VolumeMenuFirst),
            Key::Named(Named::End) => Some(Message::VolumeMenuLast),
            _ => None,
        };
    }

    // Before the conflict dialog and the job: a delete is only offered when
    // neither is there, so there is nothing to disambiguate, but it must not
    // be possible for Escape to mean "stop the job" while a question is open.
    if prompt.delete_dialog != DeleteKeys::Closed {
        let permanent = prompt.delete_dialog == DeleteKeys::Permanent;
        return match key.as_ref() {
            Key::Named(Named::Escape) => Some(Message::DeleteCancel),
            // The deliberate chord that started the permanent delete confirms it.
            Key::Named(Named::F8) if permanent && key_modifiers == Modifiers::SHIFT => {
                Some(Message::DeleteConfirm)
            }
            _ => dialog_focus_key(key, key_modifiers),
        };
    }

    if prompt.conflict_open {
        return match key.as_ref() {
            // Escape means "no" to a question, and "keep" is the no that does
            // not lose data. Cancelling the whole transfer is on Ctrl+C, which
            // is the key that already aborts a job.
            Key::Named(Named::Escape) => Some(Message::TransferConflict(ConflictChoice::ThisKeep)),
            // Space ticks "for all files", as it ticks a checkbox elsewhere.
            Key::Named(Named::Space) => Some(Message::ToggleConflictAll),
            // Ctrl+C cancels the whole operation, the same key that aborts a
            // job. Enter and Escape already covered the two single-file answers.
            Key::Character(c) if c == "c" && key_modifiers.contains(Modifiers::CTRL) => {
                Some(Message::TransferConflict(ConflictChoice::Cancel))
            }
            _ => dialog_focus_key(key, key_modifiers),
        };
    }

    // Escape stops a running job, but only then. With nothing running it stays
    // unbound, rather than bound to something that does nothing — a key that
    // means different things depending on invisible state is worse than one
    // that is simply free.
    if matches!(key.as_ref(), Key::Named(Named::Escape)) {
        return prompt.job_running.then_some(Message::AbortJob);
    }
    // The exact chord first (Shift+F8 is not F8). Only Shift may fall back to
    // the bare key: it changes the character typed (`*` is Shift+8), while Alt,
    // Ctrl and Super make a different chord that must not trigger the plain one.
    //
    // For a character, Shift is already in the character (`*` needs it on the
    // US and German layouts), so Ctrl+`*` arrives as CTRL|SHIFT and has to be
    // looked up as Ctrl. Named keys keep it: Shift+F8 is not F8.
    let modifiers = if matches!(key, Key::Character(_)) {
        key_modifiers - Modifiers::SHIFT
    } else {
        key_modifiers
    };
    keymap::map_key(key.clone(), modifiers).or_else(|| {
        (modifiers - Modifiers::SHIFT)
            .is_empty()
            .then(|| keymap::map_key(key, Modifiers::default()))
            .flatten()
    })
}

/// The keys every button dialog shares: arrows and Tab move the focus, Enter
/// presses the focused button.
fn dialog_focus_key(key: Key, modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Enter) => Some(Message::DialogActivate),
        Key::Named(Named::ArrowLeft) => Some(Message::DialogFocus(-1)),
        Key::Named(Named::ArrowRight) => Some(Message::DialogFocus(1)),
        Key::Named(Named::Tab) if modifiers.shift() => Some(Message::DialogFocus(-1)),
        Key::Named(Named::Tab) => Some(Message::DialogFocus(1)),
        _ => None,
    }
}

/// Whether holding the key down may fire the message again.
///
/// Only cursor movement, tagging and typing repeat. An action such as F5 or a
/// dialog answer must happen once per press: a held Shift+F8 would otherwise
/// confirm the permanent-delete dialog it has just opened.
fn repeats(message: &Message) -> bool {
    matches!(
        message,
        Message::MoveSelection(_)
            | Message::PageUp
            | Message::PageDown
            | Message::SelectFirst
            | Message::SelectLast
            | Message::ToggleTag
            | Message::TagMove(_)
            | Message::PromptInput(_)
            | Message::VolumeMenuMove(_)
            | Message::VolumeMenuFirst
            | Message::VolumeMenuLast
    )
}

pub fn route_key(prompt: &PromptKeyState, key: Key, modifiers: Modifiers) -> Option<Message> {
    if !prompt.open {
        return keys_without_prompt(prompt, key, modifiers);
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
        // iced delivers the space bar as a named key, not as a character.
        Key::Named(Named::Space) => {
            name.push(' ');
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
        Self::starting_in(fs::start_dir(), fs::home_dir())
    }

    /// Like `new`, with both panels' start directories given. Tests use this
    /// to stay inside their own scratch directory instead of the process-wide
    /// working directory and `$HOME`.
    pub fn starting_in(start: PathBuf, home: PathBuf) -> (Self, Task<Message>) {
        let mut app = Self {
            left_panel: PanelState::new(start.clone()),
            right_panel: PanelState::new(home.clone()),
            active_panel: PanelSide::Left,
            visible_rows: layout::visible_rows(layout::INITIAL_WINDOW_SIZE),
            lang: Language::default(),
            function_keys: keymap::function_keys(Language::default()),
            prompt: None,
            prompt_side: None,
            prompt_request_id: 0,
            pending_reload: None,
            jobs: jobs::Queue::new(),
            transfer: None,
            pending_conflict: None,
            delete_dialog: None,
            deleting: None,
            dialog_focus: 0,
            modifiers: Modifiers::default(),
            trash: Arc::new(fs::SystemTrash),
            launcher: Arc::new(fs::SystemLauncher),
            job_error: None,
            job_done: 0,
            conflict_all: false,
            conflict_rule: None,
            generation: 0,
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            volume_menu: None,
            list_volumes: Arc::new(fs::list_volumes),
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
        // `listen` yields the event stream, so the mapping can decide
        // without routing every key through a message first.
        //
        // The routing state is read through a shared cell, deliberately *not*
        // carried in the stream by `with`. `with` folds its value into the
        // subscription's hash, and iced's tracker drops any subscription whose
        // hash changes: every keystroke that opened a prompt, started a job or
        // raised a conflict killed the keyboard subscription and spawned a
        // fresh one, so the keys pressed around that moment went to a stream
        // nobody was listening to. That is why the app looked alive and then
        // ignored the keyboard, and why no test caught it — the routing itself
        // was correct.
        //
        // A cell instead: the hash stays constant, so this subscription lives
        // as long as the window does, and it reads the current state on every
        // key rather than a copy from when it was built.
        let key_state = Arc::clone(
            self.key_state
                .get_or_init(|| Arc::new(KeyStateCell(Mutex::new(self.prompt_key_state())))),
        );

        Subscription::batch([
            // `with` is unavoidable: iced requires the `filter_map` closure to
            // capture nothing, so the cell has to travel as the subscription's
            // value. What matters is *which* value. The old code carried the
            // `PromptKeyState` itself, so the hash tracked the state and
            // changed on every message; iced's tracker drops a subscription
            // whose hash changes, which killed the keyboard stream at exactly
            // the moments the app got busy — the keys pressed right after
            // opening a prompt or starting a copy went nowhere. That is why
            // the app looked alive and then ignored the keyboard.
            //
            // Carrying the cell instead gives a hash that is fixed for the
            // life of the app: the same `Arc`, hashed by its contents, which
            // never change. The stream survives every state change and reads
            // the current state on each key.
            keyboard::listen()
                .with(key_state)
                .filter_map(|(cell, event)| {
                    match event {
                        keyboard::Event::KeyPressed {
                            key,
                            modifiers,
                            repeat,
                            ..
                        } => route_key(
                            // A panic in a key handler would leave the lock
                            // poisoned. The state is a few Copy values and cannot
                            // be half-written, so reading the inner value is safe —
                            // and it keeps one panic from silencing every key
                            // afterwards.
                            &cell
                                .0
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            key,
                            modifiers,
                        )
                        .filter(|message| !repeat || repeats(message)),
                        keyboard::Event::ModifiersChanged(modifiers) => {
                            Some(Message::ModifiersChanged(modifiers))
                        }
                        _ => None,
                    }
                }),
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

        let task = match message {
            // A raw key press. Routed here rather than in the subscription,
            // because that one cannot see whether a prompt is open.
            // --- copy / move ---
            Message::Transfer(kind) => self.start_transfer(kind),

            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Message::DialogFocus(step) => {
                let buttons = self.dialog_button_count();
                if buttons > 0 {
                    // `buttons` is 2 or 3 and `step` is -1 or 1.
                    self.dialog_focus =
                        (self.dialog_focus + buttons).wrapping_add_signed(step) % buttons;
                }
                Task::none()
            }
            Message::DialogActivate => self.activate_dialog_button(),
            Message::ToggleConflictAll => {
                self.conflict_all = !self.conflict_all;
                Task::none()
            }

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
            //
            // A delete in flight finishes its current entry even so: it runs in
            // `spawn_blocking`, which cannot be cancelled, so the panel may
            // still list that entry for a moment after the abort.
            Message::AbortJob => {
                if self.jobs.abort_running() {
                    self.job_done = 0;
                    self.job_progress = None;
                    // Both halves go together: a sender kept without its
                    // receiver makes every later transfer fail on a closed
                    // channel. Dropping the receiver is also what stops the
                    // blocking copy, whose next tick cannot be sent.
                    self.progress_rx = None;
                    self.progress_tx = None;
                    if self.deleting.is_some() {
                        self.end_delete()
                    } else {
                        self.end_transfer()
                    }
                } else {
                    Task::none()
                }
            }

            // One row of a transfer finished. Continue with the next, or ask
            // about a name that is in the way.
            Message::TransferRowDone {
                result,
                index,
                generation,
            } => {
                if generation != self.generation {
                    return Task::none();
                }
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
                    // The dialog names the file itself, so the reason travels
                    // no further; it stays in the payload for the status line.
                    Err(RowFailure::Conflict(_reason)) => {
                        self.dialog_focus = 0;
                        self.pending_conflict = Some(PendingConflict { transfer, index });
                        Task::none()
                    }
                    // Rows before this one are already done, so the panels are
                    // reloaded to show them.
                    Err(RowFailure::Other(reason)) => {
                        self.set_status_error(&reason);
                        self.end_transfer()
                    }
                }
            }

            Message::OpenExternal(kind) => {
                self.open_external(kind);
                Task::none()
            }

            // --- drive menu ---
            Message::VolumeMenu(side) => {
                self.open_volume_menu(side);
                Task::none()
            }
            Message::VolumeMenuMove(delta) => {
                if let Some(menu) = self.volume_menu.as_mut() {
                    menu.move_by(delta);
                }
                Task::none()
            }
            Message::VolumeMenuFirst => {
                if let Some(menu) = self.volume_menu.as_mut() {
                    menu.selected = 0;
                }
                Task::none()
            }
            Message::VolumeMenuLast => {
                if let Some(menu) = self.volume_menu.as_mut() {
                    menu.selected = menu.volumes.len().saturating_sub(1);
                }
                Task::none()
            }
            Message::VolumeMenuActivate => self.go_to_selected_volume(),
            Message::VolumeMenuClick(index) => {
                if let Some(menu) = self.volume_menu.as_mut() {
                    menu.selected = index;
                }
                self.go_to_selected_volume()
            }
            Message::VolumeMenuClose => {
                self.volume_menu = None;
                Task::none()
            }

            // --- delete ---
            Message::Delete { permanent } => self.open_delete_dialog(permanent),
            Message::DeleteCancel => {
                self.delete_dialog = None;
                Task::none()
            }
            Message::DeleteConfirm => self.confirm_delete(),
            Message::DeleteRowDone {
                result,
                index,
                generation,
            } => {
                if generation != self.generation {
                    return Task::none();
                }
                self.jobs.finish();
                match result {
                    Ok(()) => self.run_delete_row(index + 1),
                    Err(reason) => {
                        let text = self.delete_failure(index, &reason);
                        self.set_status_error(&text);
                        self.end_delete()
                    }
                }
            }

            // --- job queue ---
            Message::JobFinished(event) => {
                // Progress arrives mid-run; only the terminal events free the
                // slot for the next job.
                let finished = matches!(event, JobEvent::Done);
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
                self.job_error = None;
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
            Message::ToggleTag => self.tag_and_move(1),
            Message::TagMove(delta) => self.tag_and_move(delta),
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

            // The overlays do not capture the wheel, so the panel behind one
            // would otherwise scroll under a dialog.
            Message::PanelScrolled { side, delta } => {
                if !self.overlay_is_open() {
                    self.panel_mut(side).scroll(delta, rows);
                }
                Task::none()
            }
            Message::SwitchPanel => {
                self.active_panel = self.active_panel.other();
                Task::none()
            }
            Message::SwitchLanguage => {
                self.lang = self.lang.other();
                self.function_keys = keymap::function_keys(self.lang);
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
                let toggle = on_tag || self.modifiers.command();
                let panel = self.panel_mut(side);
                panel.select(index, rows);
                if toggle {
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
        };

        // The keyboard subscription cannot see `&self`, so it reads the routing
        // state from a cell this method keeps current.
        //
        // This runs *after* the match, and that ordering is the point. Published
        // before it, the cell would always describe the state from before the
        // message that has just been handled — one step behind — so the keys
        // that follow a conflict would still be routed as panel keys and the
        // dialog could not be answered at all.
        self.publish_key_state();
        task
    }

    /// Writes the current routing state into the cell the keyboard reads.
    ///
    /// A no-op before the first `subscription()` call, which is the only time
    /// there is no cell — the subscription creates it.
    fn publish_key_state(&mut self) {
        let state = self.prompt_key_state();
        // `get` rather than `get_or_init`: the cell is created by the
        // subscription, and `update` must not create it — a cell nothing reads
        // would just be a value nobody sees. Before the first `subscription()`
        // call there is nothing to publish to, and that is fine: no keys have
        // been pressed yet.
        if let Some(cell) = self.key_state.get() {
            *cell
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
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
                move |delta| Message::PanelScrolled { side, delta },
            )
        };

        let panels = row![panel_view(PanelSide::Left), panel_view(PanelSide::Right)]
            .spacing(ui::theme::spacing::PANEL_GAP)
            .height(Length::Fill);

        let root = container(
            column![
                header::view(APP_NAME),
                panels,
                statusbar::view(
                    self.active_panel(),
                    self.lang,
                    self.job_status(),
                    self.job_error.as_deref(),
                ),
                fkeys::view(&self.function_keys),
            ]
            .spacing(theme::spacing::SECTION_GAP),
        )
        .padding(theme::spacing::OUTER_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root);

        if let Some(menu) = self.volume_menu.as_ref() {
            let menu_element =
                volumes_view::view(menu.side, &menu.volumes, menu.selected, self.lang);
            return Stack::with_children([root.into(), dialog::scrim(menu_element)]).into();
        }

        if let Some(deletion) = self.delete_dialog.as_ref() {
            let dialog_element = delete_dialog_view::view(
                self.delete_subject(deletion),
                deletion.permanent,
                self.dialog_focus,
                self.lang,
            );
            return Stack::with_children([root.into(), dialog::scrim(dialog_element)]).into();
        }

        // The conflict dialog comes first, before the prompt. A conflict is
        // asked *after* the prompt is gone — the user submitted, and the name
        // turned out to be taken — so behind the prompt's early return the
        // dialog was never drawn. That was a real bug, and the snapshot test is
        // what found it.
        if let Some(pending) = self.pending_conflict.as_ref() {
            let name = pending
                .transfer
                .sources
                .get(pending.index)
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let conflict_element = conflict::view(
                &name,
                self.lang,
                self.conflict_all,
                self.dialog_focus,
                Message::TransferConflict,
            );
            return Stack::with_children([root.into(), dialog::scrim(conflict_element)]).into();
        }

        // Then the create-directory prompt, for F7.
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
        let volume_menu = self.volume_menu.is_some();
        let delete_dialog = match &self.delete_dialog {
            None => DeleteKeys::Closed,
            Some(deletion) if deletion.permanent => DeleteKeys::Permanent,
            Some(_) => DeleteKeys::Trash,
        };
        match &self.prompt {
            None => PromptKeyState {
                job_running,
                delete_dialog,
                volume_menu,
                // A conflict is asked while no prompt is open — the prompt is
                // gone by then, the transfer is what is running. Without this
                // the dialog's keys would fall through to the panels.
                conflict_open: self.pending_conflict.is_some(),
                ..PromptKeyState::default()
            },
            Some(prompt) => PromptKeyState {
                open: true,
                job_running,
                delete_dialog,
                volume_menu,
                conflict_open: self.pending_conflict.is_some(),
                busy: prompt.busy(),
                typed: prompt.name().into(),
            },
        }
    }

    /// NC's rule for F5, F6 and F8: tagged rows win, otherwise the row under
    /// the cursor. The `..` entry is never a thing to act on.
    fn action_sources(&self) -> Vec<PathBuf> {
        let panel = self.active_panel();
        if panel.selection.any_tagged() {
            panel
                .selection
                .ordered(&panel.entries)
                .into_iter()
                .map(|e| e.path.clone())
                .collect()
        } else {
            panel
                .selected_entry()
                .filter(|e| !e.is_parent)
                .map(|e| vec![e.path.clone()])
                .unwrap_or_default()
        }
    }

    fn dialog_button_count(&self) -> usize {
        if self.delete_dialog.is_some() {
            2
        } else if self.pending_conflict.is_some() {
            3
        } else {
            0
        }
    }

    /// Enter in a button dialog: whatever the focused button does.
    fn activate_dialog_button(&mut self) -> Task<Message> {
        if self.delete_dialog.is_some() {
            return if self.dialog_focus == 0 {
                self.confirm_delete()
            } else {
                self.delete_dialog = None;
                Task::none()
            };
        }
        if self.pending_conflict.is_some() {
            let choice = match self.dialog_focus {
                0 => ConflictChoice::ThisOverwrite,
                1 => ConflictChoice::ThisKeep,
                _ => ConflictChoice::Cancel,
            };
            return self.answer_conflict(choice);
        }
        Task::none()
    }

    /// A prompt, dialog or the drive menu is on top of the panels.
    fn overlay_is_open(&self) -> bool {
        self.prompt.is_some()
            || self.pending_conflict.is_some()
            || self.delete_dialog.is_some()
            || self.volume_menu.is_some()
    }

    /// Alt+F1 / Alt+F2: list the drives and highlight the one `side` is on.
    /// Does nothing while a prompt or dialog holds the keyboard.
    fn open_volume_menu(&mut self, side: PanelSide) {
        if self.overlay_is_open() {
            return;
        }
        let volumes = (self.list_volumes)();
        if volumes.is_empty() {
            return;
        }
        self.volume_menu = Some(VolumeMenu::new(side, volumes, &self.panel(side).path));
    }

    /// Closes the menu and takes its panel to the highlighted drive, as any
    /// directory change does: a failing read shows its error in the status bar.
    fn go_to_selected_volume(&mut self) -> Task<Message> {
        let Some(menu) = self.volume_menu.take() else {
            return Task::none();
        };
        let Some(volume) = menu.volumes.get(menu.selected) else {
            return Task::none();
        };
        self.job_error = None;
        self.active_panel = menu.side;
        self.load(menu.side, volume.path.clone(), None)
    }

    /// Toggles the tag on the cursor row and moves by `delta`, as NC does. The
    /// `..` entry cannot be tagged but the cursor still moves past it.
    fn tag_and_move(&mut self, delta: isize) -> Task<Message> {
        let rows = self.visible_rows;
        let panel = self.active_panel_mut();
        if let Some(entry) = panel.selected_entry() {
            if !entry.is_parent {
                let name = entry.name.clone();
                panel.selection.toggle(&name);
            }
        }
        panel.move_selection(delta, rows);
        Task::none()
    }

    /// F8 / Shift+F8: ask before deleting. Nothing to act on, or something
    /// else already holding the keyboard or the queue, means nothing happens.
    fn open_delete_dialog(&mut self, permanent: bool) -> Task<Message> {
        if self.jobs.is_busy()
            || self.prompt.is_some()
            || self.pending_conflict.is_some()
            || self.delete_dialog.is_some()
        {
            return Task::none();
        }
        let sources = self.action_sources();
        if sources.is_empty() {
            return Task::none();
        }
        self.job_error = None;
        // The permanent dialog starts on "Cancel", so a reflex Enter never
        // deletes for good.
        self.dialog_focus = usize::from(permanent);
        self.delete_dialog = Some(Deletion {
            sources,
            permanent,
            side: self.active_panel,
        });
        Task::none()
    }

    /// The dialog's subject line: the name for one entry, the count for more.
    ///
    /// A permanent delete also lists the first names, because tagged rows may
    /// lie outside the visible part of the panel and the user is about to lose
    /// them for good.
    fn delete_subject(&self, deletion: &Deletion) -> String {
        const LISTED: usize = 5;
        let name_of = |path: &PathBuf| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        match deletion.sources.as_slice() {
            [only] => self.lang.text(Msg::DeleteOne).replace(
                "{name}",
                &only
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            many => {
                let mut lines = vec![self
                    .lang
                    .text(Msg::DeleteMany)
                    .replace("{count}", &many.len().to_string())];
                if deletion.permanent {
                    lines.extend(many.iter().take(LISTED).map(name_of));
                    if many.len() > LISTED {
                        lines.push(
                            self.lang
                                .text(Msg::DeleteMore)
                                .replace("{count}", &(many.len() - LISTED).to_string()),
                        );
                    }
                }
                lines.join("\n")
            }
        }
    }

    /// The dialog was confirmed: start on the first entry.
    fn confirm_delete(&mut self) -> Task<Message> {
        let Some(deletion) = self.delete_dialog.take() else {
            return Task::none();
        };
        self.deleting = Some(deletion);
        self.generation = self.generation.wrapping_add(1);
        self.run_delete_row(0)
    }

    /// Deletes the entry at `index` on the blocking pool, or finishes when
    /// there is none. One entry per job, so Escape works between two entries.
    fn run_delete_row(&mut self, index: usize) -> Task<Message> {
        let Some(deletion) = self.deleting.clone() else {
            return Task::none();
        };
        let Some(path) = deletion.sources.get(index).cloned() else {
            return self.end_delete();
        };
        let total = deletion.sources.len();
        self.job_progress = Some(fs::transfer::Tick { done: index, total });
        self.jobs.enqueue(jobs::Job {
            kind: jobs::JobKind::Delete,
            path: path.clone(),
            total: Some(total),
        });
        let generation = self.generation;
        let trash = Arc::clone(&self.trash);
        self.jobs
            .start_next(|_| {
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        if deletion.permanent {
                            fs::remove_permanently(&path).map_err(|err| err.to_string())
                        } else {
                            trash.trash(&path)
                        }
                    }),
                    move |outcome| Message::DeleteRowDone {
                        result: outcome.unwrap_or_else(|join| Err(join.to_string())),
                        index,
                        generation,
                    },
                )
            })
            .unwrap_or_else(Task::none)
    }

    /// Ends the delete, finished, failed or stopped: the tags refer to entries
    /// that may be gone, so they go, and both panels are re-read.
    fn end_delete(&mut self) -> Task<Message> {
        let side = self.deleting.take().map_or(self.active_panel, |d| d.side);
        self.generation = self.generation.wrapping_add(1);
        self.job_progress = None;
        self.panel_mut(side).selection.clear();
        let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
            let path = self.panel(each).path.clone();
            self.load(each, path, None)
        });
        Task::batch(tasks)
    }

    /// F3 / F4: the row under the cursor goes to the external program. Tags
    /// are ignored, as in Norton Commander; `..` and directories have nothing
    /// to view or edit.
    fn open_external(&mut self, kind: fs::OpenKind) {
        let Some(entry) = self.active_panel().selected_entry() else {
            return;
        };
        if entry.is_parent || entry.is_dir {
            return;
        }
        let path = entry.path.clone();
        self.job_error = None;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let executable = fs::could_execute(&path);
        if fs::open_command(kind, &path, executable).is_none() {
            let message = self
                .lang
                .text(Msg::ErrorOpenRefused)
                .replace("{name}", &name);
            self.set_status_error(&message);
            return;
        }
        if let Err(reason) = self.launcher.launch(kind, &path, executable) {
            let message = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", &name)
                .replace("{reason}", &reason);
            self.set_status_error(&message);
        }
    }

    /// The status line for an entry that could not be deleted. A failed trash
    /// points at Shift+F8 rather than falling back to it: deleting for good is
    /// the user's decision, not the app's.
    fn delete_failure(&self, index: usize, reason: &str) -> String {
        let permanent = self.deleting.as_ref().is_some_and(|d| d.permanent);
        let name = self
            .deleting
            .as_ref()
            .and_then(|d| d.sources.get(index))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let template = if permanent {
            Msg::ErrorDeleteFailed
        } else {
            Msg::ErrorTrashFailed
        };
        self.lang
            .text(template)
            .replace("{name}", &name)
            .replace("{reason}", reason)
    }

    /// F5 / F6: resolve what to transfer, then start the first row.
    ///
    /// The rows are resolved here rather than later because a conflict dialog
    /// pauses the operation, and by then the selection may have moved.
    fn start_transfer(&mut self, kind: TransferKind) -> Task<Message> {
        let sources = self.action_sources();

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
        self.conflict_rule = None;
        self.generation = self.generation.wrapping_add(1);
        self.run_from(transfer, 0, fs::transfer::OnConflict::Fail)
    }

    /// Answers a conflict and remembers an "all" answer, so the next
    /// conflict does not ask again.
    fn answer_conflict(&mut self, clicked: ConflictChoice) -> Task<Message> {
        let Some(pending) = self.pending_conflict.take() else {
            return Task::none();
        };
        // The tick belonged to the question just answered, so it goes with it.
        let choice = if self.conflict_all {
            clicked.for_all()
        } else {
            clicked
        };
        self.conflict_all = false;
        // Cancel ends the transfer; the rows already done stay done.
        if choice == ConflictChoice::Cancel {
            return self.end_transfer();
        }
        if choice.applies_to_all() {
            self.conflict_rule = Some(choice.conflict());
        }
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

    /// Ends the transfer, finished or not: forgets it and its "for all" rule, and
    /// reloads both panels, since a move changes the source directory as well as
    /// the target and rows before a failure or a cancel are already done.
    fn end_transfer(&mut self) -> Task<Message> {
        self.transfer = None;
        self.conflict_rule = None;
        self.generation = self.generation.wrapping_add(1);
        let source_side = self.active_panel;
        let target_side = source_side.other();
        let source_path = self.panel(source_side).path.clone();
        let target_path = self.panel(target_side).path.clone();
        Task::batch([
            self.load(source_side, source_path, None),
            self.load(target_side, target_path, None),
        ])
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
            return self.end_transfer();
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

        let generation = self.generation;
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
                        // The kind travels with the message rather than being
                        // formatted into it: the dialog has to recognise a
                        // conflict, and it cannot do that from wording.
                        Ok(Err(e)) => Err(RowFailure::from_io(e)),
                        // A dead blocking task is not a conflict. Its reason is
                        // a string already, so it is shown as one.
                        Err(join) => Err(RowFailure::Other(join.to_string())),
                    };
                    Message::TransferRowDone {
                        result,
                        index,
                        generation,
                    }
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
        route_key(&app.prompt_key_state(), key, Modifiers::default())
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
            press(&app, Key::Named(Named::Space)),
            Some(Message::PromptInput("abc ".into()))
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
            keymap::function_keys(Language::English)[6].is_some(),
            "F7 opens the prompt but the function key bar does not show it"
        );
    }

    // --- the whole chain, without a window -------------------------------
    //
    // Driving `update` the way a keypress would: open, type, submit. The
    // filesystem work itself is not awaited here, so this covers routing and
    // validation; the directory is checked in the async test below.

    fn press_and_update(app: &mut App, key: Key) {
        if let Some(message) = route_key(&app.prompt_key_state(), key, Modifiers::default()) {
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
            function_keys: keymap::function_keys(Language::default()),
            prompt: None,
            prompt_side: None,
            prompt_request_id: 0,
            pending_reload: None,
            jobs: jobs::Queue::new(),
            transfer: None,
            pending_conflict: None,
            delete_dialog: None,
            deleting: None,
            dialog_focus: 0,
            modifiers: Modifiers::default(),
            trash: Arc::new(fs::SystemTrash),
            launcher: Arc::new(fs::SystemLauncher),
            job_error: None,
            job_done: 0,
            conflict_all: false,
            conflict_rule: None,
            generation: 0,
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            volume_menu: None,
            list_volumes: Arc::new(fs::list_volumes),
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
        let mut app = Self::with_fixed_panels();
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

    /// Whether the job queue is empty.
    ///
    /// Read by the end-to-end tests, which cannot reach the private field.
    pub fn job_is_idle(&self) -> bool {
        !self.jobs.is_busy()
    }

    /// Whether a transfer is waiting on the overwrite dialog.
    ///
    /// Read by the end-to-end tests, which cannot reach the private field.
    pub fn conflict_is_pending(&self) -> bool {
        self.pending_conflict.is_some()
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

    /// Whether the delete confirmation is up.
    pub fn delete_dialog_is_open(&self) -> bool {
        self.delete_dialog.is_some()
    }

    /// Replaces the launcher, so a test never starts a real program.
    pub fn with_launcher(mut self, launcher: Arc<dyn fs::Launcher>) -> Self {
        self.launcher = launcher;
        self
    }

    /// Replaces the drive list, so a test offers its own directories.
    pub fn with_volumes(
        mut self,
        list: impl Fn() -> Vec<fs::Volume> + Send + Sync + 'static,
    ) -> Self {
        self.list_volumes = Arc::new(list);
        self
    }

    /// Whether the drive menu is up.
    pub fn volume_menu_is_open(&self) -> bool {
        self.volume_menu.is_some()
    }

    /// The highlighted row of the open drive menu.
    pub fn volume_menu_selected(&self) -> Option<usize> {
        self.volume_menu.as_ref().map(|menu| menu.selected)
    }

    /// Puts the drive menu in front of the user with a fixed list, for the
    /// snapshot tests.
    pub fn open_volume_menu_for_test(&mut self, side: PanelSide, volumes: Vec<fs::Volume>) {
        let current = self.panel(side).path.clone();
        self.volume_menu = Some(VolumeMenu::new(side, volumes, &current));
    }

    /// Replaces the trash, so a test never fills the developer's real one.
    pub fn with_trash(mut self, trash: Arc<dyn fs::Trash>) -> Self {
        self.trash = trash;
        self
    }

    /// Puts the delete dialog in front of the user for `count` fixed entries,
    /// for the snapshot tests.
    pub fn open_delete_dialog_for_test(&mut self, count: usize, permanent: bool) {
        self.dialog_focus = usize::from(permanent);
        self.delete_dialog = Some(Deletion {
            sources: (0..count)
                .map(|i| PathBuf::from("/left").join(format!("entry-{}.txt", i + 1)))
                .collect(),
            permanent,
            side: PanelSide::Left,
        });
    }

    /// The text of the status-line error, for the end-to-end tests.
    pub fn job_error_for_test(&self) -> Option<&str> {
        self.job_error.as_deref()
    }

    /// Puts a conflict in front of the user, for the snapshot tests.
    ///
    /// The state is set up rather than reached through a real copy: that would
    /// make the rendered image depend on what happens to be in the directory,
    /// and the test is about the dialog, not about the copy.
    pub fn open_conflict_for_test(&mut self, target: PathBuf, name: &str) {
        self.pending_conflict = Some(PendingConflict {
            transfer: Transfer {
                kind: TransferKind::Copy,
                sources: vec![PathBuf::from("/left").join(name)],
                target,
            },
            index: 0,
        });
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

        let routed = route_key(
            &app.prompt_key_state(),
            Key::Named(Named::Escape),
            Modifiers::default(),
        );
        let Some(message) = routed else {
            panic!("Escape while a job runs produced no message");
        };
        let _stopped = app.update(message);

        assert!(!app.jobs.is_busy(), "the job kept running after Escape");
        assert!(app.transfer.is_none(), "the transfer was left half-done");
    }

    /// After a stop, the next copy gets a channel somebody is listening on. A
    /// sender kept from before the stop has no receiver, and every tick of the
    /// next copy would fail on it.
    #[tokio::test]
    async fn a_copy_after_a_stop_gets_a_live_progress_channel() {
        let mut app = app_with_files();
        let _first = app.update(Message::Transfer(TransferKind::Copy));
        let _stopped = app.update(Message::AbortJob);

        let _second = app.update(Message::Transfer(TransferKind::Copy));

        let sender = app.progress_tx.as_ref().expect("a channel for the copy");
        assert!(!sender.is_closed(), "nobody listens to the new copy");
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
    ///
    /// The failure is built the way the operating system builds it and wrapped
    /// the way the app wraps it, so this exercises the classification and not a
    /// string that happens to look right. `e2e_tests.rs` covers the same ground
    /// with a real copy against a real file; this one is here because it also
    /// covers the "for all" rule, which needs several conflicts in a row.
    fn deliver_conflict(app: &mut App) {
        let _task = app.update(Message::TransferRowDone {
            result: Err(RowFailure::from_io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "alpha.txt already exists",
            ))),
            index: 0,
            generation: app.generation,
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

    /// Cancel ends the transfer: no dialog, nothing left to resume, and the
    /// "for all" rule of that transfer does not leak into the next one.
    #[tokio::test]
    async fn cancel_ends_the_transfer() {
        let (mut app, _scratch) = with_conflict();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        deliver_conflict(&mut app);
        app.conflict_rule = Some(fs::transfer::OnConflict::Skip);

        let _task = app.answer_conflict(ConflictChoice::Cancel);

        assert!(app.pending_conflict.is_none(), "the dialog is still up");
        assert!(app.transfer.is_none(), "the transfer is still running");
        assert_eq!(app.conflict_rule, None, "the rule outlived the transfer");
    }

    /// "Keep all" is a standing rule to skip, not to ask again or to fail.
    #[tokio::test]
    async fn keep_all_skips_instead_of_asking_again() {
        let (mut app, _scratch) = with_conflict();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        deliver_conflict(&mut app);

        let _task = app.answer_conflict(ConflictChoice::AllKeep);

        assert!(app.pending_conflict.is_none());
        assert_eq!(app.conflict_rule, Some(fs::transfer::OnConflict::Skip));
    }

    /// A rule from an earlier transfer must not answer the next transfer's
    /// conflicts without asking.
    #[tokio::test]
    async fn a_new_transfer_forgets_the_old_rule() {
        let (mut app, _scratch) = with_conflict();
        app.conflict_rule = Some(fs::transfer::OnConflict::Overwrite);

        let _started = app.update(Message::Transfer(TransferKind::Copy));

        assert_eq!(app.conflict_rule, None);
    }

    /// A late result from a stopped transfer must not steer the next one: here
    /// it would otherwise end the new transfer with an error.
    #[tokio::test]
    async fn a_late_result_of_a_stopped_transfer_is_ignored() {
        let (mut app, _scratch) = with_conflict();
        let _first = app.update(Message::Transfer(TransferKind::Copy));
        let stale = app.generation;
        let _stopped = app.update(Message::AbortJob);
        let _second = app.update(Message::Transfer(TransferKind::Copy));
        assert!(app.jobs.is_busy());

        let _task = app.update(Message::TransferRowDone {
            result: Err(RowFailure::Other("the app is gone".to_string())),
            index: 0,
            generation: stale,
        });

        assert!(
            app.transfer.is_some(),
            "the old result ended the new transfer"
        );
        assert!(
            app.jobs.is_busy(),
            "the old result freed the new job's slot"
        );
    }

    /// And the rule goes when the last row is done.
    #[tokio::test]
    async fn a_finished_transfer_forgets_its_rule() {
        let (mut app, _scratch) = with_conflict();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        app.conflict_rule = Some(fs::transfer::OnConflict::Overwrite);
        let transfer = app.transfer.clone().expect("a running transfer");

        let _task = app.update(Message::TransferRowDone {
            result: Ok(transfer),
            index: 0,
            generation: app.generation,
        });

        assert!(app.transfer.is_none());
        assert_eq!(app.conflict_rule, None);
    }

    /// The conflict is recognised from the error kind, not from the wording.
    /// A wording change in the filesystem layer would otherwise turn every
    /// conflict into a silent failure.
    #[test]
    fn a_conflict_is_recognised_by_its_kind() {
        // Built the way the operating system builds them, and matched the way
        // the app matches them: on the kind, not on the wording. The old test
        // fed these strings to a `starts_with` that could never be true, which
        // is why it passed while the dialog never appeared.
        let conflict = std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "/tmp/dst/alpha.txt already exists",
        );
        assert_eq!(
            RowFailure::from_io(conflict),
            RowFailure::Conflict("/tmp/dst/alpha.txt already exists".to_string())
        );
        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        assert!(matches!(RowFailure::from_io(denied), RowFailure::Other(_)));
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

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod conflict_keys {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn app_with_conflict() -> (App, tempfile::TempDir) {
        let scratch = tempfile::Builder::new()
            .prefix("ncrs-conflict-keys-")
            .tempdir()
            .expect("a scratch directory");
        let left = scratch.path().join("left");
        let right = scratch.path().join("right");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join("a.txt"), b"new").unwrap();
        std::fs::write(right.join("a.txt"), b"old").unwrap();

        let mut app = App::new().0;
        app.left_panel.path = left.clone();
        app.right_panel.path = right;
        app.left_panel.entries = vec![FileEntry {
            name: OsString::from("a.txt"),
            path: left.join("a.txt"),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 3,
            modified: None,
        }];
        app.left_panel.selected = 0;
        (app, scratch)
    }

    fn press(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
        route_key(&app.prompt_key_state(), key, modifiers)
    }

    /// The dialog has to be answerable without the mouse, and the keys are the
    /// ones a user expects from a question.
    #[test]
    fn enter_overwrites_and_escape_keeps() {
        let (mut app, _scratch) = app_with_conflict();
        // The conflict is pending, so the keys answer it rather than the panels.
        app.pending_conflict = Some(PendingConflict {
            transfer: Transfer {
                kind: TransferKind::Copy,
                sources: vec![PathBuf::from("/left/a.txt")],
                target: PathBuf::from("/right"),
            },
            index: 0,
        });

        assert_eq!(
            press(&app, Key::Named(Named::Enter), Modifiers::default()),
            Some(Message::DialogActivate),
            "Enter does not press the focused button"
        );
        assert_eq!(
            press(&app, Key::Named(Named::Escape), Modifiers::default()),
            Some(Message::TransferConflict(ConflictChoice::ThisKeep)),
            "Escape does not answer the question"
        );
    }

    /// With the dialog up, Escape must not stop the job instead: the app is
    /// waiting for an answer, and Escape means "no" to a question.
    #[tokio::test]
    async fn escape_does_not_stop_the_job_while_the_dialog_is_up() {
        let (mut app, _scratch) = app_with_conflict();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        app.pending_conflict = Some(PendingConflict {
            transfer: Transfer {
                kind: TransferKind::Copy,
                sources: vec![PathBuf::from("/left/a.txt")],
                target: PathBuf::from("/right"),
            },
            index: 0,
        });

        let message = press(&app, Key::Named(Named::Escape), Modifiers::default());
        assert!(
            !matches!(message, Some(Message::AbortJob)),
            "Escape stopped the copy instead of answering the question"
        );
    }

    /// Ctrl+C cancels the whole operation — the same key that aborts a job, and
    /// distinct from Escape so the two answers do not collide.
    #[test]
    fn ctrl_c_cancels_the_operation() {
        let (mut app, _scratch) = app_with_conflict();
        app.pending_conflict = Some(PendingConflict {
            transfer: Transfer {
                kind: TransferKind::Copy,
                sources: vec![PathBuf::from("/left/a.txt")],
                target: PathBuf::from("/right"),
            },
            index: 0,
        });

        assert_eq!(
            press(&app, Key::Character("c".into()), Modifiers::CTRL),
            Some(Message::TransferConflict(ConflictChoice::Cancel)),
            "Ctrl+C does not cancel the operation"
        );
    }

    /// With no dialog up, the same keys mean what they always meant. Otherwise
    /// the new branches would have swallowed normal navigation.
    #[test]
    fn without_a_dialog_the_keys_are_unchanged() {
        let (app, _scratch) = app_with_conflict();
        assert_eq!(
            press(&app, Key::Named(Named::Enter), Modifiers::default()),
            Some(Message::OpenSelected),
            "Enter no longer opens the selected entry"
        );
        assert_eq!(
            press(&app, Key::Named(Named::Escape), Modifiers::default()),
            None
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
mod delete_tests {
    use super::*;
    use iced::keyboard::key::Named;

    fn keys(delete_dialog: DeleteKeys) -> PromptKeyState {
        PromptKeyState {
            delete_dialog,
            ..PromptKeyState::default()
        }
    }

    fn route(state: &PromptKeyState, named: Named, modifiers: Modifiers) -> Option<Message> {
        route_key(state, Key::Named(named), modifiers)
    }

    #[test]
    fn f8_and_shift_f8_are_different_keys() {
        let idle = PromptKeyState::default();
        assert_eq!(
            route(&idle, Named::F8, Modifiers::default()),
            Some(Message::Delete { permanent: false })
        );
        assert_eq!(
            route(&idle, Named::F8, Modifiers::SHIFT),
            Some(Message::Delete { permanent: true })
        );
    }

    #[test]
    fn enter_presses_the_focused_button_and_escape_cancels() {
        let state = keys(DeleteKeys::Trash);
        assert_eq!(
            route(&state, Named::Enter, Modifiers::default()),
            Some(Message::DialogActivate)
        );
        assert_eq!(
            route(&state, Named::Escape, Modifiers::default()),
            Some(Message::DeleteCancel)
        );
        // Nothing else reaches the panels behind the dialog.
        assert_eq!(route(&state, Named::ArrowDown, Modifiers::default()), None);
    }

    #[test]
    fn the_permanent_dialog_still_confirms_on_a_second_shift_f8() {
        let state = keys(DeleteKeys::Permanent);
        assert_eq!(
            route(&state, Named::F8, Modifiers::SHIFT),
            Some(Message::DeleteConfirm)
        );
        assert_eq!(route(&state, Named::F8, Modifiers::default()), None);
    }

    #[test]
    fn arrows_and_tab_move_the_focus_in_both_dialogs() {
        let conflict = PromptKeyState {
            conflict_open: true,
            ..PromptKeyState::default()
        };
        for state in [
            keys(DeleteKeys::Trash),
            keys(DeleteKeys::Permanent),
            conflict,
        ] {
            assert_eq!(
                route(&state, Named::ArrowRight, Modifiers::default()),
                Some(Message::DialogFocus(1))
            );
            assert_eq!(
                route(&state, Named::ArrowLeft, Modifiers::default()),
                Some(Message::DialogFocus(-1))
            );
            assert_eq!(
                route(&state, Named::Tab, Modifiers::default()),
                Some(Message::DialogFocus(1))
            );
            assert_eq!(
                route(&state, Named::Tab, Modifiers::SHIFT),
                Some(Message::DialogFocus(-1))
            );
        }
    }

    #[test]
    fn space_and_shift_arrows_tag() {
        let idle = PromptKeyState::default();
        assert_eq!(
            route(&idle, Named::Space, Modifiers::default()),
            Some(Message::ToggleTag)
        );
        assert_eq!(
            route(&idle, Named::ArrowDown, Modifiers::SHIFT),
            Some(Message::TagMove(1))
        );
        assert_eq!(
            route(&idle, Named::ArrowUp, Modifiers::SHIFT),
            Some(Message::TagMove(-1))
        );
        assert_eq!(
            route(&idle, Named::ArrowDown, Modifiers::default()),
            Some(Message::MoveSelection(1))
        );
    }

    #[test]
    fn a_held_enter_does_not_press_a_button() {
        assert!(!repeats(&Message::DialogActivate));
        assert!(!repeats(&Message::DialogFocus(1)));
    }

    #[tokio::test]
    async fn focus_wraps_and_enter_presses_the_focused_button() {
        let mut app = app_with_entries(&["a.txt"]);
        let _open = app.update(Message::Delete { permanent: true });
        assert_eq!(
            app.dialog_focus, 1,
            "the permanent dialog must start on Cancel"
        );

        let _wrap = app.update(Message::DialogFocus(1));
        assert_eq!(app.dialog_focus, 0);
        let _back = app.update(Message::DialogFocus(-1));
        assert_eq!(app.dialog_focus, 1);
        let _cancel = app.update(Message::DialogActivate);
        assert!(
            app.delete_dialog.is_none(),
            "Enter on Cancel kept the dialog"
        );

        let _again = app.update(Message::Delete { permanent: false });
        assert_eq!(app.dialog_focus, 0, "the trash dialog starts on Delete");
        let _move = app.update(Message::DialogFocus(1));
        let _enter = app.update(Message::DialogActivate);
        assert!(app.delete_dialog.is_none());
        assert!(app.deleting.is_none(), "Enter on Cancel started a delete");
    }

    #[tokio::test]
    async fn a_command_click_tags_the_row() {
        let mut app = app_with_entries(&["a.txt", "b.txt"]);
        let click = |target: &mut App| {
            let _task = target.update(Message::RowClicked {
                side: PanelSide::Left,
                index: 1,
                on_tag: false,
            });
        };
        click(&mut app);
        assert!(
            !app.left_panel.selection.any_tagged(),
            "a plain click tagged"
        );

        let _held = app.update(Message::ModifiersChanged(Modifiers::COMMAND));
        click(&mut app);
        assert!(
            app.left_panel.selection.any_tagged(),
            "Cmd-click did not tag"
        );
    }

    #[tokio::test]
    async fn space_tags_and_moves_down_but_skips_the_parent_entry() {
        let mut app = app_with_entries(&["..", "a.txt", "b.txt"]);
        app.left_panel.selected = 0;
        let _parent = app.update(Message::ToggleTag);
        assert!(!app.left_panel.selection.any_tagged(), "`..` was tagged");
        assert_eq!(app.left_panel.selected, 1);

        let _tag = app.update(Message::ToggleTag);
        assert_eq!(app.left_panel.selected, 2);
        let _up = app.update(Message::TagMove(-1));
        assert_eq!(app.left_panel.selected, 1);
        assert_eq!(
            app.left_panel
                .selection
                .ordered(&app.left_panel.entries)
                .len(),
            2
        );
    }

    fn app_with_entries(names: &[&str]) -> App {
        let mut app = App::new().0;
        app.left_panel.path = PathBuf::from("/left");
        app.left_panel.entries = names
            .iter()
            .map(|name| fs::FileEntry {
                name: (*name).into(),
                path: PathBuf::from("/left").join(name),
                is_dir: false,
                is_symlink: false,
                is_parent: *name == "..",
                size: 1,
                modified: None,
            })
            .collect();
        app
    }

    #[tokio::test]
    async fn an_empty_panel_opens_no_dialog() {
        let mut app = app_with_entries(&[]);
        let _task = app.update(Message::Delete { permanent: false });
        assert!(app.delete_dialog.is_none());
    }

    #[tokio::test]
    async fn the_subject_is_the_name_for_one_and_the_count_for_many() {
        let mut app = app_with_entries(&["a.txt", "b.txt"]);
        app.left_panel.selected = 0;
        let _task = app.update(Message::Delete { permanent: false });
        let one = app.delete_dialog.clone().unwrap();
        assert_eq!(app.delete_subject(&one), "a.txt");

        app.delete_dialog = None;
        app.left_panel.selection.toggle(&"a.txt".into());
        app.left_panel.selection.toggle(&"b.txt".into());
        let _again = app.update(Message::Delete { permanent: true });
        let many = app.delete_dialog.clone().unwrap();
        assert_eq!(app.delete_subject(&many), "2 items\na.txt\nb.txt");
        assert!(many.permanent);

        // The trash dialog stays short: only the count.
        let trash = Deletion {
            permanent: false,
            ..many.clone()
        };
        assert_eq!(app.delete_subject(&trash), "2 items");

        let seven = Deletion {
            sources: (1..=7)
                .map(|i| PathBuf::from(format!("/left/f{i}")))
                .collect(),
            ..many
        };
        assert_eq!(
            app.delete_subject(&seven),
            "7 items\nf1\nf2\nf3\nf4\nf5\n… and 2 more"
        );
    }

    #[test]
    fn alt_ctrl_and_super_do_not_fall_back_to_the_bare_key() {
        let idle = PromptKeyState::default();
        for modifiers in [Modifiers::ALT, Modifiers::CTRL, Modifiers::LOGO] {
            assert_eq!(route(&idle, Named::Enter, modifiers), None, "{modifiers:?}");
            assert_eq!(route(&idle, Named::F5, modifiers), None, "{modifiers:?}");
        }
        // Ctrl+* is CTRL|SHIFT on a real keyboard, since `*` needs Shift.
        assert_eq!(
            route_key(
                &idle,
                Key::Character("*".into()),
                Modifiers::CTRL | Modifiers::SHIFT
            ),
            Some(Message::ClearTags)
        );
        // Shift alone still reaches the bare key, as `*` needs.
        assert_eq!(
            route_key(&idle, Key::Character("*".into()), Modifiers::SHIFT),
            Some(Message::TagAll)
        );
    }

    #[test]
    fn only_movement_tagging_and_typing_repeat() {
        assert!(repeats(&Message::MoveSelection(1)));
        assert!(repeats(&Message::ToggleTag));
        assert!(repeats(&Message::PromptInput("a".into())));
        assert!(!repeats(&Message::Delete { permanent: true }));
        assert!(!repeats(&Message::DeleteConfirm));
        assert!(!repeats(&Message::Transfer(TransferKind::Copy)));
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
mod volume_menu_tests {
    use super::*;

    fn volume(name: &str, path: &str) -> fs::Volume {
        fs::Volume {
            name: name.to_string(),
            path: PathBuf::from(path),
        }
    }

    fn drives() -> Vec<fs::Volume> {
        vec![
            volume("Root", "/"),
            volume("Home", "/home/test"),
            volume("Stick", "/media/stick"),
        ]
    }

    fn route(named: Named, modifiers: Modifiers) -> Option<Message> {
        route_key(&PromptKeyState::default(), Key::Named(named), modifiers)
    }

    fn open_menu() -> PromptKeyState {
        PromptKeyState {
            volume_menu: true,
            ..PromptKeyState::default()
        }
    }

    /// On macOS the Option key arrives as the ALT modifier, and F1 stays the
    /// named key: function keys have no character for Option to change.
    #[test]
    fn alt_f1_and_alt_f2_open_the_menu_for_their_panel() {
        let alt = Modifiers::ALT;
        assert!(alt.alt());
        assert_eq!(
            route(Named::F1, alt),
            Some(Message::VolumeMenu(PanelSide::Left))
        );
        assert_eq!(
            route(Named::F2, alt),
            Some(Message::VolumeMenu(PanelSide::Right))
        );
        assert_eq!(route(Named::F1, Modifiers::default()), None);
        assert_eq!(route(Named::F1, Modifiers::CTRL), None);
    }

    #[test]
    fn an_open_menu_takes_the_movement_keys_and_nothing_else() {
        let menu = open_menu();
        let key = |named| route_key(&menu, Key::Named(named), Modifiers::default());
        assert_eq!(key(Named::ArrowDown), Some(Message::VolumeMenuMove(1)));
        assert_eq!(key(Named::ArrowUp), Some(Message::VolumeMenuMove(-1)));
        assert_eq!(key(Named::Home), Some(Message::VolumeMenuFirst));
        assert_eq!(key(Named::End), Some(Message::VolumeMenuLast));
        assert_eq!(key(Named::Enter), Some(Message::VolumeMenuActivate));
        assert_eq!(key(Named::Escape), Some(Message::VolumeMenuClose));
        for ignored in [Named::F5, Named::F8, Named::Tab, Named::Backspace] {
            assert_eq!(key(ignored), None, "{ignored:?} reached the panels");
        }
    }

    #[test]
    fn movement_repeats_but_choosing_does_not() {
        assert!(repeats(&Message::VolumeMenuMove(1)));
        assert!(repeats(&Message::VolumeMenuFirst));
        assert!(repeats(&Message::VolumeMenuLast));
        assert!(!repeats(&Message::VolumeMenuActivate));
        assert!(!repeats(&Message::VolumeMenu(PanelSide::Left)));
    }

    #[test]
    fn the_menu_opens_on_the_longest_matching_drive() {
        let menu = |current: &str| VolumeMenu::new(PanelSide::Left, drives(), Path::new(current));
        assert_eq!(menu("/home/test/docs").selected, 1);
        assert_eq!(menu("/home/other").selected, 0);
        assert_eq!(menu("/media/stick").selected, 2);
        // `/home/tester` is not below `/home/test`.
        assert_eq!(menu("/home/tester").selected, 0);
        assert_eq!(menu("/elsewhere").selected, 0);
    }

    #[test]
    fn the_highlight_stops_at_both_ends() {
        let mut menu = VolumeMenu::new(PanelSide::Left, drives(), Path::new("/"));
        menu.move_by(-1);
        assert_eq!(menu.selected, 0);
        menu.move_by(5);
        assert_eq!(menu.selected, 2);
    }

    #[test]
    fn the_menu_opens_for_the_panel_that_asked() {
        let mut app = App::with_fixed_panels().with_volumes(drives);
        let _open = app.update(Message::VolumeMenu(PanelSide::Right));
        assert_eq!(app.volume_menu.as_ref().unwrap().side, PanelSide::Right);
        // `/home/test/files` is the right panel's path.
        assert_eq!(app.volume_menu_selected(), Some(1));
        assert!(app.prompt_key_state().volume_menu);

        let _close = app.update(Message::VolumeMenuClose);
        assert!(!app.volume_menu_is_open());
        assert!(!app.prompt_key_state().volume_menu);
    }

    #[test]
    fn a_dialog_keeps_the_menu_from_opening() {
        let mut app = App::with_fixed_panels().with_volumes(drives);
        app.open_delete_dialog_for_test(1, false);
        let _open = app.update(Message::VolumeMenu(PanelSide::Left));
        assert!(!app.volume_menu_is_open());
    }

    #[tokio::test]
    async fn choosing_a_drive_loads_it_and_activates_its_panel() {
        let mut app = App::with_fixed_panels().with_volumes(drives);
        let _open = app.update(Message::VolumeMenu(PanelSide::Right));
        let _down = app.update(Message::VolumeMenuMove(1));
        let _enter = app.update(Message::VolumeMenuActivate);
        assert!(!app.volume_menu_is_open());
        assert_eq!(app.active_panel, PanelSide::Right);
        assert!(app.panel(PanelSide::Right).loading);
        assert!(!app.panel(PanelSide::Left).loading);
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
mod scroll_tests {
    use super::*;
    use iced::mouse::ScrollDelta;

    /// The fixed app has 12 visible rows; the left panel gets 100 entries.
    fn app_with_long_list() -> App {
        let mut app = App::with_fixed_panels();
        app.left_panel.entries = (0..100)
            .map(|i| fs::FileEntry {
                name: format!("f{i:03}").into(),
                path: PathBuf::from(format!("/left/f{i:03}")),
                is_dir: false,
                is_symlink: false,
                is_parent: false,
                size: 1,
                modified: None,
            })
            .collect();
        app
    }

    fn scroll(app: &mut App, side: PanelSide, delta: ScrollDelta) {
        let _task = app.update(Message::PanelScrolled { side, delta });
    }

    fn pixels(y: f32) -> ScrollDelta {
        ScrollDelta::Pixels { x: 0.0, y }
    }

    fn lines(y: f32) -> ScrollDelta {
        ScrollDelta::Lines { x: 0.0, y }
    }

    /// A slow trackpad gesture is many small events; none of them is a row
    /// alone, together they are.
    #[test]
    fn small_pixel_events_add_up_to_whole_rows() {
        let mut app = app_with_long_list();
        // 22 px per row: 5 x 10 px is 2.27 rows.
        for _ in 0..5 {
            scroll(&mut app, PanelSide::Left, pixels(-10.0));
        }
        assert_eq!(app.left_panel.scroll_offset, 2);
        // The rest of each event is kept: 80 px in all is 3.6 rows.
        for _ in 0..3 {
            scroll(&mut app, PanelSide::Left, pixels(-10.0));
        }
        assert_eq!(app.left_panel.scroll_offset, 3);
        // And back up, towards the top of the list.
        scroll(&mut app, PanelSide::Left, pixels(88.0));
        assert_eq!(app.left_panel.scroll_offset, 0);
    }

    #[test]
    fn wheel_lines_scroll_by_rows() {
        let mut app = app_with_long_list();
        scroll(&mut app, PanelSide::Left, lines(-3.0));
        assert_eq!(app.left_panel.scroll_offset, 3);
        scroll(&mut app, PanelSide::Left, lines(1.0));
        assert_eq!(app.left_panel.scroll_offset, 2);
    }

    #[test]
    fn only_the_panel_under_the_pointer_scrolls_and_the_ends_hold() {
        let mut app = app_with_long_list();
        scroll(&mut app, PanelSide::Right, lines(-5.0));
        assert_eq!(app.left_panel.scroll_offset, 0);

        scroll(&mut app, PanelSide::Left, lines(500.0));
        assert_eq!(app.left_panel.scroll_offset, 0, "scrolled above the top");
        scroll(&mut app, PanelSide::Left, lines(-500.0));
        assert_eq!(app.left_panel.scroll_offset, 88, "scrolled past the end");
        // Scrolling up from the end moves at once; nothing was banked.
        scroll(&mut app, PanelSide::Left, lines(1.0));
        assert_eq!(app.left_panel.scroll_offset, 87);
    }

    #[test]
    fn the_cursor_stays_while_visible_and_is_held_at_the_edge() {
        let mut app = app_with_long_list();
        app.left_panel.select(5, 12);
        scroll(&mut app, PanelSide::Left, lines(-2.0));
        assert_eq!(app.left_panel.selected, 5, "the cursor moved while in view");

        scroll(&mut app, PanelSide::Left, lines(-10.0));
        assert_eq!(app.left_panel.scroll_offset, 12);
        assert_eq!(app.left_panel.selected, 12, "the cursor left the top");

        scroll(&mut app, PanelSide::Left, lines(20.0));
        assert_eq!(app.left_panel.scroll_offset, 0);
        assert_eq!(app.left_panel.selected, 11, "the cursor left the bottom");
    }

    #[test]
    fn a_dialog_or_the_menu_keeps_the_panel_still() {
        let mut app = app_with_long_list();
        app.open_delete_dialog_for_test(1, false);
        scroll(&mut app, PanelSide::Left, lines(-4.0));
        assert_eq!(app.left_panel.scroll_offset, 0, "scrolled under the dialog");

        let mut with_menu = app_with_long_list().with_volumes(|| {
            vec![fs::Volume {
                name: "Root".to_string(),
                path: PathBuf::from("/"),
            }]
        });
        let _open = with_menu.update(Message::VolumeMenu(PanelSide::Left));
        scroll(&mut with_menu, PanelSide::Left, lines(-4.0));
        assert_eq!(
            with_menu.left_panel.scroll_offset, 0,
            "scrolled under the menu"
        );

        let mut with_prompt = app_with_long_list();
        let _prompt = with_prompt.update(Message::CreateDirPrompt);
        scroll(&mut with_prompt, PanelSide::Left, lines(-4.0));
        assert_eq!(
            with_prompt.left_panel.scroll_offset, 0,
            "scrolled under the prompt"
        );
    }

    #[test]
    fn page_keys_and_cmd_arrows_turn_a_page() {
        let idle = PromptKeyState::default();
        for (key, modifiers, message) in [
            (Named::PageDown, Modifiers::default(), Message::PageDown),
            (Named::PageUp, Modifiers::default(), Message::PageUp),
            (Named::ArrowDown, Modifiers::COMMAND, Message::PageDown),
            (Named::ArrowUp, Modifiers::COMMAND, Message::PageUp),
        ] {
            assert_eq!(
                route_key(&idle, Key::Named(key), modifiers),
                Some(message),
                "{key:?} {modifiers:?}"
            );
        }
        assert!(repeats(&Message::PageDown));
        assert!(repeats(&Message::PageUp));

        let mut app = app_with_long_list();
        let _down = app.update(Message::PageDown);
        assert_eq!(app.left_panel.selected, 12);
        assert_eq!(app.left_panel.scroll_offset, 1);
        let _up = app.update(Message::PageUp);
        assert_eq!(app.left_panel.selected, 0);
        assert_eq!(app.left_panel.scroll_offset, 0);
    }
}
