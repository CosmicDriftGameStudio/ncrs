//! Root application: state, `update` (the only place state changes) and
//! `view` (pure composition of UI components).
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use std::sync::{Arc, Mutex, OnceLock};

use iced::futures;
use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::widget::{column, container, mouse_area, opaque, operation, row, Column, Space, Stack};
use iced::{event, window, Element, Length, Point, Size, Subscription, Task, Theme};

use crate::ai_model::{
    download_ai_model, AiModelCard, DownloadError as AiDownloadError, DownloadJob,
};
use crate::command_palette::{CommandPalette, RECENT_LIMIT};
use crate::config::{
    self, Config, ConfigError, ContrastWarning, Filing, FilingWriteError, VscodeWarning,
};
use crate::context_menu::{ContextAction, ContextMenu, InputState};
use crate::dialog::{Prompt, PromptKind};
use crate::execute;
use crate::filing::{FilingMessage, FilingModule, LocationStatus, Phase, Work, WorkOutcome};
use crate::filing_setup::{self, FilingSetup, SetupNote};
use crate::filing_status::{FilingStatus, SNAPSHOT_EVERY};
use crate::filing_work::FilingSummary;
use crate::fs::{self, CreateDirError};
use crate::i18n::{Language, Msg};
use crate::jobs::{self, JobEvent};
use crate::keymap::{self, KeyAction, KeyScope, Keymap};
#[cfg(any(target_os = "macos", test))]
use crate::menu::Availability;
use crate::messages::{
    AiModelMessage, ClipboardKind, CommandPaletteMessage, ConflictChoice, FilingSetupMessage,
    FilingStatusMessage, HermesPage, Message, OcrModelsMessage, PanelSide, QuickOpenMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage, SearchMessage,
    ThemeFetchMessage, TitleBarMessage, TransferKind,
};
#[cfg(target_os = "macos")]
use crate::native_menu;
use crate::ocr_models::{download_models, DownloadError, OcrModelsCard};
use crate::open_vsx::{self, ExtensionId, FetchError, FetchedExtension};
use crate::palette::Palette;
use crate::quick_open::{QuickOpen, QUICK_OPEN_FILE_LIMIT};
use crate::reorganisation_plan::ReorganisationView;
use crate::reorganisation_work::{ExecutionSummary, PlannedItem};
use crate::review_list::ReviewList;
use crate::review_work::ReviewAccess;
use crate::rule_suggestions::RuleSuggestions;
use crate::search_view::{stays_on_file_system, SearchField, SearchView};
use crate::suggestion_work::{SuggestedRule, SuggestionAccess, INDEX_NOT_OPEN};
use crate::theme_fetch::{Fetched, Installed, Pick, Stage, ThemeFetch};
use crate::ui::ai_model as ai_model_ui;
use crate::ui::command_palette as command_palette_ui;
use crate::ui::context_menu as context_menu_view;
use crate::ui::delete as delete_dialog_view;
use crate::ui::dialog::FIELD_ID;
use crate::ui::filing as filing_ui;
use crate::ui::filing_setup as filing_setup_ui;
use crate::ui::filing_status as filing_status_ui;
use crate::ui::hermes::Badges;
use crate::ui::ocr_models as ocr_models_ui;
use crate::ui::quick_open as quick_open_ui;
use crate::ui::reorganisation_plan as reorganisation_plan_ui;
use crate::ui::review_list as review_list_ui;
use crate::ui::rule_suggestions as rule_suggestions_ui;
use crate::ui::search as search_ui;
use crate::ui::theme_fetch as theme_fetch_ui;
use crate::ui::titlebar;
use crate::ui::volumes as volumes_view;
use crate::ui::{
    self, conflict, dialog, fkeys, layout, panel, statusbar, theme, PanelProps, PanelState,
};
use ncrs_index::{KeyStore, KeyringKeyStore};
use ncrs_intake::LocationMode;
use ncrs_pipeline::{Load, RunnerState};
use tokio::sync::mpsc;

/// How long the search view waits for the next keystroke before it searches.
const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);

/// The most events one message carries, so a search that finds thousands of
/// files does not make thousands of updates.
const SEARCH_BATCH: usize = 256;

fn search_batch(generation: u64, events: Vec<fs::search::SearchEvent>) -> Message {
    Message::Search(SearchMessage::Events { generation, events })
}

fn quick_open_batch(generation: u64, events: Vec<fs::search::SearchEvent>) -> Message {
    Message::QuickOpen(QuickOpenMessage::Events { generation, events })
}

/// The messages of one search, in batches. Ends when the engine is done or the
/// task is aborted.
///
/// The engine is started inside the stream: it spawns onto the blocking pool,
/// which needs the runtime a task runs in, and `update` does not run in one.
fn search_events(
    compiled: fs::search::CompiledSearch,
    generation: u64,
    to_message: fn(u64, Vec<fs::search::SearchEvent>) -> Message,
) -> impl futures::Stream<Item = Message> + Send + 'static {
    use futures::StreamExt as _;
    futures::stream::once(async move { fs::search::spawn(compiled) })
        .map(move |receiver| {
            futures::stream::unfold(receiver, move |mut open| async move {
                let mut events = Vec::new();
                let received = open.recv_many(&mut events, SEARCH_BATCH).await;
                (received > 0).then(|| (to_message(generation, events), open))
            })
        })
        .flatten()
}

/// How long a confirmation stays in the status bar.
const NOTICE_DURATION: std::time::Duration = std::time::Duration::from_secs(3);

const APP_NAME: &str = "NC-rs";

#[cfg(target_os = "macos")]
fn native_menu_events() -> Subscription<Message> {
    native_menu::events()
}

#[cfg(not(target_os = "macos"))]
fn native_menu_events() -> Subscription<Message> {
    Subscription::none()
}

/// A message rather than a call: it is handled in `update`, on the thread
/// that owns the app's windows, once the app has started and the default menu
/// winit installed is there to be replaced.
#[cfg(target_os = "macos")]
fn then_install_native_menu(startup: Task<Message>) -> Task<Message> {
    Task::batch([startup, Task::done(Message::InstallNativeMenu)])
}

#[cfg(not(target_os = "macos"))]
fn then_install_native_menu(startup: Task<Message>) -> Task<Message> {
    startup
}

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

/// The pointer position and the Option tap, shared with the raw event
/// subscription. Like [`KeyStateCell`], hashed by nothing so the subscription
/// keeps its identity while the contents change.
#[derive(Debug, Default)]
struct InputCell(Mutex<InputState>);

impl InputCell {
    fn lock(&self) -> std::sync::MutexGuard<'_, InputState> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl std::hash::Hash for InputCell {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "ncrs-input-state".hash(state);
    }
}

/// How long a typed character still belongs to the same prefix.
const TYPE_AHEAD_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

/// What has been typed to jump to an entry.
#[derive(Debug, Default)]
struct TypeAhead {
    prefix: String,
    last_typed: Option<std::time::Instant>,
}

impl TypeAhead {
    /// The prefix after `typed` arrives at `now`: it extends the previous one
    /// within the window and starts a new one after it.
    fn extend(&mut self, typed: &str, now: std::time::Instant) -> &str {
        let continues = self
            .last_typed
            .is_some_and(|last| now.saturating_duration_since(last) <= TYPE_AHEAD_WINDOW);
        if !continues {
            self.prefix.clear();
        }
        self.prefix.push_str(typed);
        self.last_typed = Some(now);
        &self.prefix
    }

    fn reset(&mut self) {
        self.prefix.clear();
        self.last_typed = None;
    }
}

/// Moving the cursor or the focus by other means ends a prefix.
fn resets_type_ahead(message: &Message) -> bool {
    matches!(
        message,
        Message::MoveSelection(_)
            | Message::PageUp
            | Message::PageDown
            | Message::SelectFirst
            | Message::SelectLast
            | Message::SwitchPanel
            | Message::ActivatePanel(_)
            | Message::RowClicked { .. }
            | Message::TagMove(_)
            | Message::ToggleTag
            | Message::OpenSelected
            | Message::GoUp
    )
}

/// Other things the user does end the editing of the path: the text field
/// loses the focus to them without telling the app.
fn leaves_path_field(message: &Message) -> bool {
    matches!(
        message,
        Message::RowClicked { .. }
            | Message::OpenSearch
            | Message::QuickOpen(QuickOpenMessage::Open)
            | Message::CommandPalette(CommandPaletteMessage::Toggle)
            | Message::ThemeFetch(ThemeFetchMessage::Open)
            | Message::RuleSuggestions(RuleSuggestionsMessage::Open)
            | Message::ReviewList(ReviewListMessage::Open)
            | Message::HermesPage(_)
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Open)
            | Message::OcrModels(OcrModelsMessage::Open)
            | Message::AiModel(AiModelMessage::Open)
            | Message::FilingStatus(FilingStatusMessage::Open)
            | Message::FilingSetup(
                FilingSetupMessage::Toggle
                    | FilingSetupMessage::Close
                    | FilingSetupMessage::PickFolder(_)
                    | FilingSetupMessage::AddPanelFolder(_)
                    | FilingSetupMessage::SetTarget(_)
                    | FilingSetupMessage::ToggleImages(_)
                    | FilingSetupMessage::Remove(_)
                    | FilingSetupMessage::Save
            )
            | Message::ContextMenuAt { .. }
            | Message::ContextMenuKey
            | Message::CopyToClipboard(_)
            | Message::SwitchPanel
            | Message::ActivatePanel(_)
            | Message::SortBy { .. }
    )
}

/// The window is the runtime's to move and resize: the id comes from the
/// runtime, since the app opens exactly one window.
fn update_title_bar(message: TitleBarMessage) -> Task<Message> {
    let window = iced::window::latest();
    match message {
        TitleBarMessage::Drag => window.and_then(iced::window::drag),
        TitleBarMessage::ToggleMaximize => window.and_then(iced::window::toggle_maximize),
        TitleBarMessage::Minimize => window.and_then(|id| iced::window::minimize(id, true)),
        TitleBarMessage::Resize(edge) => {
            window.and_then(move |id| iced::window::drag_resize(id, edge.direction()))
        }
    }
}

/// Takes the keyboard focus from the path field, so the keys go to the panels
/// again once the editing is over.
fn release_focus() -> Task<Message> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
}

/// The blocking part of the download: the package and the themes it offers.
fn download_theme(
    fetch: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>,
    id: &ExtensionId,
) -> Result<Fetched, FetchError> {
    let extension = fetch(id)?;
    let themes = crate::vsix::list_themes(&extension.vsix)?;
    Ok(Fetched { extension, themes })
}

/// The blocking part of the installation: files next to the config file, and
/// the config pointing at them.
fn install_picked_theme(pick: &Pick, config_path: &Path) -> Result<Box<Installed>, FetchError> {
    let (fetched, theme) = pick;
    let config = crate::vsix::install_theme(
        &fetched.extension.vsix,
        theme,
        &fetched.extension.id,
        config_path,
    )?;
    Ok(Box::new(Installed {
        config,
        label: theme.label.clone(),
        license: fetched.extension.license.clone(),
    }))
}

/// Runs one blocking step of the filing module off the UI thread.
fn run_filing_work(work: Work) -> Task<Message> {
    let generation = work.generation();
    Task::perform(
        tokio::task::spawn_blocking(move || work.run()),
        move |joined| {
            Message::Filing(joined.unwrap_or_else(|join| FilingMessage::WorkDone {
                generation,
                outcome: WorkOutcome::Crashed(join.to_string()),
            }))
        },
    )
}

/// Bottom-center, above the status and function key bars. No scrim, and
/// `opaque` only on the card itself, so clicks beside it reach the panels.
fn filing_card_layer(card: Element<'_, Message>) -> Element<'_, Message> {
    let bars = layout::STATUSBAR_HEIGHT
        + layout::FKEYBAR_HEIGHT
        + 2.0 * theme::spacing::SECTION_GAP
        + theme::spacing::OUTER_PADDING;
    container(opaque(card))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            bottom: bars + theme::spacing::SECTION_GAP,
            ..iced::Padding::ZERO
        })
        .into()
}

/// Ctrl+click is the Mac's right click.
fn is_context_click(modifiers: Modifiers) -> bool {
    cfg!(target_os = "macos") && modifiers.control()
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
    /// The permanent-delete key (Shift+F8 by default): no trash.
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
    /// Enter cancels; only the permanent-delete key (Shift+F8) pressed again confirms.
    Permanent,
}

/// One file waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingConflict {
    transfer: Transfer,
    /// Which of `sources` is in the way.
    index: usize,
}

/// What a click on the "access blocked" error sends. Only macOS has a privacy
/// settings page to open, and only there does the message exist.
#[cfg(target_os = "macos")]
fn blocked_access_press() -> Option<Message> {
    Some(Message::OpenPrivacySettings)
}

#[cfg(not(target_os = "macos"))]
fn blocked_access_press() -> Option<Message> {
    None
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

/// When a saved filing list takes effect.
enum SavedFilingApplies {
    Now,
    AtNextStart,
}

/// What is wrong with the config, kept to word it again after F9.
#[derive(Debug)]
enum ConfigProblem {
    /// The file was not used; the defaults apply.
    Invalid(ConfigError),
    /// The file is in use, but one pairing of colors is hard to read.
    LowContrast(ContrastWarning),
    /// The file is in use, but the VS Code theme it names was not applied in full.
    VscodeTheme(VscodeWarning),
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
    /// The bar while the search has the keys: only the actions that work there.
    search_function_keys: [Option<&'static str>; keymap::FUNCTION_KEY_COUNT],
    /// The keys in effect: the built-in ones, or those of the config file.
    keymap: Arc<Keymap>,
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
    /// The `[open]` programs from the config. What it leaves out is filled in
    /// per OS by `open_command`.
    open_programs: config::OpenPrograms,
    /// Why the config file was not used. Stays until Escape; moving the cursor
    /// does not clear it, and a job error covers it while that is shown.
    config_error: Option<String>,
    /// The error behind `config_error`, kept to word it again after F9.
    config_problem: Option<ConfigProblem>,
    /// The colors every view draws with.
    palette: Palette,
    /// Why the last job failed, in the active language. Shown in the status bar
    /// until the next thing happens.
    job_error: Option<String>,
    /// A neutral confirmation, such as "Path copied". Goes the way of `job_error`.
    notice: Option<String>,
    /// Counts the notices shown, so a timer can tell if it is still the latest.
    notice_generation: u64,
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
    /// The context menu, while it is up.
    context_menu: Option<ContextMenu>,
    type_ahead: TypeAhead,
    /// The path as typed while the status bar's path field is being edited.
    path_field: Option<String>,
    /// Id of the latest path check; an older answer is ignored.
    path_check_id: u64,
    /// What a leading `~` in a typed path stands for.
    home: PathBuf,
    /// The time type-ahead measures against. Injected so tests control it.
    clock: Arc<dyn Fn() -> std::time::Instant + Send + Sync>,
    /// The window's size, for keeping the context menu inside it.
    window_size: Size,
    /// The pointer and the Option tap, fed by the raw event subscription.
    input: Arc<InputCell>,
    /// Where the drive menu gets its list. Injected so tests offer their own
    /// directories instead of the machine's disks.
    list_volumes: Arc<dyn Fn() -> Vec<fs::Volume> + Send + Sync>,
    /// The search view. Kept when it is closed, so reopening shows the last
    /// results and fields again.
    search: Option<SearchView>,
    /// Whether the search view is up, in place of the panel on `search_side`.
    search_open: bool,
    search_side: PanelSide,
    /// Result rows that fit into the search view.
    search_rows: usize,
    /// Counts the searches started, so a late timer or batch can tell it is
    /// not the latest. Never reset: a closed view must not accept the events
    /// of a search it abandoned.
    search_generation: u64,
    /// Stops the running search when dropped, which also drops the receiver
    /// the engine's worker sends into.
    search_handle: Option<iced::task::Handle>,
    /// Quick Open (Cmd/Ctrl+P), while its box is up.
    quick_open: Option<QuickOpen>,
    /// Counts the walks started, so the late batches of a closed one are dropped.
    quick_open_walk_generation: u64,
    /// Stops the file walk when dropped.
    quick_open_walk: Option<iced::task::Handle>,
    /// The command palette (Cmd/Ctrl+Shift+P), while its box is up.
    command_palette: Option<CommandPalette>,
    /// The commands run from the command list this session, most recent
    /// first. Not kept across runs.
    recent_commands: Vec<KeyAction>,
    /// The filing module, when the config switches it on.
    filing: Option<FilingModule>,
    /// Started once the filing module is ready; dropping it stops the workers.
    pipeline: Option<ncrs_pipeline::Runner>,
    /// What the filing status page and the status bar show of the pipeline.
    filing_status: FilingStatus,
    /// The time the status page measures how long a worker has been busy by.
    wall_clock: Arc<dyn ncrs_pipeline::Clock>,
    /// Where the filing setup writes. `None` when the system names no place.
    config_path: Option<PathBuf>,
    /// The filing settings of the config file, as last loaded or written.
    filing_config: Filing,
    /// What a new filing module opens and keeps its key in; set by
    /// `with_filing`. Without a store no module is started.
    filing_index_path: Option<PathBuf>,
    filing_key_store: Option<Arc<dyn KeyStore + Send + Sync>>,
    /// The filing setup card, while it is up.
    filing_setup: Option<FilingSetup>,
    /// Looks up a folder the setup adds, on a blocking thread. Injected so
    /// tests add their fixed folders without a disk.
    resolve_folder: fn(&Path) -> Result<PathBuf, String>,
    /// Must be called from `update`: on macOS rfd sets up the panel as soon as
    /// the dialog is asked for, and AppKit allows that only on the main thread,
    /// which is the one that runs `update`.
    pick_folder: fn(&Path) -> Task<Option<PathBuf>>,
    /// The card that fetches a color theme, while it is up.
    theme_fetch: Option<ThemeFetch>,
    /// Counts the cards opened, so the late answer of a closed one is dropped.
    theme_fetch_generation: u64,
    rule_suggestions: Option<RuleSuggestions>,
    /// Tells the answer of the work that is current from a stale one.
    rule_suggestions_generation: u64,
    review_list: Option<ReviewList>,
    /// Tells the answer of the work that is current from a stale one.
    review_list_generation: u64,
    reorganisation: Option<ReorganisationView>,
    /// Tells the answer of the plan that is current from a stale one.
    reorganisation_generation: u64,
    /// `Snapshot::plans_made` the filing job was last asked for.
    plans_filed_for: usize,
    /// The filing job was due but another job was running.
    filing_due: bool,
    /// Downloads an extension, on a blocking thread. Injected so tests need
    /// no network.
    fetch_extension: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>,
    /// Writes the picked theme and switches it on, on a blocking thread.
    /// Injected so tests write no files.
    install_theme: fn(&Pick, &Path) -> Result<Box<Installed>, FetchError>,
    /// The card that asks before the OCR models are downloaded, while it is up.
    ocr_models: Option<OcrModelsCard>,
    /// Counts the cards opened, so the late answer of a closed one is dropped.
    ocr_models_generation: u64,
    /// Downloads the OCR models into the folder, on a blocking thread.
    /// Injected so tests need no network.
    download_ocr_models: fn(&Path) -> Result<(), DownloadError>,
    /// The card that asks before the AI model is downloaded, while it is up.
    ai_model: Option<AiModelCard>,
    /// Counts the cards opened, so the late answer of a closed one is dropped.
    ai_model_generation: u64,
    /// The card opened by itself for a missing model; once per start, so a
    /// "Cancel" is not asked again.
    ai_model_asked: bool,
    /// Downloads the AI model, on a blocking thread. Injected so tests need no
    /// network.
    download_ai_model: fn(&DownloadJob) -> Result<(), AiDownloadError>,
}

/// The part of the prompt the key routing needs, cheap to clone.
///
/// A subscription outlives the call that created it, so it cannot borrow the
/// app. It gets this instead: two flags and the name typed so far. `Arc<str>`
/// rather than `&'static str` because leaking a copy per keypress would grow
/// without bound, and a `String` per keypress is what this avoids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromptKeyState {
    /// The keys in effect. A shared handle: cloning the state per message
    /// must not copy the table.
    pub keymap: Arc<Keymap>,
    /// A prompt is open, so keys belong to it.
    pub open: bool,
    /// A job is running, so Escape stops it rather than doing nothing.
    pub job_running: bool,
    /// The config error is up, so Escape dismisses it.
    pub config_error: bool,
    /// The conflict dialog is up, so keys answer it rather than the panels.
    pub conflict_open: bool,
    /// The path field is being edited: its keys are the text field's.
    pub path_editing: bool,
    /// The search view is up, so the cursor keys move in its results.
    pub search: bool,
    /// Quick Open is up, so the up and down keys move in its hits.
    pub quick_open: bool,
    /// The command palette is up, so the up and down keys move in its entries.
    pub command_palette: bool,
    /// The filing status page is up, so its keys act on the pipeline.
    pub filing_status: bool,
    /// The filing setup is up, so Escape closes it.
    pub filing_setup: bool,
    /// A filing card that "Not now" may leave is up, so Escape leaves it.
    pub filing_card: bool,
    /// The theme download card is up, so Enter and the arrows are its.
    pub theme_fetch: bool,
    /// The rule suggestions view is up, so Enter, the arrows and its letters are its.
    pub rule_suggestions: bool,
    /// Its form is up, so the text fields take the letters.
    pub rule_suggestions_editing: bool,
    /// The review list is up, so Enter, the arrows and its letters are its.
    pub review_list: bool,
    /// Its form is up, so the text fields take the letters.
    pub review_list_editing: bool,
    /// The reorganisation plan view is up, so its keys are the card's.
    pub reorganisation_plan: bool,
    /// The OCR models card is up, so keys answer it like a button dialog.
    pub ocr_models: bool,
    /// The AI model card is up, so keys answer it like a button dialog.
    pub ai_model: bool,
    /// The delete dialog is up, so keys answer it.
    pub delete_dialog: DeleteKeys,
    /// The drive menu is up, so keys move its highlight.
    pub volume_menu: bool,
    /// The context menu is up, so keys move its highlight.
    pub context_menu: bool,
    /// Whether the operation is running; keys are ignored then.
    pub busy: bool,
    /// The name typed so far.
    pub typed: std::sync::Arc<str>,
}

impl Default for PromptKeyState {
    fn default() -> Self {
        Self {
            keymap: Keymap::built_in(),
            open: false,
            job_running: false,
            config_error: false,
            conflict_open: false,
            path_editing: false,
            search: false,
            quick_open: false,
            command_palette: false,
            filing_status: false,
            filing_setup: false,
            filing_card: false,
            theme_fetch: false,
            rule_suggestions: false,
            rule_suggestions_editing: false,
            review_list: false,
            review_list_editing: false,
            reorganisation_plan: false,
            ocr_models: false,
            ai_model: false,
            delete_dialog: DeleteKeys::default(),
            volume_menu: false,
            context_menu: false,
            busy: false,
            typed: Arc::default(),
        }
    }
}

/// The plan is a card: Enter runs the selection, Space selects or deselects
/// what the cursor is on, the arrows move.
fn reorganisation_plan_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let plan = |message| Some(Message::ReorganisationPlan(message));
    let plain = key_modifiers.is_empty();
    match key.as_ref() {
        Key::Named(Named::Escape) => plan(ReorganisationPlanMessage::Close),
        Key::Named(Named::Enter) => plan(ReorganisationPlanMessage::Execute),
        Key::Named(Named::Space) if plain => plan(ReorganisationPlanMessage::ToggleCursor),
        Key::Named(Named::ArrowUp) if plain => plan(ReorganisationPlanMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => plan(ReorganisationPlanMessage::Move(1)),
        Key::Character("r") if plain => plan(ReorganisationPlanMessage::Replan),
        _ => bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers)),
    }
}

/// The review list is a card: Enter confirms, the arrows move in the list, `e`
/// opens the form. With the form up the fields take the typing; Escape is
/// `InputState`'s then.
fn review_list_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let review = |message| Some(Message::ReviewList(message));
    let plain = key_modifiers.is_empty();
    match key.as_ref() {
        Key::Named(Named::Escape) => review(ReviewListMessage::Close),
        Key::Named(Named::Enter) => review(ReviewListMessage::Confirm),
        Key::Named(Named::Tab) if prompt.review_list_editing => {
            review(ReviewListMessage::FocusField {
                backwards: key_modifiers.shift(),
            })
        }
        Key::Named(Named::ArrowUp) if plain && !prompt.review_list_editing => {
            review(ReviewListMessage::Move(-1))
        }
        Key::Named(Named::ArrowDown) if plain && !prompt.review_list_editing => {
            review(ReviewListMessage::Move(1))
        }
        Key::Character("e") if plain && !prompt.review_list_editing => {
            review(ReviewListMessage::Edit)
        }
        _ => bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers)),
    }
}

/// The view is a card: Enter confirms, the arrows move in the list, and while
/// no form is up the letters decide (`e`dit, `d`iscard, `r`e-analyse). With the
/// form up the fields take the typing; Escape is `InputState`'s then.
fn rule_suggestions_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let suggestions = |message| Some(Message::RuleSuggestions(message));
    let plain = key_modifiers.is_empty();
    match key.as_ref() {
        Key::Named(Named::Escape) => suggestions(RuleSuggestionsMessage::Close),
        Key::Named(Named::Enter) => suggestions(RuleSuggestionsMessage::Confirm),
        Key::Named(Named::Tab) if prompt.rule_suggestions_editing => {
            suggestions(RuleSuggestionsMessage::FocusField {
                backwards: key_modifiers.shift(),
            })
        }
        Key::Named(Named::ArrowUp) if plain && !prompt.rule_suggestions_editing => {
            suggestions(RuleSuggestionsMessage::Move(-1))
        }
        Key::Named(Named::ArrowDown) if plain && !prompt.rule_suggestions_editing => {
            suggestions(RuleSuggestionsMessage::Move(1))
        }
        Key::Character("e") if plain && !prompt.rule_suggestions_editing => {
            suggestions(RuleSuggestionsMessage::Edit)
        }
        Key::Character("d") if plain && !prompt.rule_suggestions_editing => {
            suggestions(RuleSuggestionsMessage::Discard)
        }
        Key::Character("r") if plain && !prompt.rule_suggestions_editing => {
            suggestions(RuleSuggestionsMessage::Reanalyse)
        }
        _ => bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers)),
    }
}

fn ocr_models_key(key: Key, key_modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::OcrModels(OcrModelsMessage::Close)),
        _ => dialog_focus_key(key, key_modifiers),
    }
}

fn ai_model_key(key: Key, key_modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::AiModel(AiModelMessage::Close)),
        _ => dialog_focus_key(key, key_modifiers),
    }
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
    let scope = if prompt.search {
        KeyScope::Search
    } else {
        KeyScope::Panel
    };
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
    if prompt.context_menu {
        return match key.as_ref() {
            Key::Named(Named::Escape) => Some(Message::ContextMenuClose),
            Key::Named(Named::Enter) => Some(Message::ContextMenuActivate),
            Key::Named(Named::ArrowUp) => Some(Message::ContextMenuMove(-1)),
            Key::Named(Named::ArrowDown) => Some(Message::ContextMenuMove(1)),
            Key::Named(Named::Home) => Some(Message::ContextMenuFirst),
            Key::Named(Named::End) => Some(Message::ContextMenuLast),
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
            // The fixed dialog keys come first: a configured chord must not
            // take Enter or an arrow, or one press could confirm a deletion.
            // Only then the chord that started the permanent delete confirms it.
            _ => dialog_focus_key(key.clone(), key_modifiers).or_else(|| {
                (permanent
                    && bound_message(&prompt.keymap, &key, key_modifiers, scope)
                        == Some(Message::Delete { permanent: true }))
                .then_some(Message::DeleteConfirm)
            }),
        };
    }

    // Cancel has the focus when it opens, so Enter alone never downloads.
    if prompt.ocr_models {
        return ocr_models_key(key, key_modifiers);
    }
    if prompt.ai_model {
        return ai_model_key(key, key_modifiers);
    }

    if prompt.conflict_open {
        return match key.as_ref() {
            // Escape means "no" to a question, and "keep" is the no that does
            // not lose data. Cancelling the whole transfer is on the key that
            // already aborts a job (Ctrl+C unless rebound).
            Key::Named(Named::Escape) => Some(Message::TransferConflict(ConflictChoice::ThisKeep)),
            // Space ticks "for all files", as it ticks a checkbox elsewhere.
            Key::Named(Named::Space) => Some(Message::ToggleConflictAll),
            // The fixed dialog keys before the configured one, so a rebound
            // abort key cannot take Enter or an arrow. The key that aborts a
            // job then cancels the whole operation.
            _ => dialog_focus_key(key.clone(), key_modifiers).or_else(|| {
                (bound_message(&prompt.keymap, &key, key_modifiers, scope)
                    == Some(Message::AbortJob))
                .then_some(Message::TransferConflict(ConflictChoice::Cancel))
            }),
        };
    }

    // The field takes the typing and Enter; Escape is `InputState`'s there and
    // here, so it closes the palette with a job running too. The arrows are the
    // entries'. Of the bindings only quitting reaches past it.
    if prompt.command_palette {
        return match key.as_ref() {
            Key::Named(Named::Escape) => {
                Some(Message::CommandPalette(CommandPaletteMessage::Close))
            }
            Key::Named(Named::Enter) => Some(Message::CommandPalette(CommandPaletteMessage::Run)),
            Key::Named(Named::ArrowUp) if key_modifiers.is_empty() => {
                Some(Message::CommandPalette(CommandPaletteMessage::Move(-1)))
            }
            Key::Named(Named::ArrowDown) if key_modifiers.is_empty() => {
                Some(Message::CommandPalette(CommandPaletteMessage::Move(1)))
            }
            _ => bound_message(&prompt.keymap, &key, key_modifiers, scope)
                .filter(|message| *message == Message::Quit)
                .filter(|_| is_not_typed_by_text_field(&key, key_modifiers)),
        };
    }

    if prompt.reorganisation_plan {
        return reorganisation_plan_key(prompt, &key, key_modifiers, scope);
    }

    if prompt.rule_suggestions {
        return rule_suggestions_key(prompt, &key, key_modifiers, scope);
    }

    if prompt.review_list {
        return review_list_key(prompt, &key, key_modifiers, scope);
    }

    // Like the command palette: the field takes the typing, the card the
    // rest. Enter reaches the routing when the list has the keys, not the field.
    if prompt.theme_fetch {
        return match key.as_ref() {
            Key::Named(Named::Escape) => Some(Message::ThemeFetch(ThemeFetchMessage::Close)),
            Key::Named(Named::Enter) => Some(Message::ThemeFetch(ThemeFetchMessage::Activate)),
            Key::Named(Named::ArrowUp) if key_modifiers.is_empty() => {
                Some(Message::ThemeFetch(ThemeFetchMessage::Move(-1)))
            }
            Key::Named(Named::ArrowDown) if key_modifiers.is_empty() => {
                Some(Message::ThemeFetch(ThemeFetchMessage::Move(1)))
            }
            _ => bound_message(&prompt.keymap, &key, key_modifiers, scope)
                .filter(|message| *message == Message::Quit)
                .filter(|_| is_not_typed_by_text_field(&key, key_modifiers)),
        };
    }

    // The field takes the typing, Enter and Escape (`InputState`); the arrows
    // are the hits'. Of the bindings only quitting reaches past it, as it does
    // for the path field.
    if prompt.quick_open {
        return match key.as_ref() {
            Key::Named(Named::ArrowUp) if key_modifiers.is_empty() => {
                Some(Message::QuickOpen(QuickOpenMessage::Move(-1)))
            }
            Key::Named(Named::ArrowDown) if key_modifiers.is_empty() => {
                Some(Message::QuickOpen(QuickOpenMessage::Move(1)))
            }
            _ => bound_message(&prompt.keymap, &key, key_modifiers, scope)
                .filter(|message| *message == Message::Quit)
                .filter(|_| is_not_typed_by_text_field(&key, key_modifiers)),
        };
    }

    // The page owns the keyboard; of the bindings only its own key (which
    // closes it) and quitting reach past it.
    if prompt.filing_status {
        return filing_status_key(prompt, &key, key_modifiers, scope);
    }

    // The text field takes the typing and Enter on its own, and `InputState`
    // sees Escape and Tab leave it. Of the bindings only quitting reaches past
    // it: the field has no use for F10 or Cmd+Q, and a program that cannot be
    // closed while a path is half typed is a trap. Only a key the field does
    // not type: a quit rebound to `*` or Backspace must not end the program in
    // the middle of a path.
    if prompt.path_editing {
        return bound_message(&prompt.keymap, &key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(&key, key_modifiers));
    }

    // Escape is `InputState`'s: the text fields capture it, so it only ever
    // reaches the routing when no field has the focus.
    if prompt.search {
        return search_key(prompt, &key, key_modifiers);
    }

    if matches!(key.as_ref(), Key::Named(Named::Escape)) {
        return escape_message(prompt);
    }
    bound_message(&prompt.keymap, &key, key_modifiers, scope)
        .or_else(|| type_ahead_message(&key, key_modifiers))
}

// A visible card whose footer promises "Esc: close" goes before the job
// (same thought as the delete dialog): the filing setup, then a filing
// card, then a running job, then the config error. Unbound rather than
// bound to something that does nothing — a key that means different things
// depending on invisible state is worse than one that is simply free.
fn escape_message(prompt: &PromptKeyState) -> Option<Message> {
    if prompt.filing_setup {
        Some(Message::FilingSetup(FilingSetupMessage::Close))
    } else if prompt.filing_card {
        Some(Message::Filing(FilingMessage::Dismiss))
    } else if prompt.job_running {
        Some(Message::AbortJob)
    } else if prompt.config_error {
        Some(Message::DismissConfigError)
    } else {
        None
    }
}

/// Keys while the search view is up. A text field captures what it types, so
/// what arrives here is either not text (arrows, function keys, chords) or the
/// field has no focus, and then the result list has the keys.
fn search_key(prompt: &PromptKeyState, key: &Key, key_modifiers: Modifiers) -> Option<Message> {
    let search = |message| Some(Message::Search(message));
    let bound = bound_message(&prompt.keymap, key, key_modifiers, KeyScope::Search);
    // Tagging and the toggles skip the filter below, which would drop Space,
    // `*` and the Alt chords as typing: a key only gets here when no text field
    // took it. Before the arrow match too, which ignores Shift and would turn
    // Shift+arrows into plain moves.
    if let Some(message) = bound.as_ref().and_then(tag_or_toggle) {
        return search(message);
    }
    if (key_modifiers - Modifiers::SHIFT).is_empty() {
        match key.as_ref() {
            Key::Named(Named::ArrowUp) => return search(SearchMessage::Move(-1)),
            Key::Named(Named::ArrowDown) => return search(SearchMessage::Move(1)),
            Key::Named(Named::PageUp) => return search(SearchMessage::Page(-1)),
            Key::Named(Named::PageDown) => return search(SearchMessage::Page(1)),
            Key::Named(Named::Home) => return search(SearchMessage::First),
            Key::Named(Named::End) => return search(SearchMessage::Last),
            Key::Named(Named::ArrowLeft) => return search(SearchMessage::Expand(false)),
            Key::Named(Named::ArrowRight) => return search(SearchMessage::Expand(true)),
            Key::Named(Named::Enter) => return search(SearchMessage::Activate),
            _ => {}
        }
    }
    bound
        .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
        .or_else(|| match type_ahead_message(key, key_modifiers) {
            Some(Message::TypeAhead(typed)) => search(SearchMessage::AppendToQuery(typed)),
            _ => None,
        })
}

fn is_search_toggle(message: &SearchMessage) -> bool {
    matches!(
        message,
        SearchMessage::ToggleCase | SearchMessage::ToggleWholeWord | SearchMessage::ToggleRegex
    )
}

/// The search's version of a bound tagging or toggle message.
fn tag_or_toggle(message: &Message) -> Option<SearchMessage> {
    match message {
        Message::ToggleTag => Some(SearchMessage::ToggleTag),
        Message::TagMove(delta) => Some(SearchMessage::TagMove(*delta)),
        Message::TagAll => Some(SearchMessage::TagAll),
        Message::ClearTags => Some(SearchMessage::ClearTags),
        Message::Search(toggle) if is_search_toggle(toggle) => Some(toggle.clone()),
        _ => None,
    }
}

/// The search toggle a press is bound to, resolved as the key routing does it.
/// The search fields use it to claim the press before they type its character.
pub fn search_toggle_for_press(
    keymap: &Keymap,
    key: Key,
    physical_key: keyboard::key::Physical,
    modifiers: Modifiers,
) -> Option<SearchMessage> {
    let bound_key = binding_key(key, physical_key, modifiers);
    match bound_message(keymap, &bound_key, modifiers, KeyScope::Search)? {
        Message::Search(toggle) if is_search_toggle(&toggle) => Some(toggle),
        _ => None,
    }
}

/// The binding a press hits, with the Shift rules of the panels: see the
/// comments inside.
fn bound_message(
    keymap: &Keymap,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    // The exact chord first (Shift+F8 is not F8). Only Shift may fall back to
    // the bare key: it changes the character typed (`*` is Shift+8), while Alt,
    // Ctrl and Super make a different chord that must not trigger the plain one.
    //
    // For a character, Shift is already in the character (`*` needs it on the
    // US and German layouts), so Ctrl+`*` arrives as CTRL|SHIFT and has to be
    // looked up as Ctrl. Named keys keep it: Shift+F8 is not F8.
    //
    // Except for a chord that names Shift itself (Ctrl+Shift+C): tried first,
    // with the letter lowercased, because Shift made it a capital.
    if key_modifiers.shift() {
        if let Some(message) = keymap.map_key(&lowercase_character(key), key_modifiers, scope) {
            return Some(message);
        }
    }
    let modifiers = if matches!(key, Key::Character(_)) {
        key_modifiers - Modifiers::SHIFT
    } else {
        key_modifiers
    };
    keymap.map_key(key, modifiers, scope).or_else(|| {
        (modifiers - Modifiers::SHIFT)
            .is_empty()
            .then(|| keymap.map_key(key, Modifiers::default(), scope))
            .flatten()
    })
}

/// Fills the placeholders of a translated template in one pass, so a value
/// that itself contains `{key}` or `{reason}` (a file named `{key}.txt`) is
/// shown as it is and not filled again.
pub fn fill_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while !rest.is_empty() {
        let placeholder = values
            .iter()
            .find_map(|(name, value)| rest.strip_prefix(name).map(|after| (value, after)));
        if let Some((value, after)) = placeholder {
            filled.push_str(value);
            rest = after;
        } else {
            let mut chars = rest.chars();
            filled.extend(chars.next());
            rest = chars.as_str();
        }
    }
    filled
}

/// A function key, or a chord with Super or Ctrl (without Alt): nothing a
/// text field would type.
fn is_not_typed_by_text_field(key: &Key, modifiers: Modifiers) -> bool {
    let is_function_key = matches!(
        key,
        Key::Named(
            Named::F1
                | Named::F2
                | Named::F3
                | Named::F4
                | Named::F5
                | Named::F6
                | Named::F7
                | Named::F8
                | Named::F9
                | Named::F10
                | Named::F11
                | Named::F12
        )
    );
    // Alt types characters on a Mac, and AltGr arrives as Ctrl+Alt on Windows.
    let is_chord = modifiers.logo() || (modifiers.control() && !modifiers.alt());
    is_function_key || is_chord
}

/// A printable character with at most Shift held. With Option held a Mac types
/// another character, so Alt, Ctrl and Cmd all rule it out.
fn type_ahead_message(key: &Key, modifiers: Modifiers) -> Option<Message> {
    let Key::Character(typed) = key else {
        return None;
    };
    ((modifiers - Modifiers::SHIFT).is_empty() && typed.chars().all(|c| !c.is_control()))
        .then(|| Message::TypeAhead(typed.to_string()))
}

fn lowercase_character(key: &Key) -> Key {
    match key {
        Key::Character(c) => Key::Character(c.to_lowercase().into()),
        other => other.clone(),
    }
}

/// The key a binding is written against.
///
/// With Option held a Mac types another character: Option+C is "ç", and with
/// Ctrl it can be a control character (Ctrl+X types U+0018). The binding is
/// for the letter or digit on the key, so such a press is taken from the
/// control character or, for a non-ASCII character, the physical key instead.
///
/// Only with Ctrl, Alt or Super held, and never for printable ASCII: that is
/// what the user typed (`*` on Ctrl+Shift+8, punctuation on the AZERTY digit
/// row), and the binding is written against it.
fn binding_key(key: Key, physical_key: keyboard::key::Physical, modifiers: Modifiers) -> Key {
    use keyboard::key::{Code, Physical};
    let Key::Character(typed) = &key else {
        return key;
    };
    if !modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO) {
        return key;
    }
    let mut typed_chars = typed.chars();
    if let (Some(control @ '\u{1}'..='\u{1a}'), None) = (typed_chars.next(), typed_chars.next()) {
        if let Some(letter) = char::from_u32('a' as u32 + control as u32 - 1) {
            return Key::Character(letter.to_string().into());
        }
    }
    if typed.is_ascii() {
        return key;
    }
    let Physical::Code(code) = physical_key else {
        return key;
    };
    let character = match code {
        Code::KeyA => "a",
        Code::KeyB => "b",
        Code::KeyC => "c",
        Code::KeyD => "d",
        Code::KeyE => "e",
        Code::KeyF => "f",
        Code::KeyG => "g",
        Code::KeyH => "h",
        Code::KeyI => "i",
        Code::KeyJ => "j",
        Code::KeyK => "k",
        Code::KeyL => "l",
        Code::KeyM => "m",
        Code::KeyN => "n",
        Code::KeyO => "o",
        Code::KeyP => "p",
        Code::KeyQ => "q",
        Code::KeyR => "r",
        Code::KeyS => "s",
        Code::KeyT => "t",
        Code::KeyU => "u",
        Code::KeyV => "v",
        Code::KeyW => "w",
        Code::KeyX => "x",
        Code::KeyY => "y",
        Code::KeyZ => "z",
        Code::Digit0 => "0",
        Code::Digit1 => "1",
        Code::Digit2 => "2",
        Code::Digit3 => "3",
        Code::Digit4 => "4",
        Code::Digit5 => "5",
        Code::Digit6 => "6",
        Code::Digit7 => "7",
        Code::Digit8 => "8",
        Code::Digit9 => "9",
        _ => return key,
    };
    Key::Character(character.into())
}

/// A key press as the subscription sees it: the routed message, unless the
/// press is a key repeat of something that must happen once.
pub fn route_press(
    prompt: &PromptKeyState,
    key: Key,
    physical_key: keyboard::key::Physical,
    modifiers: Modifiers,
    repeat: bool,
) -> Option<Message> {
    // A text prompt types what the key produced, so only the bindings see the
    // physical letter.
    let routed_key = if prompt.open {
        key
    } else {
        binding_key(key, physical_key, modifiers)
    };
    route_key(prompt, routed_key, modifiers).filter(|message| !repeat || repeats(message))
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

fn filing_status_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    use FilingStatusMessage as Page;
    let page = |message| Some(Message::FilingStatus(message));
    // Shift only changes the character typed (`+` is Shift+= on many layouts).
    let plain = (key_modifiers - Modifiers::SHIFT).is_empty();
    match key.as_ref() {
        Key::Named(Named::Escape) => page(Page::Close),
        Key::Named(Named::ArrowUp) if key_modifiers.is_empty() => page(Page::MoveError(-1)),
        Key::Named(Named::ArrowDown) if key_modifiers.is_empty() => page(Page::MoveError(1)),
        Key::Named(Named::Enter) if key_modifiers.is_empty() => page(Page::ActivateSelectedError),
        Key::Character("p" | "P") if plain => page(Page::TogglePause),
        Key::Character("1") if plain => page(Page::SetLoad(Load::Gentle)),
        Key::Character("2") if plain => page(Page::SetLoad(Load::Balanced)),
        Key::Character("3") if plain => page(Page::SetLoad(Load::Full)),
        Key::Character("+") if plain => page(Page::StepWorkers(1)),
        Key::Character("-") if plain => page(Page::StepWorkers(-1)),
        _ => match bound_message(&prompt.keymap, key, key_modifiers, scope) {
            Some(Message::FilingStatus(Page::Toggle)) => page(Page::Close),
            Some(Message::Quit) => Some(Message::Quit),
            _ => None,
        },
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
            | Message::ContextMenuMove(_)
            | Message::ContextMenuFirst
            | Message::ContextMenuLast
            | Message::QuickOpen(QuickOpenMessage::Move(_))
            | Message::CommandPalette(CommandPaletteMessage::Move(_))
            | Message::ThemeFetch(ThemeFetchMessage::Move(_))
            | Message::RuleSuggestions(RuleSuggestionsMessage::Move(_))
            | Message::ReviewList(ReviewListMessage::Move(_))
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Move(_))
            | Message::FilingStatus(FilingStatusMessage::MoveError(_))
            | Message::Search(
                SearchMessage::ToggleTag
                    | SearchMessage::TagMove(_)
                    | SearchMessage::Move(_)
                    | SearchMessage::Page(_)
                    | SearchMessage::First
                    | SearchMessage::Last
                    | SearchMessage::AppendToQuery(_),
            )
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

/// What goes on the clipboard, and for how many rows.
#[derive(Debug, PartialEq, Eq)]
struct ClipboardText {
    text: String,
    count: usize,
}

/// The tagged rows, else the cursor row, one per line in listing order. On `..`
/// without tags the directory of the panel itself; the root has no file name,
/// so its path is its name. Paths are joined, not canonicalised.
fn clipboard_text(panel: &PanelState, kind: ClipboardKind) -> Option<ClipboardText> {
    if kind == ClipboardKind::Directory {
        return Some(ClipboardText {
            count: 1,
            text: panel.path.to_string_lossy().into_owned(),
        });
    }
    let render = |path: &std::path::Path, name: &std::ffi::OsStr| match kind {
        ClipboardKind::Name => name.to_string_lossy().into_owned(),
        ClipboardKind::Path | ClipboardKind::Directory => path.to_string_lossy().into_owned(),
    };
    let lines: Vec<String> = if panel.selection.any_tagged() {
        panel
            .selection
            .ordered(&panel.entries)
            .into_iter()
            .map(|entry| render(&panel.path.join(&entry.name), &entry.name))
            .collect()
    } else {
        let entry = panel.selected_entry()?;
        if entry.is_parent {
            let own_name = panel.path.file_name().unwrap_or(panel.path.as_os_str());
            vec![render(&panel.path, own_name)]
        } else {
            vec![render(&panel.path.join(&entry.name), &entry.name)]
        }
    };
    (!lines.is_empty()).then(|| ClipboardText {
        count: lines.len(),
        text: lines.join("\n"),
    })
}

/// Where the app finds its config and index and what holds the index key.
pub struct Environment {
    pub config_path: Option<PathBuf>,
    pub index_path: Option<PathBuf>,
    pub key_store: Arc<dyn KeyStore + Send + Sync>,
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("config_path", &self.config_path)
            .field("index_path", &self.index_path)
            .finish_non_exhaustive()
    }
}

impl Environment {
    /// The config, index and keychain of the person running ncrs.
    fn of_the_user() -> Self {
        Self {
            config_path: config::default_path(),
            index_path: config::default_index_path(),
            key_store: Arc::new(user_key_store()),
        }
    }

    /// No config file, no index, a key store in memory: nothing outside the
    /// test.
    #[cfg(test)]
    pub fn isolated() -> Self {
        Self {
            config_path: None,
            index_path: None,
            key_store: Arc::new(ncrs_index::InMemoryKeyStore::empty()),
        }
    }
}

/// The one place the app builds the system keychain store.
fn user_key_store() -> KeyringKeyStore {
    config::forbid_user_machine_in_tests("the system keychain");
    config::portable_data_dir()
        .map_or_else(KeyringKeyStore::new, |dir| KeyringKeyStore::portable(&dir))
}

impl App {
    /// Initial state plus tasks that load both panels.
    /// Left panel starts in the working directory, right in `$HOME`, so the
    /// two panels are not the same directory on launch.
    pub fn new() -> (Self, Task<Message>) {
        let (app, loading) = Self::with_environment(Environment::of_the_user());
        (app, then_install_native_menu(loading))
    }

    /// `new` with the places it reads and writes given instead of looked up.
    pub fn with_environment(environment: Environment) -> (Self, Task<Message>) {
        let Environment {
            config_path,
            index_path,
            key_store,
        } = environment;
        let loaded = config_path
            .as_deref()
            .map_or_else(|| Ok(Config::default()), config::load);
        if let Err(err) = &loaded {
            eprintln!("ncrs: {err}");
        }
        let filing = loaded
            .as_ref()
            .map(|config| config.filing.clone())
            .unwrap_or_default();
        let (mut panels, panel_loads) = Self::starting_in(fs::start_dir(), fs::home_dir());
        panels.config_path = config_path;
        let (app, filing_start) = panels
            .with_config(loaded)
            .with_filing(&filing, index_path, key_store);
        (app, Task::batch([panel_loads, filing_start]))
    }

    /// Starts the filing module when `filing` switches it on. The database
    /// and its directory are not created here: only after the user chose at
    /// the encryption question. Switched on without locations, the module
    /// has nothing to work on, so the setup opens.
    pub fn with_filing(
        mut self,
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: Arc<dyn KeyStore + Send + Sync>,
    ) -> (Self, Task<Message>) {
        self.filing_index_path = db_path;
        self.filing_key_store = Some(store);
        let task = self.start_filing_module(filing);
        if filing.enabled && filing.locations.is_empty() {
            self.filing_setup = Some(FilingSetup::new(&[]));
        }
        (self, task)
    }

    /// Replaces nothing: the caller has dropped any module and pipeline before.
    fn start_filing_module(&mut self, filing: &Filing) -> Task<Message> {
        self.filing_config = filing.clone();
        let Some(store) = self.filing_key_store.clone() else {
            return Task::none();
        };
        let Some((module, work)) =
            FilingModule::start(filing, self.filing_index_path.clone(), store)
        else {
            return Task::none();
        };
        self.filing = Some(module);
        work.map_or_else(Task::none, run_filing_work)
    }

    /// Applies a loaded config. A broken one leaves the defaults and says why
    /// in the status line.
    pub fn with_config(mut self, loaded: Result<Config, ConfigError>) -> Self {
        match loaded {
            Ok(config) => {
                // First: `function_keys` and `report` word themselves in `self.lang`.
                if let Some(lang) = config.lang {
                    self.lang = lang;
                }
                self.open_programs = config.open;
                self.keymap = config.keymap;
                self.refresh_function_keys();
                self.palette = config.palette;
                self.report_look_warnings(config.contrast_warning, config.vscode_warning);
            }
            Err(err) => self.report(ConfigProblem::Invalid(err)),
        }
        self
    }

    /// Says what is off about the look a config asks for. The VS Code warning
    /// goes last, so it wins: it explains why the look is not the one asked for.
    fn report_look_warnings(
        &mut self,
        contrast: Option<ContrastWarning>,
        vscode: Option<VscodeWarning>,
    ) {
        if let Some(warning) = contrast {
            self.report(ConfigProblem::LowContrast(warning));
        }
        if let Some(warning) = vscode {
            self.report(ConfigProblem::VscodeTheme(warning));
        }
    }

    /// F9. A config error still up is worded again in the new language.
    fn switch_language(&mut self) {
        self.lang = self.lang.other();
        self.refresh_function_keys();
        if let Some(problem) = &self.config_problem {
            self.config_error = Some(self.config_error_text(problem));
        }
    }

    fn refresh_function_keys(&mut self) {
        self.function_keys = self.keymap.function_keys(self.lang, KeyScope::Panel);
        self.search_function_keys = self.keymap.function_keys(self.lang, KeyScope::Search);
    }

    fn report(&mut self, problem: ConfigProblem) {
        self.config_error = Some(self.config_error_text(&problem));
        self.config_problem = Some(problem);
    }

    fn config_error_text(&self, problem: &ConfigProblem) -> String {
        match problem {
            ConfigProblem::Invalid(err) => {
                let path = err.path().display().to_string();
                match err.line() {
                    Some(line) => fill_template(
                        self.lang.text(Msg::ErrorConfigAtLine),
                        &[
                            ("{path}", &path),
                            ("{line}", &line.to_string()),
                            ("{reason}", err.reason()),
                        ],
                    ),
                    None => fill_template(
                        self.lang.text(Msg::ErrorConfig),
                        &[("{path}", &path), ("{reason}", err.reason())],
                    ),
                }
            }
            ConfigProblem::VscodeTheme(warning) => {
                let path = warning.path.display().to_string();
                let theme = warning.theme.display().to_string();
                let values = [
                    ("{path}", path.as_str()),
                    ("{theme}", theme.as_str()),
                    ("{reason}", warning.reason.as_str()),
                ];
                match warning.line {
                    Some(number) => {
                        let line = number.to_string();
                        let mut with_line = values.to_vec();
                        with_line.push(("{line}", line.as_str()));
                        fill_template(self.lang.text(Msg::ErrorVscodeThemeAtLine), &with_line)
                    }
                    None => fill_template(self.lang.text(Msg::ErrorVscodeTheme), &values),
                }
            }
            ConfigProblem::LowContrast(warning) => {
                let path = warning.path.display().to_string();
                let ratio = format!("{:.1}", warning.ratio);
                let colors = [
                    ("{path}", path.as_str()),
                    ("{foreground}", warning.foreground.name()),
                    ("{background}", warning.background.name()),
                    ("{ratio}", ratio.as_str()),
                ];
                match warning.line {
                    Some(number) => {
                        let line = number.to_string();
                        let mut values = colors.to_vec();
                        values.push(("{line}", line.as_str()));
                        fill_template(self.lang.text(Msg::ErrorContrastAtLine), &values)
                    }
                    None => fill_template(self.lang.text(Msg::ErrorContrast), &colors),
                }
            }
        }
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
            function_keys: Keymap::built_in().function_keys(Language::default(), KeyScope::Panel),
            search_function_keys: Keymap::built_in()
                .function_keys(Language::default(), KeyScope::Search),
            keymap: Keymap::built_in(),
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
            open_programs: config::OpenPrograms::default(),
            config_error: None,
            config_problem: None,
            palette: Palette::default(),
            job_error: None,
            notice: None,
            notice_generation: 0,
            job_done: 0,
            conflict_all: false,
            conflict_rule: None,
            generation: 0,
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            volume_menu: None,
            context_menu: None,
            type_ahead: TypeAhead::default(),
            path_field: None,
            path_check_id: 0,
            home: home.clone(),
            clock: Arc::new(std::time::Instant::now),
            window_size: layout::INITIAL_WINDOW_SIZE,
            input: Arc::default(),
            list_volumes: Arc::new(fs::list_volumes),
            search: None,
            search_open: false,
            search_side: PanelSide::Left,
            search_rows: layout::search_visible_rows(layout::INITIAL_WINDOW_SIZE),
            search_generation: 0,
            search_handle: None,
            quick_open: None,
            quick_open_walk_generation: 0,
            quick_open_walk: None,
            command_palette: None,
            recent_commands: Vec::new(),
            filing: None,
            pipeline: None,
            filing_status: FilingStatus::default(),
            wall_clock: Arc::new(ncrs_pipeline::SystemClock),
            config_path: None,
            filing_config: Filing::default(),
            filing_index_path: None,
            filing_key_store: None,
            filing_setup: None,
            resolve_folder: filing_setup::resolve_folder,
            pick_folder: filing_setup::pick_folder_with_system_dialog,
            theme_fetch: None,
            theme_fetch_generation: 0,
            rule_suggestions: None,
            rule_suggestions_generation: 0,
            review_list: None,
            review_list_generation: 0,
            reorganisation: None,
            reorganisation_generation: 0,
            plans_filed_for: 0,
            filing_due: false,
            fetch_extension: open_vsx::fetch_extension,
            install_theme: install_picked_theme,
            ocr_models: None,
            ocr_models_generation: 0,
            download_ocr_models: download_models,
            ai_model: None,
            ai_model_generation: 0,
            ai_model_asked: false,
            download_ai_model,
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
                            physical_key,
                            modifiers,
                            repeat,
                            ..
                        } => route_press(
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
                            physical_key,
                            modifiers,
                            repeat,
                        ),
                        keyboard::Event::ModifiersChanged(modifiers) => {
                            Some(Message::ModifiersChanged(modifiers))
                        }
                        _ => None,
                    }
                }),
            // Keeps the status bar's progress and the open page current.
            if self.pipeline.is_some() || self.filing_status.page_open() {
                iced::time::every(SNAPSHOT_EVERY)
                    .map(|_| Message::FilingStatus(FilingStatusMessage::Tick))
            } else {
                Subscription::none()
            },
            // Mouse, focus and modifier events of every status: widgets capture
            // the clicks and the pointer moves the menu has to know about.
            event::listen_with(|event, status, _window| Some((event, status)))
                .with(Arc::clone(&self.input))
                .filter_map(|(input, (event, status))| input.lock().handle(&event, status)),
            window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
            native_menu_events(),
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
        if resets_type_ahead(&message) {
            self.type_ahead.reset();
        }
        if let Message::NoticeExpired(generation) = message {
            if generation == self.notice_generation {
                self.notice = None;
            }
            return Task::none();
        }
        if leaves_path_field(&message) {
            self.set_path_field(None);
        }
        let had_search_keys = self.search_has_keys();
        let task = self.apply(message);
        // The text field keeps typing into the query until it is told to let
        // go, so leaving the search's panel releases it.
        if had_search_keys && !self.search_has_keys() {
            return Task::batch([task, release_focus()]);
        }
        task
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
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

            Message::DismissConfigError => {
                self.config_error = None;
                self.config_problem = None;
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
                        self.dialog_focus = conflict::OVERWRITE_FOCUS;
                        self.pending_conflict = Some(PendingConflict { transfer, index });
                        // Else the dialog would be asked behind the box.
                        Task::batch([self.close_quick_open(), self.close_command_palette()])
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

            #[cfg(target_os = "macos")]
            Message::OpenPrivacySettings => {
                self.open_privacy_settings();
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

            // --- context menu ---
            menu_message @ (Message::ContextMenuAt { .. }
            | Message::ContextMenuKey
            | Message::ContextMenuMove(_)
            | Message::ContextMenuFirst
            | Message::ContextMenuLast
            | Message::ContextMenuHover(_)
            | Message::ContextMenuActivate
            | Message::ContextMenuClick(_)
            | Message::ContextMenuClose) => self.update_context_menu(menu_message),

            // --- path field ---
            path_message @ (Message::PathFieldInput(_)
            | Message::PathFieldSubmit
            | Message::PathFieldCancel
            | Message::PathChecked { .. }
            | Message::ClickedOutside) => self.update_path_field(path_message),

            // --- delete ---
            Message::Delete { permanent } => self.open_delete_dialog(permanent),
            Message::CopyToClipboard(kind) => self.copy_to_clipboard(kind),
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
                sorted_by,
                select,
            } => {
                let panel = self.panel_mut(side);
                if panel.request_id == request_id {
                    panel.apply_sorted_listing(
                        path,
                        entries,
                        error,
                        select.as_deref(),
                        rows,
                        Some(sorted_by),
                    );
                    self.close_menu_of_reloaded(side);
                }
                Task::none()
            }

            Message::MoveSelection(delta) => {
                self.job_error = None;
                self.notice = None;
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
            Message::TypeAhead(typed) => {
                self.jump_to_typed(&typed);
                Task::none()
            }

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
            Message::ActivatePanel(side) => {
                self.active_panel = side;
                Task::none()
            }
            Message::TitleBar(title_bar) => update_title_bar(title_bar),
            Message::SortBy { .. } | Message::SortActive(_) => self.update_sort(message),
            Message::SwitchLanguage => {
                self.switch_language();
                Task::none()
            }
            // A click in the tag column toggles the tag; anywhere else it moves
            // the cursor. Without this, tagging is keyboard-only, and the
            // column of stars the view draws cannot be clicked at all.
            // A Ctrl+click is a right click: it replaces an open menu.
            Message::RowClicked { side, index, .. } if is_context_click(self.modifiers) => {
                let pointer = self.input.lock().pointer;
                self.open_context_menu(side, index, Some(pointer));
                Task::none()
            }
            Message::RowClicked { .. } if self.context_menu.is_some() => {
                // A click beside the menu only closes it.
                self.context_menu = None;
                Task::none()
            }
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
                self.window_size = size;
                self.resize_command_palette(size);
                self.visible_rows = layout::visible_rows(size);
                self.resize_search(size);
                self.left_panel.ensure_visible(self.visible_rows);
                self.right_panel.ensure_visible(self.visible_rows);
                Task::none()
            }
            Message::OpenSearch => self.open_search(),
            Message::Search(search) => self.update_search(search),
            Message::QuickOpen(quick_open) => self.update_quick_open(quick_open),
            Message::CommandPalette(command_palette) => {
                self.update_command_palette(command_palette)
            }
            Message::ThemeFetch(fetch_message) => self.update_theme_fetch(fetch_message),
            Message::RuleSuggestions(suggestions) => self.update_rule_suggestions(suggestions),
            Message::ReviewList(review) => self.update_review_list(review),
            Message::HermesPage(page) => self.go_to_hermes_page(page),
            Message::ReorganisationPlan(plan) => self.update_reorganisation(plan),
            // #73 hangs the executor here; until then the selection goes nowhere.
            Message::ExecuteReorganisation(selection) => self.run_reorganisation(selection),
            Message::ReorganisationExecuted(result) => self.reorganisation_executed(result),
            Message::UndoReorganisation => self.undo_reorganisation(),
            Message::DocumentsFiled(result) => self.documents_filed(result),
            Message::ReorganisationUndone(result) => self.reorganisation_undone(result),
            Message::OcrModels(models_message) => self.update_ocr_models(models_message),
            Message::AiModel(model_message) => self.update_ai_model(model_message),
            Message::Filing(filing_message) => self.update_filing(filing_message),
            Message::FilingStatus(page_message) => self.update_filing_status(page_message),
            Message::FilingSetup(setup_message) => self.update_filing_setup(setup_message),
            #[cfg(any(target_os = "macos", test))]
            Message::RunCommand(action) => self.run_command(action),
            #[cfg(target_os = "macos")]
            Message::InstallNativeMenu => {
                native_menu::install(self.lang, &self.keymap, self.menu_availability());
                Task::none()
            }
            Message::Quit => iced::exit(),
            // Handled in `update`, which owns the timer's generation check.
            Message::NoticeExpired(_) => Task::none(),
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
        #[cfg(target_os = "macos")]
        native_menu::sync(self.lang, &self.keymap, self.menu_availability());
        task
    }

    /// What the menu bar lets the user run: nothing while a dialog or box is
    /// open, otherwise what works in the area the keys go to.
    #[cfg(any(target_os = "macos", test))]
    fn menu_availability(&self) -> Availability {
        if self.overlay_is_open() {
            Availability::Blocked
        } else if self.search_has_keys() {
            Availability::Open(KeyScope::Search)
        } else {
            Availability::Open(KeyScope::Panel)
        }
    }

    fn update_sort(&mut self, message: Message) -> Task<Message> {
        let rows = self.visible_rows;
        match message {
            // A click beside an open menu only closes it.
            Message::SortBy { .. } if self.context_menu.is_some() => self.context_menu = None,
            Message::SortBy { side, column } => {
                self.active_panel = side;
                self.panel_mut(side).request_sort(column, rows);
            }
            Message::SortActive(column) => self.active_panel_mut().request_sort(column, rows),
            _ => {}
        }
        Task::none()
    }

    fn update_context_menu(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ContextMenuAt { side, index } => {
                let pointer = self.input.lock().pointer;
                self.open_context_menu(side, index, Some(pointer));
                Task::none()
            }
            Message::ContextMenuKey => {
                let side = self.active_panel;
                let index = self.panel(side).selected;
                self.open_context_menu(side, index, None);
                Task::none()
            }
            Message::ContextMenuMove(delta) => {
                if let Some(menu) = self.context_menu.as_mut() {
                    menu.move_by(delta);
                }
                Task::none()
            }
            Message::ContextMenuFirst => {
                if let Some(menu) = self.context_menu.as_mut() {
                    menu.select_first();
                }
                Task::none()
            }
            Message::ContextMenuLast => {
                if let Some(menu) = self.context_menu.as_mut() {
                    menu.select_last();
                }
                Task::none()
            }
            Message::ContextMenuHover(index) => {
                if let Some(menu) = self.context_menu.as_mut() {
                    menu.select(index);
                }
                Task::none()
            }
            Message::ContextMenuActivate => self.run_context_menu_entry(),
            Message::ContextMenuClick(index) => {
                if let Some(menu) = self.context_menu.as_mut() {
                    menu.select(index);
                }
                self.run_context_menu_entry()
            }
            Message::ContextMenuClose => {
                self.context_menu = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn update_path_field(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PathFieldInput(typed) => {
                self.set_path_field(Some(typed));
                Task::none()
            }
            Message::PathFieldSubmit => self.go_to_typed_path(),
            Message::PathChecked {
                request_id,
                side,
                target,
                result,
            } => self.finish_path_check(request_id, side, &target, result),
            Message::PathFieldCancel => {
                self.set_path_field(None);
                release_focus()
            }
            Message::ClickedOutside => {
                self.context_menu = None;
                self.set_path_field(None);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The rows the menu was opened on are gone. It acts on the active panel,
    /// so only that panel's reload closes it.
    fn close_menu_of_reloaded(&mut self, side: PanelSide) {
        if side == self.active_panel {
            self.context_menu = None;
        }
    }

    /// Sets what the path field shows, or ends the editing with `None`. Any
    /// change makes an answer still on its way stale.
    fn set_path_field(&mut self, value: Option<String>) {
        self.path_check_id = self.path_check_id.wrapping_add(1);
        self.path_field = value;
    }

    /// Enter in the path field: looks at the typed path on the blocking pool and
    /// opens it in the active panel once the answer is in.
    fn go_to_typed_path(&mut self) -> Task<Message> {
        let Some(raw_path) = self.path_field.clone() else {
            return Task::none();
        };
        let typed = raw_path.trim();
        if typed.is_empty() {
            self.set_path_field(None);
            return release_focus();
        }
        let side = self.active_panel;
        let target = fs::resolve_typed_path(&self.active_panel().path, &self.home, typed);
        self.path_check_id = self.path_check_id.wrapping_add(1);
        let request_id = self.path_check_id;
        Task::perform(fs::locate(target.clone()), move |result| {
            Message::PathChecked {
                request_id,
                side,
                target: target.clone(),
                result,
            }
        })
    }

    /// The answer to [`Self::go_to_typed_path`]: a destination closes the field
    /// and loads it, a problem is reported and leaves the field open. An answer
    /// to text that has since changed, or to a field that is closed, is dropped.
    fn finish_path_check(
        &mut self,
        request_id: u64,
        side: PanelSide,
        target: &Path,
        result: Result<fs::Destination, fs::PathProblem>,
    ) -> Task<Message> {
        if request_id != self.path_check_id || self.path_field.is_none() {
            return Task::none();
        }
        match result {
            Ok(destination) => {
                self.set_path_field(None);
                self.job_error = None;
                self.notice = None;
                Task::batch([
                    self.load(side, destination.directory, destination.select),
                    release_focus(),
                ])
            }
            Err(problem) => {
                let shown = target.display().to_string();
                let reason = match problem {
                    fs::PathProblem::Missing => self
                        .lang
                        .text(Msg::StatusPathMissing)
                        .replace("{path}", &shown),
                    fs::PathProblem::Unreadable { reason } => self
                        .lang
                        .text(Msg::StatusPathUnreadable)
                        .replace("{path}", &shown)
                        .replace("{reason}", &reason),
                };
                self.set_status_error(&reason);
                Task::none()
            }
        }
    }

    /// Moves the cursor to the first entry named like what has been typed.
    /// Nothing happens under an overlay or when no entry matches.
    fn jump_to_typed(&mut self, typed: &str) {
        if self.overlay_is_open() {
            return;
        }
        let rows = self.visible_rows;
        let now = (self.clock)();
        let prefix = self.type_ahead.extend(typed, now).to_lowercase();
        let panel = self.active_panel_mut();
        let found = panel.entries.iter().position(|entry| {
            !entry.is_parent
                && entry
                    .name
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with(&prefix)
        });
        if let Some(index) = found {
            panel.select(index, rows);
        }
    }

    /// Writes the current routing state into the cell the keyboard reads.
    ///
    /// A no-op before the first `subscription()` call, which is the only time
    /// there is no cell — the subscription creates it.
    fn publish_key_state(&mut self) {
        let state = self.prompt_key_state();
        // An overlay answers Escape itself.
        let search_open = self.search_has_keys() && !self.overlay_is_open();
        let job_running = state.job_running;
        let mut input = self.input.lock();
        input.path_editing = state.path_editing;
        input.search_open = search_open;
        input.job_running = job_running;
        input.quick_open = self.quick_open.is_some();
        input.command_palette = self.command_palette.is_some();
        input.theme_fetch = self.theme_fetch.is_some();
        input.rule_suggestions = self.rule_suggestions.is_some();
        input.review_list = self.review_list.is_some();
        input.reorganisation_plan = self.reorganisation.is_some();
        drop(input);
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
                    palette: self.palette,
                },
                self.lang,
                move |index, on_tag| Message::RowClicked {
                    side,
                    index,
                    on_tag,
                },
                move |index| Message::ContextMenuAt { side, index },
                move |column| Message::SortBy { side, column },
                move |delta| Message::PanelScrolled { side, delta },
            )
        };

        let side_view = |side: PanelSide| match self.search.as_ref() {
            Some(search) if self.search_open && side == self.search_side => search_ui::view(
                search,
                self.search_rows,
                self.active_panel == side,
                self.lang,
                &self.palette,
                &self.keymap,
                &self.toggle_shortcuts(),
            ),
            _ => panel_view(side),
        };

        let panels = row![side_view(PanelSide::Left), side_view(PanelSide::Right)]
            .spacing(ui::theme::spacing::PANEL_GAP)
            .height(Length::Fill);

        let below_title_bar = container(
            column![
                panels,
                statusbar::view(
                    self.active_panel(),
                    self.lang,
                    &self.palette,
                    statusbar::Activity {
                        job: self.job_status(),
                        filing: self.filing_badge(),
                        on_blocked_press: blocked_access_press(),
                        notice: self.notice.as_deref(),
                    },
                    self.status_error(),
                    self.search_status(),
                    statusbar::PathBar {
                        editing: self.path_field.as_deref(),
                        on_input: Message::PathFieldInput,
                        on_submit: Message::PathFieldSubmit,
                        on_copy: Message::CopyToClipboard(ClipboardKind::Directory),
                    },
                ),
                fkeys::view(
                    if self.search_has_keys() {
                        &self.search_function_keys
                    } else {
                        &self.function_keys
                    },
                    &self.palette,
                ),
            ]
            .spacing(theme::spacing::SECTION_GAP),
        )
        .padding(theme::spacing::OUTER_PADDING)
        .width(Length::Fill)
        .height(Length::Fill);

        let window = container(column![self.title_bar_view(), below_title_bar])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::root(&self.palette));

        // The card has no scrim: the panels stay usable around it. Every modal
        // below stacks on this base, so it stays above the card.
        let module_card = self
            .filing
            .as_ref()
            .filter(|module| module.has_card())
            .and_then(|module| filing_ui::view(module, self.window_size, self.lang, &self.palette));
        let setup_card = self.filing_setup.as_ref().map(|setup| {
            let context = filing_setup_ui::SetupContext {
                current_folder: &self.active_panel().path,
                home: &self.home,
                index_is_plain: self
                    .filing
                    .as_ref()
                    .is_some_and(FilingModule::index_is_plain),
                lang: self.lang,
                window: self.window_size,
            };
            filing_setup_ui::view(setup, &context, &self.palette)
        });
        let cards: Vec<Element<'_, Message>> = setup_card.into_iter().chain(module_card).collect();
        let base: Element<'_, Message> = if cards.is_empty() {
            window.into()
        } else {
            let card = Column::with_children(cards)
                .spacing(theme::spacing::SECTION_GAP)
                .align_x(iced::Alignment::Center);
            Stack::with_children([window.into(), filing_card_layer(card.into())]).into()
        };
        let root = if cfg!(target_os = "macos") {
            base
        } else {
            titlebar::with_resize_edges(base)
        };

        if let Some(menu) = self.context_menu.as_ref() {
            // A left press anywhere beside the menu closes it, the path field
            // and the column titles included: those take their own clicks and
            // would never report one. A right press passes through, so another
            // row's menu replaces this one.
            // A Ctrl+click on a Mac is a right click and passes through too.
            let mut click_catcher =
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill));
            if !is_context_click(self.modifiers) {
                click_catcher = click_catcher.on_press(Message::ContextMenuClose);
            }
            return Stack::with_children([
                root,
                click_catcher.into(),
                context_menu_view::view(menu, self.lang, &self.keymap, &self.palette),
            ])
            .into();
        }

        if let Some(menu) = self.volume_menu.as_ref() {
            let menu_element = volumes_view::view(
                menu.side,
                &menu.volumes,
                menu.selected,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(menu_element)]).into();
        }

        if let Some(deletion) = self.delete_dialog.as_ref() {
            let dialog_element = delete_dialog_view::view(
                &self.delete_subject(deletion),
                deletion.permanent,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        if let Some(card) = self.ocr_models.as_ref() {
            let card_element = ocr_models_ui::view(
                card,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        if let Some(card) = self.ai_model.as_ref() {
            let card_element = ai_model_ui::view(
                card,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
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
                &self.palette,
                self.conflict_all,
                self.dialog_focus,
                self.window_size,
                Message::TransferConflict,
            );
            return Stack::with_children([root, dialog::scrim(conflict_element)]).into();
        }

        if self.filing_status.page_open() {
            let index_open = self
                .filing
                .as_ref()
                .is_some_and(|module| matches!(module.phase(), Phase::Ready));
            let page = filing_status_ui::view(
                &self.filing_status,
                self.filing.as_ref().map(FilingModule::load),
                index_open,
                self.window_size,
                self.hermes_badges(),
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(page)]).into();
        }

        if let Some(quick_open) = self.quick_open.as_ref() {
            let box_element =
                quick_open_ui::view(quick_open, self.window_size, self.lang, &self.palette);
            return Stack::with_children([root, dialog::scrim(box_element)]).into();
        }

        if let Some(command_palette) = self.command_palette.as_ref() {
            let box_element = command_palette_ui::view(
                command_palette,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(box_element)]).into();
        }

        if let Some(card) = self.theme_fetch.as_ref() {
            let card_element =
                theme_fetch_ui::view(card, self.window_size, self.lang, &self.palette);
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        if let Some(view) = self.rule_suggestions.as_ref() {
            let card_element = rule_suggestions_ui::view(
                view,
                self.window_size,
                self.hermes_badges(),
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        if let Some(view) = self.review_list.as_ref() {
            let card_element = review_list_ui::view(
                view,
                self.window_size,
                self.hermes_badges(),
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        if let Some(view) = self.reorganisation.as_ref() {
            let card_element = reorganisation_plan_ui::view(
                view,
                self.window_size,
                self.hermes_badges(),
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        // Then the create-directory prompt, for F7.
        let Some(prompt) = self.prompt.as_ref() else {
            return root;
        };
        let overlay = dialog::view(
            prompt,
            self.window_size,
            self.lang,
            &self.palette,
            Message::PromptInput,
            Message::PromptSubmit,
            Message::PromptCancel,
        );
        let Some(prompt_element) = overlay else {
            return root;
        };

        // The prompt sits above everything and takes every keystroke; the scrim
        // swallows clicks meant for the panels underneath.
        Stack::with_children([root, dialog::scrim(prompt_element)]).into()
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

    /// The error the status line shows: a job's, else the config's.
    pub fn status_error(&self) -> Option<&str> {
        self.job_error.as_deref().or(self.config_error.as_deref())
    }

    /// What the status bar shows instead of the panel's entry while the search
    /// has the keys: the cursor's file below the root, with `:line` on a line.
    /// Empty without a cursor, so the panel's entry does not pose as the
    /// search's.
    fn search_status(&self) -> Option<String> {
        if !self.search_has_keys() {
            return None;
        }
        let view = self.search.as_ref()?;
        let Some((path, line)) = view.cursor_target() else {
            return Some(String::new());
        };
        let relative = path.strip_prefix(&view.root).unwrap_or(&path);
        Some(match line {
            Some(number) => format!("{}:{number}", relative.display()),
            None => relative.display().to_string(),
        })
    }

    /// The state the key routing needs, cheap to clone.
    fn prompt_key_state(&self) -> PromptKeyState {
        let job_running = self.jobs.is_busy();
        let config_error = self.config_error.is_some();
        let volume_menu = self.volume_menu.is_some();
        let context_menu = self.context_menu.is_some();
        let path_editing = self.path_field.is_some();
        let search = self.search_has_keys();
        let quick_open = self.quick_open.is_some();
        let command_palette = self.command_palette.is_some();
        let filing_status = self.filing_status.page_open();
        let filing_setup = self.filing_setup.is_some();
        let filing_card = self
            .filing
            .as_ref()
            .is_some_and(FilingModule::card_can_be_dismissed);
        let theme_fetch = self.theme_fetch.is_some();
        let rule_suggestions = self.rule_suggestions.is_some();
        let rule_suggestions_editing = self
            .rule_suggestions
            .as_ref()
            .is_some_and(|view| view.edit().is_some());
        let review_list = self.review_list.is_some();
        let review_list_editing = self
            .review_list
            .as_ref()
            .is_some_and(ReviewList::is_editing);
        let reorganisation_plan = self.reorganisation.is_some();
        let ocr_models = self.ocr_models.is_some();
        let ai_model = self.ai_model.is_some();
        let delete_dialog = match &self.delete_dialog {
            None => DeleteKeys::Closed,
            Some(deletion) if deletion.permanent => DeleteKeys::Permanent,
            Some(_) => DeleteKeys::Trash,
        };
        match &self.prompt {
            None => PromptKeyState {
                keymap: Arc::clone(&self.keymap),
                job_running,
                config_error,
                delete_dialog,
                volume_menu,
                context_menu,
                path_editing,
                search,
                quick_open,
                command_palette,
                filing_status,
                filing_setup,
                filing_card,
                theme_fetch,
                rule_suggestions,
                rule_suggestions_editing,
                review_list,
                review_list_editing,
                reorganisation_plan,
                ocr_models,
                ai_model,
                // A conflict is asked while no prompt is open — the prompt is
                // gone by then, the transfer is what is running. Without this
                // the dialog's keys would fall through to the panels.
                conflict_open: self.pending_conflict.is_some(),
                ..PromptKeyState::default()
            },
            Some(prompt) => PromptKeyState {
                keymap: Arc::clone(&self.keymap),
                open: true,
                job_running,
                config_error,
                delete_dialog,
                volume_menu,
                context_menu,
                path_editing,
                search,
                quick_open,
                command_palette,
                filing_status,
                filing_setup,
                filing_card,
                theme_fetch,
                rule_suggestions,
                rule_suggestions_editing,
                review_list,
                review_list_editing,
                reorganisation_plan,
                ocr_models,
                ai_model,
                conflict_open: self.pending_conflict.is_some(),
                busy: prompt.busy(),
                typed: prompt.name().into(),
            },
        }
    }

    /// NC's rule for F5, F6 and F8: tagged rows win, otherwise the row under
    /// the cursor. The `..` entry is never a thing to act on. In the search
    /// view the same holds for its files, and never for all files found.
    fn action_sources(&self) -> Vec<PathBuf> {
        if let Some(search) = self.open_search_view() {
            return search.action_files();
        }
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

    /// Puts the path or name of the target on the clipboard and says so in the
    /// status bar. Nothing happens under an overlay or with nothing to copy.
    fn copy_to_clipboard(&mut self, kind: ClipboardKind) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let Some(copied) = clipboard_text(self.active_panel(), kind) else {
            return Task::none();
        };
        self.job_error = None;
        let shown = self.show_notice(self.copied_notice(kind, copied.count));
        Task::batch([iced::clipboard::write(copied.text), shown])
    }

    fn update_filing(&mut self, message: FilingMessage) -> Task<Message> {
        let Some(module) = self.filing.as_mut() else {
            return Task::none();
        };
        if matches!(message, FilingMessage::CopyRecoveryKey) {
            let Some(key_text) = module.recovery_key_text() else {
                return Task::none();
            };
            let copied = self.show_notice(self.lang.text(Msg::FilingKeyCopied).to_owned());
            return Task::batch([iced::clipboard::write(key_text), copied]);
        }
        let work_finished = matches!(message, FilingMessage::WorkDone { .. });
        let was_starting = matches!(module.phase(), Phase::Starting);
        let work = module.update(message);
        let mut pipeline_started = false;
        if self.pipeline.is_none() && matches!(module.phase(), Phase::Ready) {
            self.pipeline = module.start_pipeline().ok();
            pipeline_started = self.pipeline.is_some();
        }
        let skipped = if was_starting && !matches!(module.phase(), Phase::Starting) {
            module
                .locations()
                .iter()
                .filter(|entry| !matches!(entry.status, LocationStatus::Ready(_)))
                .count()
        } else {
            0
        };
        // A text field only gets the keys once it has the focus.
        let field = match module.phase() {
            Phase::ShowRecoveryKey { .. } if work_finished => Some(filing_ui::GROUP_FIELD_ID),
            Phase::AskRecoveryKey { .. } if work_finished => Some(filing_ui::KEY_FIELD_ID),
            _ => None,
        };
        let ask_for_ai_model =
            matches!(module.phase(), Phase::Ready) && module.ai_model_is_missing();
        let mut tasks: Vec<Task<Message>> = work.map(run_filing_work).into_iter().collect();
        tasks.extend(field.map(operation::focus));
        if pipeline_started {
            // The index is open and `reconcile` has settled what a crash left;
            // what an earlier run planned but did not move goes now.
            tasks.push(self.run_filing());
        }
        if ask_for_ai_model && !self.ai_model_asked {
            tasks.push(self.open_ai_model());
            self.ai_model_asked = self.ai_model.is_some();
        }
        if skipped > 0 {
            let text = fill_template(
                self.lang.text(Msg::FilingLocationsSkipped),
                &[("{count}", &skipped.to_string())],
            );
            tasks.push(self.show_notice(text));
        }
        Task::batch(tasks)
    }

    fn update_filing_setup(&mut self, message: FilingSetupMessage) -> Task<Message> {
        match message {
            FilingSetupMessage::Toggle => {
                self.filing_setup = match self.filing_setup.take() {
                    Some(_) => None,
                    None => Some(FilingSetup::new(&self.filing_config.locations)),
                };
                Task::none()
            }
            FilingSetupMessage::Close => {
                self.filing_setup = None;
                Task::none()
            }
            FilingSetupMessage::PickFolder(mode) => {
                let Some(setup) = self.filing_setup.as_mut() else {
                    return Task::none();
                };
                if !setup.start_adding() {
                    return Task::none();
                }
                (self.pick_folder)(&self.active_panel().path).map(move |folder| {
                    Message::FilingSetup(FilingSetupMessage::FolderPicked { mode, folder })
                })
            }
            FilingSetupMessage::FolderPicked { mode, folder } => {
                let Some(setup) = self.filing_setup.as_mut() else {
                    return Task::none();
                };
                match folder {
                    Some(picked) => self.resolve_setup_folder(mode, picked),
                    None => {
                        setup.cancel_adding();
                        Task::none()
                    }
                }
            }
            FilingSetupMessage::AddPanelFolder(mode) => {
                let Some(setup) = self.filing_setup.as_mut() else {
                    return Task::none();
                };
                if !setup.start_adding() {
                    return Task::none();
                }
                let folder = self.active_panel().path.clone();
                self.resolve_setup_folder(mode, folder)
            }
            FilingSetupMessage::Added {
                mode,
                folder,
                resolved,
            } => {
                if let Some(setup) = self.filing_setup.as_mut() {
                    setup.finish_adding(mode, folder, resolved);
                }
                Task::none()
            }
            FilingSetupMessage::SetTarget(index) => {
                if let Some(setup) = self.filing_setup.as_mut() {
                    setup.set_target(index);
                }
                Task::none()
            }
            FilingSetupMessage::ToggleImages(index) => {
                if let Some(setup) = self.filing_setup.as_mut() {
                    setup.toggle_images(index);
                }
                Task::none()
            }
            FilingSetupMessage::Remove(index) => {
                if let Some(setup) = self.filing_setup.as_mut() {
                    setup.remove(index);
                }
                Task::none()
            }
            FilingSetupMessage::Save => {
                let Some(setup) = self.filing_setup.as_mut() else {
                    return Task::none();
                };
                let Some(config_path) = self.config_path.clone() else {
                    setup.refuse_saving(SetupNote::NoConfigPath);
                    return Task::none();
                };
                let Some(chosen) = setup.start_saving() else {
                    return Task::none();
                };
                let written_path = config_path.clone();
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        filing_setup::write_locations(&written_path, &chosen)
                    }),
                    move |joined| {
                        Message::FilingSetup(FilingSetupMessage::Saved(joined.unwrap_or_else(
                            |join| {
                                Err(FilingWriteError::Io {
                                    path: config_path,
                                    reason: join.to_string(),
                                })
                            },
                        )))
                    },
                )
            }
            FilingSetupMessage::Saved(result) => {
                // The file is written even when the card was closed meanwhile,
                // so the module follows it all the same.
                match result {
                    Ok(filing) => {
                        let (applies, task) = self.apply_saved_filing(filing);
                        self.filing_setup = None;
                        let notice = self.show_notice(self.saved_filing_notice(applies));
                        Task::batch([task, notice])
                    }
                    Err(error) => {
                        if let Some(setup) = self.filing_setup.as_mut() {
                            setup.finish_saving(SetupNote::SaveFailed(error));
                        }
                        Task::none()
                    }
                }
            }
        }
    }

    /// Looks up `folder` on a blocking thread; the answer is an `Added`.
    fn resolve_setup_folder(&self, mode: LocationMode, folder: PathBuf) -> Task<Message> {
        let resolve_folder = self.resolve_folder;
        Task::perform(
            tokio::task::spawn_blocking({
                let looked_up = folder.clone();
                move || resolve_folder(&looked_up)
            }),
            move |joined| {
                Message::FilingSetup(FilingSetupMessage::Added {
                    mode,
                    folder,
                    resolved: joined.unwrap_or_else(|join| Err(join.to_string())),
                })
            },
        )
    }

    fn saved_filing_notice(&self, applies: SavedFilingApplies) -> String {
        match applies {
            SavedFilingApplies::AtNextStart => {
                self.lang.text(Msg::FilingSetupSavedNextStart).to_owned()
            }
            SavedFilingApplies::Now => {
                match self
                    .keymap
                    .shortcut_label(&Message::FilingStatus(FilingStatusMessage::Toggle))
                {
                    Some(key) => fill_template(
                        self.lang.text(Msg::FilingSetupSavedRunning),
                        &[("{key}", &key)],
                    ),
                    None => self
                        .lang
                        .text(Msg::FilingSetupSavedRunningUnbound)
                        .to_owned(),
                }
            }
        }
    }

    /// A module that works on configured locations keeps running on them:
    /// there is no reload, and the saved list takes over at the next start.
    /// One without locations, or none at all, is replaced at once.
    fn apply_saved_filing(&mut self, filing: Filing) -> (SavedFilingApplies, Task<Message>) {
        let replaceable = self.filing_key_store.is_some()
            && self.filing.as_ref().is_none_or(|module| {
                !module.has_configured_locations() && module.can_be_replaced()
            });
        if !replaceable {
            self.filing_config = filing;
            return (SavedFilingApplies::AtNextStart, Task::none());
        }
        // Dropping the runner stops it and lets go of the index before the
        // new module opens it again.
        self.pipeline = None;
        self.filing = None;
        let task = self.start_filing_module(&filing);
        self.refresh_filing_snapshot();
        (SavedFilingApplies::Now, task)
    }

    /// Shows `text` in the status bar for [`NOTICE_DURATION`]. The timer carries
    /// the generation, so it cannot clear a notice that came after it.
    fn show_notice(&mut self, text: String) -> Task<Message> {
        self.notice_generation = self.notice_generation.wrapping_add(1);
        self.notice = Some(text);
        let generation = self.notice_generation;
        Task::perform(
            async { tokio::time::sleep(NOTICE_DURATION).await },
            move |()| Message::NoticeExpired(generation),
        )
    }

    fn copied_notice(&self, kind: ClipboardKind, count: usize) -> String {
        let (one, many) = match kind {
            ClipboardKind::Path | ClipboardKind::Directory => {
                (Msg::StatusPathCopied, Msg::StatusPathsCopied)
            }
            ClipboardKind::Name => (Msg::StatusNameCopied, Msg::StatusNamesCopied),
        };
        if count == 1 {
            self.lang.text(one).to_string()
        } else {
            self.lang.text(many).replace("{count}", &count.to_string())
        }
    }

    fn dialog_button_count(&self) -> usize {
        if self.delete_dialog.is_some() || self.ocr_models.is_some() || self.ai_model.is_some() {
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
            return if self.dialog_focus == delete_dialog_view::CONFIRM_FOCUS {
                self.confirm_delete()
            } else {
                self.delete_dialog = None;
                Task::none()
            };
        }
        if self.ocr_models.is_some() {
            return if self.dialog_focus == ocr_models_ui::DOWNLOAD_FOCUS {
                self.start_ocr_models_download()
            } else {
                self.close_ocr_models()
            };
        }
        if self.ai_model.is_some() {
            return if self.dialog_focus == ai_model_ui::DOWNLOAD_FOCUS {
                self.press_ai_model_button()
            } else {
                self.close_ai_model()
            };
        }
        if self.pending_conflict.is_some() {
            let choice = match self.dialog_focus {
                conflict::OVERWRITE_FOCUS => ConflictChoice::ThisOverwrite,
                conflict::KEEP_FOCUS => ConflictChoice::ThisKeep,
                _ => ConflictChoice::Cancel,
            };
            return self.answer_conflict(choice);
        }
        Task::none()
    }

    /// A prompt, dialog or menu is on top of the panels.
    fn overlay_is_open(&self) -> bool {
        self.blocking_overlay_is_open() || self.context_menu.is_some()
    }

    /// Everything on top of the panels except the context menu, which a new
    /// right click may replace.
    fn blocking_overlay_is_open(&self) -> bool {
        self.prompt.is_some()
            || self.pending_conflict.is_some()
            || self.delete_dialog.is_some()
            || self.volume_menu.is_some()
            || self.quick_open.is_some()
            || self.command_palette.is_some()
            || self.theme_fetch.is_some()
            || self.rule_suggestions.is_some()
            || self.review_list.is_some()
            || self.reorganisation.is_some()
            || self.ocr_models.is_some()
            || self.ai_model.is_some()
            || self.filing_status.page_open()
    }

    /// Whether the context menu entry can do something for the current
    /// cursor row and tags.
    fn context_action_enabled(&self, action: ContextAction) -> bool {
        let panel = self.active_panel();
        let cursor_row = panel.selected_entry();
        match action {
            ContextAction::Open => cursor_row.is_some(),
            ContextAction::View | ContextAction::Edit => {
                cursor_row.is_some_and(|entry| !entry.is_dir && !entry.is_parent)
            }
            ContextAction::Copy
            | ContextAction::Move
            | ContextAction::Trash
            | ContextAction::DeletePermanently => !self.action_sources().is_empty(),
            ContextAction::NewFolder => true,
            ContextAction::CopyPath => clipboard_text(panel, ClipboardKind::Path).is_some(),
            ContextAction::CopyName => clipboard_text(panel, ClipboardKind::Name).is_some(),
        }
    }

    /// Opens the context menu on row `index` of `side`, at `anchor` or, without
    /// one, below that row. The cursor goes to the row; tags stay, so the menu
    /// acts on them as F5 and F8 do.
    fn open_context_menu(&mut self, side: PanelSide, index: usize, anchor: Option<Point>) {
        if self.blocking_overlay_is_open() || (self.search_open && side == self.search_side) {
            return;
        }
        // An open menu is replaced by a right click and left alone by the keys.
        if anchor.is_none() && self.context_menu.is_some() {
            return;
        }
        let rows = self.visible_rows;
        self.active_panel = side;
        self.job_error = None;
        self.notice = None;
        let panel = self.panel_mut(side);
        // As in the Finder: a click on a row outside the tags drops them, so
        // the menu cannot act on rows the user did not point at.
        let on_untagged_row = panel
            .entries
            .get(index)
            .is_some_and(|entry| !panel.selection.is_tagged(&entry.name));
        if anchor.is_some() && on_untagged_row {
            panel.selection.clear();
        }
        panel.select(index, rows);
        let window = self.window_size;
        let cursor_panel = self.panel(side);
        let place_at = anchor.unwrap_or_else(|| {
            layout::cursor_row_anchor(
                side == PanelSide::Right,
                cursor_panel
                    .selected
                    .saturating_sub(cursor_panel.scroll_offset),
                window,
            )
        });
        self.context_menu = Some(ContextMenu::new(place_at, self.window_size, |action| {
            self.context_action_enabled(action)
        }));
    }

    /// Closes the menu and sends the message its highlighted entry stands for,
    /// the same one its key sends. Closed first: the actions refuse to run
    /// under an overlay.
    fn run_context_menu_entry(&mut self) -> Task<Message> {
        let Some(menu) = self.context_menu.take() else {
            return Task::none();
        };
        match menu.selected_action() {
            Some(action) => self.update(action.message()),
            None => Task::none(),
        }
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

    /// A click on the "access blocked" error: the Full Disk Access page of
    /// System Settings. A launch that fails says so in the status bar.
    #[cfg(target_os = "macos")]
    fn open_privacy_settings(&mut self) {
        self.job_error = None;
        let command = fs::privacy_settings_command();
        if let Err(reason) = self.launcher.launch(&command) {
            let failure = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", self.lang.text(Msg::StatusSystemSettings))
                .replace("{reason}", &reason);
            self.set_status_error(&failure);
        }
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
        self.notice = None;
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
        self.notice = None;
        self.dialog_focus = delete_dialog_view::default_focus(permanent);
        self.delete_dialog = Some(Deletion {
            sources,
            permanent,
            side: self.source_side(),
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
        let side = self.deleting.take().map_or(self.source_side(), |d| d.side);
        self.generation = self.generation.wrapping_add(1);
        self.job_progress = None;
        self.panel_mut(side).selection.clear();
        let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
            let path = self.panel(each).path.clone();
            self.load(each, path, None)
        });
        Task::batch(tasks.into_iter().chain([self.restart_search()]))
    }

    /// F3 / F4: the row under the cursor goes to the external program. Tags
    /// are ignored, as in Norton Commander; `..` and directories have nothing
    /// to view or edit.
    fn open_external(&mut self, kind: fs::OpenKind) {
        let target = match self.open_search_view() {
            Some(search) => search.cursor_target().filter(|(path, _)| !path.is_dir()),
            None => self
                .active_panel()
                .selected_entry()
                .filter(|entry| !entry.is_parent && !entry.is_dir)
                .map(|entry| (entry.path.clone(), None)),
        };
        if let Some((path, line)) = target {
            self.launch_external(kind, path, line);
        }
    }

    /// Hands `path` to the program for `kind`, at `line` when the program
    /// takes one.
    fn launch_external(&mut self, kind: fs::OpenKind, path: PathBuf, line: Option<u64>) {
        self.job_error = None;
        self.notice = None;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let executable = fs::could_execute(&path);
        let Some(command) = fs::open_command(&self.open_programs, kind, &path, executable, line)
        else {
            let keys: Vec<String> = [
                Message::OpenExternal(fs::OpenKind::View),
                Message::OpenExternal(fs::OpenKind::Edit),
            ]
            .iter()
            .filter_map(|opener| self.keymap.shortcut_label(opener))
            .collect();
            let message = if keys.is_empty() {
                fill_template(
                    self.lang.text(Msg::ErrorOpenRefusedNoKey),
                    &[("{name}", &name)],
                )
            } else {
                fill_template(
                    self.lang.text(Msg::ErrorOpenRefused),
                    &[("{name}", &name), ("{keys}", &keys.join("/"))],
                )
            };
            self.set_status_error(&message);
            return;
        };
        if let Err(reason) = self.launcher.launch(&command) {
            let message = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", &name)
                .replace("{reason}", &reason);
            self.set_status_error(&message);
        }
    }

    /// The status line for an entry that could not be deleted. A failed trash
    /// points at the permanent-delete key rather than falling back to it:
    /// deleting for good is the user's decision, not the app's.
    fn delete_failure(&self, index: usize, reason: &str) -> String {
        let permanent = self.deleting.as_ref().is_some_and(|d| d.permanent);
        let name = self
            .deleting
            .as_ref()
            .and_then(|d| d.sources.get(index))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let permanent_key = self
            .keymap
            .shortcut_label(&Message::Delete { permanent: true });
        let template = match (permanent, &permanent_key) {
            (true, _) => Msg::ErrorDeleteFailed,
            (false, Some(_)) => Msg::ErrorTrashFailed,
            (false, None) => Msg::ErrorTrashFailedNoKey,
        };
        fill_template(
            self.lang.text(template),
            &[
                ("{name}", &name),
                ("{reason}", reason),
                ("{key}", permanent_key.as_deref().unwrap_or_default()),
            ],
        )
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

        let target = self.panel(self.source_side().other()).path.clone();
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
        let source_side = self.source_side();
        let target_side = source_side.other();
        let source_path = self.panel(source_side).path.clone();
        let target_path = self.panel(target_side).path.clone();
        Task::batch([
            self.load(source_side, source_path, None),
            self.load(target_side, target_path, None),
            self.restart_search(),
        ])
    }

    fn progress_sender(&mut self) -> mpsc::UnboundedSender<fs::transfer::Tick> {
        match &self.progress_tx {
            Some(tx) => tx.clone(),
            None => {
                let (created, receiver) = tokio::sync::mpsc::unbounded_channel();
                self.progress_tx = Some(created.clone());
                self.progress_rx = Some(Arc::new(tokio::sync::Mutex::new(receiver)));
                created
            }
        }
    }

    /// Runs the selected plan entries on the blocking pool, as one job. Escape
    /// closes the progress channel, which stops the run between two files.
    fn run_reorganisation(&mut self, selection: Vec<PlannedItem>) -> Task<Message> {
        if self.jobs.is_busy() || selection.is_empty() {
            return Task::none();
        }
        let Some(access) = self.filing.as_ref().and_then(FilingModule::plan_access) else {
            self.set_status_error(INDEX_NOT_OPEN);
            return Task::none();
        };
        let progress_tx = self.progress_sender();
        let total = selection.len();
        self.job_progress = Some(fs::transfer::Tick { done: 0, total });
        self.jobs.enqueue(jobs::Job {
            kind: jobs::JobKind::Reorganise,
            path: selection
                .first()
                .map(|entry| entry.location.clone())
                .unwrap_or_default(),
            total: Some(total),
        });
        self.jobs
            .start_next(|_| {
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        let mut progress = fs::transfer::Reporting::new(&progress_tx);
                        access.execute(&selection, &mut progress)
                    }),
                    |joined| {
                        Message::ReorganisationExecuted(
                            joined.unwrap_or_else(|join| Err(join.to_string())),
                        )
                    },
                )
            })
            .unwrap_or_else(Task::none)
    }

    /// One job files every planned document. Waits for a running job instead
    /// of queueing behind it, and is asked for again on the next tick.
    fn run_filing(&mut self) -> Task<Message> {
        if self.jobs.is_busy() {
            self.filing_due = true;
            return Task::none();
        }
        self.filing_due = false;
        let Some(access) = self.filing.as_ref().and_then(FilingModule::filing_access) else {
            return Task::none();
        };
        let progress_tx = self.progress_sender();
        self.jobs.enqueue(jobs::Job {
            kind: jobs::JobKind::File,
            path: PathBuf::new(),
            total: None,
        });
        self.jobs
            .start_next(|_| {
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        let mut progress = fs::transfer::Reporting::new(&progress_tx);
                        access.file_planned(&mut progress)
                    }),
                    |joined| {
                        Message::DocumentsFiled(joined.unwrap_or_else(|join| Err(join.to_string())))
                    },
                )
            })
            .unwrap_or_else(Task::none)
    }

    fn documents_filed(&mut self, result: Result<FilingSummary, String>) -> Task<Message> {
        self.jobs.finish();
        self.job_progress = None;
        match result {
            Ok(summary) if summary.filed + summary.reviewed > 0 => {
                let text = fill_template(
                    self.lang.text(Msg::StatusDocumentsFiled),
                    &[
                        ("{filed}", &summary.filed.to_string()),
                        ("{reviewed}", &summary.reviewed.to_string()),
                    ],
                );
                self.show_notice(text)
            }
            Ok(_) => Task::none(),
            Err(detail) => {
                self.set_status_error(&detail);
                Task::none()
            }
        }
    }

    fn reorganisation_executed(
        &mut self,
        result: Result<ExecutionSummary, String>,
    ) -> Task<Message> {
        self.jobs.finish();
        self.job_progress = None;
        match result {
            Ok(summary) => {
                let text = self
                    .lang
                    .text(Msg::StatusReorganised)
                    .replace("{moved}", &summary.moved.to_string())
                    .replace("{skipped}", &summary.skipped.to_string());
                Task::batch([self.show_notice(text), self.replan_if_open()])
            }
            Err(detail) => {
                self.set_status_error(&detail);
                self.replan_if_open()
            }
        }
    }

    fn undo_reorganisation(&mut self) -> Task<Message> {
        if self.jobs.is_busy() {
            return Task::none();
        }
        let Some(access) = self.filing.as_ref().and_then(FilingModule::plan_access) else {
            self.set_status_error(INDEX_NOT_OPEN);
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || access.undo_last()),
            |joined| {
                Message::ReorganisationUndone(joined.unwrap_or_else(|join| Err(join.to_string())))
            },
        )
    }

    fn reorganisation_undone(
        &mut self,
        result: Result<Option<execute::UndoReport>, String>,
    ) -> Task<Message> {
        match result {
            Ok(Some(report)) => {
                let text = self
                    .lang
                    .text(Msg::StatusReorganisationUndone)
                    .replace("{moved}", &report.undone.to_string())
                    .replace("{skipped}", &report.left.to_string());
                Task::batch([self.show_notice(text), self.replan_if_open()])
            }
            Ok(None) => {
                let text = self.lang.text(Msg::StatusNothingToUndo).to_string();
                self.show_notice(text)
            }
            Err(detail) => {
                self.set_status_error(&detail);
                Task::none()
            }
        }
    }

    /// The files moved, so an open plan is out of date.
    fn replan_if_open(&mut self) -> Task<Message> {
        let Some(view) = self.reorganisation.as_mut() else {
            return Task::none();
        };
        view.begin_planning();
        self.reorganisation_generation = self.reorganisation_generation.wrapping_add(1);
        self.run_planning()
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
        let progress_tx = self.progress_sender();

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
        self.notice = None;
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

    fn toggle_shortcuts(&self) -> search_ui::ToggleShortcuts {
        let label = |toggle| self.keymap.shortcut_label(&Message::Search(toggle));
        search_ui::ToggleShortcuts {
            case: label(SearchMessage::ToggleCase),
            whole_word: label(SearchMessage::ToggleWholeWord),
            regex: label(SearchMessage::ToggleRegex),
        }
    }

    /// The search view, while it is up.
    fn open_search_view(&self) -> Option<&SearchView> {
        self.search.as_ref().filter(|_| self.search_has_keys())
    }

    /// The search view is up and its panel is the active one: the keys and the
    /// file actions are the search's. After Tab or a click on the other panel
    /// they are that panel's again, while the search stays on screen.
    fn search_has_keys(&self) -> bool {
        self.search_open && self.active_panel == self.search_side
    }

    /// The panel that file actions take their files from: the one the search
    /// view stands in for, else the active one.
    fn source_side(&self) -> PanelSide {
        if self.search_has_keys() {
            self.search_side
        } else {
            self.active_panel
        }
    }

    fn resize_search(&mut self, window: Size) {
        self.search_rows = layout::search_visible_rows(window);
        if let Some(view) = self.search.as_mut() {
            view.ensure_visible(self.search_rows);
        }
    }

    fn focus_search_field(&self) -> Task<Message> {
        let id = self.search.as_ref().map_or(search_ui::QUERY_ID, |view| {
            search_ui::field_id(view.focused)
        });
        operation::focus(id).chain(operation::select_all(id))
    }

    /// Shows the search view in place of the active panel, below that panel's
    /// directory. A view kept from before keeps its fields and options, and
    /// searches anew, so no result is older than the files. Pressed while it is
    /// up, the key moves to the next field.
    fn open_search(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        if self.search_open {
            if self.search_has_keys() {
                if let Some(view) = self.search.as_mut() {
                    view.focused = view.focused.next();
                }
            } else {
                self.active_panel = self.search_side;
            }
            return self.focus_search_field();
        }
        self.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let side = self.active_panel;
        let root = self.panel(side).path.clone();
        self.search_side = side;
        self.search_open = true;
        self.search_rows = layout::search_visible_rows(self.window_size);
        match self.search.as_mut() {
            Some(view) => view.root = root,
            None => self.search = Some(SearchView::new(root)),
        }
        let search = self.restart_search();
        Task::batch([self.focus_search_field(), search])
    }

    fn next_search_generation(&mut self) -> u64 {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_generation
    }

    /// Searches again once the typing pauses; the running search stops now.
    fn schedule_search(&mut self) -> Task<Message> {
        self.search_handle = None;
        let generation = self.next_search_generation();
        if let Some(view) = self.search.as_mut() {
            view.generation = generation;
            view.running = false;
        }
        Task::perform(
            async { tokio::time::sleep(SEARCH_DEBOUNCE).await },
            move |()| Message::Search(SearchMessage::Debounced(generation)),
        )
    }

    /// Searches again now, for when the files changed under the results.
    fn restart_search(&mut self) -> Task<Message> {
        if !self.search_open {
            return Task::none();
        }
        self.search_handle = None;
        let generation = self.next_search_generation();
        self.start_search(generation)
    }

    fn start_search(&mut self, generation: u64) -> Task<Message> {
        let Some(view) = self.search.as_mut() else {
            return Task::none();
        };
        view.generation = generation;
        match fs::search::compile(&view.query_for_engine()) {
            // Nothing to look for yet: the empty list is the answer.
            Err(fs::search::SearchError::Empty) => {
                view.clear_results();
                Task::none()
            }
            Err(error) => {
                view.clear_results();
                view.error = Some(error);
                Task::none()
            }
            Ok(compiled) => {
                view.begin(generation);
                let (task, handle) = Task::run(
                    search_events(compiled, generation, search_batch),
                    std::convert::identity,
                )
                .abortable();
                self.search_handle = Some(handle.abort_on_drop());
                task
            }
        }
    }

    fn update_search(&mut self, message: SearchMessage) -> Task<Message> {
        if !self.search_open {
            return Task::none();
        }
        // Tagging comes from the keys only, and the keys are the panel's when
        // the other one is active. The toggles are not guarded the same way: a
        // click on one has to work from the other panel and activates this one.
        if matches!(
            message,
            SearchMessage::ToggleTag
                | SearchMessage::TagMove(_)
                | SearchMessage::TagAll
                | SearchMessage::ClearTags
        ) && !self.search_has_keys()
        {
            return Task::none();
        }
        // Using the view makes its panel the active one; what runs by itself
        // does not.
        if !matches!(
            message,
            SearchMessage::Events { .. }
                | SearchMessage::Debounced(_)
                | SearchMessage::Close
                | SearchMessage::Scrolled(_)
        ) {
            self.active_panel = self.search_side;
        }
        let rows = self.search_rows;
        let Some(view) = self.search.as_mut() else {
            return Task::none();
        };
        match message {
            SearchMessage::QueryChanged(text) => {
                view.query = text;
                view.focused = SearchField::Query;
                self.schedule_search()
            }
            SearchMessage::IncludeChanged(text) => {
                view.include = text;
                view.focused = SearchField::Include;
                self.schedule_search()
            }
            SearchMessage::ExcludeChanged(text) => {
                view.exclude = text;
                view.focused = SearchField::Exclude;
                self.schedule_search()
            }
            SearchMessage::AppendToQuery(typed) => {
                view.query.push_str(&typed);
                view.focused = SearchField::Query;
                let search = self.schedule_search();
                let id = search_ui::QUERY_ID;
                Task::batch([
                    search,
                    operation::focus(id).chain(operation::move_cursor_to_end(id)),
                ])
            }
            SearchMessage::ToggleCase => {
                view.case_sensitive = !view.case_sensitive;
                self.schedule_search()
            }
            SearchMessage::ToggleWholeWord => {
                view.whole_word = !view.whole_word;
                self.schedule_search()
            }
            SearchMessage::ToggleRegex => {
                view.regex = !view.regex;
                self.schedule_search()
            }
            SearchMessage::ToggleTag => {
                view.tag_and_move(1, rows);
                Task::none()
            }
            SearchMessage::TagMove(delta) => {
                view.tag_and_move(delta, rows);
                Task::none()
            }
            SearchMessage::TagAll => {
                view.tag_all();
                Task::none()
            }
            SearchMessage::ClearTags => {
                view.tagged.clear();
                Task::none()
            }
            SearchMessage::Debounced(generation) => {
                if generation == view.generation {
                    self.start_search(generation)
                } else {
                    Task::none()
                }
            }
            SearchMessage::Events { generation, events } => {
                if generation == view.generation {
                    for event in events {
                        view.apply_event(event);
                    }
                    view.ensure_visible(rows);
                    if !view.running {
                        self.search_handle = None;
                    }
                }
                Task::none()
            }
            SearchMessage::Close => {
                // Late timers and batches of the stopped search find another
                // generation and are dropped.
                view.running = false;
                self.search_open = false;
                self.search_handle = None;
                let generation = self.next_search_generation();
                if let Some(kept) = self.search.as_mut() {
                    kept.generation = generation;
                }
                release_focus()
            }
            SearchMessage::Move(delta) => {
                view.move_cursor(delta, rows);
                Task::none()
            }
            SearchMessage::Page(direction) => {
                view.page(direction, rows);
                Task::none()
            }
            SearchMessage::First => {
                view.first(rows);
                Task::none()
            }
            SearchMessage::Last => {
                view.last(rows);
                Task::none()
            }
            SearchMessage::Expand(expanded) => {
                view.set_expanded(expanded, rows);
                Task::none()
            }
            SearchMessage::Activate => self.activate_search_cursor(),
            SearchMessage::RowClicked(index) => {
                view.select_row(index, rows);
                release_focus()
            }
            SearchMessage::RowActivated(index) => {
                view.select_row(index, rows);
                self.activate_search_cursor()
            }
            SearchMessage::ToggleGroup(file) => {
                view.toggle_group(file, rows);
                Task::none()
            }
            SearchMessage::Scrolled(delta) => {
                view.scroll(delta, rows);
                Task::none()
            }
        }
    }

    /// Enter on a result: a file is shown in the other panel, a line is opened
    /// in the editor at that line.
    fn activate_search_cursor(&mut self) -> Task<Message> {
        let Some(target) = self.open_search_view().and_then(SearchView::cursor_target) else {
            return Task::none();
        };
        match target {
            (path, Some(line)) => {
                self.launch_external(fs::OpenKind::Edit, path, Some(line));
                Task::none()
            }
            (path, None) => {
                let (Some(directory), Some(file)) = (path.parent(), path.file_name()) else {
                    return Task::none();
                };
                let name = file.to_string_lossy().into_owned();
                self.load(
                    self.search_side.other(),
                    directory.to_path_buf(),
                    Some(name),
                )
            }
        }
    }

    /// Cmd/Ctrl+P: lists the files below the active panel's directory. Pressed
    /// while the box is up, it only gives the field the focus again.
    fn open_quick_open(&mut self) -> Task<Message> {
        let focus = operation::focus(quick_open_ui::QUICK_OPEN_ID);
        if self.quick_open.is_some() {
            return focus;
        }
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        let root = self.active_panel().path.clone();
        self.quick_open_walk_generation = self.quick_open_walk_generation.wrapping_add(1);
        let generation = self.quick_open_walk_generation;
        let walk = fs::search::all_files(
            root.clone(),
            stays_on_file_system(&root),
            QUICK_OPEN_FILE_LIMIT,
        );
        let (task, handle) = Task::run(
            search_events(walk, generation, quick_open_batch),
            std::convert::identity,
        )
        .abortable();
        self.quick_open_walk = Some(handle.abort_on_drop());
        self.quick_open = Some(QuickOpen::new(root, generation));
        Task::batch([focus, task])
    }

    fn update_rule_suggestions(&mut self, message: RuleSuggestionsMessage) -> Task<Message> {
        match message {
            RuleSuggestionsMessage::Open => self.open_rule_suggestions(),
            RuleSuggestionsMessage::Close => self.close_rule_suggestions(),
            RuleSuggestionsMessage::Reanalyse => {
                let Some(view) = self.rule_suggestions.as_mut() else {
                    return Task::none();
                };
                if view.is_busy() {
                    return Task::none();
                }
                view.begin_analysis();
                self.run_suggestion_work(SuggestionAccess::analyse)
            }
            RuleSuggestionsMessage::Loaded { generation, result } => {
                if generation != self.rule_suggestions_generation {
                    return Task::none();
                }
                let Some(view) = self.rule_suggestions.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(rows) => view.loaded(rows),
                    Err(detail) => view.failed(detail),
                }
                self.look_up_suggestion_matches()
            }
            RuleSuggestionsMessage::Move(delta) => {
                if let Some(view) = self.rule_suggestions.as_mut() {
                    view.move_cursor(delta);
                }
                self.look_up_suggestion_matches()
            }
            RuleSuggestionsMessage::Select(index) => {
                if let Some(view) = self.rule_suggestions.as_mut() {
                    view.select(index);
                }
                self.look_up_suggestion_matches()
            }
            RuleSuggestionsMessage::Edit => {
                let Some(view) = self.rule_suggestions.as_mut() else {
                    return Task::none();
                };
                view.start_edit();
                if view.edit().is_some() {
                    operation::focus(rule_suggestions_ui::CONDITION_ID)
                } else {
                    Task::none()
                }
            }
            RuleSuggestionsMessage::CancelEdit => {
                if let Some(view) = self.rule_suggestions.as_mut() {
                    view.cancel_edit();
                }
                release_focus()
            }
            RuleSuggestionsMessage::ConditionChanged(text) => {
                if let Some(view) = self.rule_suggestions.as_mut() {
                    view.set_condition(text);
                }
                Task::none()
            }
            RuleSuggestionsMessage::KindChanged(text) => {
                if let Some(view) = self.rule_suggestions.as_mut() {
                    view.set_kind(text);
                }
                Task::none()
            }
            RuleSuggestionsMessage::PatternChanged(text) => {
                if let Some(view) = self.rule_suggestions.as_mut() {
                    view.set_pattern(text);
                }
                Task::none()
            }
            RuleSuggestionsMessage::FocusField { backwards } => {
                if backwards {
                    operation::focus_previous()
                } else {
                    operation::focus_next()
                }
            }
            RuleSuggestionsMessage::Confirm => {
                let Some((id, spec)) = self
                    .rule_suggestions
                    .as_mut()
                    .and_then(RuleSuggestions::begin_confirm)
                else {
                    return Task::none();
                };
                self.run_suggestion_work(move |access| access.confirm(id, &spec))
            }
            RuleSuggestionsMessage::Discard => {
                let Some(id) = self
                    .rule_suggestions
                    .as_mut()
                    .and_then(RuleSuggestions::begin_discard)
                else {
                    return Task::none();
                };
                self.run_suggestion_work(move |access| access.discard(id))
            }
            RuleSuggestionsMessage::MatchesFound { id, result } => {
                if let (Some(view), Ok(paths)) = (self.rule_suggestions.as_mut(), result) {
                    view.matches_found(id, paths);
                }
                Task::none()
            }
        }
    }

    /// The analysis starts with the view; the list appears when it is done.
    fn open_rule_suggestions(&mut self) -> Task<Message> {
        if self.rule_suggestions.is_some() || self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.rule_suggestions_generation = self.rule_suggestions_generation.wrapping_add(1);
        self.rule_suggestions = Some(RuleSuggestions::new());
        self.run_suggestion_work(SuggestionAccess::analyse)
    }

    /// Escape leaves the form first, the view second.
    fn close_rule_suggestions(&mut self) -> Task<Message> {
        match self.rule_suggestions.as_mut() {
            Some(view) if view.edit().is_some() => {
                view.cancel_edit();
                release_focus()
            }
            Some(_) => {
                self.rule_suggestions = None;
                release_focus()
            }
            None => Task::none(),
        }
    }

    /// Runs `work` on a blocking thread: the index is shared with the intake,
    /// which may hold it while it hashes a large file.
    fn run_suggestion_work(
        &mut self,
        work: impl FnOnce(&SuggestionAccess) -> Result<Vec<SuggestedRule>, String> + Send + 'static,
    ) -> Task<Message> {
        let generation = self.rule_suggestions_generation;
        let Some(access) = self
            .filing
            .as_ref()
            .and_then(FilingModule::suggestion_access)
        else {
            if let Some(view) = self.rule_suggestions.as_mut() {
                view.failed(INDEX_NOT_OPEN.to_owned());
            }
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || work(&access)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                Message::RuleSuggestions(RuleSuggestionsMessage::Loaded { generation, result })
            },
        )
    }

    /// Looks up the documents of the selected suggestion, once per selection.
    fn look_up_suggestion_matches(&mut self) -> Task<Message> {
        let Some((id, condition)) = self
            .rule_suggestions
            .as_mut()
            .and_then(RuleSuggestions::matches_wanted)
        else {
            return Task::none();
        };
        let Some(access) = self
            .filing
            .as_ref()
            .and_then(FilingModule::suggestion_access)
        else {
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || access.matching_documents(&condition)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                Message::RuleSuggestions(RuleSuggestionsMessage::MatchesFound { id, result })
            },
        )
    }

    fn update_review_list(&mut self, message: ReviewListMessage) -> Task<Message> {
        match message {
            ReviewListMessage::Open => self.open_review_list(),
            ReviewListMessage::Close => self.close_review_list(),
            ReviewListMessage::Loaded { generation, result } => {
                if generation != self.review_list_generation {
                    return Task::none();
                }
                let Some(view) = self.review_list.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(rows) => view.loaded(rows),
                    Err(detail) => view.failed(detail),
                }
                Task::none()
            }
            ReviewListMessage::Move(delta) => {
                if let Some(view) = self.review_list.as_mut() {
                    view.move_cursor(delta);
                }
                Task::none()
            }
            ReviewListMessage::Select(index) => {
                if let Some(view) = self.review_list.as_mut() {
                    view.select(index);
                }
                Task::none()
            }
            ReviewListMessage::Edit => {
                let Some(view) = self.review_list.as_mut() else {
                    return Task::none();
                };
                view.start_edit();
                if view.is_editing() {
                    operation::focus(review_list_ui::FIRST_FIELD_ID)
                } else {
                    Task::none()
                }
            }
            ReviewListMessage::CancelEdit => {
                if let Some(view) = self.review_list.as_mut() {
                    view.cancel_edit();
                }
                release_focus()
            }
            ReviewListMessage::FieldChanged(field, text) => {
                if let Some(view) = self.review_list.as_mut() {
                    view.set_text(field, text);
                }
                Task::none()
            }
            ReviewListMessage::TaxContextChosen(choice) => {
                if let Some(view) = self.review_list.as_mut() {
                    view.set_tax_context(choice);
                }
                Task::none()
            }
            ReviewListMessage::FocusField { backwards } => {
                if backwards {
                    operation::focus_previous()
                } else {
                    operation::focus_next()
                }
            }
            ReviewListMessage::Confirm => {
                let Some(request) = self
                    .review_list
                    .as_mut()
                    .and_then(ReviewList::begin_confirm)
                else {
                    return Task::none();
                };
                let generation = self.review_list_generation;
                let Some(access) = self.review_access() else {
                    return Task::none();
                };
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        access.confirm(
                            request.entry,
                            request.document_id,
                            &request.fields,
                            &request.pattern,
                        )
                    }),
                    move |joined| {
                        let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                        Message::ReviewList(ReviewListMessage::Confirmed { generation, result })
                    },
                )
            }
            ReviewListMessage::Confirmed { generation, result } => {
                if generation != self.review_list_generation {
                    return Task::none();
                }
                let Some(view) = self.review_list.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(outcome) => view.confirmed(outcome),
                    Err(detail) => {
                        view.failed(detail);
                        return Task::none();
                    }
                }
                self.run_review_load()
            }
            ReviewListMessage::CreateRule => {
                let Some(request) = self.review_list.as_mut().and_then(ReviewList::begin_rule)
                else {
                    return Task::none();
                };
                let generation = self.review_list_generation;
                let Some(access) = self.review_access() else {
                    return Task::none();
                };
                Task::perform(
                    tokio::task::spawn_blocking(move || {
                        access.create_rule_for_sender(
                            &request.sender,
                            &request.kind,
                            request.tax_context,
                            request.tags,
                            &request.pattern,
                        )
                    }),
                    move |joined| {
                        let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                        Message::ReviewList(ReviewListMessage::RuleCreated { generation, result })
                    },
                )
            }
            ReviewListMessage::RuleCreated { generation, result } => {
                if generation != self.review_list_generation {
                    return Task::none();
                }
                let Some(view) = self.review_list.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(_) => view.rule_created(),
                    Err(detail) => view.failed(detail),
                }
                Task::none()
            }
        }
    }

    fn review_access(&mut self) -> Option<ReviewAccess> {
        let access = self.filing.as_ref().and_then(FilingModule::review_access);
        if access.is_none() {
            if let Some(view) = self.review_list.as_mut() {
                view.failed(INDEX_NOT_OPEN.to_owned());
            }
        }
        access
    }

    fn open_review_list(&mut self) -> Task<Message> {
        if self.review_list.is_some() || self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.review_list_generation = self.review_list_generation.wrapping_add(1);
        self.review_list = Some(ReviewList::new());
        self.run_review_load()
    }

    /// Escape leaves the form first, the list second.
    fn close_review_list(&mut self) -> Task<Message> {
        match self.review_list.as_mut() {
            Some(view) if view.is_editing() => {
                view.cancel_edit();
                release_focus()
            }
            Some(_) => {
                self.review_list = None;
                release_focus()
            }
            None => Task::none(),
        }
    }

    /// Loads on a blocking thread: the load plans every open entry again, and
    /// the index is shared with the intake.
    fn run_review_load(&mut self) -> Task<Message> {
        let generation = self.review_list_generation;
        let Some(access) = self.review_access() else {
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || access.load()),
            move |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                Message::ReviewList(ReviewListMessage::Loaded { generation, result })
            },
        )
    }

    /// The Hermes card that is up, if any.
    fn open_hermes_page(&self) -> Option<HermesPage> {
        if self.filing_status.page_open() {
            Some(HermesPage::Inspection)
        } else if self.review_list.is_some() {
            Some(HermesPage::Review)
        } else if self.rule_suggestions.is_some() {
            Some(HermesPage::RuleSuggestions)
        } else if self.reorganisation.is_some() {
            Some(HermesPage::Reorganisation)
        } else {
            None
        }
    }

    /// A click in the sidebar: the card that is up closes without its Escape
    /// steps, the chosen page opens. Nothing happens when it is the open one,
    /// or when no Hermes card is up.
    fn go_to_hermes_page(&mut self, page: HermesPage) -> Task<Message> {
        match self.open_hermes_page() {
            None => return Task::none(),
            Some(open) if open == page => return Task::none(),
            Some(_) => {}
        }
        self.filing_status.close_page();
        self.rule_suggestions = None;
        self.review_list = None;
        self.reorganisation = None;
        let open = match page {
            HermesPage::Inspection => self.open_filing_status(),
            HermesPage::Review => self.open_review_list(),
            HermesPage::RuleSuggestions => self.open_rule_suggestions(),
            HermesPage::Reorganisation => self.open_reorganisation(),
        };
        Task::batch([release_focus(), open])
    }

    /// The counts the sidebar shows, from what the app already holds.
    fn hermes_badges(&self) -> Badges {
        let figures = self
            .filing_status
            .index_figures()
            .and_then(|figures| figures.as_ref().ok())
            .and_then(|figures| usize::try_from(figures.statistics.open_reviews).ok());
        Badges {
            review: self
                .review_list
                .as_ref()
                .and_then(ReviewList::open_count)
                .or(figures),
            rule_suggestions: self
                .rule_suggestions
                .as_ref()
                .filter(|view| !view.is_busy())
                .map(|view| view.rows().len()),
        }
    }

    fn update_reorganisation(&mut self, message: ReorganisationPlanMessage) -> Task<Message> {
        match message {
            ReorganisationPlanMessage::Open => self.open_reorganisation(),
            ReorganisationPlanMessage::Close => {
                if self.reorganisation.take().is_some() {
                    release_focus()
                } else {
                    Task::none()
                }
            }
            ReorganisationPlanMessage::Replan => {
                let Some(view) = self.reorganisation.as_mut() else {
                    return Task::none();
                };
                if view.is_busy() {
                    return Task::none();
                }
                view.begin_planning();
                self.run_planning()
            }
            ReorganisationPlanMessage::Loaded { generation, result } => {
                if generation != self.reorganisation_generation {
                    return Task::none();
                }
                match (self.reorganisation.as_mut(), result) {
                    (Some(view), Ok(rows)) => view.loaded(rows),
                    (Some(view), Err(detail)) => view.failed(detail),
                    (None, _) => {}
                }
                Task::none()
            }
            ReorganisationPlanMessage::Move(delta) => {
                if let Some(view) = self.reorganisation.as_mut() {
                    view.move_cursor(delta);
                }
                Task::none()
            }
            ReorganisationPlanMessage::Toggle(line) => {
                if let Some(view) = self.reorganisation.as_mut() {
                    view.toggle(line);
                }
                Task::none()
            }
            ReorganisationPlanMessage::ToggleCursor => {
                if let Some(view) = self.reorganisation.as_mut() {
                    view.toggle_cursor();
                }
                Task::none()
            }
            ReorganisationPlanMessage::Execute => match self.reorganisation.as_ref() {
                Some(view) if view.has_selection() && !view.is_busy() => {
                    Task::done(Message::ExecuteReorganisation(view.selection()))
                }
                _ => Task::none(),
            },
        }
    }

    /// The plan is made as the view opens; the entries appear when it is done.
    fn open_reorganisation(&mut self) -> Task<Message> {
        if self.reorganisation.is_some() || self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.reorganisation_generation = self.reorganisation_generation.wrapping_add(1);
        self.reorganisation = Some(ReorganisationView::new());
        self.run_planning()
    }

    /// Plans on a blocking thread: the plan reads the whole folder tree and
    /// holds the index the intake shares.
    fn run_planning(&mut self) -> Task<Message> {
        let generation = self.reorganisation_generation;
        let Some(access) = self.filing.as_ref().and_then(FilingModule::plan_access) else {
            if let Some(view) = self.reorganisation.as_mut() {
                view.failed(INDEX_NOT_OPEN.to_owned());
            }
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || access.plan()),
            move |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                Message::ReorganisationPlan(ReorganisationPlanMessage::Loaded {
                    generation,
                    result,
                })
            },
        )
    }

    fn update_theme_fetch(&mut self, message: ThemeFetchMessage) -> Task<Message> {
        match message {
            ThemeFetchMessage::Open => self.open_theme_fetch(),
            ThemeFetchMessage::Close => self.close_theme_fetch(),
            ThemeFetchMessage::TextChanged(text) => {
                if let Some(card) = self.theme_fetch.as_mut() {
                    card.set_text(text);
                }
                Task::none()
            }
            ThemeFetchMessage::Activate => self.activate_theme_fetch(),
            ThemeFetchMessage::Move(delta) => {
                if let Some(card) = self.theme_fetch.as_mut() {
                    card.move_cursor(delta);
                }
                Task::none()
            }
            ThemeFetchMessage::Pick(index) => {
                if let Some(card) = self.theme_fetch.as_mut() {
                    card.select(index);
                }
                self.activate_theme_fetch()
            }
            ThemeFetchMessage::Fetched { generation, result } => {
                self.theme_fetched(generation, result)
            }
            ThemeFetchMessage::Installed { generation, result } => {
                self.theme_installed(generation, result)
            }
        }
    }

    fn open_theme_fetch(&mut self) -> Task<Message> {
        let focus = operation::focus(theme_fetch_ui::FIELD_ID);
        if self.theme_fetch.is_some() {
            return focus;
        }
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.theme_fetch_generation = self.theme_fetch_generation.wrapping_add(1);
        self.theme_fetch = Some(ThemeFetch::new());
        focus
    }

    /// Not while the theme is written: the config change would have no card
    /// left to report to.
    fn close_theme_fetch(&mut self) -> Task<Message> {
        if self
            .theme_fetch
            .as_ref()
            .is_some_and(|card| matches!(card.stage(), Stage::Installing))
        {
            return Task::none();
        }
        if self.theme_fetch.take().is_some() {
            release_focus()
        } else {
            Task::none()
        }
    }

    /// Enter or a click: fetches what was typed, or installs the chosen theme.
    fn activate_theme_fetch(&mut self) -> Task<Message> {
        let Some(card) = self.theme_fetch.as_mut() else {
            return Task::none();
        };
        match card.stage() {
            Stage::Input => match card.start() {
                Some(id) => self.start_theme_download(id),
                None => Task::none(),
            },
            Stage::Choosing { .. } => match card.choose() {
                Some(pick) => self.start_theme_install(pick),
                None => Task::none(),
            },
            Stage::Fetching | Stage::Installing => Task::none(),
        }
    }

    fn start_theme_download(&self, id: ExtensionId) -> Task<Message> {
        let generation = self.theme_fetch_generation;
        let fetch = self.fetch_extension;
        Task::perform(
            tokio::task::spawn_blocking(move || download_theme(fetch, &id)),
            move |joined| {
                let result =
                    joined.unwrap_or_else(|join| Err(FetchError::Network(join.to_string())));
                Message::ThemeFetch(ThemeFetchMessage::Fetched { generation, result })
            },
        )
    }

    fn start_theme_install(&mut self, pick: Pick) -> Task<Message> {
        let Some(config_path) = self.config_path.clone() else {
            return self.fail_theme_fetch(FetchError::NoConfigPath);
        };
        let generation = self.theme_fetch_generation;
        let install = self.install_theme;
        let failed_path = config_path.display().to_string();
        Task::perform(
            tokio::task::spawn_blocking(move || install(&pick, &config_path)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| {
                    Err(FetchError::WriteFailed {
                        path: failed_path,
                        reason: join.to_string(),
                    })
                });
                Message::ThemeFetch(ThemeFetchMessage::Installed { generation, result })
            },
        )
    }

    /// Back to the text field with the reason shown.
    fn fail_theme_fetch(&mut self, error: FetchError) -> Task<Message> {
        let Some(card) = self.theme_fetch.as_mut() else {
            return Task::none();
        };
        card.fail(error);
        operation::focus(theme_fetch_ui::FIELD_ID)
    }

    fn theme_fetched(
        &mut self,
        generation: u64,
        result: Result<crate::theme_fetch::Fetched, FetchError>,
    ) -> Task<Message> {
        if generation != self.theme_fetch_generation {
            return Task::none();
        }
        let Some(card) = self.theme_fetch.as_mut() else {
            return Task::none();
        };
        if !matches!(card.stage(), Stage::Fetching) {
            return Task::none();
        }
        match card.finish_fetching(result) {
            Some(pick) => self.start_theme_install(pick),
            None if matches!(card.stage(), Stage::Input) => {
                operation::focus(theme_fetch_ui::FIELD_ID)
            }
            None => Task::none(),
        }
    }

    fn theme_installed(
        &mut self,
        generation: u64,
        result: Result<Box<Installed>, FetchError>,
    ) -> Task<Message> {
        if generation != self.theme_fetch_generation || self.theme_fetch.is_none() {
            return Task::none();
        }
        let installed = match result {
            Ok(installed) => *installed,
            Err(error) => return self.fail_theme_fetch(error),
        };
        self.theme_fetch = None;
        let Installed {
            config,
            label,
            license,
        } = installed;
        self.palette = config.palette;
        // The warnings of the look before this one no longer apply.
        if matches!(
            self.config_problem,
            Some(ConfigProblem::LowContrast(_) | ConfigProblem::VscodeTheme(_))
        ) {
            self.config_problem = None;
            self.config_error = None;
        }
        self.report_look_warnings(config.contrast_warning, config.vscode_warning);
        let shown_license =
            license.unwrap_or_else(|| self.lang.text(Msg::ThemeFetchLicenseNotStated).to_owned());
        let notice = fill_template(
            self.lang.text(Msg::ThemeFetchInstalled),
            &[
                ("{label}", label.as_str()),
                ("{license}", shown_license.as_str()),
            ],
        );
        Task::batch([release_focus(), self.show_notice(notice)])
    }

    fn update_ocr_models(&mut self, message: OcrModelsMessage) -> Task<Message> {
        match message {
            OcrModelsMessage::Open => self.open_ocr_models(),
            OcrModelsMessage::Close => self.close_ocr_models(),
            OcrModelsMessage::Download => self.start_ocr_models_download(),
            OcrModelsMessage::Downloaded { generation, result } => {
                self.ocr_models_downloaded(generation, result)
            }
        }
    }

    /// Only asks. Nothing is sent before the download button is pressed.
    fn open_ocr_models(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.ocr_models_generation = self.ocr_models_generation.wrapping_add(1);
        self.dialog_focus = ocr_models_ui::CANCEL_FOCUS;
        let target = self
            .filing_index_path
            .as_deref()
            .and_then(crate::filing::ocr_models_dir);
        self.ocr_models = Some(OcrModelsCard::new(target));
        Task::none()
    }

    /// Not while the files are written: closing would leave a download nobody
    /// reports on.
    fn close_ocr_models(&mut self) -> Task<Message> {
        if self
            .ocr_models
            .as_ref()
            .is_some_and(OcrModelsCard::is_downloading)
        {
            return Task::none();
        }
        self.ocr_models = None;
        Task::none()
    }

    fn start_ocr_models_download(&mut self) -> Task<Message> {
        let Some(target) = self.ocr_models.as_mut().and_then(OcrModelsCard::start) else {
            return Task::none();
        };
        let generation = self.ocr_models_generation;
        let download = self.download_ocr_models;
        Task::perform(
            tokio::task::spawn_blocking(move || download(&target)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| {
                    Err(DownloadError::Network(FetchError::Network(
                        join.to_string(),
                    )))
                });
                Message::OcrModels(OcrModelsMessage::Downloaded { generation, result })
            },
        )
    }

    fn ocr_models_downloaded(
        &mut self,
        generation: u64,
        result: Result<(), DownloadError>,
    ) -> Task<Message> {
        if generation != self.ocr_models_generation {
            return Task::none();
        }
        let Some(card) = self.ocr_models.as_mut() else {
            return Task::none();
        };
        if !card.finish(result) {
            return Task::none();
        }
        self.ocr_models = None;
        let notice = self.lang.text(Msg::OcrModelsInstalled).to_owned();
        self.show_notice(notice)
    }

    fn update_ai_model(&mut self, message: AiModelMessage) -> Task<Message> {
        match message {
            AiModelMessage::Open => self.open_ai_model(),
            AiModelMessage::Close => self.close_ai_model(),
            AiModelMessage::Download => self.start_ai_model_download(),
            AiModelMessage::Stop => {
                if let Some(card) = self.ai_model.as_mut() {
                    card.stop();
                }
                Task::none()
            }
            AiModelMessage::Downloaded { generation, result } => {
                self.ai_model_downloaded(generation, result)
            }
        }
    }

    /// Only asks. Nothing is fetched before the download button is pressed.
    fn open_ai_model(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.ai_model_generation = self.ai_model_generation.wrapping_add(1);
        self.dialog_focus = ai_model_ui::CANCEL_FOCUS;
        let target = self
            .filing_index_path
            .as_deref()
            .and_then(crate::filing::ai_models_dir);
        let has_partial = target
            .as_deref()
            .is_some_and(|dir| ncrs_ai::has_partial_download(dir, &ncrs_ai::LOCAL_MODEL));
        self.ai_model = Some(AiModelCard::new(target, has_partial));
        Task::none()
    }

    /// Not while the file is written: stopping comes first, then closing.
    fn close_ai_model(&mut self) -> Task<Message> {
        if self
            .ai_model
            .as_ref()
            .is_some_and(AiModelCard::is_downloading)
        {
            return Task::none();
        }
        self.ai_model = None;
        Task::none()
    }

    /// The button next to Cancel: it stops a running download, otherwise it
    /// starts or continues one.
    fn press_ai_model_button(&mut self) -> Task<Message> {
        if self
            .ai_model
            .as_ref()
            .is_some_and(AiModelCard::is_downloading)
        {
            return self.update_ai_model(AiModelMessage::Stop);
        }
        self.start_ai_model_download()
    }

    fn start_ai_model_download(&mut self) -> Task<Message> {
        let Some(job) = self.ai_model.as_mut().and_then(AiModelCard::start) else {
            return Task::none();
        };
        let generation = self.ai_model_generation;
        let download = self.download_ai_model;
        Task::perform(
            tokio::task::spawn_blocking(move || download(&job)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| {
                    Err(AiDownloadError::Network(FetchError::Network(
                        join.to_string(),
                    )))
                });
                Message::AiModel(AiModelMessage::Downloaded { generation, result })
            },
        )
    }

    fn ai_model_downloaded(
        &mut self,
        generation: u64,
        result: Result<(), AiDownloadError>,
    ) -> Task<Message> {
        if generation != self.ai_model_generation {
            return Task::none();
        }
        let Some(card) = self.ai_model.as_mut() else {
            return Task::none();
        };
        if !card.finish(result) {
            return Task::none();
        }
        self.ai_model = None;
        if let Some(module) = self.filing.as_ref() {
            module.refresh_ai_model();
        }
        let notice = self.lang.text(Msg::AiModelInstalled).to_owned();
        self.show_notice(notice)
    }

    /// Cmd/Ctrl+Shift+P: lists the actions that work where the keys are now.
    /// Pressed while the box is up, it only gives the field the focus again.
    fn open_command_palette(&mut self) -> Task<Message> {
        let focus = operation::focus(command_palette_ui::COMMAND_PALETTE_ID);
        if self.command_palette.is_some() {
            return focus;
        }
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        let scope = if self.search_has_keys() {
            KeyScope::Search
        } else {
            KeyScope::Panel
        };
        let mut palette =
            CommandPalette::new(scope, self.lang, &self.keymap, &self.recent_commands);
        palette.set_visible_rows(command_palette_ui::visible_rows(self.window_size));
        self.command_palette = Some(palette);
        focus
    }

    fn resize_command_palette(&mut self, window: Size) {
        if let Some(palette) = self.command_palette.as_mut() {
            palette.set_visible_rows(command_palette_ui::visible_rows(window));
        }
    }

    fn close_command_palette(&mut self) -> Task<Message> {
        if self.command_palette.take().is_some() {
            release_focus()
        } else {
            Task::none()
        }
    }

    fn update_command_palette(&mut self, message: CommandPaletteMessage) -> Task<Message> {
        match message {
            CommandPaletteMessage::Toggle => return self.open_command_palette(),
            CommandPaletteMessage::Close => return self.close_command_palette(),
            _ => {}
        }
        let Some(palette) = self.command_palette.as_mut() else {
            return Task::none();
        };
        match message {
            CommandPaletteMessage::QueryChanged(text) => palette.set_query(text),
            CommandPaletteMessage::Move(delta) => palette.move_cursor(delta),
            CommandPaletteMessage::Select(index) => {
                palette.select(index);
                return self.run_command_palette();
            }
            CommandPaletteMessage::Run => return self.run_command_palette(),
            CommandPaletteMessage::Toggle | CommandPaletteMessage::Close => {}
        }
        Task::none()
    }

    /// Closes the palette, then does what its key would. The closing task
    /// comes first and is chained, not batched: a prompt the action opens
    /// takes the focus after the palette gave it up.
    fn run_command_palette(&mut self) -> Task<Message> {
        let Some(action) = self
            .command_palette
            .as_ref()
            .and_then(CommandPalette::selected_action)
        else {
            return Task::none();
        };
        let closed = self.close_command_palette();
        closed.chain(self.run_command(action))
    }

    /// Runs a command chosen from the command list or the menu bar: what its
    /// key would do, and it counts among the recent commands.
    fn run_command(&mut self, action: KeyAction) -> Task<Message> {
        self.remember_command(action);
        self.update(action.message())
    }

    /// Puts `action` first among the recent commands, once.
    fn remember_command(&mut self, action: KeyAction) {
        self.recent_commands.retain(|&recent| recent != action);
        self.recent_commands.insert(0, action);
        self.recent_commands.truncate(RECENT_LIMIT);
    }

    /// Closes the box and stops its walk.
    fn close_quick_open(&mut self) -> Task<Message> {
        self.quick_open_walk = None;
        if self.quick_open.take().is_some() {
            release_focus()
        } else {
            Task::none()
        }
    }

    fn update_quick_open(&mut self, message: QuickOpenMessage) -> Task<Message> {
        match message {
            QuickOpenMessage::Open => return self.open_quick_open(),
            QuickOpenMessage::Close => return self.close_quick_open(),
            _ => {}
        }
        let Some(quick_open) = self.quick_open.as_mut() else {
            return Task::none();
        };
        match message {
            QuickOpenMessage::QueryChanged(text) => match quick_open.set_query(text) {
                Some(ranking) => Task::perform(ranking.run_in_background(), std::convert::identity),
                None => Task::none(),
            },
            QuickOpenMessage::Events { generation, events } => {
                if generation == quick_open.walk_generation() {
                    quick_open.apply_events(events);
                    if !quick_open.walking() {
                        self.quick_open_walk = None;
                    }
                }
                Task::none()
            }
            QuickOpenMessage::Ranked {
                walk_generation,
                generation,
                hits,
                chunks_scored,
            } => {
                quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
                Task::none()
            }
            QuickOpenMessage::Move(delta) => {
                quick_open.move_cursor(delta);
                Task::none()
            }
            QuickOpenMessage::Activate => self.activate_quick_open(),
            QuickOpenMessage::RowActivated(index) => {
                quick_open.select(index);
                self.activate_quick_open()
            }
            QuickOpenMessage::Open | QuickOpenMessage::Close => Task::none(),
        }
    }

    /// Enter on a hit: the active panel shows the file's directory with the
    /// cursor on the file.
    fn activate_quick_open(&mut self) -> Task<Message> {
        // Typed fast, Enter before the background ranking is back.
        if let Some(quick_open) = self.quick_open.as_mut() {
            quick_open.rank_now();
        }
        let Some(path) = self.quick_open.as_ref().and_then(QuickOpen::selected_path) else {
            return Task::none();
        };
        let closed = self.close_quick_open();
        let revealed = self.reveal_file(&path);
        Task::batch([closed, revealed])
    }

    /// The active panel shows the file's directory with the cursor on the file.
    /// Only navigates: the file is never opened or run.
    fn reveal_file(&mut self, path: &Path) -> Task<Message> {
        let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
            return Task::none();
        };
        let directory = parent.to_path_buf();
        let name = file.to_string_lossy().into_owned();
        self.load(self.active_panel, directory, Some(name))
    }

    fn title_bar_view(&self) -> Element<'_, Message> {
        let tab = |side: PanelSide| {
            let title = match self.search.as_ref() {
                Some(search) if self.search_open && side == self.search_side => {
                    titlebar::TabTitle::Search(search_ui::title(search, self.lang))
                }
                _ => titlebar::TabTitle::Path {
                    path: &self.panel(side).path,
                    loading: self.panel(side).loading,
                },
            };
            titlebar::Tab {
                side,
                title,
                active: self.active_panel == side,
            }
        };
        titlebar::view(&titlebar::TitleBar {
            tabs: [tab(PanelSide::Left), tab(PanelSide::Right)],
            home: &self.home,
            hermes: self.hermes_chip(),
            lab_shortcut: self
                .keymap
                .shortcut_label(&KeyAction::CommandPalette.message()),
            window_width: self.window_size.width,
            lang: self.lang,
            palette: &self.palette,
        })
    }

    /// The title bar chip: Hermes is on, and how fast it files. Absent while
    /// the module is off.
    fn hermes_chip(&self) -> Option<String> {
        let snapshot = self.filing_status.snapshot();
        let text = match snapshot.state {
            RunnerState::Off => return None,
            RunnerState::Paused => self.lang.text(Msg::TitleBarHermesPaused).to_owned(),
            RunnerState::Running | RunnerState::Idle => {
                match self.filing_status.documents_per_minute() {
                    Some(rate) => fill_template(
                        self.lang.text(Msg::TitleBarHermesRate),
                        &[("{rate}", &rate.to_string())],
                    ),
                    None => self.lang.text(Msg::ActionFilingSetupTheme).to_owned(),
                }
            }
        };
        Some(text)
    }

    fn filing_badge(&self) -> Option<statusbar::FilingBadge<Message>> {
        let progress = self.filing_status.progress()?;
        let template = if progress.paused {
            Msg::FilingProgressPaused
        } else {
            Msg::FilingProgress
        };
        Some(statusbar::FilingBadge {
            text: fill_template(
                self.lang.text(template),
                &[
                    ("{done}", &progress.done.to_string()),
                    ("{total}", &progress.total.to_string()),
                ],
            ),
            on_press: Message::FilingStatus(FilingStatusMessage::Open),
        })
    }

    fn refresh_filing_snapshot(&mut self) {
        let snapshot = self.pipeline.as_ref().map_or_else(
            ncrs_pipeline::Snapshot::off,
            ncrs_pipeline::Runner::snapshot,
        );
        self.filing_status
            .set_snapshot(snapshot, self.wall_clock.now());
        self.filing_status.set_ai(
            self.filing
                .as_ref()
                .map(FilingModule::ai_status)
                .unwrap_or_default(),
        );
    }

    /// The pipeline, unless it is stopped.
    fn live_pipeline(&self) -> Option<&ncrs_pipeline::Runner> {
        self.pipeline
            .as_ref()
            .filter(|runner| runner.snapshot().state != ncrs_pipeline::RunnerState::Off)
    }

    /// Reads the figures on the blocking pool: the intake may hold the index
    /// for as long as it hashes a large file.
    fn request_index_figures(&mut self) -> Task<Message> {
        let Some(reader) = self
            .filing
            .as_ref()
            .and_then(FilingModule::index_figures_reader)
        else {
            return Task::none();
        };
        self.filing_status.figures_requested();
        Task::perform(
            tokio::task::spawn_blocking(move || reader.read()),
            |joined| {
                Message::FilingStatus(FilingStatusMessage::IndexFigures(
                    joined.unwrap_or_else(|err| Err(err.to_string())),
                ))
            },
        )
    }

    fn open_filing_status(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.context_menu = None;
        self.filing_status.open_page();
        self.refresh_filing_snapshot();
        self.request_index_figures()
    }

    fn set_pipeline_load(&mut self, load: Load) {
        let Some(runner) = self.live_pipeline() else {
            return;
        };
        runner.set_load(load);
        if let Some(module) = self.filing.as_mut() {
            module.set_load(load);
        }
        self.refresh_filing_snapshot();
    }

    fn update_filing_status(&mut self, message: FilingStatusMessage) -> Task<Message> {
        match message {
            FilingStatusMessage::Open => self.open_filing_status(),
            FilingStatusMessage::Close => {
                self.filing_status.close_page();
                Task::none()
            }
            FilingStatusMessage::Toggle => {
                if self.filing_status.page_open() {
                    self.filing_status.close_page();
                    Task::none()
                } else {
                    self.open_filing_status()
                }
            }
            FilingStatusMessage::Tick => {
                self.refresh_filing_snapshot();
                let plans_made = self
                    .live_pipeline()
                    .map_or(0, |runner| runner.snapshot().plans_made);
                let filing = if plans_made > self.plans_filed_for || self.filing_due {
                    self.plans_filed_for = plans_made;
                    self.run_filing()
                } else {
                    Task::none()
                };
                if self.filing_status.tick_wants_figures() {
                    Task::batch([filing, self.request_index_figures()])
                } else {
                    filing
                }
            }
            FilingStatusMessage::TogglePause => {
                if let Some(runner) = self.live_pipeline() {
                    match runner.snapshot().state {
                        ncrs_pipeline::RunnerState::Paused => runner.resume(),
                        ncrs_pipeline::RunnerState::Running | ncrs_pipeline::RunnerState::Idle => {
                            runner.pause();
                        }
                        ncrs_pipeline::RunnerState::Off => {}
                    }
                    self.refresh_filing_snapshot();
                }
                Task::none()
            }
            FilingStatusMessage::SetLoad(load) => {
                self.set_pipeline_load(load);
                Task::none()
            }
            FilingStatusMessage::StepWorkers(delta) => {
                let Some(runner) = self.live_pipeline() else {
                    return Task::none();
                };
                let current = match self.filing.as_ref().map(FilingModule::load) {
                    Some(Load::Workers(count)) => count.get(),
                    _ => runner.snapshot().worker_target,
                };
                let most = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
                let wanted = current
                    .saturating_add_signed(isize::from(delta))
                    .clamp(1, most);
                if let Some(count) = NonZeroUsize::new(wanted) {
                    self.set_pipeline_load(Load::Workers(count));
                }
                Task::none()
            }
            FilingStatusMessage::IndexFigures(figures) => {
                self.filing_status.figures_arrived(figures);
                Task::none()
            }
            FilingStatusMessage::ErrorActivated(path) => {
                self.filing_status.close_page();
                self.reveal_file(&path)
            }
            FilingStatusMessage::MoveError(delta) => {
                self.filing_status.move_error_selection(delta);
                Task::none()
            }
            FilingStatusMessage::ActivateSelectedError => {
                let Some(path) = self
                    .filing_status
                    .selected_error_file()
                    .map(Path::to_path_buf)
                else {
                    return Task::none();
                };
                self.filing_status.close_page();
                self.reveal_file(&path)
            }
        }
    }

    /// Starts an async directory read for `side`.
    fn load(&mut self, side: PanelSide, path: PathBuf, select: Option<String>) -> Task<Message> {
        let panel = self.panel_mut(side);
        let request_id = panel.begin_load();
        Task::perform(fs::read_directory(path, panel.sort), move |listing| {
            Message::DirectoryLoaded {
                side,
                request_id,
                path: listing.path,
                entries: listing.entries,
                error: listing.error,
                sorted_by: listing.sorted_by,
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
        let mut app = App::with_environment(Environment::isolated()).0;
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
mod perf_numbers {
    //! Numbers for comparing ncrs with other file managers, printed as a
    //! Markdown table. Not a pass/fail check, so it is ignored by default and
    //! run with `cargo test --release --bin ncrs perf_numbers -- --ignored
    //! --nocapture`.
    //!
    //! The directories are created right before they are read, so the page
    //! cache is warm. "Build view" is the widget tree `view` returns on every
    //! keystroke; rasterising it on the CPU or GPU happens later in iced and
    //! is not part of any number here.

    use super::*;
    use crate::fs::{read_directory, sort_entries, FileEntry, SortColumn, SortKey};
    use std::time::{Duration, Instant};

    const ENTRY_COUNTS: [usize; 3] = [1_000, 10_000, 100_000];
    const MIDDLE_RUN: usize = 3;
    const RUNS: usize = 2 * MIDDLE_RUN + 1;
    const STEPS: u32 = 1_000;

    fn median(mut samples: Vec<Duration>) -> Duration {
        assert_eq!(samples.len(), RUNS);
        samples.sort();
        samples[MIDDLE_RUN]
    }

    /// Microseconds below a millisecond, so a 30 µs view does not print as
    /// "0.03 ms" next to a 266 ms read.
    fn shown(duration: Duration) -> String {
        let micros = duration.as_secs_f64() * 1e6;
        if micros < 1000.0 {
            format!("{micros:.1} µs")
        } else {
            format!("{:.2} ms", micros / 1000.0)
        }
    }

    /// Median of `RUNS` timings of `operation` after one warm-up. `prepare`
    /// and dropping the result stay outside the clock, so copying a 100k
    /// listing is not counted as sorting it.
    fn median_time<I, O>(
        mut prepare: impl FnMut() -> I,
        mut operation: impl FnMut(I) -> O,
    ) -> Duration {
        drop(operation(prepare()));
        let samples = std::iter::repeat_with(|| {
            let input = prepare();
            let start = Instant::now();
            let output = operation(input);
            let elapsed = start.elapsed();
            drop(output);
            elapsed
        })
        .take(RUNS)
        .collect();
        median(samples)
    }

    /// One directory in twenty, files of up to 4 KiB.
    fn populated_dir(entry_count: usize) -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("ncrs-perf-")
            .tempdir()
            .unwrap();
        for i in 0..entry_count {
            if i % 20 == 0 {
                std::fs::create_dir(dir.path().join(format!("dir_{i:06}"))).unwrap();
            } else {
                let content = vec![b'x'; i % 4096];
                std::fs::write(dir.path().join(format!("File_{i:06}.txt")), content).unwrap();
            }
        }
        dir
    }

    async fn read_median(path: &std::path::Path, entry_count: usize) -> (Duration, Vec<FileEntry>) {
        let mut samples = Vec::with_capacity(RUNS + 1);
        let mut entries = Vec::new();
        for _ in 0..=RUNS {
            let start = Instant::now();
            let listing = read_directory(path.to_path_buf(), SortKey::default()).await;
            samples.push(start.elapsed());
            assert!(listing.error.is_none(), "{:?}", listing.error);
            assert_eq!(listing.entries.len(), entry_count + 1);
            entries = listing.entries;
        }
        samples.remove(0);
        (median(samples), entries)
    }

    /// What the panel does when a listing arrives for a directory it was not
    /// showing before.
    fn enter(app: &mut App, path: &std::path::Path, entries: Vec<FileEntry>) {
        app.panel_mut(PanelSide::Left).path = PathBuf::from("/");
        let request_id = app.panel_mut(PanelSide::Left).begin_load();
        drop(app.update(Message::DirectoryLoaded {
            side: PanelSide::Left,
            request_id,
            path: path.to_path_buf(),
            entries,
            error: None,
            sorted_by: SortKey::default(),
            select: None,
        }));
    }

    fn per_step(app: &mut App, message: &Message) -> Duration {
        let start = Instant::now();
        for _ in 0..STEPS {
            drop(app.update(message.clone()));
            drop(app.view());
        }
        start.elapsed() / STEPS
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "prints timings, run with --release --ignored --nocapture"]
    async fn print_performance_table() {
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        println!(
            "\n{} {}, {threads} threads, release build: {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH,
            !cfg!(debug_assertions)
        );
        println!(
            "| Entries | Open directory | Sort by name / size / date | Hand to panel \
             | Build view (top / end) | ↓ + view | PgDn + view | End + view |"
        );
        println!("|---:|---:|---:|---:|---:|---:|---:|---:|");

        for entry_count in ENTRY_COUNTS {
            let dir = populated_dir(entry_count);
            let path = dir.path();
            let (open, entries) = read_median(path, entry_count).await;

            let sort_by = |column| {
                median_time(
                    || entries.clone(),
                    |mut listing| {
                        sort_entries(
                            &mut listing,
                            SortKey {
                                column,
                                descending: false,
                            },
                        );
                        listing
                    },
                )
            };
            let by_name = sort_by(SortColumn::Name);
            let by_size = sort_by(SortColumn::Size);
            let by_date = sort_by(SortColumn::Modified);

            let mut app = App::with_environment(Environment::isolated()).0;
            app.visible_rows = 35;
            let hand_to_panel =
                median_time(|| entries.clone(), |listing| enter(&mut app, path, listing));
            let view_top = median_time(|| (), |()| drop(app.view()));
            let arrow = per_step(&mut app, &Message::MoveSelection(1));
            let page_down = per_step(&mut app, &Message::PageDown);
            let end = per_step(&mut app, &Message::SelectLast);
            let view_end = median_time(|| (), |()| drop(app.view()));

            println!(
                "| {entry_count} | {} | {} / {} / {} | {} | {} / {} | {} | {} | {} |",
                shown(open),
                shown(by_name),
                shown(by_size),
                shown(by_date),
                shown(hand_to_panel),
                shown(view_top),
                shown(view_end),
                shown(arrow),
                shown(page_down),
                shown(end),
            );
        }
        println!(
            "\nOpen directory: readdir, one stat per entry and the sort by name, warm \
             page cache. Build view: the widget tree only, without rasterising.\n"
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
        App::with_environment(Environment::isolated()).0
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
            Keymap::built_in().function_keys(Language::English, KeyScope::Panel)[6].is_some(),
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

        let mut app = App::with_environment(Environment::isolated()).0;
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

        let mut app = App::with_environment(Environment::isolated()).0;
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

        let mut app = App::with_environment(Environment::isolated()).0;
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
        let mut app = App::with_environment(Environment::isolated()).0;
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

/// What the snapshot of the search view shows: a plain file and a source file
/// with two hits.
#[cfg(test)]
fn fixed_search_results(root: &Path) -> Vec<(PathBuf, Vec<crate::fs::search::LineMatch>)> {
    use crate::fs::search::LineMatch;
    let hit = |number: u64, text: &str, from: usize, to: usize| LineMatch {
        number,
        text: text.to_string(),
        ranges: std::iter::once(from..to).collect(),
    };
    vec![
        (
            root.join("notes.txt"),
            vec![hit(3, "say hello to the world", 4, 9)],
        ),
        (
            root.join("src/main.rs"),
            vec![
                hit(1, "// hello world example", 3, 8),
                hit(12, "println!(\"hello\");", 10, 15),
            ],
        ),
    ]
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
            function_keys: Keymap::built_in().function_keys(Language::default(), KeyScope::Panel),
            search_function_keys: Keymap::built_in()
                .function_keys(Language::default(), KeyScope::Search),
            keymap: Keymap::built_in(),
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
            open_programs: config::OpenPrograms::default(),
            config_error: None,
            config_problem: None,
            palette: Palette::default(),
            job_error: None,
            notice: None,
            notice_generation: 0,
            job_done: 0,
            conflict_all: false,
            conflict_rule: None,
            generation: 0,
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            volume_menu: None,
            context_menu: None,
            type_ahead: TypeAhead::default(),
            path_field: None,
            path_check_id: 0,
            home: PathBuf::from("/home/test"),
            clock: Arc::new(std::time::Instant::now),
            window_size: layout::INITIAL_WINDOW_SIZE,
            input: Arc::default(),
            list_volumes: Arc::new(fs::list_volumes),
            search: None,
            search_open: false,
            search_side: PanelSide::Left,
            search_rows: layout::search_visible_rows(layout::INITIAL_WINDOW_SIZE),
            search_generation: 0,
            search_handle: None,
            quick_open: None,
            quick_open_walk_generation: 0,
            quick_open_walk: None,
            command_palette: None,
            recent_commands: Vec::new(),
            filing: None,
            pipeline: None,
            filing_status: FilingStatus::default(),
            wall_clock: Arc::new(ncrs_pipeline::SystemClock),
            config_path: None,
            filing_config: Filing::default(),
            filing_index_path: None,
            filing_key_store: None,
            filing_setup: None,
            resolve_folder: |folder| Ok(folder.to_path_buf()),
            pick_folder: |_| Task::none(),
            theme_fetch: None,
            theme_fetch_generation: 0,
            rule_suggestions: None,
            rule_suggestions_generation: 0,
            review_list: None,
            review_list_generation: 0,
            reorganisation: None,
            reorganisation_generation: 0,
            plans_filed_for: 0,
            filing_due: false,
            fetch_extension: |_| Err(FetchError::NoConnection("no network in tests".to_owned())),
            install_theme: |_, _| Err(FetchError::NoConfigPath),
            ocr_models: None,
            ocr_models_generation: 0,
            download_ocr_models: |_| Err(DownloadError::NoDataFolder),
            ai_model: None,
            ai_model_generation: 0,
            ai_model_asked: false,
            download_ai_model: |_| Err(AiDownloadError::NoDataFolder),
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

    /// Fixed panels with the filing card of `phase` up. The module is not
    /// running: no work, an in-memory store, no file anywhere.
    pub fn with_fixed_filing(phase: Phase) -> Self {
        let mut app = Self::with_fixed_panels();
        app.filing = Some(FilingModule::in_phase(phase));
        app
    }

    /// Fixed panels with the filing setup up, holding `chosen`. With
    /// `index_is_plain` a module stands behind it that opened an unencrypted
    /// index.
    pub fn with_filing_setup(chosen: &[config::FilingLocation], index_is_plain: bool) -> Self {
        let mut app = Self::with_fixed_panels();
        app.filing_setup = Some(FilingSetup::new(chosen));
        if index_is_plain {
            app.filing = Some(FilingModule::in_phase_with_index(Phase::Ready, false));
        }
        app
    }

    /// The same app, worded in `lang`.
    pub fn in_language(mut self, lang: Language) -> Self {
        self.lang = lang;
        self.refresh_function_keys();
        self
    }

    /// Fixed panels that have been shown `snapshot` of a pipeline at
    /// `taken_at`; no runner exists.
    pub fn with_filing_snapshot(
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_fixed_panels();
        app.filing_status.set_snapshot(snapshot, taken_at);
        app
    }

    /// Fixed panels that saw `began` and then `now`, so progress has moved.
    pub fn with_filing_progress(
        began: ncrs_pipeline::Snapshot,
        now: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_filing_snapshot(began, taken_at);
        app.filing_status.set_snapshot(now, taken_at);
        app
    }

    /// Like [`Self::with_filing_snapshot`], with the status page open.
    pub fn with_filing_status_page(
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_filing_snapshot(snapshot, taken_at);
        app.filing_status.open_page();
        app
    }

    /// Like [`Self::with_filing_status_page`], with the index open and
    /// `figures` read from it.
    pub fn with_filing_inspection(
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
        figures: crate::filing::IndexFigures,
    ) -> Self {
        let mut app = Self::with_filing_status_page(snapshot, taken_at);
        app.filing = Some(FilingModule::in_phase(Phase::Ready));
        app.filing_status.figures_arrived(Ok(figures));
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
    /// An app with the search view up on fixed results, for the snapshot.
    pub fn with_fixed_search() -> Self {
        let mut app = Self::with_fixed_panels();
        let root = app.panel(PanelSide::Left).path.clone();
        app.show_search_results_for_test("hello", fixed_search_results(&root));
        // One tagged file next to the cursor, so the reference shows the tint.
        if let Some(view) = app.search.as_mut() {
            view.tagged.insert(root.join("notes.txt"));
        }
        app
    }

    /// Opens the search view the way Alt+F7 does and fills it with `files`, as
    /// a finished search for `query` would, with the cursor on the second row.
    pub fn show_search_results_for_test(
        &mut self,
        query: &str,
        files: Vec<(PathBuf, Vec<crate::fs::search::LineMatch>)>,
    ) {
        use crate::fs::search::{SearchEvent, SearchTotals};
        drop(self.update(Message::OpenSearch));
        let totals = SearchTotals {
            scanned: 14,
            files: files.len(),
            matches: files.iter().map(|(_, lines)| lines.len()).sum(),
            skipped: 0,
            truncated: false,
        };
        if let Some(view) = self.search.as_mut() {
            view.query = query.to_string();
            view.begin(1);
            for (path, lines) in files {
                view.apply_event(SearchEvent::File { path, lines });
            }
            view.apply_event(SearchEvent::Finished(totals));
            view.select_row(2, self.search_rows);
        }
    }

    pub fn search_for_test(&self) -> Option<&SearchView> {
        self.search.as_ref()
    }

    pub fn search_is_open_for_test(&self) -> bool {
        self.search_open
    }

    pub fn command_palette_for_test(&self) -> Option<&CommandPalette> {
        self.command_palette.as_ref()
    }

    pub fn theme_fetch_for_test(&self) -> Option<&ThemeFetch> {
        self.theme_fetch.as_ref()
    }

    pub fn ocr_models_for_test(&self) -> Option<&OcrModelsCard> {
        self.ocr_models.as_ref()
    }

    /// Fixed panels with the text recognition models card up.
    pub fn with_ocr_models_card(card: OcrModelsCard) -> Self {
        let mut app = Self::with_fixed_panels();
        app.dialog_focus = ocr_models_ui::CANCEL_FOCUS;
        app.ocr_models = Some(card);
        app
    }

    pub fn ai_model_for_test(&self) -> Option<&AiModelCard> {
        self.ai_model.as_ref()
    }

    /// Fixed panels with the AI model card up.
    pub fn with_ai_model_card(card: AiModelCard) -> Self {
        let mut app = Self::with_fixed_panels();
        app.dialog_focus = ai_model_ui::CANCEL_FOCUS;
        app.ai_model = Some(card);
        app
    }

    /// The status page open, with the AI line in the state of `ai`.
    pub fn with_filing_status_ai(ai: crate::filing::AiStatus) -> Self {
        let mut app = Self::with_filing_status_page(
            ncrs_pipeline::Snapshot::off(),
            std::time::SystemTime::UNIX_EPOCH,
        );
        app.filing_status.set_ai(ai);
        app
    }

    /// Fixed panels with the theme download card up, in the state of `card`.
    pub fn with_theme_fetch_card(card: ThemeFetch) -> Self {
        let mut app = Self::with_fixed_panels();
        app.theme_fetch = Some(card);
        app
    }

    pub fn rule_suggestions_for_test(&self) -> Option<&RuleSuggestions> {
        self.rule_suggestions.as_ref()
    }

    /// Fixed panels with the rule suggestions view up, in the state of `view`.
    pub fn with_rule_suggestions(view: RuleSuggestions) -> Self {
        let mut app = Self::with_fixed_panels();
        app.rule_suggestions = Some(view);
        app
    }

    pub fn review_list_for_test(&self) -> Option<&ReviewList> {
        self.review_list.as_ref()
    }

    /// Fixed panels with the review list up, in the state of `view`.
    pub fn with_review_list(view: ReviewList) -> Self {
        let mut app = Self::with_fixed_panels();
        app.review_list = Some(view);
        app
    }

    pub fn reorganisation_for_test(&self) -> Option<&ReorganisationView> {
        self.reorganisation.as_ref()
    }

    /// Fixed panels with the reorganisation plan view up, in the state of `view`.
    pub fn with_reorganisation_plan(view: ReorganisationView) -> Self {
        let mut app = Self::with_fixed_panels();
        app.reorganisation = Some(view);
        app
    }

    pub fn quick_open_for_test(&self) -> Option<&QuickOpen> {
        self.quick_open.as_ref()
    }

    /// Opens Quick Open as Cmd/Ctrl+P does and fills it as a finished walk
    /// over `files` (relative to the active panel's directory) would, ranked
    /// for `query` at once.
    pub fn open_quick_open_for_test(&mut self, files: &[&str], query: &str) {
        use crate::fs::search::{SearchEvent, SearchTotals};
        drop(self.update(Message::QuickOpen(QuickOpenMessage::Open)));
        let Some(quick_open) = self.quick_open.as_mut() else {
            return;
        };
        let mut events: Vec<SearchEvent> = files
            .iter()
            .map(|file| SearchEvent::File {
                path: quick_open.root().join(file),
                lines: Vec::new(),
            })
            .collect();
        events.push(SearchEvent::Finished(SearchTotals {
            scanned: files.len(),
            files: files.len(),
            matches: files.len(),
            skipped: 0,
            truncated: false,
        }));
        quick_open.apply_events(events);
        if let Some(ranking) = quick_open.set_query(query.to_string()) {
            if let QuickOpenMessage::Ranked {
                walk_generation,
                generation,
                hits,
                chunks_scored,
            } = ranking.run()
            {
                quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
            }
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

    /// Replaces the clock type-ahead reads.
    pub fn path_field_for_test(&self) -> Option<&str> {
        self.path_field.as_deref()
    }

    /// A config warning in the status bar, for the snapshot tests.
    pub fn with_config_warning(mut self, text: &str) -> Self {
        self.config_error = Some(text.to_owned());
        self
    }

    /// The status bar's notice, for the snapshot tests.
    pub fn notice_for_test(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn with_clock(
        mut self,
        clock: impl Fn() -> std::time::Instant + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
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

    /// Whether the context menu is up.
    pub fn context_menu_is_open(&self) -> bool {
        self.context_menu.is_some()
    }

    /// The entry Enter would run in the open context menu.
    pub fn context_menu_selected_action(&self) -> Option<ContextAction> {
        self.context_menu
            .as_ref()
            .and_then(ContextMenu::selected_action)
    }

    pub fn context_menu_for_test(&self) -> Option<&ContextMenu> {
        self.context_menu.as_ref()
    }

    /// Where the pointer is for the next right click, as the raw event
    /// subscription would have recorded it.
    pub fn set_pointer_for_test(&self, position: Point) {
        self.input.lock().pointer = position;
    }

    pub fn set_modifiers_for_test(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn filing_status_page_is_open_for_test(&self) -> bool {
        self.filing_status.page_open()
    }

    pub fn active_panel_side_for_test(&self) -> PanelSide {
        self.active_panel
    }

    /// Replaces the trash, so a test never fills the developer's real one.
    pub fn with_trash(mut self, trash: Arc<dyn fs::Trash>) -> Self {
        self.trash = trash;
        self
    }

    /// Puts the delete dialog in front of the user for `count` fixed entries,
    /// for the snapshot tests.
    pub fn open_delete_dialog_for_test(&mut self, count: usize, permanent: bool) {
        self.dialog_focus = delete_dialog_view::default_focus(permanent);
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

    /// The config error alone, for the end-to-end tests.
    pub fn config_error_for_test(&self) -> Option<&str> {
        self.config_error.as_deref()
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
        let mut app = App::with_environment(Environment::isolated()).0;
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
        let mut app = App::with_environment(Environment::isolated()).0;
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

    fn reload(p: &mut PanelState, names: &[&str]) {
        p.apply_listing(
            PathBuf::from("/left"),
            panel_with(names).entries,
            None,
            None,
            10,
        );
    }

    fn name_under_cursor(p: &PanelState) -> String {
        p.entries[p.selected].name.to_string_lossy().into_owned()
    }

    #[test]
    fn deleting_the_middle_file_puts_the_cursor_on_the_next_one() {
        let mut p = panel_with(&["a", "b", "c", "d", "e"]);
        p.selected = 2;
        reload(&mut p, &["a", "b", "d", "e"]);
        assert_eq!(name_under_cursor(&p), "d");
    }

    #[test]
    fn deleting_the_last_file_puts_the_cursor_on_the_new_last() {
        let mut p = panel_with(&["a", "b", "c", "d", "e"]);
        p.selected = 4;
        reload(&mut p, &["a", "b", "c", "d"]);
        assert_eq!(name_under_cursor(&p), "d");
    }

    #[test]
    fn a_refresh_without_changes_keeps_cursor_and_scroll() {
        let names: Vec<String> = (0..30).map(|n| format!("f{n:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut p = panel_with(&refs);
        p.selected = 12;
        p.scroll_offset = 8;
        reload(&mut p, &refs);
        assert_eq!(name_under_cursor(&p), "f12");
        assert_eq!(p.scroll_offset, 8);
    }

    #[test]
    fn the_other_panel_on_the_same_directory_does_not_jump_to_the_top() {
        let mut active = panel_with(&["a", "b", "c", "d", "e"]);
        let mut other = panel_with(&["a", "b", "c", "d", "e"]);
        active.selected = 1;
        other.selected = 3;
        reload(&mut active, &["a", "c", "d", "e"]);
        reload(&mut other, &["a", "c", "d", "e"]);
        assert_eq!(name_under_cursor(&active), "c");
        assert_eq!(name_under_cursor(&other), "d");
    }

    #[test]
    fn another_directory_starts_at_the_top() {
        let mut p = panel_with(&["a", "b", "c"]);
        p.selected = 2;
        p.apply_listing(
            PathBuf::from("/elsewhere"),
            panel_with(&["x", "y", "z"]).entries,
            None,
            None,
            10,
        );
        assert_eq!(p.selected, 0);
        assert_eq!(p.scroll_offset, 0);
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
        let mut app = App::with_environment(Environment::isolated()).0;
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

        let mut app = App::with_environment(Environment::isolated()).0;
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
        let mut app = App::with_environment(Environment::isolated()).0;
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

        let mut app = App::with_environment(Environment::isolated()).0;
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
    fn escape_leaves_a_filing_card_that_may_be_left() {
        let card = PromptKeyState {
            filing_card: true,
            ..PromptKeyState::default()
        };
        assert_eq!(
            route(&card, Named::Escape, Modifiers::default()),
            Some(Message::Filing(FilingMessage::Dismiss))
        );
        assert_eq!(
            route(
                &PromptKeyState::default(),
                Named::Escape,
                Modifiers::default()
            ),
            None
        );
    }

    #[test]
    fn escape_closes_the_filing_setup_and_leaves_a_running_job_alone() {
        let setup_over_job = PromptKeyState {
            filing_setup: true,
            job_running: true,
            ..PromptKeyState::default()
        };
        assert_eq!(
            route(&setup_over_job, Named::Escape, Modifiers::default()),
            Some(Message::FilingSetup(FilingSetupMessage::Close))
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
            app.dialog_focus,
            delete_dialog_view::CANCEL_FOCUS,
            "the permanent dialog must start on Cancel"
        );

        let _wrap = app.update(Message::DialogFocus(1));
        assert_eq!(app.dialog_focus, delete_dialog_view::CONFIRM_FOCUS);
        let _back = app.update(Message::DialogFocus(-1));
        assert_eq!(app.dialog_focus, delete_dialog_view::CANCEL_FOCUS);
        let _cancel = app.update(Message::DialogActivate);
        assert!(
            app.delete_dialog.is_none(),
            "Enter on Cancel kept the dialog"
        );

        let _again = app.update(Message::Delete { permanent: false });
        assert_eq!(
            app.dialog_focus,
            delete_dialog_view::CONFIRM_FOCUS,
            "the trash dialog starts on Delete"
        );
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
        let mut app = App::with_environment(Environment::isolated()).0;
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
    fn the_sort_chord_is_routed_and_not_taken_for_type_ahead() {
        let binding = keymap::sort_binding(crate::fs::SortColumn::Size);
        assert_eq!(
            route_key(&PromptKeyState::default(), binding.key, binding.modifiers),
            Some(Message::SortActive(crate::fs::SortColumn::Size))
        );
    }

    #[test]
    fn alt_ctrl_and_super_do_not_fall_back_to_the_bare_key() {
        let idle = PromptKeyState::default();
        for modifiers in [Modifiers::ALT, Modifiers::CTRL, Modifiers::LOGO] {
            assert_eq!(route(&idle, Named::Enter, modifiers), None, "{modifiers:?}");
            assert_eq!(route(&idle, Named::F8, modifiers), None, "{modifiers:?}");
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
        assert_eq!(
            route(Named::F1, Modifiers::default()),
            Some(Message::CommandPalette(CommandPaletteMessage::Toggle))
        );
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

#[cfg(test)]
mod copy_to_clipboard_tests {
    use super::*;
    use crate::fs::FileEntry;
    use iced::keyboard::key::{Code, Physical};
    use std::ffi::OsString;

    fn entry(name: &str, is_parent: bool) -> FileEntry {
        FileEntry {
            name: OsString::from(name),
            path: PathBuf::from("/left").join(name),
            is_dir: false,
            is_symlink: false,
            is_parent,
            size: 0,
            modified: None,
        }
    }

    fn panel() -> PanelState {
        let mut panel = PanelState::new(PathBuf::from("/left/dir"));
        panel.entries = vec![
            entry("..", true),
            entry("zulu.txt", false),
            entry("alpha.txt", false),
        ];
        panel
    }

    fn text(panel: &PanelState, kind: ClipboardKind) -> Option<String> {
        clipboard_text(panel, kind).map(|copied| copied.text)
    }

    /// Joined as the code joins, so Windows gets its own separator.
    fn in_dir(name: &str) -> String {
        PathBuf::from("/left/dir").join(name).display().to_string()
    }

    #[test]
    fn the_cursor_row_is_copied_as_path_or_name() {
        let mut panel = panel();
        panel.selected = 1;
        assert_eq!(text(&panel, ClipboardKind::Path), Some(in_dir("zulu.txt")));
        assert_eq!(
            text(&panel, ClipboardKind::Name).as_deref(),
            Some("zulu.txt")
        );
    }

    #[test]
    fn tagged_rows_win_one_per_line_in_listing_order_without_trailing_newline() {
        let mut panel = panel();
        panel.selected = 0;
        panel.selection.toggle(&OsString::from("alpha.txt"));
        panel.selection.toggle(&OsString::from("zulu.txt"));
        assert_eq!(
            clipboard_text(&panel, ClipboardKind::Path),
            Some(ClipboardText {
                text: format!("{}\n{}", in_dir("zulu.txt"), in_dir("alpha.txt")),
                count: 2,
            })
        );
        assert_eq!(
            text(&panel, ClipboardKind::Name).as_deref(),
            Some("zulu.txt\nalpha.txt")
        );
    }

    #[test]
    fn the_parent_row_copies_the_panel_directory_itself() {
        let panel = panel();
        assert_eq!(
            text(&panel, ClipboardKind::Path).as_deref(),
            Some("/left/dir")
        );
        assert_eq!(text(&panel, ClipboardKind::Name).as_deref(), Some("dir"));
    }

    #[test]
    fn at_the_root_the_path_is_the_name() {
        let mut panel = panel();
        panel.path = PathBuf::from("/");
        assert_eq!(text(&panel, ClipboardKind::Name).as_deref(), Some("/"));
    }

    #[test]
    fn an_empty_panel_has_nothing_to_copy() {
        let panel = PanelState::new(PathBuf::from("/left"));
        assert_eq!(clipboard_text(&panel, ClipboardKind::Path), None);
    }

    fn path_chord_routes(key: &str, physical: Code, modifiers: Modifiers) -> Option<Message> {
        route_press(
            &PromptKeyState::default(),
            Key::Character(key.into()),
            Physical::Code(physical),
            modifiers,
            false,
        )
    }

    #[test]
    fn the_path_chord_routes_on_every_platform() {
        assert_eq!(
            path_chord_routes("c", Code::KeyC, Modifiers::ALT | Modifiers::COMMAND),
            Some(Message::CopyToClipboard(ClipboardKind::Path))
        );
    }

    /// Option+C types "ç" on a Mac; the binding has to follow the key.
    #[cfg(target_os = "macos")]
    #[test]
    fn option_cmd_c_works_although_the_mac_types_a_cedilla() {
        assert_eq!(
            path_chord_routes("ç", Code::KeyC, Modifiers::ALT | Modifiers::LOGO),
            Some(Message::CopyToClipboard(ClipboardKind::Path))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ctrl_cmd_c_works_although_the_mac_may_type_a_control_character() {
        for typed in ["c", "\u{3}"] {
            assert_eq!(
                path_chord_routes(typed, Code::KeyC, Modifiers::CTRL | Modifiers::LOGO),
                Some(Message::CopyToClipboard(ClipboardKind::Name)),
                "{typed:?}"
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_shift_c_copies_the_name_and_ctrl_c_still_aborts() {
        assert_eq!(
            path_chord_routes("C", Code::KeyC, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Message::CopyToClipboard(ClipboardKind::Name))
        );
        assert_eq!(
            path_chord_routes("c", Code::KeyC, Modifiers::CTRL),
            Some(Message::AbortJob)
        );
    }

    #[test]
    fn a_held_key_does_not_copy_again() {
        let routed = route_press(
            &PromptKeyState::default(),
            Key::Character("c".into()),
            Physical::Code(Code::KeyC),
            Modifiers::ALT | Modifiers::COMMAND,
            true,
        );
        assert_eq!(routed, None);
    }

    #[tokio::test]
    async fn copying_says_so_in_the_status_bar_and_an_overlay_blocks_it() {
        let mut app = App::with_environment(Environment::isolated()).0;
        app.left_panel.path = PathBuf::from("/left");
        app.left_panel.entries = vec![entry("a.txt", false)];
        app.left_panel.selected = 0;

        let _open = app.update(Message::Delete { permanent: false });
        assert!(app.overlay_is_open(), "the delete dialog did not open");
        let _blocked = app.update(Message::CopyToClipboard(ClipboardKind::Path));
        assert_eq!(app.notice, None);
        let _closed = app.update(Message::DeleteCancel);

        let _path = app.update(Message::CopyToClipboard(ClipboardKind::Path));
        assert_eq!(app.notice.as_deref(), Some("Path copied"));
        let _name = app.update(Message::CopyToClipboard(ClipboardKind::Name));
        assert_eq!(app.notice.as_deref(), Some("Name copied"));
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
mod context_menu_tests {
    use super::*;
    use crate::context_menu::MenuEntry;

    const LEFT: PanelSide = PanelSide::Left;
    // Rows of the fixed left panel: `..`, a directory, two files.
    const PARENT: usize = 0;
    const DIRECTORY: usize = 1;
    const FILE: usize = 2;

    fn open_at(app: &mut App, side: PanelSide, index: usize) {
        drop(app.update(Message::ContextMenuAt { side, index }));
    }

    fn loaded(app: &App, side: PanelSide) -> Message {
        Message::DirectoryLoaded {
            side,
            request_id: app.panel(side).request_id,
            path: app.panel(side).path.clone(),
            entries: Vec::new(),
            error: None,
            sorted_by: fs::SortKey::default(),
            select: None,
        }
    }

    #[test]
    fn a_reload_of_the_active_panel_closes_the_menu_and_the_other_ones_does_not() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, FILE);
        assert!(app.context_menu_for_test().is_some());

        let other = loaded(&app, PanelSide::Right);
        drop(app.update(other));
        assert!(
            app.context_menu_for_test().is_some(),
            "the other side loaded"
        );

        let active = loaded(&app, LEFT);
        drop(app.update(active));
        assert!(app.context_menu_for_test().is_none(), "its rows are gone");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_control_click_on_another_row_replaces_the_menu() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, FILE);
        let first = enabled_actions(&app);
        app.set_modifiers_for_test(Modifiers::CTRL);
        drop(app.update(Message::RowClicked {
            side: LEFT,
            index: DIRECTORY,
            on_tag: false,
        }));
        assert!(app.context_menu_for_test().is_some(), "a new menu");
        let second = enabled_actions(&app);
        assert_ne!(first, second, "the directory's menu, not the file's");
    }

    #[test]
    fn a_stale_reload_leaves_the_menu_open() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, FILE);
        let mut stale = loaded(&app, LEFT);
        if let Message::DirectoryLoaded { request_id, .. } = &mut stale {
            *request_id = request_id.wrapping_add(1);
        }
        drop(app.update(stale));
        assert!(app.context_menu_for_test().is_some());
    }

    fn enabled_actions(app: &App) -> Vec<ContextAction> {
        app.context_menu_for_test()
            .unwrap()
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item {
                    action,
                    enabled: true,
                } => Some(*action),
                _ => None,
            })
            .collect()
    }

    fn index_of(app: &App, wanted: ContextAction) -> usize {
        app.context_menu_for_test()
            .unwrap()
            .entries()
            .iter()
            .position(|entry| matches!(entry, MenuEntry::Item { action, .. } if *action == wanted))
            .unwrap()
    }

    fn route(menu_open: bool, named: Named, modifiers: Modifiers) -> Option<Message> {
        let state = PromptKeyState {
            context_menu: menu_open,
            ..PromptKeyState::default()
        };
        route_key(&state, Key::Named(named), modifiers)
    }

    #[test]
    fn a_right_click_on_an_untagged_row_moves_the_cursor_and_the_pointer_places_the_menu() {
        let mut app = App::with_fixed_panels();
        app.set_pointer_for_test(Point::new(100.0, 120.0));
        app.active_panel = PanelSide::Right;
        open_at(&mut app, LEFT, FILE);

        assert!(app.context_menu_is_open());
        assert_eq!(app.active_panel_side_for_test(), LEFT);
        assert_eq!(app.panel(LEFT).selected, FILE);
        assert_eq!(
            app.context_menu_for_test().unwrap().origin(),
            Point::new(100.0, 120.0)
        );
    }

    #[test]
    fn a_right_click_on_a_tagged_row_keeps_the_tags_and_acts_on_all_of_them() {
        let mut app = App::with_fixed_panels();
        app.toggle_tag_for_test(DIRECTORY);
        app.toggle_tag_for_test(3);
        open_at(&mut app, LEFT, 3);

        assert!(app.left_panel_selection_tagged_for_test(DIRECTORY));
        assert!(app.left_panel_selection_tagged_for_test(3));
        let trash = index_of(&app, ContextAction::Trash);
        drop(app.update(Message::ContextMenuClick(trash)));

        assert!(!app.context_menu_is_open());
        assert_eq!(app.delete_dialog.as_ref().unwrap().sources.len(), 2);
    }

    #[test]
    fn the_parent_row_offers_only_what_makes_sense() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, PARENT);
        assert_eq!(
            enabled_actions(&app),
            [
                ContextAction::Open,
                ContextAction::NewFolder,
                ContextAction::CopyPath,
                ContextAction::CopyName
            ]
        );
    }

    #[test]
    fn a_right_click_outside_the_tags_drops_them_and_acts_on_the_clicked_row_only() {
        let mut app = App::with_fixed_panels();
        app.toggle_tag_for_test(DIRECTORY);
        app.toggle_tag_for_test(3);
        open_at(&mut app, LEFT, FILE);

        assert!(!app.left_panel_selection_tagged_for_test(DIRECTORY));
        assert!(!app.left_panel_selection_tagged_for_test(3));
        let trash = index_of(&app, ContextAction::Trash);
        drop(app.update(Message::ContextMenuClick(trash)));
        assert_eq!(app.delete_dialog.as_ref().unwrap().sources.len(), 1);
    }

    #[test]
    fn the_parent_row_outside_the_tags_offers_the_parent_actions_only() {
        let mut app = App::with_fixed_panels();
        app.toggle_tag_for_test(FILE);
        open_at(&mut app, LEFT, PARENT);
        assert!(!enabled_actions(&app).contains(&ContextAction::Copy));
        assert!(!enabled_actions(&app).contains(&ContextAction::Trash));
    }

    #[test]
    fn the_keyboard_keeps_the_tags() {
        let mut app = App::with_fixed_panels();
        app.toggle_tag_for_test(DIRECTORY);
        app.panel_mut(LEFT).selected = FILE;
        drop(app.update(Message::ContextMenuKey));
        assert!(app.left_panel_selection_tagged_for_test(DIRECTORY));
    }

    #[test]
    fn a_directory_cannot_be_viewed_or_edited_but_a_file_can() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, DIRECTORY);
        let on_directory = enabled_actions(&app);
        assert!(!on_directory.contains(&ContextAction::View));
        assert!(!on_directory.contains(&ContextAction::Edit));
        assert!(on_directory.contains(&ContextAction::Copy));

        open_at(&mut app, LEFT, FILE);
        let on_file = enabled_actions(&app);
        assert!(on_file.contains(&ContextAction::View));
        assert!(on_file.contains(&ContextAction::Edit));
    }

    #[test]
    fn an_empty_panel_still_offers_a_new_folder() {
        let mut app = App::with_fixed_panels();
        app.panel_mut(LEFT).entries.clear();
        open_at(&mut app, LEFT, 0);
        assert_eq!(enabled_actions(&app), [ContextAction::NewFolder]);
    }

    #[test]
    fn enter_runs_the_highlighted_entry_after_closing_the_menu() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, FILE);
        let copy_path = index_of(&app, ContextAction::CopyPath);
        drop(app.update(Message::ContextMenuHover(copy_path)));
        assert_eq!(
            app.context_menu_selected_action(),
            Some(ContextAction::CopyPath)
        );
        drop(app.update(Message::ContextMenuActivate));

        assert!(!app.context_menu_is_open());
        assert_eq!(app.notice.as_deref(), Some("Path copied"));
    }

    #[test]
    fn an_entry_runs_the_same_message_as_its_key() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, FILE);
        let new_folder = index_of(&app, ContextAction::NewFolder);
        drop(app.update(Message::ContextMenuClick(new_folder)));
        assert!(app.prompt.is_some(), "F7 opens the create-directory prompt");
    }

    #[test]
    fn arrows_skip_what_cannot_be_chosen() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, PARENT);
        // Open, then the next usable entry is the new folder: View, Edit, Copy
        // and Move are greyed out and the separator is not an entry.
        drop(app.update(Message::ContextMenuMove(1)));
        assert_eq!(
            app.context_menu_selected_action(),
            Some(ContextAction::NewFolder)
        );
        drop(app.update(Message::ContextMenuLast));
        assert_eq!(
            app.context_menu_selected_action(),
            Some(ContextAction::CopyName)
        );
        drop(app.update(Message::ContextMenuFirst));
        assert_eq!(
            app.context_menu_selected_action(),
            Some(ContextAction::Open)
        );
    }

    #[test]
    fn the_keys_of_the_open_menu() {
        assert_eq!(
            route(true, Named::Escape, Modifiers::default()),
            Some(Message::ContextMenuClose)
        );
        assert_eq!(
            route(true, Named::Enter, Modifiers::default()),
            Some(Message::ContextMenuActivate)
        );
        assert_eq!(
            route(true, Named::ArrowDown, Modifiers::default()),
            Some(Message::ContextMenuMove(1))
        );
        assert_eq!(
            route(true, Named::ArrowUp, Modifiers::default()),
            Some(Message::ContextMenuMove(-1))
        );
        assert_eq!(
            route(true, Named::Home, Modifiers::default()),
            Some(Message::ContextMenuFirst)
        );
        assert_eq!(
            route(true, Named::End, Modifiers::default()),
            Some(Message::ContextMenuLast)
        );
        assert_eq!(route(true, Named::F5, Modifiers::default()), None);
        assert!(repeats(&Message::ContextMenuMove(1)));
        assert!(!repeats(&Message::ContextMenuActivate));
    }

    #[test]
    fn shift_f10_opens_the_menu_and_does_not_quit() {
        assert_eq!(
            route(false, Named::F10, Modifiers::SHIFT),
            Some(Message::ContextMenuKey)
        );
        assert_eq!(
            route(false, Named::F10, Modifiers::default()),
            Some(Message::Quit)
        );
    }

    #[test]
    fn escape_closes_and_the_cursor_stays() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, FILE);
        drop(app.update(Message::ContextMenuClose));
        assert!(!app.context_menu_is_open());
        assert_eq!(app.panel(LEFT).selected, FILE);
    }

    #[test]
    fn the_keyboard_opens_the_menu_at_the_cursor_row_of_the_active_panel() {
        let mut app = App::with_fixed_panels();
        app.active_panel = PanelSide::Right;
        app.panel_mut(PanelSide::Right).selected = 2;
        drop(app.update(Message::ContextMenuKey));

        let menu = app.context_menu_for_test().unwrap();
        let anchor = layout::cursor_row_anchor(true, 2, app.window_size);
        assert_eq!(menu.origin(), anchor);
        assert_eq!(app.active_panel_side_for_test(), PanelSide::Right);
        assert_eq!(app.panel(PanelSide::Right).selected, 2);
    }

    #[test]
    fn the_keyboard_does_nothing_under_an_overlay_or_a_menu_that_is_already_up() {
        let mut prompt = App::with_prompt_open();
        drop(prompt.update(Message::ContextMenuKey));
        assert!(!prompt.context_menu_is_open());

        let mut delete = App::with_fixed_panels();
        delete.open_delete_dialog_for_test(1, false);
        drop(delete.update(Message::ContextMenuKey));
        assert!(!delete.context_menu_is_open());
        drop(delete.update(Message::ContextMenuAt {
            side: LEFT,
            index: FILE,
        }));
        assert!(!delete.context_menu_is_open());

        let mut open = App::with_fixed_panels();
        open_at(&mut open, LEFT, FILE);
        let before = open.context_menu_for_test().cloned();
        drop(open.update(Message::ContextMenuKey));
        assert_eq!(open.context_menu_for_test().cloned(), before);
    }

    #[test]
    fn a_right_click_on_another_row_reopens_the_menu_there() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, DIRECTORY);
        app.set_pointer_for_test(Point::new(300.0, 200.0));
        open_at(&mut app, PanelSide::Right, 1);

        assert!(app.context_menu_is_open());
        assert_eq!(app.active_panel_side_for_test(), PanelSide::Right);
        assert_eq!(app.panel(PanelSide::Right).selected, 1);
        assert_eq!(
            app.context_menu_for_test().unwrap().origin(),
            Point::new(300.0, 200.0)
        );
    }

    #[test]
    fn a_left_click_on_a_row_only_closes_the_menu() {
        let mut app = App::with_fixed_panels();
        open_at(&mut app, LEFT, DIRECTORY);
        drop(app.update(Message::RowClicked {
            side: LEFT,
            index: FILE,
            on_tag: false,
        }));
        assert!(!app.context_menu_is_open());
        assert_eq!(app.panel(LEFT).selected, DIRECTORY);
    }

    #[test]
    fn control_click_is_a_right_click_on_a_mac_only() {
        assert_eq!(is_context_click(Modifiers::CTRL), cfg!(target_os = "macos"));
        assert!(!is_context_click(Modifiers::default()));
        assert!(!is_context_click(Modifiers::SHIFT));

        let mut app = App::with_fixed_panels();
        app.set_modifiers_for_test(Modifiers::CTRL);
        drop(app.update(Message::RowClicked {
            side: LEFT,
            index: FILE,
            on_tag: false,
        }));
        assert_eq!(app.context_menu_is_open(), cfg!(target_os = "macos"));
        assert_eq!(app.panel(LEFT).selected, FILE);
    }

    #[test]
    fn the_panels_do_not_scroll_under_the_menu() {
        let wheel = Message::PanelScrolled {
            side: LEFT,
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        };
        let mut app = App::with_fixed_panels();
        app.set_visible_rows_for_test(2);
        drop(app.update(wheel.clone()));
        assert_eq!(app.panel(LEFT).scroll_offset, 1, "the wheel scrolls");

        let mut covered = App::with_fixed_panels();
        covered.set_visible_rows_for_test(2);
        open_at(&mut covered, LEFT, 0);
        drop(covered.update(wheel));
        assert_eq!(covered.panel(LEFT).scroll_offset, 0);
    }

    #[test]
    fn the_menu_is_an_overlay() {
        let mut app = App::with_fixed_panels();
        assert!(!app.overlay_is_open());
        open_at(&mut app, LEFT, FILE);
        assert!(app.overlay_is_open());
        assert!(app.prompt_key_state().context_menu);
        drop(app.update(Message::VolumeMenu(LEFT)));
        assert!(!app.volume_menu_is_open());
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
mod type_ahead_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// An app on a panel of `names` (after `..`) and a clock the test moves.
    fn app_with(names: &[&str]) -> (App, Arc<Mutex<Instant>>) {
        let now = Arc::new(Mutex::new(Instant::now()));
        let clock = Arc::clone(&now);
        let mut app = App::with_fixed_panels().with_clock(move || *clock.lock().unwrap());
        app.panel_mut(PanelSide::Left).entries = std::iter::once("..")
            .chain(names.iter().copied())
            .enumerate()
            .map(|(index, name)| fs::FileEntry {
                name: name.into(),
                path: PathBuf::from("/home/test").join(name),
                is_dir: false,
                is_symlink: false,
                is_parent: index == 0,
                size: 0,
                modified: None,
            })
            .collect();
        (app, now)
    }

    fn type_key(app: &mut App, typed: &str) {
        drop(app.update(Message::TypeAhead(typed.to_string())));
    }

    fn advance(clock: &Arc<Mutex<Instant>>, by: Duration) {
        *clock.lock().unwrap() += by;
    }

    fn cursor(app: &App) -> usize {
        app.panel(PanelSide::Left).selected
    }

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn q_is_type_ahead_and_only_command_q_quits() {
        let (mut app, _clock) = app_with(&["alpha", "quarry"]);
        let state = app.prompt_key_state();
        let q = || Key::Character("q".into());
        let typed = route_key(&state, q(), Modifiers::default());
        assert_eq!(typed, Some(Message::TypeAhead("q".to_string())));
        drop(app.update(typed.unwrap()));
        assert_eq!(cursor(&app), 2);
        assert_eq!(
            route_key(&state, q(), Modifiers::COMMAND),
            Some(Message::Quit)
        );
    }

    #[test]
    fn a_character_jumps_to_the_first_entry_it_starts_ignoring_case() {
        let (mut app, _clock) = app_with(&["alpha", "Beta", "bravo"]);
        type_key(&mut app, "b");
        assert_eq!(cursor(&app), 2);
        type_key(&mut app, "r");
        assert_eq!(cursor(&app), 3, "br extends the prefix");
    }

    #[test]
    fn a_character_within_a_second_extends_the_prefix_and_later_starts_anew() {
        let (mut app, clock) = app_with(&["ab", "ac", "ba", "bc"]);
        type_key(&mut app, "b");
        assert_eq!(cursor(&app), 3);
        advance(&clock, SECOND);
        type_key(&mut app, "c");
        assert_eq!(cursor(&app), 4, "bc, one second after b");
        advance(&clock, SECOND + Duration::from_millis(1));
        type_key(&mut app, "a");
        assert_eq!(cursor(&app), 1, "a on its own, not bca");
    }

    #[test]
    fn no_match_leaves_the_cursor() {
        let (mut app, _clock) = app_with(&["alpha", "beta"]);
        type_key(&mut app, "b");
        assert_eq!(cursor(&app), 2);
        type_key(&mut app, "z");
        assert_eq!(cursor(&app), 2);
    }

    #[test]
    fn the_parent_row_does_not_count() {
        let (mut app, _clock) = app_with(&["alpha"]);
        type_key(&mut app, ".");
        assert_eq!(cursor(&app), 0);
    }

    #[test]
    fn moving_the_cursor_or_the_panel_ends_the_prefix() {
        for reset in [
            Message::MoveSelection(1),
            Message::SwitchPanel,
            Message::SelectFirst,
            Message::PageDown,
        ] {
            let (mut app, _clock) = app_with(&["ab", "ba", "bb", "ca"]);
            type_key(&mut app, "b");
            drop(app.update(reset.clone()));
            app.active_panel = PanelSide::Left;
            type_key(&mut app, "a");
            assert_eq!(cursor(&app), 1, "{reset:?} did not end the prefix");
        }
    }

    #[test]
    fn the_view_follows_the_cursor() {
        let names: Vec<String> = (0..40).map(|i| format!("file{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let (mut app, _clock) = app_with(&refs);
        app.set_visible_rows_for_test(5);
        type_key(&mut app, "f");
        for digit in ["i", "l", "e", "3", "5"] {
            type_key(&mut app, digit);
        }
        assert_eq!(cursor(&app), 36);
        let panel = app.panel(PanelSide::Left);
        assert!(panel.scroll_offset <= 36 && 36 < panel.scroll_offset + 5);
    }

    #[test]
    fn nothing_happens_under_an_overlay() {
        let (mut app, _clock) = app_with(&["alpha", "beta"]);
        app.open_delete_dialog_for_test(1, false);
        type_key(&mut app, "b");
        assert_eq!(cursor(&app), 0);
    }

    #[test]
    fn printable_keys_route_to_type_ahead_but_not_with_option_ctrl_or_cmd() {
        let route = |key: &str, modifiers: Modifiers| {
            route_key(
                &PromptKeyState::default(),
                Key::Character(key.into()),
                modifiers,
            )
        };
        assert_eq!(
            route("a", Modifiers::default()),
            Some(Message::TypeAhead("a".into()))
        );
        assert_eq!(
            route("A", Modifiers::SHIFT),
            Some(Message::TypeAhead("A".into()))
        );
        assert_eq!(route("ç", Modifiers::ALT), None);
        assert_eq!(route("a", Modifiers::CTRL), None);
        assert_eq!(route("a", Modifiers::LOGO), None);
        assert_eq!(route("*", Modifiers::SHIFT), Some(Message::TagAll));
        assert_eq!(
            route_key(
                &PromptKeyState::default(),
                Key::Named(Named::Space),
                Modifiers::default()
            ),
            Some(Message::ToggleTag)
        );
        let prompt = PromptKeyState {
            open: true,
            ..PromptKeyState::default()
        };
        assert_eq!(
            route_key(&prompt, Key::Character("a".into()), Modifiers::default()),
            Some(Message::PromptInput("a".into()))
        );
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod path_field_tests {
    use super::*;

    /// Edits the field: its first keystroke, with the panel's path as it was.
    fn open(app: &mut App) {
        let shown = app.active_panel().path.display().to_string();
        drop(app.update(Message::PathFieldInput(shown)));
    }

    fn type_path(app: &mut App, typed: &str) {
        drop(app.update(Message::PathFieldInput(typed.to_string())));
    }

    #[test]
    fn typing_replaces_what_the_field_shows_and_a_cancel_restores_the_panels_path() {
        let mut app = App::with_fixed_panels();
        assert_eq!(app.path_field, None, "unedited, the field shows the panel");
        type_path(&mut app, "/home/test/links/Docu");
        assert_eq!(app.path_field.as_deref(), Some("/home/test/links/Docu"));
        drop(app.update(Message::PathFieldCancel));
        assert_eq!(app.path_field, None);
    }

    fn submit(app: &mut App, typed: &str) -> u64 {
        open(app);
        type_path(app, typed);
        drop(app.update(Message::PathFieldSubmit));
        app.path_check_id
    }

    fn checked(app: &mut App, request_id: u64, result: Result<fs::Destination, fs::PathProblem>) {
        drop(app.update(Message::PathChecked {
            request_id,
            side: PanelSide::Left,
            target: PathBuf::from("/typed/path"),
            result,
        }));
    }

    #[test]
    fn submitting_does_not_touch_the_field_or_the_panel_until_the_answer_is_in() {
        let mut app = App::with_fixed_panels();
        let before = app.active_panel().request_id;
        submit(&mut app, "/typed/path");
        assert_eq!(app.path_field.as_deref(), Some("/typed/path"));
        assert_eq!(app.active_panel().request_id, before);
    }

    #[test]
    fn a_destination_closes_the_field_and_loads_it() {
        let mut app = App::with_fixed_panels();
        let id = submit(&mut app, "/typed/path");
        let before = app.active_panel().request_id;
        checked(
            &mut app,
            id,
            Ok(fs::Destination {
                directory: PathBuf::from("/typed/path"),
                select: None,
            }),
        );
        assert_eq!(app.path_field, None);
        assert_ne!(app.active_panel().request_id, before, "a load started");
    }

    #[test]
    fn a_missing_path_is_reported_and_the_field_stays_open() {
        let mut app = App::with_fixed_panels();
        let id = submit(&mut app, "/typed/path");
        let before = app.active_panel().request_id;
        checked(&mut app, id, Err(fs::PathProblem::Missing));
        assert_eq!(app.path_field.as_deref(), Some("/typed/path"));
        assert_eq!(app.active_panel().request_id, before, "nothing was loaded");
        assert!(app
            .job_error
            .as_deref()
            .is_some_and(|error| error.contains("/typed/path") && error.contains("exist")));
    }

    #[test]
    fn an_unreadable_path_says_why_and_is_not_called_missing() {
        let mut app = App::with_fixed_panels();
        let id = submit(&mut app, "/typed/path");
        checked(
            &mut app,
            id,
            Err(fs::PathProblem::Unreadable {
                reason: "Permission denied".to_string(),
            }),
        );
        let error = app.job_error.clone().unwrap();
        assert!(error.contains("Permission denied"), "{error}");
        assert!(!error.contains("does not exist"), "{error}");
    }

    #[test]
    fn an_answer_to_text_that_has_changed_or_closed_is_dropped() {
        let destination = || {
            Ok(fs::Destination {
                directory: PathBuf::from("/typed/path"),
                select: None,
            })
        };
        let mut app = App::with_fixed_panels();
        let first_id = submit(&mut app, "/typed/path");
        type_path(&mut app, "/typed/path/more");
        let before = app.active_panel().request_id;
        checked(&mut app, first_id, destination());
        assert_eq!(app.path_field.as_deref(), Some("/typed/path/more"));
        assert_eq!(app.active_panel().request_id, before);

        let second_id = submit(&mut app, "/typed/path");
        drop(app.update(Message::PathFieldCancel));
        checked(&mut app, second_id, destination());
        assert_eq!(
            app.active_panel().request_id,
            before,
            "the field was closed"
        );
    }

    #[test]
    fn escape_click_outside_and_other_actions_end_the_editing() {
        for leave in [
            Message::PathFieldCancel,
            Message::ClickedOutside,
            Message::SwitchPanel,
            Message::RowClicked {
                side: PanelSide::Left,
                index: 1,
                on_tag: false,
            },
            Message::SortBy {
                side: PanelSide::Left,
                column: fs::SortColumn::Size,
            },
        ] {
            let mut app = App::with_fixed_panels();
            open(&mut app);
            drop(app.update(leave));
            assert_eq!(app.path_field, None);
        }
    }

    #[test]
    fn while_editing_the_panels_get_no_keys_but_quit() {
        let mut app = App::with_fixed_panels();
        open(&mut app);
        let state = app.prompt_key_state();
        assert!(state.path_editing);
        let none = Modifiers::default();
        assert_eq!(route_key(&state, Key::Named(Named::ArrowDown), none), None);
        // Escape and Tab reach the app from the input state, not from here.
        assert_eq!(route_key(&state, Key::Named(Named::Escape), none), None);
        assert_eq!(
            route_key(&state, Key::Named(Named::F10), none),
            Some(Message::Quit)
        );
        let quit_chord = if cfg!(target_os = "macos") {
            Modifiers::COMMAND
        } else {
            Modifiers::CTRL
        };
        assert_eq!(
            route_key(&state, Key::Character("q".into()), quit_chord),
            Some(Message::Quit)
        );
    }

    #[test]
    fn the_copy_icon_copies_the_active_panels_directory() {
        let app = App::with_fixed_panels();
        let copied = clipboard_text(app.active_panel(), ClipboardKind::Directory).unwrap();
        assert_eq!(copied.text, "/home/test/links");
        assert_eq!(copied.count, 1);
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod notice_tests {
    use super::*;

    fn copy_path(app: &mut App) {
        drop(app.update(Message::CopyToClipboard(ClipboardKind::Path)));
    }

    #[test]
    fn the_notice_goes_when_its_own_timer_fires() {
        let mut app = App::with_fixed_panels();
        copy_path(&mut app);
        assert_eq!(app.notice.as_deref(), Some("Path copied"));
        drop(app.update(Message::NoticeExpired(app.notice_generation)));
        assert_eq!(app.notice, None);
    }

    #[test]
    fn an_older_timer_leaves_a_newer_notice_alone() {
        let mut app = App::with_fixed_panels();
        copy_path(&mut app);
        let first = app.notice_generation;
        drop(app.update(Message::CopyToClipboard(ClipboardKind::Name)));
        assert_ne!(app.notice_generation, first);

        drop(app.update(Message::NoticeExpired(first)));
        assert_eq!(app.notice.as_deref(), Some("Name copied"));

        drop(app.update(Message::NoticeExpired(app.notice_generation)));
        assert_eq!(app.notice, None);
    }

    #[test]
    fn a_config_warning_stays_while_the_notice_comes_and_goes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[theme]\npanel = \"#f4f0ff\"\n").unwrap();
        let mut app = App::with_fixed_panels().with_config(config::load(&path));
        assert!(app.status_error().is_some());
        copy_path(&mut app);
        assert_eq!(app.notice.as_deref(), Some("Path copied"));
        assert!(app.status_error().is_some());
        drop(app.update(Message::NoticeExpired(app.notice_generation)));
        assert_eq!(app.notice, None);
        assert!(app.status_error().is_some());
    }

    #[test]
    fn a_failure_replaces_the_notice() {
        let mut app = App::with_fixed_panels();
        copy_path(&mut app);
        app.set_status_error("it failed");
        assert_eq!(app.notice, None);
        assert_eq!(app.job_error.as_deref(), Some("it failed"));
    }
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod config_status_tests {
    use super::*;

    fn invalid(line: Option<usize>) -> ConfigError {
        ConfigError::Invalid {
            path: PathBuf::from("/cfg/ncrs/config.toml"),
            line,
            reason: "unknown field `colour`".to_string(),
        }
    }

    #[test]
    fn an_error_with_a_line_names_path_line_and_reason() {
        let app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
        let shown = app.status_error().unwrap_or_default();
        assert!(shown.contains("/cfg/ncrs/config.toml"), "{shown}");
        assert!(shown.contains("line 3"), "{shown}");
        assert!(shown.contains("unknown field `colour`"), "{shown}");
    }

    #[test]
    fn an_unreadable_file_names_path_and_reason() {
        let err = ConfigError::Unreadable {
            path: PathBuf::from("/cfg/ncrs/config.toml"),
            reason: "not a regular file".to_string(),
        };
        let app = App::with_fixed_panels().with_config(Err(err));
        let shown = app.status_error().unwrap_or_default();
        assert!(shown.contains("/cfg/ncrs/config.toml"), "{shown}");
        assert!(shown.contains("not a regular file"), "{shown}");
    }

    #[test]
    fn an_error_without_a_line_still_reports() {
        let app = App::with_fixed_panels().with_config(Err(invalid(None)));
        let shown = app.status_error().unwrap_or_default();
        assert!(shown.contains("unknown field"), "{shown}");
        assert!(!shown.contains("line"), "{shown}");
    }

    #[test]
    fn a_good_config_shows_nothing_and_is_applied() {
        let mut config = Config::default();
        config.open.view = Some(config::ProgramLine::new("viewer", &[]));
        let app = App::with_fixed_panels().with_config(Ok(config.clone()));
        assert_eq!(app.status_error(), None);
        assert_eq!(app.open_programs, config.open);
    }

    #[test]
    fn a_config_error_is_its_own_field_and_shows_in_the_status_line() {
        let app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
        assert_eq!(app.job_error, None);
        assert!(app.config_error.is_some());
        assert_eq!(app.status_error(), app.config_error.as_deref());
    }

    #[test]
    fn a_job_error_covers_the_config_error_until_it_goes() {
        let mut app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
        let config_text = app.config_error.clone();
        app.set_status_error("x");
        assert_eq!(app.status_error(), Some("x"));

        drop(app.update(Message::MoveSelection(1)));

        assert_eq!(app.status_error(), config_text.as_deref());
    }

    #[test]
    fn escape_dismisses_the_config_error_and_a_job_comes_first() {
        let escape = Key::Named(Named::Escape);
        let state = |job_running, config_error| PromptKeyState {
            job_running,
            config_error,
            ..PromptKeyState::default()
        };
        assert_eq!(
            route_key(&state(false, true), escape.clone(), Modifiers::default()),
            Some(Message::DismissConfigError)
        );
        assert_eq!(
            route_key(&state(true, true), escape.clone(), Modifiers::default()),
            Some(Message::AbortJob)
        );
        assert_eq!(
            route_key(&state(false, false), escape, Modifiers::default()),
            None
        );
    }

    #[test]
    fn the_config_error_follows_a_language_switch() {
        let mut app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
        drop(app.update(Message::SwitchLanguage));
        let shown = app.status_error().unwrap_or_default();
        assert!(shown.contains("Zeile 3"), "{shown}");
    }

    fn loaded(content: &str) -> Config {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        config::load(&path).unwrap()
    }

    #[test]
    fn a_vscode_theme_that_is_not_found_is_reported_in_both_languages_and_beats_a_contrast_warning()
    {
        let mut app = App::with_fixed_panels().with_config(Ok(loaded(
            "lang = \"de\"\n[theme]\npanel = \"#f4f0ff\"\nvscode = \"gone.json\"\n",
        )));
        let german = app.status_error().unwrap_or_default();
        assert!(german.contains("Zeile 4"), "{german}");
        assert!(german.contains("VS-Code-Theme"), "{german}");
        assert!(german.contains("gone.json: no such file"), "{german}");
        assert!(!german.contains("Kontrast"), "{german}");

        drop(app.update(Message::SwitchLanguage));
        let english = app.status_error().unwrap_or_default();
        assert!(english.contains("line 4"), "{english}");
        assert!(english.contains("VS Code theme"), "{english}");
        assert!(english.contains("the preset applies"), "{english}");
    }

    #[test]
    fn a_configured_color_reaches_the_app() {
        let app =
            App::with_fixed_panels().with_config(Ok(loaded("[theme]\nerror = \"#f0a0a0\"\n")));
        assert_eq!(app.palette.error, iced::Color::from_rgb8(0xF0, 0xA0, 0xA0));
        assert_eq!(app.status_error(), None);
    }

    #[test]
    fn the_configured_start_language_words_the_first_message() {
        let app = App::with_fixed_panels()
            .with_config(Ok(loaded("lang = \"de\"\n[theme]\npanel = \"#f4f0ff\"\n")));
        assert_eq!(app.lang, Language::German);
        let shown = app.status_error().unwrap_or_default();
        assert!(shown.contains("Zeile 3"), "{shown}");
        let english = App::with_fixed_panels().with_config(Ok(loaded("")));
        assert_eq!(english.lang, Language::English);
    }

    #[test]
    fn a_low_contrast_is_shown_follows_the_language_and_is_dismissed() {
        let mut app =
            App::with_fixed_panels().with_config(Ok(loaded("[theme]\npanel = \"#f4f0ff\"\n")));
        assert_eq!(app.palette.panel, iced::Color::from_rgb8(0xF4, 0xF0, 0xFF));
        let shown = app.status_error().unwrap_or_default();
        assert!(shown.contains("line 2"), "{shown}");
        assert!(
            shown.contains("directory on panel has a contrast of only"),
            "{shown}"
        );
        assert!(!shown.contains("Using the defaults"), "{shown}");

        drop(app.update(Message::SwitchLanguage));
        let german = app.status_error().unwrap_or_default();
        assert!(german.contains("Zeile 2"), "{german}");
        assert!(
            german.contains("directory auf panel hat nur ein Kontrastverhältnis"),
            "{german}"
        );

        drop(app.update(Message::DismissConfigError));
        assert_eq!(app.status_error(), None);
        assert_eq!(app.palette.panel, iced::Color::from_rgb8(0xF4, 0xF0, 0xFF));
    }

    #[test]
    fn dismissing_clears_the_config_error() {
        let mut app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
        drop(app.update(Message::DismissConfigError));
        assert_eq!(app.status_error(), None);
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
mod configured_keys_tests {
    use super::*;
    use keyboard::key::{Code, Physical};

    fn config_with(content: &str) -> Config {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        config::load(&path).unwrap()
    }

    fn state_for(content: &str) -> PromptKeyState {
        PromptKeyState {
            keymap: config_with(content).keymap,
            ..PromptKeyState::default()
        }
    }

    fn named(key: Named) -> Key {
        Key::Named(key)
    }

    fn character(text: &str) -> Key {
        Key::Character(text.into())
    }

    #[test]
    fn a_rebound_action_routes_on_the_new_key_only() {
        let state = state_for("[keys]\ncopy = \"F2\"\n");
        assert_eq!(
            route_key(&state, named(Named::F2), Modifiers::default()),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            route_key(&state, named(Named::F5), Modifiers::default()),
            None
        );
    }

    #[test]
    fn the_bar_and_the_context_menu_show_the_configured_key() {
        let config = config_with("[keys]\ncopy = \"F2\"\n");
        let mut app = App::with_fixed_panels().with_config(Ok(config));
        assert_eq!(app.function_keys[1], Some("Copy"));
        assert_eq!(app.function_keys[4], None);
        assert_eq!(
            app.keymap
                .shortcut_label(&ContextAction::Copy.message())
                .as_deref(),
            Some("F2")
        );

        drop(app.update(Message::SwitchLanguage));
        assert_eq!(app.lang, Language::German);
        assert!(app.function_keys[1].is_some());
        assert_eq!(app.function_keys[4], None);
    }

    #[test]
    fn a_failed_config_leaves_the_built_in_keys() {
        let app = App::with_fixed_panels().with_config(Err(ConfigError::Unreadable {
            path: PathBuf::from("/cfg"),
            reason: "x".into(),
        }));
        assert_eq!(*app.keymap, *Keymap::built_in());
        assert_eq!(app.function_keys[4], Some("Copy"));
    }

    #[test]
    fn the_configured_permanent_delete_key_confirms_its_dialog() {
        let state = PromptKeyState {
            delete_dialog: DeleteKeys::Permanent,
            ..state_for("[keys]\ndelete_permanently = \"ctrl+d\"\n")
        };
        assert_eq!(
            route_key(&state, character("d"), Modifiers::CTRL),
            Some(Message::DeleteConfirm)
        );
        assert_eq!(route_key(&state, named(Named::F8), Modifiers::SHIFT), None);
    }

    #[test]
    fn a_configured_key_cannot_take_the_fixed_dialog_keys() {
        let delete = PromptKeyState {
            delete_dialog: DeleteKeys::Permanent,
            ..state_for("[keys]\ndelete_permanently = \"right\"\n")
        };
        assert_eq!(
            route_key(&delete, named(Named::ArrowRight), Modifiers::default()),
            Some(Message::DialogFocus(1))
        );
        assert_eq!(
            route_key(&delete, named(Named::Enter), Modifiers::default()),
            Some(Message::DialogActivate)
        );
        let conflict = PromptKeyState {
            conflict_open: true,
            ..state_for("[keys]\nabort_job = \"left\"\n")
        };
        assert_eq!(
            route_key(&conflict, named(Named::ArrowLeft), Modifiers::default()),
            Some(Message::DialogFocus(-1))
        );
    }

    #[test]
    fn the_built_in_permanent_delete_key_still_confirms_its_dialog() {
        let state = PromptKeyState {
            delete_dialog: DeleteKeys::Permanent,
            ..PromptKeyState::default()
        };
        assert_eq!(
            route_key(&state, named(Named::F8), Modifiers::SHIFT),
            Some(Message::DeleteConfirm)
        );
    }

    #[test]
    fn the_configured_abort_key_cancels_a_conflict() {
        let state = PromptKeyState {
            conflict_open: true,
            ..state_for("[keys]\nabort_job = \"ctrl+k\"\n")
        };
        assert_eq!(
            route_key(&state, character("k"), Modifiers::CTRL),
            Some(Message::TransferConflict(ConflictChoice::Cancel))
        );
        assert_eq!(route_key(&state, character("c"), Modifiers::CTRL), None);
    }

    #[test]
    fn quitting_from_the_path_field_needs_a_key_the_field_does_not_type() {
        let state = PromptKeyState {
            path_editing: true,
            ..state_for("[keys]\nquit = [\"*\", \"backspace\", \"F10\", \"primary+q\"]\ntag_all = []\ngo_up = []\n")
        };
        assert_eq!(
            route_key(&state, character("*"), Modifiers::default()),
            None
        );
        assert_eq!(
            route_key(&state, named(Named::Backspace), Modifiers::default()),
            None
        );
        assert_eq!(
            route_key(&state, named(Named::F10), Modifiers::default()),
            Some(Message::Quit)
        );
        assert_eq!(
            route_key(&state, character("q"), Modifiers::COMMAND),
            Some(Message::Quit)
        );
    }

    #[test]
    fn a_failed_trash_names_the_configured_key_or_none() {
        let deleting = Deletion {
            sources: vec![PathBuf::from("/left/a.txt")],
            permanent: false,
            side: PanelSide::Left,
        };
        let mut app = App::with_fixed_panels()
            .with_config(Ok(config_with("[keys]\ndelete_permanently = \"ctrl+d\"\n")));
        app.deleting = Some(deleting.clone());
        let shown = app.delete_failure(0, "denied");
        assert!(
            shown.contains("a.txt") && shown.contains("denied"),
            "{shown}"
        );
        assert!(
            shown.contains(
                &app.keymap
                    .shortcut_label(&Message::Delete { permanent: true })
                    .unwrap()
            ),
            "{shown}"
        );
        assert!(!shown.contains("Shift+F8"), "{shown}");

        let mut unbound = App::with_fixed_panels()
            .with_config(Ok(config_with("[keys]\ndelete_permanently = []\n")));
        unbound.deleting = Some(deleting);
        let shown_unbound = unbound.delete_failure(0, "denied");
        assert!(shown_unbound.contains("denied"), "{shown_unbound}");
        assert!(
            !shown_unbound.contains("{key}") && !shown_unbound.contains("permanently"),
            "{shown_unbound}"
        );
    }

    fn binding_key_of(typed: &str, physical: Code, modifiers: Modifiers) -> Key {
        binding_key(character(typed), Physical::Code(physical), modifiers)
    }

    #[test]
    fn a_control_character_with_ctrl_becomes_its_letter() {
        assert_eq!(
            binding_key_of("\u{18}", Code::KeyX, Modifiers::CTRL),
            character("x")
        );
        assert_eq!(
            binding_key_of("\u{18}", Code::KeyQ, Modifiers::CTRL),
            character("x"),
            "the layout wins over the physical position"
        );
    }

    fn press_physical(typed: &str, physical: Code, modifiers: Modifiers) -> Option<Message> {
        route_press(
            &PromptKeyState::default(),
            character(typed),
            Physical::Code(physical),
            modifiers,
            false,
        )
    }

    #[test]
    fn a_typed_star_with_ctrl_still_clears_the_tags() {
        assert_eq!(
            press_physical("*", Code::Digit8, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Message::ClearTags)
        );
    }

    #[test]
    fn option_c_cedilla_still_copies_the_path() {
        assert_eq!(
            press_physical("ç", Code::KeyC, Modifiers::ALT | Modifiers::COMMAND),
            Some(Message::CopyToClipboard(ClipboardKind::Path))
        );
    }

    #[test]
    fn the_sort_chord_routes_by_its_digit_or_function_key() {
        let binding = keymap::sort_binding(crate::fs::SortColumn::Name);
        let routed = match &binding.key {
            Key::Character(digit) => press_physical(digit, Code::Digit1, binding.modifiers),
            other => route_key(&PromptKeyState::default(), other.clone(), binding.modifiers),
        };
        assert_eq!(
            routed,
            Some(Message::SortActive(crate::fs::SortColumn::Name))
        );
    }

    #[test]
    fn a_name_with_a_placeholder_in_it_is_shown_as_written() {
        let mut app = App::with_fixed_panels();
        app.deleting = Some(Deletion {
            sources: vec![PathBuf::from("/left/{key}.txt")],
            permanent: false,
            side: PanelSide::Left,
        });
        let shown = app.delete_failure(0, "{name}");
        assert!(shown.contains("{key}.txt"), "{shown}");
        assert!(shown.contains("{name}"), "{shown}");

        let err = ConfigError::Invalid {
            path: PathBuf::from("/cfg/{reason}/config.toml"),
            line: Some(2),
            reason: "bad {line}".into(),
        };
        let config_shown = app.config_error_text(&ConfigProblem::Invalid(err));
        assert!(
            config_shown.contains("/cfg/{reason}/config.toml"),
            "{config_shown}"
        );
        assert!(config_shown.contains("bad {line}"), "{config_shown}");
    }

    #[test]
    fn quit_on_alt_does_not_end_the_program_from_the_path_field() {
        let state = PromptKeyState {
            path_editing: true,
            ..state_for("[keys]\nquit = [\"alt+q\", \"primary+q\"]\n")
        };
        assert_eq!(route_key(&state, character("q"), Modifiers::ALT), None);
        assert_eq!(
            route_key(&state, character("q"), Modifiers::COMMAND),
            Some(Message::Quit)
        );
    }

    #[test]
    fn without_ctrl_alt_or_super_a_typed_character_is_left_alone() {
        assert_eq!(
            binding_key_of("&", Code::Digit1, Modifiers::default()),
            character("&")
        );
        assert_eq!(
            binding_key_of("!", Code::Digit1, Modifiers::SHIFT),
            character("!")
        );
        assert_eq!(
            binding_key_of("¡", Code::Digit1, Modifiers::ALT),
            character("1")
        );
        assert_eq!(
            binding_key_of("*", Code::Digit8, Modifiers::CTRL | Modifiers::SHIFT),
            character("*")
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
mod search_tests {
    use super::*;
    use crate::fs::search::{LineMatch, SearchEvent};
    use crate::search_view::SearchRow;

    fn route(search_open: bool, key: Key, modifiers: Modifiers) -> Option<Message> {
        let state = PromptKeyState {
            search: search_open,
            ..PromptKeyState::default()
        };
        route_key(&state, key, modifiers)
    }

    fn named(named: Named) -> Key {
        Key::Named(named)
    }

    fn character(typed: &str) -> Key {
        Key::Character(typed.into())
    }

    fn found(path: &str, lines: &[u64]) -> SearchEvent {
        SearchEvent::File {
            path: PathBuf::from(path),
            lines: lines
                .iter()
                .map(|&number| LineMatch {
                    number,
                    text: format!("line {number}"),
                    ranges: std::iter::once(0..4).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_status_bar_names_the_cursor_file_only_while_the_search_has_the_keys() {
        let mut app = App::with_fixed_search();
        let shown = app.search_status().unwrap();
        assert!(!shown.is_empty() && !shown.starts_with('/'), "{shown}");

        drop(app.update(Message::SwitchPanel));
        assert_eq!(app.search_status(), None);
    }

    fn app_with_results(files: &[(&str, &[u64])]) -> App {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));
        let generation = app.search_for_test().unwrap().generation;
        let events = files
            .iter()
            .map(|(path, lines)| found(path, lines))
            .collect();
        drop(app.update(Message::Search(SearchMessage::Events {
            generation,
            events,
        })));
        app
    }

    #[derive(Default)]
    struct RecordingLauncher(Mutex<Vec<fs::OpenCommand>>);

    impl fs::Launcher for RecordingLauncher {
        fn launch(&self, command: &fs::OpenCommand) -> Result<(), String> {
            self.0
                .lock()
                .map_err(|poisoned| poisoned.to_string())?
                .push(command.clone());
            Ok(())
        }
    }

    #[test]
    fn the_cursor_keys_move_in_the_results_while_the_search_is_up() {
        let search = |message| Some(Message::Search(message));
        let none = Modifiers::default();
        assert_eq!(
            route(true, named(Named::ArrowDown), none),
            search(SearchMessage::Move(1))
        );
        assert_eq!(
            route(true, named(Named::ArrowUp), none),
            search(SearchMessage::Move(-1))
        );
        assert_eq!(
            route(true, named(Named::PageDown), none),
            search(SearchMessage::Page(1))
        );
        assert_eq!(
            route(true, named(Named::Home), none),
            search(SearchMessage::First)
        );
        assert_eq!(
            route(true, named(Named::End), none),
            search(SearchMessage::Last)
        );
        assert_eq!(
            route(true, named(Named::ArrowRight), none),
            search(SearchMessage::Expand(true))
        );
        assert_eq!(
            route(true, named(Named::ArrowLeft), none),
            search(SearchMessage::Expand(false))
        );
        assert_eq!(
            route(true, named(Named::Enter), none),
            search(SearchMessage::Activate)
        );
        assert_eq!(
            route(false, named(Named::ArrowDown), none),
            Some(Message::MoveSelection(1)),
            "without the search the arrows are the panel's"
        );
    }

    #[test]
    fn a_printable_key_without_a_focused_field_goes_to_the_query() {
        assert_eq!(
            route(true, character("a"), Modifiers::default()),
            Some(Message::Search(SearchMessage::AppendToQuery(
                "a".to_string()
            )))
        );
        assert_eq!(
            route(true, character("A"), Modifiers::SHIFT),
            Some(Message::Search(SearchMessage::AppendToQuery(
                "A".to_string()
            )))
        );
    }

    #[test]
    fn the_keys_that_would_act_on_the_hidden_panel_do_nothing() {
        let none = Modifiers::default();
        for key in [
            named(Named::Backspace),
            named(Named::Tab),
            named(Named::Escape),
            named(Named::F7),
        ] {
            assert_eq!(route(true, key.clone(), none), None, "{key:?}");
        }
    }

    #[test]
    fn the_file_actions_and_quit_still_work_over_the_search() {
        let none = Modifiers::default();
        assert_eq!(
            route(true, named(Named::F5), none),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            route(true, named(Named::F3), none),
            Some(Message::OpenExternal(fs::OpenKind::View))
        );
        assert_eq!(
            route(true, named(Named::F4), none),
            Some(Message::OpenExternal(fs::OpenKind::Edit))
        );
        assert_eq!(route(true, named(Named::F10), none), Some(Message::Quit));
    }

    #[test]
    fn alt_f7_and_the_find_chord_open_the_search_also_over_it() {
        let find = Modifiers::COMMAND | Modifiers::SHIFT;
        for open in [false, true] {
            assert_eq!(
                route(open, named(Named::F7), Modifiers::ALT),
                Some(Message::OpenSearch)
            );
            assert_eq!(route(open, character("F"), find), Some(Message::OpenSearch));
            assert_eq!(route(open, character("f"), find), Some(Message::OpenSearch));
        }
    }

    #[test]
    fn opening_the_search_covers_the_active_panel_and_gives_it_the_keys() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));
        let view = app.search_for_test().unwrap();
        assert_eq!(view.root, app.panel(PanelSide::Left).path);
        assert!(app.search_is_open_for_test());
        assert!(app.prompt_key_state().search);
    }

    #[test]
    fn closing_keeps_the_fields_and_results_and_stops_taking_batches() {
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
        drop(app.update(Message::Search(SearchMessage::QueryChanged(
            "line".to_string(),
        ))));
        let generation = app.search_for_test().unwrap().generation;
        drop(app.update(Message::Search(SearchMessage::Close)));

        assert!(!app.search_is_open_for_test());
        assert!(!app.prompt_key_state().search);
        let view = app.search_for_test().unwrap();
        assert_eq!(view.query, "line");
        assert_eq!(view.results.len(), 1);

        drop(app.update(Message::Search(SearchMessage::Events {
            generation,
            events: vec![found("/home/test/links/late.txt", &[1])],
        })));
        assert_eq!(app.search_for_test().unwrap().results.len(), 1);
    }

    #[test]
    fn a_batch_of_an_older_search_is_dropped() {
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
        let current = app.search_for_test().unwrap().generation;
        drop(app.update(Message::Search(SearchMessage::Events {
            generation: current + 7,
            events: vec![found("/home/test/links/b.txt", &[1])],
        })));
        assert_eq!(app.search_for_test().unwrap().results.len(), 1);
    }

    #[test]
    fn a_pause_that_a_later_keystroke_overtook_starts_nothing() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));
        drop(app.update(Message::Search(SearchMessage::QueryChanged(
            "h".to_string(),
        ))));
        let first = app.search_for_test().unwrap().generation;
        drop(app.update(Message::Search(SearchMessage::QueryChanged(
            "he".to_string(),
        ))));
        assert_ne!(app.search_for_test().unwrap().generation, first);

        drop(app.update(Message::Search(SearchMessage::Debounced(first))));
        assert!(
            !app.search_for_test().unwrap().running,
            "the old timer started a search"
        );
    }

    #[test]
    fn the_right_click_menu_does_not_open_over_the_search() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));
        drop(app.update(Message::ContextMenuAt {
            side: PanelSide::Left,
            index: 1,
        }));
        assert!(app.context_menu_for_test().is_none());
    }

    fn search_message(message: SearchMessage) -> Message {
        Message::Search(message)
    }

    #[test]
    fn file_actions_take_the_tagged_files_else_the_cursors_file() {
        let mut app = app_with_results(&[
            ("/home/test/links/b.txt", &[2]),
            ("/home/test/links/a.txt", &[1, 3]),
            ("/home/test/links/c.txt", &[]),
        ]);
        let a = PathBuf::from("/home/test/links/a.txt");
        let c = PathBuf::from("/home/test/links/c.txt");
        assert_eq!(
            app.action_sources(),
            std::slice::from_ref(&a),
            "not every file found"
        );

        // On a line of a file, the file is the source.
        drop(app.update(search_message(SearchMessage::Move(1))));
        assert_eq!(app.action_sources(), std::slice::from_ref(&a));

        // Tagged files win over the cursor, in display order.
        drop(app.update(search_message(SearchMessage::Last)));
        drop(app.update(search_message(SearchMessage::ToggleTag)));
        drop(app.update(search_message(SearchMessage::First)));
        drop(app.update(search_message(SearchMessage::ToggleTag)));
        assert_eq!(app.action_sources(), [a, c]);

        drop(app.update(Message::Search(SearchMessage::Close)));
        assert_ne!(app.action_sources().len(), 3);
    }

    #[test]
    fn without_a_cursor_and_tags_f8_opens_no_dialog() {
        let mut app = app_with_results(&[]);
        assert!(app.action_sources().is_empty());
        drop(app.update(Message::Delete { permanent: false }));
        assert!(app.delete_dialog.is_none());
        drop(app.update(Message::Delete { permanent: true }));
        assert!(app.delete_dialog.is_none());
    }

    #[test]
    fn f8_over_tagged_results_asks_about_exactly_those() {
        let mut app = app_with_results(&[
            ("/home/test/links/a.txt", &[]),
            ("/home/test/links/b.txt", &[]),
            ("/home/test/links/c.txt", &[]),
        ]);
        drop(app.update(search_message(SearchMessage::ToggleTag)));
        drop(app.update(search_message(SearchMessage::ToggleTag)));
        drop(app.update(Message::Delete { permanent: false }));
        assert_eq!(app.delete_dialog.as_ref().unwrap().sources.len(), 2);
    }

    #[test]
    fn tagging_in_the_results_moves_on_and_a_new_search_forgets_the_tags() {
        let mut app = app_with_results(&[
            ("/home/test/links/a.txt", &[]),
            ("/home/test/links/b.txt", &[]),
        ]);
        drop(app.update(search_message(SearchMessage::ToggleTag)));
        let view = app.search_for_test().unwrap();
        assert!(view.is_tagged(0));
        assert_eq!(view.cursor, Some(SearchRow::File(1)));

        drop(app.update(search_message(SearchMessage::TagAll)));
        assert_eq!(app.search_for_test().unwrap().tagged.len(), 2);
        drop(app.update(search_message(SearchMessage::ClearTags)));
        assert!(app.search_for_test().unwrap().tagged.is_empty());

        drop(app.update(search_message(SearchMessage::TagAll)));
        drop(app.update(search_message(SearchMessage::QueryChanged("x".to_string()))));
        let generation = app.search_for_test().unwrap().generation;
        drop(app.update(search_message(SearchMessage::Debounced(generation))));
        assert!(app.search_for_test().unwrap().tagged.is_empty());
    }

    #[test]
    fn the_tag_keys_tag_in_the_results_instead_of_typing() {
        let none = Modifiers::default();
        let search = |message| Some(search_message(message));
        assert_eq!(
            route(true, named(Named::Space), none),
            search(SearchMessage::ToggleTag)
        );
        assert_eq!(
            route(true, named(Named::Insert), none),
            search(SearchMessage::ToggleTag)
        );
        assert_eq!(
            route(true, named(Named::ArrowDown), Modifiers::SHIFT),
            search(SearchMessage::TagMove(1))
        );
        assert_eq!(
            route(true, named(Named::ArrowUp), Modifiers::SHIFT),
            search(SearchMessage::TagMove(-1))
        );
        assert_eq!(
            route(true, character("*"), Modifiers::SHIFT),
            search(SearchMessage::TagAll)
        );
        assert_eq!(
            route(true, character("*"), Modifiers::CTRL | Modifiers::SHIFT),
            search(SearchMessage::ClearTags)
        );
        assert_eq!(
            route(true, named(Named::ArrowDown), none),
            search(SearchMessage::Move(1)),
            "plain arrows still move"
        );
    }

    fn press(
        search: bool,
        typed: &str,
        physical: keyboard::key::Code,
        modifiers: Modifiers,
    ) -> Option<Message> {
        let state = PromptKeyState {
            search,
            ..PromptKeyState::default()
        };
        route_press(
            &state,
            character(typed),
            keyboard::key::Physical::Code(physical),
            modifiers,
            false,
        )
    }

    #[test]
    fn alt_c_w_r_toggle_the_options_while_the_results_have_the_keys() {
        use keyboard::key::Code;
        // "ç" is what Option+C types on a Mac.
        assert_eq!(
            press(true, "ç", Code::KeyC, Modifiers::ALT),
            Some(search_message(SearchMessage::ToggleCase))
        );
        assert_eq!(
            press(true, "w", Code::KeyW, Modifiers::ALT),
            Some(search_message(SearchMessage::ToggleWholeWord))
        );
        assert_eq!(
            press(true, "®", Code::KeyR, Modifiers::ALT),
            Some(search_message(SearchMessage::ToggleRegex))
        );
    }

    #[test]
    fn ctrl_alt_c_is_altgr_on_windows_and_no_toggle() {
        use keyboard::key::Code;
        let altgr = Modifiers::CTRL | Modifiers::ALT;
        for typed in ["c", "ć"] {
            assert_ne!(
                press(true, typed, Code::KeyC, altgr),
                Some(search_message(SearchMessage::ToggleCase)),
                "{typed}"
            );
        }
    }

    #[test]
    fn alt_c_w_r_are_not_swallowed_in_panel_mode() {
        use keyboard::key::Code;
        for (typed, code) in [("ç", Code::KeyC), ("w", Code::KeyW), ("®", Code::KeyR)] {
            assert_eq!(press(false, typed, code, Modifiers::ALT), None, "{typed}");
        }
    }

    #[test]
    fn a_panel_action_on_alt_c_and_the_case_toggle_each_fire_in_their_own_area() {
        use keyboard::key::{Code, Physical};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[keys]\nmkdir = \"alt+c\"\n").unwrap();
        let keymap = crate::config::load(&path).unwrap().keymap;
        let route = |search, typed| {
            let state = PromptKeyState {
                keymap: Arc::clone(&keymap),
                search,
                ..PromptKeyState::default()
            };
            route_press(
                &state,
                character(typed),
                Physical::Code(Code::KeyC),
                Modifiers::ALT,
                false,
            )
        };
        // Option+C types `ç` on a Mac; the physical key decides.
        for typed in ["c", "ç"] {
            assert_eq!(route(false, typed), Some(Message::CreateDirPrompt));
            assert_eq!(
                route(true, typed),
                Some(search_message(SearchMessage::ToggleCase))
            );
        }
        assert_eq!(
            search_toggle_for_press(
                &keymap,
                character("c"),
                Physical::Code(Code::KeyC),
                Modifiers::ALT
            ),
            Some(SearchMessage::ToggleCase)
        );
    }

    #[test]
    fn the_toggle_lookup_of_a_field_is_the_routings_and_leaves_option_characters_alone() {
        use keyboard::key::{Code, Physical};
        let keymap = Keymap::built_in();
        assert_eq!(
            search_toggle_for_press(
                &keymap,
                character("ç"),
                Physical::Code(Code::KeyC),
                Modifiers::ALT
            ),
            Some(SearchMessage::ToggleCase)
        );
        // Option+5 types "[" on a German Mac, and a regex needs it.
        assert_eq!(
            search_toggle_for_press(
                &keymap,
                character("["),
                Physical::Code(Code::Digit5),
                Modifiers::ALT
            ),
            None
        );
        assert_eq!(
            search_toggle_for_press(
                &keymap,
                character("c"),
                Physical::Code(Code::KeyC),
                Modifiers::default()
            ),
            None
        );
    }

    #[test]
    fn the_function_key_bar_of_the_search_drops_what_does_nothing_there() {
        let app = App::with_fixed_panels();
        assert!(app.function_keys[6].is_some());
        for slot in [2, 3, 4, 5, 7, 8, 9] {
            assert!(app.search_function_keys[slot].is_some(), "F{}", slot + 1);
        }
        assert_eq!(app.search_function_keys[6], None, "F7 makes no directory");
    }

    #[test]
    fn after_a_switch_to_the_other_panel_the_keys_and_actions_are_the_panels() {
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
        drop(app.update(Message::SwitchPanel));

        assert!(!app.prompt_key_state().search);
        assert_eq!(
            route_key(
                &app.prompt_key_state(),
                named(Named::ArrowDown),
                Modifiers::default()
            ),
            Some(Message::MoveSelection(1))
        );
        let other_cursor = app
            .panel(PanelSide::Right)
            .selected_entry()
            .unwrap()
            .path
            .clone();
        assert!(
            app.panel(PanelSide::Right)
                .selected_entry()
                .unwrap()
                .is_parent
        );
        app.panel_mut_for_test(PanelSide::Right).selected = 1;
        let expected = app
            .panel(PanelSide::Right)
            .selected_entry()
            .unwrap()
            .path
            .clone();
        assert_ne!(other_cursor, expected);
        assert_eq!(app.action_sources(), [expected]);

        drop(app.update(Message::ContextMenuAt {
            side: PanelSide::Right,
            index: 1,
        }));
        assert!(app.context_menu_for_test().is_some());
        assert!(app.search_is_open_for_test(), "the search stays up");
    }

    #[test]
    fn opening_the_search_again_brings_the_keys_back_without_changing_the_field() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));
        drop(app.update(Message::OpenSearch));
        assert_eq!(app.search_for_test().unwrap().focused, SearchField::Include);
        drop(app.update(Message::SwitchPanel));
        assert!(!app.prompt_key_state().search);

        drop(app.update(Message::OpenSearch));

        assert!(app.prompt_key_state().search);
        assert_eq!(app.search_for_test().unwrap().focused, SearchField::Include);
    }

    #[test]
    fn using_the_search_makes_its_panel_active_again() {
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
        drop(app.update(Message::SwitchPanel));
        drop(app.update(Message::Search(SearchMessage::RowClicked(0))));
        assert!(app.prompt_key_state().search);

        drop(app.update(Message::SwitchPanel));
        drop(app.update(Message::Search(SearchMessage::ToggleCase)));
        assert!(app.prompt_key_state().search);

        drop(app.update(Message::SwitchPanel));
        let generation = app.search_for_test().unwrap().generation;
        drop(app.update(Message::Search(SearchMessage::Debounced(generation))));
        assert!(
            !app.prompt_key_state().search,
            "a timer is no use of the view"
        );
    }

    #[test]
    fn pressing_the_search_key_again_moves_to_the_next_field() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::OpenSearch));
        assert_eq!(app.search_for_test().unwrap().focused, SearchField::Query);
        drop(app.update(Message::OpenSearch));
        assert_eq!(app.search_for_test().unwrap().focused, SearchField::Include);
        drop(app.update(Message::OpenSearch));
        drop(app.update(Message::OpenSearch));
        assert_eq!(app.search_for_test().unwrap().focused, SearchField::Query);
    }

    #[test]
    fn reopening_searches_again_with_the_same_fields() {
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[1])]);
        drop(app.update(Message::Search(SearchMessage::QueryChanged(
            "line".to_string(),
        ))));
        drop(app.update(Message::Search(SearchMessage::Close)));
        drop(app.update(Message::OpenSearch));
        let view = app.search_for_test().unwrap();
        assert_eq!(view.query, "line");
        assert!(view.running, "the old results are not trusted");
        assert!(view.results.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn opening_the_privacy_settings_launches_the_full_disk_access_page() {
        let launcher = Arc::new(RecordingLauncher::default());
        let mut app =
            App::with_fixed_panels().with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);

        drop(app.update(Message::OpenPrivacySettings));

        let calls = launcher.0.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "/usr/bin/open");
        assert_eq!(
            calls[0].args,
            ["x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"]
        );
    }

    #[test]
    fn enter_on_a_line_opens_the_editor_at_that_line() {
        let launcher = Arc::new(RecordingLauncher::default());
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[7])])
            .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
        app.open_programs = config::OpenPrograms {
            edit: Some(config::ProgramLine::new("editor", &["+{line}"])),
            ..config::OpenPrograms::default()
        };
        drop(app.update(Message::Search(SearchMessage::Move(1))));
        assert_eq!(
            app.search_for_test().unwrap().cursor,
            Some(SearchRow::Line(0, 0))
        );

        drop(app.update(Message::Search(SearchMessage::Activate)));

        let calls = launcher.0.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "editor");
        assert_eq!(calls[0].args[0], "+7");
        assert_eq!(calls[0].args.last().unwrap(), "/home/test/links/a.txt");
    }

    #[test]
    fn a_click_selects_the_row_and_the_chevron_folds_the_file() {
        let mut app = app_with_results(&[("/home/test/links/a.txt", &[1, 2])]);
        drop(app.update(Message::Search(SearchMessage::RowClicked(2))));
        assert_eq!(
            app.search_for_test().unwrap().cursor,
            Some(SearchRow::Line(0, 1))
        );
        drop(app.update(Message::Search(SearchMessage::ToggleGroup(0))));
        assert_eq!(app.search_for_test().unwrap().rows(), [SearchRow::File(0)]);
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
mod quick_open_tests {
    use super::*;

    fn route(state: &PromptKeyState, key: Named, modifiers: Modifiers) -> Option<Message> {
        route_key(state, Key::Named(key), modifiers)
    }

    fn open_state() -> PromptKeyState {
        PromptKeyState {
            quick_open: true,
            ..PromptKeyState::default()
        }
    }

    fn ctrl_p(state: &PromptKeyState) -> Option<Message> {
        route_key(state, Key::Character("p".into()), Modifiers::COMMAND)
    }

    #[test]
    fn the_arrows_move_the_hits_and_the_panel_keys_are_dead_while_it_is_open() {
        let state = open_state();
        let none = Modifiers::default();
        assert_eq!(
            route(&state, Named::ArrowDown, none),
            Some(Message::QuickOpen(QuickOpenMessage::Move(1)))
        );
        assert_eq!(
            route(&state, Named::ArrowUp, none),
            Some(Message::QuickOpen(QuickOpenMessage::Move(-1)))
        );
        assert_eq!(route(&state, Named::F7, none), None);
        assert_eq!(route(&state, Named::F5, none), None);
        assert_eq!(route(&state, Named::Backspace, none), None);
        assert_eq!(route(&state, Named::F10, none), Some(Message::Quit));
    }

    #[test]
    fn the_chord_opens_it_over_the_panels_and_not_over_the_search() {
        let panel = PromptKeyState::default();
        assert_eq!(
            ctrl_p(&panel),
            Some(Message::QuickOpen(QuickOpenMessage::Open))
        );
        let search = PromptKeyState {
            search: true,
            ..PromptKeyState::default()
        };
        assert_eq!(ctrl_p(&search), None);
    }

    #[test]
    fn opening_lists_below_the_active_panel_and_takes_the_keys() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
        let quick_open = app.quick_open_for_test().unwrap();
        assert_eq!(quick_open.root(), app.panel(PanelSide::Left).path);
        assert!(app.prompt_key_state().quick_open);
        assert!(app.blocking_overlay_is_open());
    }

    #[test]
    fn it_does_not_open_over_a_dialog() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::CreateDirPrompt));
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
        assert!(app.quick_open_for_test().is_none());
    }

    #[test]
    fn a_batch_of_a_closed_walk_is_dropped() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
        let old = app.quick_open_for_test().unwrap().walk_generation();
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Close)));
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Open)));
        let root = app.quick_open_for_test().unwrap().root().to_path_buf();
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Events {
            generation: old,
            events: vec![crate::fs::search::SearchEvent::File {
                path: root.join("late.txt"),
                lines: Vec::new(),
            }],
        })));
        assert_eq!(app.quick_open_for_test().unwrap().file_count(), 0);
    }

    #[test]
    fn enter_closes_it_and_loads_the_hit_into_the_active_panel() {
        let mut app = App::with_fixed_panels();
        app.open_quick_open_for_test(&["src/config.rs", "README.md"], "cfgrs");
        assert_eq!(app.quick_open_for_test().unwrap().hits().len(), 1);
        let before = app.panel(PanelSide::Left).request_id;
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
        assert!(app.quick_open_for_test().is_none());
        assert!(app.quick_open_walk.is_none());
        assert!(app.panel(PanelSide::Left).loading);
        assert_ne!(app.panel(PanelSide::Left).request_id, before);
    }

    #[test]
    fn enter_before_the_background_ranking_is_back_still_loads_the_hit() {
        let mut app = App::with_fixed_panels();
        app.open_quick_open_for_test(&["src/config.rs", "README.md"], "");
        // The task that would rank in the background is dropped unrun.
        drop(
            app.update(Message::QuickOpen(QuickOpenMessage::QueryChanged(
                "cfgrs".to_string(),
            ))),
        );
        assert!(app.quick_open_for_test().unwrap().hits().is_empty());
        let before = app.panel(PanelSide::Left).request_id;
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
        assert!(app.quick_open_for_test().is_none());
        assert_ne!(app.panel(PanelSide::Left).request_id, before);
    }

    #[test]
    fn enter_without_a_hit_does_nothing() {
        let mut app = App::with_fixed_panels();
        app.open_quick_open_for_test(&["README.md"], "zzz");
        drop(app.update(Message::QuickOpen(QuickOpenMessage::Activate)));
        assert!(app.quick_open_for_test().is_some());
    }

    #[test]
    fn a_conflict_closes_it() {
        let mut app = App::with_fixed_panels();
        app.open_quick_open_for_test(&["a.txt"], "a");
        let transfer = Transfer {
            kind: TransferKind::Copy,
            sources: vec![PathBuf::from("/left/a.txt")],
            target: PathBuf::from("/right"),
        };
        app.transfer = Some(transfer);
        drop(app.update(Message::TransferRowDone {
            result: Err(RowFailure::Conflict("exists".to_string())),
            index: 0,
            generation: app.generation,
        }));
        assert!(app.pending_conflict.is_some());
        assert!(app.quick_open_for_test().is_none());
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
mod filing_tests {
    use super::*;
    use ncrs_index::InMemoryKeyStore;

    #[test]
    fn a_module_that_is_off_leaves_no_module_and_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("data").join("index.db");
        let store = Arc::new(InMemoryKeyStore::empty());
        let off = Filing {
            enabled: false,
            locations: Vec::new(),
            load: ncrs_pipeline::Load::default(),
            ai: crate::config::AiSetting::Off,
        };

        let (app, _task) = App::with_fixed_panels().with_filing(&off, Some(db.clone()), store);

        assert!(app.filing.is_none());
        assert!(app.pipeline.is_none());
        assert!(!db.exists());
        assert!(!dir.path().join("data").exists());
    }

    #[test]
    fn copying_the_recovery_key_needs_a_shown_key() {
        let mut without_card = App::with_fixed_panels();
        let _nothing = without_card.update(Message::Filing(FilingMessage::CopyRecoveryKey));
        assert!(without_card.notice.is_none());

        let mut app = App::with_fixed_filing(Phase::ShowRecoveryKey {
            key: FilingModule::fixed_recovery_key(),
            group: 2,
            typed: crate::filing::SecretText::default(),
            mismatch: false,
        });
        let _task = app.update(Message::Filing(FilingMessage::CopyRecoveryKey));
        let notice = app.notice.clone().unwrap();
        assert!(!notice.contains("AAAA"), "{notice}");
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
mod filing_setup_tests {
    use super::*;
    use crate::config::FilingLocation;
    use crate::filing_setup::SetupProblem;
    use ncrs_index::InMemoryKeyStore;
    use ncrs_intake::LocationMode;

    fn setup_message(message: FilingSetupMessage) -> Message {
        Message::FilingSetup(message)
    }

    fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: PathBuf::from(path),
            mode,
            images: false,
            is_target,
        }
    }

    fn saved_filing() -> Filing {
        Filing {
            enabled: true,
            locations: vec![
                location("/home/test/inbox", LocationMode::Inbox, false),
                location("/home/test/target", LocationMode::Archive, true),
            ],
            load: Load::default(),
            ai: crate::config::AiSetting::Off,
        }
    }

    fn running_pipeline() -> ncrs_pipeline::Runner {
        ncrs_pipeline::Runner::start(
            ncrs_pipeline::Settings::new(Load::Gentle, NonZeroUsize::MIN),
            Vec::new(),
            Vec::new(),
            Arc::new(ncrs_pipeline::SystemClock),
        )
    }

    /// An app that can start a module: a store, but no data directory, so a
    /// started module fails at once instead of opening an index.
    fn app_that_can_start_a_module() -> App {
        let (mut app, _start) = App::with_fixed_panels().with_filing(
            &Filing::default(),
            None,
            Arc::new(InMemoryKeyStore::empty()),
        );
        assert!(app.filing.is_none(), "the module is off");
        app.filing_setup = Some(FilingSetup::new(&[]));
        app
    }

    fn note(app: &App) -> Option<&SetupNote> {
        app.filing_setup.as_ref().and_then(FilingSetup::note)
    }

    #[test]
    fn the_setup_key_opens_the_setup_with_the_configured_locations_and_closes_it() {
        let mut app = App::with_fixed_panels();
        app.filing_config = saved_filing();

        drop(app.update(setup_message(FilingSetupMessage::Toggle)));
        assert_eq!(
            app.filing_setup.as_ref().unwrap().chosen(),
            saved_filing().locations
        );

        drop(app.update(setup_message(FilingSetupMessage::Toggle)));
        assert!(app.filing_setup.is_none());
    }

    #[test]
    fn escape_closes_the_setup_when_nothing_else_wants_it() {
        let mut app = App::with_filing_setup(&[], false);
        let escape = route_key(
            &app.prompt_key_state(),
            Key::Named(Named::Escape),
            Modifiers::default(),
        );
        assert_eq!(escape, Some(setup_message(FilingSetupMessage::Close)));

        drop(app.update(escape.unwrap()));
        assert!(app.filing_setup.is_none());
        assert_eq!(
            route_key(
                &app.prompt_key_state(),
                Key::Named(Named::Escape),
                Modifiers::default()
            ),
            None
        );
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
    }

    fn pick(app: &mut App, mode: LocationMode, path: &str) {
        drop(app.update(setup_message(FilingSetupMessage::PickFolder(mode))));
        assert!(app.filing_setup.as_ref().unwrap().is_busy());
        let folder = PathBuf::from(path);
        drop(app.update(setup_message(FilingSetupMessage::FolderPicked {
            mode,
            folder: Some(folder.clone()),
        })));
        drop(app.update(setup_message(FilingSetupMessage::Added {
            mode,
            folder: folder.clone(),
            resolved: Ok(folder),
        })));
        assert!(!app.filing_setup.as_ref().unwrap().is_busy());
    }

    #[test]
    fn folders_from_the_dialog_fill_both_sections_without_touching_the_panels() {
        let runtime = runtime();
        let _inside = runtime.enter();
        let mut app = App::with_filing_setup(&[], false);
        let panel_folder = app.active_panel().path.clone();

        pick(&mut app, LocationMode::Inbox, "/home/test/Downloads/Scans");
        pick(&mut app, LocationMode::Inbox, "/home/test/Scanner");
        pick(
            &mut app,
            LocationMode::Archive,
            "/home/test/Documents/Filed",
        );
        pick(
            &mut app,
            LocationMode::Archive,
            "/home/test/Documents/Old-2019",
        );

        let setup = app.filing_setup.as_ref().unwrap();
        let count = |mode| setup.chosen().iter().filter(|l| l.mode == mode).count();
        assert_eq!(count(LocationMode::Inbox), 2);
        assert_eq!(count(LocationMode::Archive), 2);
        assert_eq!(app.active_panel().path, panel_folder);
        assert_eq!(setup.problem(), Some(SetupProblem::ChooseCentralFiling));

        let filed = setup
            .chosen()
            .iter()
            .position(|l| l.path == Path::new("/home/test/Documents/Filed"))
            .unwrap();
        drop(app.update(setup_message(FilingSetupMessage::SetTarget(filed))));
        assert!(app.filing_setup.as_ref().unwrap().can_save());
    }

    #[test]
    fn a_cancelled_dialog_adds_nothing() {
        let mut app = App::with_filing_setup(&[], false);

        drop(app.update(setup_message(FilingSetupMessage::PickFolder(
            LocationMode::Inbox,
        ))));
        assert!(app.filing_setup.as_ref().unwrap().is_busy());
        drop(app.update(setup_message(FilingSetupMessage::FolderPicked {
            mode: LocationMode::Inbox,
            folder: None,
        })));

        let setup = app.filing_setup.as_ref().unwrap();
        assert!(!setup.is_busy());
        assert!(setup.chosen().is_empty());
    }

    #[test]
    fn the_same_folder_twice_gives_a_note_and_adds_nothing() {
        let runtime = runtime();
        let _inside = runtime.enter();
        let mut app = App::with_filing_setup(&[], false);

        pick(
            &mut app,
            LocationMode::Archive,
            "/home/test/Documents/Filed",
        );
        pick(
            &mut app,
            LocationMode::Archive,
            "/home/test/Documents/Filed",
        );

        assert_eq!(app.filing_setup.as_ref().unwrap().chosen().len(), 1);
        assert_eq!(
            note(&app),
            Some(&SetupNote::AlreadyAdded {
                folder: PathBuf::from("/home/test/Documents/Filed")
            })
        );
    }

    #[test]
    fn the_panel_fallback_adds_the_active_panels_folder() {
        let runtime = runtime();
        let _inside = runtime.enter();
        let mut app = App::with_filing_setup(&[], false);
        let folder = app.active_panel().path.clone();

        drop(app.update(setup_message(FilingSetupMessage::AddPanelFolder(
            LocationMode::Archive,
        ))));
        assert!(app.filing_setup.as_ref().unwrap().is_busy());

        drop(app.update(setup_message(FilingSetupMessage::Added {
            mode: LocationMode::Archive,
            folder: folder.clone(),
            resolved: Ok(folder.clone()),
        })));
        let setup = app.filing_setup.as_ref().unwrap();
        assert!(!setup.is_busy());
        assert_eq!(setup.chosen().len(), 1);
        assert_eq!(setup.chosen()[0].path, folder);
        assert_eq!(setup.chosen()[0].mode, LocationMode::Archive);
    }

    #[test]
    fn saving_without_a_config_path_says_so_and_writes_nothing() {
        let mut app = App::with_filing_setup(&saved_filing().locations, false);

        drop(app.update(setup_message(FilingSetupMessage::Save)));

        assert_eq!(note(&app), Some(&SetupNote::NoConfigPath));
        assert!(!app.filing_setup.as_ref().unwrap().is_busy());
    }

    #[test]
    fn a_saved_list_starts_the_module_when_none_was_on() {
        let mut app = app_that_can_start_a_module();

        drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

        assert!(app.filing_setup.is_none(), "saving closes the card");
        let key = app
            .keymap
            .shortcut_label(&Message::FilingStatus(FilingStatusMessage::Toggle))
            .unwrap();
        let notice = app.notice.as_deref().unwrap();
        assert!(notice.contains("Hermes is on"), "{notice}");
        assert!(notice.contains(&key), "{notice}");
        assert!(app.filing.as_ref().unwrap().has_configured_locations());
        assert_eq!(app.filing_config, saved_filing());
    }

    #[test]
    fn a_module_without_locations_is_replaced_and_its_runner_stopped() {
        let mut app = app_that_can_start_a_module();
        app.filing = Some(FilingModule::in_phase(Phase::Ready));
        app.pipeline = Some(running_pipeline());

        drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

        assert!(app.filing_setup.is_none());
        assert!(app.notice.as_deref().unwrap().contains("Hermes is on"));
        assert!(app.pipeline.is_none());
        assert!(app.filing.as_ref().unwrap().has_configured_locations());
    }

    #[test]
    fn a_running_module_with_locations_keeps_them_until_the_next_start() {
        let mut app = app_that_can_start_a_module();
        app.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
        app.pipeline = Some(running_pipeline());

        drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

        assert!(app.filing_setup.is_none());
        assert_eq!(
            app.notice.as_deref(),
            Some(app.lang.text(Msg::FilingSetupSavedNextStart))
        );
        assert!(app.pipeline.is_some());
        assert!(matches!(app.filing.as_ref().unwrap().phase(), Phase::Ready));
        assert_eq!(app.filing_config, saved_filing());
    }

    #[test]
    fn a_module_busy_with_its_start_is_not_replaced() {
        let mut app = app_that_can_start_a_module();
        app.filing = Some(FilingModule::in_phase(Phase::Starting));

        drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

        assert!(app.filing_setup.is_none());
        assert_eq!(
            app.notice.as_deref(),
            Some(app.lang.text(Msg::FilingSetupSavedNextStart))
        );
        assert!(matches!(
            app.filing.as_ref().unwrap().phase(),
            Phase::Starting
        ));
    }

    #[test]
    fn a_failed_save_is_shown_and_changes_nothing() {
        let mut app = app_that_can_start_a_module();
        let failure = FilingWriteError::Rule(crate::config::FilingRuleViolation::NoTarget);

        drop(app.update(setup_message(FilingSetupMessage::Saved(Err(
            failure.clone(),
        )))));

        assert_eq!(note(&app), Some(&SetupNote::SaveFailed(failure)));
        assert!(app.filing.is_none());
        assert_eq!(app.filing_config, Filing::default());
    }

    #[test]
    fn switched_on_without_locations_the_setup_opens_at_start() {
        let store = || -> Arc<dyn KeyStore + Send + Sync> { Arc::new(InMemoryKeyStore::empty()) };
        let without_locations = Filing {
            enabled: true,
            locations: Vec::new(),
            load: Load::default(),
            ai: crate::config::AiSetting::Off,
        };
        let (empty, _empty_start) =
            App::with_fixed_panels().with_filing(&without_locations, None, store());
        assert!(empty.filing_setup.is_some());

        let (configured, _configured_start) =
            App::with_fixed_panels().with_filing(&saved_filing(), None, store());
        assert!(configured.filing_setup.is_none());

        let (off, _off_start) =
            App::with_fixed_panels().with_filing(&Filing::default(), None, store());
        assert!(off.filing_setup.is_none());
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
mod filing_status_tests {
    use std::time::SystemTime;

    use super::*;
    use crate::filing_status::INDEX_FIGURES_EVERY_TICKS;
    use ncrs_pipeline::{
        ErrorEntry, RunnerState, Settings, Snapshot, SystemClock, Task as PipelineTask,
    };

    fn status(message: FilingStatusMessage) -> Message {
        Message::FilingStatus(message)
    }

    fn tick(app: &mut App) {
        drop(app.update(status(FilingStatusMessage::Tick)));
    }

    /// An app whose pipeline has no steps and no folders, standing on 8 cores.
    fn app_with_empty_pipeline() -> App {
        let mut app = App::with_fixed_filing(Phase::Ready);
        app.pipeline = Some(ncrs_pipeline::Runner::start(
            Settings::new(Load::Gentle, NonZeroUsize::new(8).unwrap()),
            Vec::new(),
            Vec::new(),
            Arc::new(SystemClock),
        ));
        app
    }

    fn worker_target(app: &App) -> usize {
        app.pipeline.as_ref().unwrap().snapshot().worker_target
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
    }

    #[test]
    fn pause_and_resume_act_on_the_pipeline_at_once() {
        let mut app = app_with_empty_pipeline();

        drop(app.update(status(FilingStatusMessage::TogglePause)));
        assert_eq!(
            app.pipeline.as_ref().unwrap().snapshot().state,
            RunnerState::Paused
        );

        drop(app.update(status(FilingStatusMessage::TogglePause)));
        assert_ne!(
            app.pipeline.as_ref().unwrap().snapshot().state,
            RunnerState::Paused
        );
    }

    #[test]
    fn the_load_is_set_on_the_pipeline_and_remembered_by_the_module() {
        let mut app = app_with_empty_pipeline();
        assert_eq!(worker_target(&app), 2);

        drop(app.update(status(FilingStatusMessage::SetLoad(Load::Full))));
        assert_eq!(worker_target(&app), 8);
        assert_eq!(app.filing.as_ref().unwrap().load(), Load::Full);

        drop(app.update(status(FilingStatusMessage::SetLoad(Load::Gentle))));
        assert_eq!(worker_target(&app), 2);
        assert_eq!(app.filing.as_ref().unwrap().load(), Load::Gentle);
    }

    #[test]
    fn stepping_the_workers_never_goes_below_one() {
        let mut app = app_with_empty_pipeline();

        drop(app.update(status(FilingStatusMessage::StepWorkers(-1))));
        assert_eq!(worker_target(&app), 1);
        assert_eq!(
            app.filing.as_ref().unwrap().load(),
            Load::Workers(NonZeroUsize::MIN)
        );

        drop(app.update(status(FilingStatusMessage::StepWorkers(-1))));
        assert_eq!(worker_target(&app), 1);
    }

    #[test]
    fn without_a_pipeline_pause_and_load_do_nothing() {
        let mut app = App::with_fixed_filing(Phase::Ready);

        drop(app.update(status(FilingStatusMessage::TogglePause)));
        drop(app.update(status(FilingStatusMessage::SetLoad(Load::Full))));
        drop(app.update(status(FilingStatusMessage::StepWorkers(1))));

        assert!(app.pipeline.is_none());
        assert_eq!(app.filing.as_ref().unwrap().load(), Load::default());
    }

    #[test]
    fn index_figures_are_asked_for_on_opening_and_then_every_fifth_tick() {
        let runtime = runtime();
        let _inside = runtime.enter();
        let mut app = App::with_fixed_filing(Phase::Ready);

        drop(app.update(status(FilingStatusMessage::Open)));
        assert!(app.filing_status.figures_in_flight());

        for _ in 0..2 * INDEX_FIGURES_EVERY_TICKS {
            tick(&mut app);
        }
        assert!(app.filing_status.figures_in_flight());

        drop(app.update(status(FilingStatusMessage::IndexFigures(Err(
            "index not open".to_owned(),
        )))));
        assert!(!app.filing_status.figures_in_flight());
        for _ in 1..INDEX_FIGURES_EVERY_TICKS {
            tick(&mut app);
            assert!(!app.filing_status.figures_in_flight());
        }
        tick(&mut app);
        assert!(app.filing_status.figures_in_flight());
    }

    #[test]
    fn index_figures_are_never_asked_for_while_the_page_is_closed() {
        let runtime = runtime();
        let _inside = runtime.enter();
        let mut app = App::with_fixed_filing(Phase::Ready);

        for _ in 0..3 * INDEX_FIGURES_EVERY_TICKS {
            tick(&mut app);
        }
        assert!(!app.filing_status.figures_in_flight());

        drop(app.update(status(FilingStatusMessage::Open)));
        drop(app.update(status(FilingStatusMessage::Close)));
        drop(app.update(status(FilingStatusMessage::IndexFigures(Err(
            "x".to_owned(),
        )))));
        for _ in 0..3 * INDEX_FIGURES_EVERY_TICKS {
            tick(&mut app);
        }
        assert!(!app.filing_status.figures_in_flight());
    }

    #[test]
    fn opening_the_page_is_refused_under_another_overlay() {
        let mut app = App::with_fixed_prompt();

        drop(app.update(status(FilingStatusMessage::Open)));

        assert!(!app.filing_status.page_open());
    }

    #[test]
    fn the_page_owns_the_keys_while_it_is_open() {
        let open = PromptKeyState {
            filing_status: true,
            ..PromptKeyState::default()
        };
        let route = |key| route_key(&open, key, Modifiers::default());

        assert_eq!(
            route(Key::Named(Named::Escape)),
            Some(status(FilingStatusMessage::Close))
        );
        assert_eq!(
            route(Key::Character("p".into())),
            Some(status(FilingStatusMessage::TogglePause))
        );
        assert_eq!(
            route(Key::Character("2".into())),
            Some(status(FilingStatusMessage::SetLoad(Load::Balanced)))
        );
        assert_eq!(
            route(Key::Named(Named::ArrowDown)),
            Some(status(FilingStatusMessage::MoveError(1)))
        );
        assert_eq!(route(Key::Named(Named::F5)), None);
        for chord in [Modifiers::COMMAND, Modifiers::CTRL, Modifiers::ALT] {
            for character in ["p", "1", "+"] {
                assert_eq!(
                    route_key(&open, Key::Character(character.into()), chord),
                    None,
                    "{chord:?}+{character}"
                );
            }
        }
    }

    #[test]
    fn the_binding_toggles_the_page_and_closes_it_again() {
        let chord = Modifiers::COMMAND | Modifiers::SHIFT;
        let key = || Key::Character("d".into());

        assert_eq!(
            route_key(&PromptKeyState::default(), key(), chord),
            Some(status(FilingStatusMessage::Toggle))
        );
        let open = PromptKeyState {
            filing_status: true,
            ..PromptKeyState::default()
        };
        assert_eq!(
            route_key(&open, key(), chord),
            Some(status(FilingStatusMessage::Close))
        );
    }

    #[test]
    fn an_error_row_closes_the_page_and_shows_the_file_in_the_active_panel() {
        let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);
        let before = app.panel(PanelSide::Left).request_id;

        drop(
            app.update(status(FilingStatusMessage::ErrorActivated(PathBuf::from(
                "/tmp/inbox/company-a-invoice.pdf",
            )))),
        );

        assert!(!app.filing_status.page_open());
        assert!(app.panel(PanelSide::Left).loading);
        assert_ne!(app.panel(PanelSide::Left).request_id, before);
    }

    #[test]
    fn enter_shows_the_selected_error_and_does_nothing_without_one() {
        let mut snapshot = Snapshot::off();
        snapshot.errors = vec![ErrorEntry {
            file: PathBuf::from("/tmp/inbox/company-a-invoice.pdf"),
            task: PipelineTask::Step("probe"),
            message: "the step stopped unexpectedly".to_owned(),
            at: SystemTime::UNIX_EPOCH,
        }];
        let mut app = App::with_filing_status_page(snapshot, SystemTime::UNIX_EPOCH);

        drop(app.update(status(FilingStatusMessage::ActivateSelectedError)));
        assert!(app.filing_status.page_open());

        drop(app.update(status(FilingStatusMessage::MoveError(1))));
        drop(app.update(status(FilingStatusMessage::ActivateSelectedError)));
        assert!(!app.filing_status.page_open());
        assert!(app.panel(PanelSide::Left).loading);
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
mod command_palette_tests {
    use super::*;
    use crate::fs::FileEntry;
    use std::ffi::OsString;

    fn chord() -> Modifiers {
        Modifiers::COMMAND | Modifiers::SHIFT
    }

    fn palette_message(message: CommandPaletteMessage) -> Message {
        Message::CommandPalette(message)
    }

    fn open_palette(app: &mut App) {
        drop(app.update(palette_message(CommandPaletteMessage::Toggle)));
    }

    fn type_into_palette(app: &mut App, text: &str) {
        drop(
            app.update(palette_message(CommandPaletteMessage::QueryChanged(
                text.to_owned(),
            ))),
        );
    }

    fn route(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
        route_key(&app.prompt_key_state(), key, modifiers)
    }

    fn app_with_files() -> App {
        let mut app = App::with_environment(Environment::isolated()).0;
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

    fn app_with_keys(content: &str) -> App {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        App::with_fixed_panels().with_config(Ok(config::load(&path).unwrap()))
    }

    #[test]
    fn the_default_key_does_not_collide_with_another_default() {
        let keymap = Keymap::default();
        for scope in [KeyScope::Panel, KeyScope::Search] {
            assert_eq!(
                keymap.map_key(&Key::Character("p".into()), chord(), scope),
                Some(palette_message(CommandPaletteMessage::Toggle)),
                "{scope:?}"
            );
        }
    }

    #[test]
    fn the_chord_opens_it_over_the_panels_and_over_the_search() {
        let key = || Key::Character("P".into());
        let panel = PromptKeyState::default();
        let search = PromptKeyState {
            search: true,
            ..PromptKeyState::default()
        };
        for state in [&panel, &search] {
            assert_eq!(
                route_key(state, Key::Character("p".into()), chord()),
                Some(palette_message(CommandPaletteMessage::Toggle))
            );
        }
        // Shift makes the letter a capital on most layouts.
        assert_eq!(
            route_key(&panel, key(), chord()),
            Some(palette_message(CommandPaletteMessage::Toggle))
        );
    }

    #[test]
    fn opening_lists_the_actions_and_takes_the_keys() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        let palette = app.command_palette_for_test().unwrap();
        assert!(palette.hits().next().is_some());
        assert!(app.prompt_key_state().command_palette);
        assert!(app.blocking_overlay_is_open());
    }

    #[test]
    fn the_search_view_gets_the_actions_of_the_search() {
        let mut app = App::with_fixed_search();
        open_palette(&mut app);
        let palette = app.command_palette_for_test().unwrap();
        let regex = palette
            .hits()
            .filter_map(|hit| palette.entry(hit))
            .any(|entry| entry.action == keymap::KeyAction::SearchRegex);
        assert!(regex);
    }

    #[test]
    fn the_arrows_move_and_the_other_bindings_are_dead_while_it_is_open() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        let none = Modifiers::default();
        assert_eq!(
            route(&app, Key::Named(Named::ArrowDown), none),
            Some(palette_message(CommandPaletteMessage::Move(1)))
        );
        assert_eq!(
            route(&app, Key::Named(Named::ArrowUp), none),
            Some(palette_message(CommandPaletteMessage::Move(-1)))
        );
        assert_eq!(
            route(&app, Key::Named(Named::Enter), none),
            Some(palette_message(CommandPaletteMessage::Run))
        );
        assert_eq!(
            route(&app, Key::Named(Named::Escape), none),
            Some(palette_message(CommandPaletteMessage::Close))
        );
        for key in [Named::F5, Named::F7, Named::F8, Named::F9, Named::Backspace] {
            assert_eq!(route(&app, Key::Named(key), none), None, "{key:?}");
        }
        assert_eq!(route(&app, Key::Character("q".into()), none), None);
        assert_eq!(
            route(&app, Key::Named(Named::F10), none),
            Some(Message::Quit)
        );
    }

    #[test]
    fn f8_with_the_palette_open_opens_no_delete_dialog() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        let routed = route(&app, Key::Named(Named::F8), Modifiers::default());
        assert_eq!(routed, None);
        assert!(app.delete_dialog.is_none());
    }

    #[test]
    fn typing_filters_the_entries() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        type_into_palette(&mut app, "inspection");
        let palette = app.command_palette_for_test().unwrap();
        assert_eq!(palette.hits().count(), 1);
        assert_eq!(palette.query(), "inspection");
    }

    #[test]
    fn enter_on_filing_status_opens_the_page_and_closes_the_palette() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        type_into_palette(&mut app, "inspection");
        drop(app.update(palette_message(CommandPaletteMessage::Run)));
        assert!(app.command_palette_for_test().is_none());
        assert!(app.filing_status.page_open());
        assert!(!app.prompt_key_state().command_palette);
    }

    #[test]
    fn a_click_on_an_entry_runs_it() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        type_into_palette(&mut app, "inspection");
        drop(app.update(palette_message(CommandPaletteMessage::Select(1))));
        assert!(app.command_palette_for_test().is_none());
        assert!(app.filing_status.page_open());
    }

    #[test]
    fn run_without_a_match_keeps_the_palette_open() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        type_into_palette(&mut app, "zzzzzz");
        drop(app.update(palette_message(CommandPaletteMessage::Run)));
        assert!(app.command_palette_for_test().is_some());
    }

    #[test]
    fn a_menu_entry_does_what_the_same_entry_in_the_lab_does() {
        let mut lab = App::with_fixed_panels();
        open_palette(&mut lab);
        type_into_palette(&mut lab, "switch panel");
        drop(lab.update(palette_message(CommandPaletteMessage::Run)));

        let mut menu = App::with_fixed_panels();
        let message = crate::menu::command_message(KeyAction::SwitchPanel.name()).unwrap();
        drop(menu.update(message));

        assert_eq!(menu.active_panel, PanelSide::Right);
        assert_eq!(menu.active_panel, lab.active_panel);
        assert_eq!(menu.recent_commands, [KeyAction::SwitchPanel]);
        assert_eq!(menu.recent_commands, lab.recent_commands);
    }

    #[test]
    fn the_menu_is_off_while_a_box_is_open_and_follows_the_search() {
        let mut app = App::with_fixed_panels();
        assert_eq!(app.menu_availability(), Availability::Open(KeyScope::Panel));
        open_palette(&mut app);
        let blocked = app.menu_availability();
        assert_eq!(blocked, Availability::Blocked);
        assert!(blocked.enables(KeyAction::Quit));
        assert!(!blocked.enables(KeyAction::Mkdir));
        drop(app.update(palette_message(CommandPaletteMessage::Close)));
        drop(app.update(Message::OpenSearch));
        assert_eq!(
            app.menu_availability(),
            Availability::Open(KeyScope::Search)
        );
    }

    #[test]
    fn the_commands_run_last_head_the_list_next_time() {
        let mut app = App::with_fixed_panels();
        for query in ["switch panel", "tag all", "switch panel"] {
            open_palette(&mut app);
            type_into_palette(&mut app, query);
            drop(app.update(palette_message(CommandPaletteMessage::Run)));
        }
        open_palette(&mut app);
        let palette = app.command_palette_for_test().unwrap();
        let first_hits: Vec<keymap::KeyAction> = palette
            .rows()
            .iter()
            .skip(1)
            .take(2)
            .filter_map(|row| match row {
                crate::command_palette::Row::Hit(hit) => palette.entry(hit),
                crate::command_palette::Row::Header(_) => None,
            })
            .map(|entry| entry.action)
            .collect();
        assert_eq!(
            first_hits,
            [keymap::KeyAction::SwitchPanel, keymap::KeyAction::TagAll]
        );
        assert_eq!(
            palette.selected_action(),
            Some(keymap::KeyAction::SwitchPanel)
        );
    }

    #[test]
    fn mkdir_from_the_palette_leaves_the_prompt_open() {
        let mut app = App::with_fixed_panels();
        open_palette(&mut app);
        type_into_palette(&mut app, "create folder");
        drop(app.update(palette_message(CommandPaletteMessage::Run)));
        assert!(app.command_palette_for_test().is_none());
        assert!(app.prompt.is_some(), "the prompt did not open");
        assert!(app.prompt_key_state().open);
    }

    #[test]
    fn a_rebound_key_shows_in_the_entry() {
        let mut app = app_with_keys("[keys]\nfiling_status = \"F11\"\n");
        open_palette(&mut app);
        let palette = app.command_palette_for_test().unwrap();
        let entry = palette
            .hits()
            .filter_map(|hit| palette.entry(hit))
            .find(|entry| entry.action == keymap::KeyAction::FilingStatus)
            .unwrap();
        assert_eq!(entry.shortcut.as_deref(), Some("F11"));
    }

    #[test]
    fn an_action_without_a_key_has_no_key_in_its_entry() {
        let mut app = app_with_keys("[keys]\ndelete_permanently = []\n");
        open_palette(&mut app);
        let palette = app.command_palette_for_test().unwrap();
        let entry = palette
            .hits()
            .filter_map(|hit| palette.entry(hit))
            .find(|entry| entry.action == keymap::KeyAction::DeletePermanently)
            .unwrap();
        assert_eq!(entry.shortcut, None);
    }

    #[tokio::test]
    async fn escape_closes_it_and_the_running_job_keeps_going() {
        let mut app = app_with_files();
        let _started = app.update(Message::Transfer(TransferKind::Copy));
        assert!(app.jobs.is_busy(), "no job to keep");
        open_palette(&mut app);
        assert!(app.command_palette_for_test().is_some());

        let routed = route(&app, Key::Named(Named::Escape), Modifiers::default());
        assert_eq!(routed, Some(palette_message(CommandPaletteMessage::Close)));
        let _closed = app.update(routed.unwrap());

        assert!(app.command_palette_for_test().is_none());
        assert!(app.jobs.is_busy(), "Escape stopped the job");
    }

    #[test]
    fn the_input_state_closes_it_on_escape_too() {
        let mut input = InputState {
            command_palette: true,
            job_running: true,
            ..InputState::default()
        };
        let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            modified_key: Key::Named(Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::default(),
            repeat: false,
            text: None,
        });
        assert_eq!(
            input.handle(&escape, event::Status::Ignored),
            Some(palette_message(CommandPaletteMessage::Close))
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
mod theme_fetch_tests {
    use super::*;
    use crate::palette::ThemePreset;
    use crate::vsix::tests::{manifest, package, THEME_JSON};

    const CONFIG: &str = "/example/config.toml";

    fn extension(themes: &[(&str, &str, &str)]) -> FetchedExtension {
        let manifest = manifest(themes);
        let vsix = package(&[
            ("package.json", manifest.as_bytes()),
            ("themes/dark.json", THEME_JSON.as_bytes()),
            ("themes/light.json", THEME_JSON.as_bytes()),
        ]);
        FetchedExtension {
            id: ExtensionId::parse("example.dark-themes").unwrap(),
            display_name: "Example Themes".to_owned(),
            version: "1.2.3".to_owned(),
            license: Some("MIT".to_owned()),
            vsix: vsix.into(),
        }
    }

    fn two_themes(_: &ExtensionId) -> Result<FetchedExtension, FetchError> {
        Ok(extension(&[
            ("Example Dark", "vs-dark", "./themes/dark.json"),
            ("Example Light", "vs", "./themes/light.json"),
        ]))
    }

    fn one_theme(_: &ExtensionId) -> Result<FetchedExtension, FetchError> {
        Ok(extension(&[(
            "Example Dark",
            "vs-dark",
            "./themes/dark.json",
        )]))
    }

    fn not_found(id: &ExtensionId) -> Result<FetchedExtension, FetchError> {
        Err(FetchError::NotFound(id.to_string()))
    }

    /// What the real install would hand back, without a file anywhere.
    fn installed(pick: &Pick, _: &Path) -> Result<Box<Installed>, FetchError> {
        Ok(Box::new(Installed {
            config: Config {
                palette: ThemePreset::Classic.palette(),
                ..Config::default()
            },
            label: pick.1.label.clone(),
            license: pick.0.extension.license.clone(),
        }))
    }

    fn warned(pick: &Pick, config_path: &Path) -> Result<Box<Installed>, FetchError> {
        let mut result = installed(pick, config_path)?;
        result.config.vscode_warning = Some(VscodeWarning {
            path: config_path.to_path_buf(),
            line: None,
            theme: PathBuf::from("themes/dark.json"),
            reason: "no colors".to_owned(),
        });
        Ok(result)
    }

    fn fetch_message(message: ThemeFetchMessage) -> Message {
        Message::ThemeFetch(message)
    }

    fn app_fetching_with(fetch: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>) -> App {
        let mut app = App::with_fixed_panels();
        app.config_path = Some(PathBuf::from(CONFIG));
        app.fetch_extension = fetch;
        app.install_theme = installed;
        app
    }

    fn send(app: &mut App, message: ThemeFetchMessage) {
        drop(app.update(fetch_message(message)));
    }

    fn type_id(app: &mut App) {
        send(app, ThemeFetchMessage::Open);
        send(
            app,
            ThemeFetchMessage::TextChanged("example.dark-themes".to_owned()),
        );
    }

    /// Hands the card the answer of the download it started, the way the
    /// task does.
    fn deliver_download(app: &mut App) -> Result<Fetched, FetchError> {
        let id = ExtensionId::parse(app.theme_fetch_for_test().unwrap().text()).unwrap();
        let result = download_theme(app.fetch_extension, &id);
        let generation = app.theme_fetch_generation;
        send(
            app,
            ThemeFetchMessage::Fetched {
                generation,
                result: result.clone(),
            },
        );
        result
    }

    fn deliver_install(app: &mut App, fetched: &Fetched, index: usize) {
        let pick = (fetched.clone(), fetched.themes[index].clone());
        let result = (app.install_theme)(&pick, Path::new(CONFIG));
        let generation = app.theme_fetch_generation;
        send(app, ThemeFetchMessage::Installed { generation, result });
    }

    fn stage(app: &App) -> &Stage {
        app.theme_fetch_for_test().unwrap().stage()
    }

    #[test]
    fn the_palette_entry_opens_the_card_and_it_takes_the_keys() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
        drop(app.update(Message::CommandPalette(
            CommandPaletteMessage::QueryChanged("color theme".to_owned()),
        )));
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

        assert!(app.command_palette_for_test().is_none());
        assert_eq!(stage(&app), &Stage::Input);
        assert!(app.prompt_key_state().theme_fetch);
        assert!(app.blocking_overlay_is_open());
    }

    #[test]
    fn it_is_not_opened_over_another_modal() {
        let mut app = App::with_fixed_panels();
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
        send(&mut app, ThemeFetchMessage::Open);
        assert!(app.theme_fetch_for_test().is_none());
    }

    #[tokio::test]
    async fn two_themes_are_listed_and_the_picked_one_is_installed() {
        let mut app = app_fetching_with(two_themes);
        let before = app.palette;
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        assert_eq!(stage(&app), &Stage::Fetching);

        let fetched = deliver_download(&mut app).unwrap();
        let Stage::Choosing {
            fetched: offered,
            cursor,
        } = stage(&app)
        else {
            panic!("not choosing: {:?}", stage(&app));
        };
        let labels: Vec<_> = offered.themes.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["Example Dark", "Example Light"]);
        assert_eq!(*cursor, 0);

        send(&mut app, ThemeFetchMessage::Move(1));
        send(&mut app, ThemeFetchMessage::Activate);
        assert_eq!(stage(&app), &Stage::Installing);

        deliver_install(&mut app, &fetched, 1);
        assert!(app.theme_fetch_for_test().is_none(), "the card closes");
        assert_ne!(app.palette, before, "the look changes at once");
        assert_eq!(app.palette, ThemePreset::Classic.palette());
        assert_eq!(
            app.notice_for_test(),
            Some("Color theme \"Example Light\" installed from Open VSX (license: MIT).")
        );
        assert_eq!(app.config_error, None);
    }

    #[tokio::test]
    async fn a_click_on_a_theme_installs_it() {
        let mut app = app_fetching_with(two_themes);
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        deliver_download(&mut app).unwrap();

        send(&mut app, ThemeFetchMessage::Pick(1));

        assert_eq!(stage(&app), &Stage::Installing);
    }

    #[tokio::test]
    async fn a_single_theme_is_installed_without_asking() {
        let mut app = app_fetching_with(one_theme);
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);

        let fetched = deliver_download(&mut app).unwrap();
        assert_eq!(stage(&app), &Stage::Installing);

        deliver_install(&mut app, &fetched, 0);
        assert!(app.theme_fetch_for_test().is_none());
        assert!(app
            .notice_for_test()
            .is_some_and(|notice| notice.contains("Example Dark")));
    }

    #[tokio::test]
    async fn a_failed_download_keeps_the_input_and_says_why() {
        let mut app = app_fetching_with(not_found);
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);

        deliver_download(&mut app).unwrap_err();

        let card = app.theme_fetch_for_test().unwrap();
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.text(), "example.dark-themes");
        assert_eq!(
            card.error(),
            Some(&FetchError::NotFound("example.dark-themes".to_owned()))
        );
    }

    #[test]
    fn a_text_that_names_no_extension_fetches_nothing() {
        let mut app = app_fetching_with(two_themes);
        send(&mut app, ThemeFetchMessage::Open);
        send(
            &mut app,
            ThemeFetchMessage::TextChanged("../etc".to_owned()),
        );
        send(&mut app, ThemeFetchMessage::Activate);

        let card = app.theme_fetch_for_test().unwrap();
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.error(), Some(&FetchError::InvalidId));
    }

    #[tokio::test]
    async fn a_late_answer_for_a_closed_card_is_dropped() {
        let mut app = app_fetching_with(two_themes);
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        let generation = app.theme_fetch_generation;
        let result = download_theme(
            two_themes,
            &ExtensionId::parse("example.dark-themes").unwrap(),
        );

        let routed = route_key(
            &app.prompt_key_state(),
            Key::Named(Named::Escape),
            Modifiers::default(),
        );
        assert_eq!(routed, Some(fetch_message(ThemeFetchMessage::Close)));
        drop(app.update(routed.unwrap()));
        assert!(app.theme_fetch_for_test().is_none());

        send(
            &mut app,
            ThemeFetchMessage::Fetched {
                generation,
                result: result.clone(),
            },
        );
        assert!(app.theme_fetch_for_test().is_none(), "nothing reopens");

        // A card opened since is not the one the answer was for.
        send(&mut app, ThemeFetchMessage::Open);
        send(&mut app, ThemeFetchMessage::Fetched { generation, result });
        assert_eq!(stage(&app), &Stage::Input);
    }

    #[tokio::test]
    async fn closing_waits_while_the_theme_is_written() {
        let mut app = app_fetching_with(one_theme);
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        let fetched = deliver_download(&mut app).unwrap();
        assert_eq!(stage(&app), &Stage::Installing);

        send(&mut app, ThemeFetchMessage::Close);
        assert_eq!(stage(&app), &Stage::Installing);

        deliver_install(&mut app, &fetched, 0);
        assert!(app.theme_fetch_for_test().is_none());
    }

    #[tokio::test]
    async fn without_a_config_file_the_theme_cannot_be_installed() {
        let mut app = app_fetching_with(one_theme);
        app.config_path = None;
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);

        deliver_download(&mut app).unwrap();

        let card = app.theme_fetch_for_test().unwrap();
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.error(), Some(&FetchError::NoConfigPath));
    }

    #[tokio::test]
    async fn a_failed_installation_returns_to_the_input() {
        let mut app = app_fetching_with(one_theme);
        let before = app.palette;
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        deliver_download(&mut app).unwrap();
        let generation = app.theme_fetch_generation;
        let error = FetchError::WriteFailed {
            path: CONFIG.to_owned(),
            reason: "disk full".to_owned(),
        };

        send(
            &mut app,
            ThemeFetchMessage::Installed {
                generation,
                result: Err(error.clone()),
            },
        );

        let card = app.theme_fetch_for_test().unwrap();
        assert_eq!(card.stage(), &Stage::Input);
        assert_eq!(card.error(), Some(&error));
        assert_eq!(card.text(), "example.dark-themes");
        assert_eq!(app.palette, before);
    }

    #[tokio::test]
    async fn a_theme_that_was_not_applied_in_full_is_reported_like_a_config_does() {
        let mut app = app_fetching_with(one_theme);
        app.install_theme = warned;
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        let fetched = deliver_download(&mut app).unwrap();

        deliver_install(&mut app, &fetched, 0);

        assert!(app
            .config_error
            .as_deref()
            .is_some_and(|text| text.contains("no colors")));
        assert!(app.notice_for_test().is_some());
    }

    #[tokio::test]
    async fn the_warning_of_the_look_before_goes_away_with_a_clean_install() {
        let mut app = app_fetching_with(one_theme);
        app.report(ConfigProblem::VscodeTheme(VscodeWarning {
            path: PathBuf::from(CONFIG),
            line: None,
            theme: PathBuf::from("old.json"),
            reason: "unreadable".to_owned(),
        }));
        assert!(app.config_error.is_some());
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        let fetched = deliver_download(&mut app).unwrap();

        deliver_install(&mut app, &fetched, 0);

        assert_eq!(app.config_error, None);
    }

    #[tokio::test]
    async fn an_unreadable_config_error_stays_when_a_theme_is_installed() {
        let mut app = app_fetching_with(one_theme);
        app.report(ConfigProblem::Invalid(ConfigError::Unreadable {
            path: PathBuf::from(CONFIG),
            reason: "denied".to_owned(),
        }));
        type_id(&mut app);
        send(&mut app, ThemeFetchMessage::Activate);
        let fetched = deliver_download(&mut app).unwrap();

        deliver_install(&mut app, &fetched, 0);

        assert!(app.config_error.is_some());
    }

    #[test]
    fn the_keys_are_the_cards_while_it_is_up() {
        let mut app = App::with_fixed_panels();
        send(&mut app, ThemeFetchMessage::Open);
        let route = |key: Named| {
            route_key(
                &app.prompt_key_state(),
                Key::Named(key),
                Modifiers::default(),
            )
        };
        assert_eq!(
            route(Named::Enter),
            Some(fetch_message(ThemeFetchMessage::Activate))
        );
        assert_eq!(
            route(Named::ArrowDown),
            Some(fetch_message(ThemeFetchMessage::Move(1)))
        );
        assert_eq!(
            route(Named::ArrowUp),
            Some(fetch_message(ThemeFetchMessage::Move(-1)))
        );
        assert_eq!(
            route(Named::Escape),
            Some(fetch_message(ThemeFetchMessage::Close))
        );
        for dead in [Named::F5, Named::F7, Named::F8, Named::Backspace] {
            assert_eq!(route(dead), None, "{dead:?}");
        }
        assert_eq!(route(Named::F10), Some(Message::Quit));
    }

    #[test]
    fn the_input_state_closes_it_on_escape_too() {
        let mut input = InputState {
            theme_fetch: true,
            job_running: true,
            ..InputState::default()
        };
        let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            modified_key: Key::Named(Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::default(),
            repeat: false,
            text: None,
        });
        assert_eq!(
            input.handle(&escape, event::Status::Ignored),
            Some(fetch_message(ThemeFetchMessage::Close))
        );
    }

    #[test]
    fn holding_an_arrow_moves_on_but_enter_acts_once() {
        assert!(repeats(&fetch_message(ThemeFetchMessage::Move(1))));
        assert!(!repeats(&fetch_message(ThemeFetchMessage::Activate)));
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
mod ocr_models_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Only the tests that never press Download use the counting one.
    static DOWNLOADS: AtomicUsize = AtomicUsize::new(0);

    fn counting_download(_: &Path) -> Result<(), DownloadError> {
        DOWNLOADS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn quiet_download(_: &Path) -> Result<(), DownloadError> {
        Ok(())
    }

    fn send(app: &mut App, message: OcrModelsMessage) {
        drop(app.update(Message::OcrModels(message)));
    }

    fn app_with_data_folder() -> App {
        let mut app = App::with_fixed_panels();
        app.filing_index_path = Some(PathBuf::from("/example/data/index.db"));
        app.download_ocr_models = quiet_download;
        app
    }

    fn route(app: &App, key: Named) -> Option<Message> {
        route_key(
            &app.prompt_key_state(),
            Key::Named(key),
            Modifiers::default(),
        )
    }

    #[test]
    fn the_palette_entry_opens_the_card_and_nothing_is_downloaded() {
        let before = DOWNLOADS.load(Ordering::SeqCst);
        let mut app = app_with_data_folder();
        app.download_ocr_models = counting_download;
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
        drop(app.update(Message::CommandPalette(
            CommandPaletteMessage::QueryChanged("text recognition".to_owned()),
        )));
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

        let card = app.ocr_models_for_test().unwrap();
        assert_eq!(
            card.target(),
            Some(Path::new("/example/data/ocr-models")),
            "the folder next to the index"
        );
        assert!(!card.is_downloading());
        assert!(app.blocking_overlay_is_open());
        assert_eq!(DOWNLOADS.load(Ordering::SeqCst), before);
    }

    #[test]
    fn cancel_and_escape_close_the_card_without_a_download() {
        let before = DOWNLOADS.load(Ordering::SeqCst);
        let mut app = app_with_data_folder();
        app.download_ocr_models = counting_download;
        send(&mut app, OcrModelsMessage::Open);
        assert_eq!(
            route(&app, Named::Escape),
            Some(Message::OcrModels(OcrModelsMessage::Close))
        );
        assert_eq!(
            route(&app, Named::Enter),
            Some(Message::DialogActivate),
            "Enter presses the focused button"
        );

        drop(app.update(Message::DialogActivate));
        assert!(
            app.ocr_models_for_test().is_none(),
            "Cancel has the focus when it opens"
        );
        assert_eq!(DOWNLOADS.load(Ordering::SeqCst), before);
    }

    #[tokio::test]
    async fn the_download_button_starts_one_download_only() {
        let mut app = app_with_data_folder();
        send(&mut app, OcrModelsMessage::Open);
        send(&mut app, OcrModelsMessage::Download);
        assert!(app.ocr_models_for_test().unwrap().is_downloading());

        send(&mut app, OcrModelsMessage::Close);
        assert!(
            app.ocr_models_for_test().is_some(),
            "closing waits while the files are written"
        );
    }

    #[tokio::test]
    async fn a_finished_download_closes_the_card_and_says_so() {
        let mut app = app_with_data_folder();
        send(&mut app, OcrModelsMessage::Open);
        send(&mut app, OcrModelsMessage::Download);
        let generation = app.ocr_models_generation;
        send(
            &mut app,
            OcrModelsMessage::Downloaded {
                generation,
                result: Ok(()),
            },
        );
        assert!(app.ocr_models_for_test().is_none());
        assert_eq!(
            app.notice_for_test(),
            Some("Text recognition models downloaded. Scanned pages are read from now on.")
        );
    }

    #[tokio::test]
    async fn a_failed_download_stays_on_the_card_and_can_be_tried_again() {
        let mut app = app_with_data_folder();
        send(&mut app, OcrModelsMessage::Open);
        send(&mut app, OcrModelsMessage::Download);
        let generation = app.ocr_models_generation;
        send(
            &mut app,
            OcrModelsMessage::Downloaded {
                generation,
                result: Err(DownloadError::Network(FetchError::Timeout)),
            },
        );
        let card = app.ocr_models_for_test().unwrap();
        assert!(!card.is_downloading());
        assert_eq!(
            card.error(),
            Some(&DownloadError::Network(FetchError::Timeout))
        );
        send(&mut app, OcrModelsMessage::Download);
        assert!(app.ocr_models_for_test().unwrap().is_downloading());
    }

    #[test]
    fn an_answer_for_a_closed_card_is_dropped() {
        let mut app = app_with_data_folder();
        send(&mut app, OcrModelsMessage::Open);
        let stale = app.ocr_models_generation;
        send(&mut app, OcrModelsMessage::Close);
        send(&mut app, OcrModelsMessage::Open);
        send(
            &mut app,
            OcrModelsMessage::Downloaded {
                generation: stale,
                result: Ok(()),
            },
        );
        assert!(app.ocr_models_for_test().is_some());
        assert_eq!(app.notice_for_test(), None);
    }

    #[test]
    fn without_a_data_folder_the_button_says_why() {
        let mut app = App::with_fixed_panels();
        app.filing_index_path = None;
        send(&mut app, OcrModelsMessage::Open);
        send(&mut app, OcrModelsMessage::Download);
        let card = app.ocr_models_for_test().unwrap();
        assert_eq!(card.error(), Some(&DownloadError::NoDataFolder));
        assert!(!card.is_downloading());
    }

    #[test]
    fn it_is_not_opened_over_another_modal() {
        let mut app = app_with_data_folder();
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
        send(&mut app, OcrModelsMessage::Open);
        assert!(app.ocr_models_for_test().is_none());
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
mod ai_model_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static DOWNLOADS: AtomicUsize = AtomicUsize::new(0);

    fn counting_download(_: &DownloadJob) -> Result<(), AiDownloadError> {
        DOWNLOADS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn quiet_download(_: &DownloadJob) -> Result<(), AiDownloadError> {
        Ok(())
    }

    fn send(app: &mut App, message: AiModelMessage) {
        drop(app.update(Message::AiModel(message)));
    }

    fn app_with_data_folder() -> App {
        let mut app = App::with_fixed_panels();
        app.filing_index_path = Some(PathBuf::from("/example/data/index.db"));
        app.download_ai_model = quiet_download;
        app
    }

    #[test]
    fn the_palette_entry_opens_the_card_and_nothing_is_downloaded() {
        let before = DOWNLOADS.load(Ordering::SeqCst);
        let mut app = app_with_data_folder();
        app.download_ai_model = counting_download;
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
        drop(app.update(Message::CommandPalette(
            CommandPaletteMessage::QueryChanged("Download AI model".to_owned()),
        )));
        drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

        let card = app.ai_model_for_test().unwrap();
        assert_eq!(
            card.target(),
            Some(Path::new("/example/data/ai-models")),
            "the folder next to the index"
        );
        assert!(!card.is_downloading());
        assert!(app.blocking_overlay_is_open());
        assert_eq!(DOWNLOADS.load(Ordering::SeqCst), before);
    }

    #[test]
    fn escape_closes_the_card_and_enter_presses_cancel() {
        let mut app = app_with_data_folder();
        send(&mut app, AiModelMessage::Open);
        let route = |current: &App, key: Named| {
            route_key(
                &current.prompt_key_state(),
                Key::Named(key),
                Modifiers::default(),
            )
        };
        assert_eq!(
            route(&app, Named::Escape),
            Some(Message::AiModel(AiModelMessage::Close))
        );
        drop(app.update(Message::DialogActivate));
        assert!(app.ai_model_for_test().is_none());
    }

    #[tokio::test]
    async fn the_download_button_starts_one_download_and_closing_waits() {
        let mut app = app_with_data_folder();
        send(&mut app, AiModelMessage::Open);
        send(&mut app, AiModelMessage::Download);
        assert!(app.ai_model_for_test().unwrap().is_downloading());
        send(&mut app, AiModelMessage::Close);
        assert!(app.ai_model_for_test().is_some());
    }

    #[tokio::test]
    async fn a_finished_download_closes_the_card_and_says_so() {
        let mut app = app_with_data_folder();
        send(&mut app, AiModelMessage::Open);
        send(&mut app, AiModelMessage::Download);
        let generation = app.ai_model_generation;
        send(
            &mut app,
            AiModelMessage::Downloaded {
                generation,
                result: Ok(()),
            },
        );
        assert!(app.ai_model_for_test().is_none());
        assert_eq!(
            app.notice_for_test(),
            Some("AI model downloaded. Documents the rules cannot read are passed to it from now on.")
        );
    }

    #[test]
    fn an_answer_for_a_closed_card_is_dropped() {
        let mut app = app_with_data_folder();
        send(&mut app, AiModelMessage::Open);
        let stale = app.ai_model_generation;
        send(&mut app, AiModelMessage::Close);
        send(&mut app, AiModelMessage::Open);
        send(
            &mut app,
            AiModelMessage::Downloaded {
                generation: stale,
                result: Ok(()),
            },
        );
        assert!(app.ai_model_for_test().is_some());
        assert_eq!(app.notice_for_test(), None);
    }

    #[test]
    fn without_a_data_folder_the_button_says_why() {
        let mut app = App::with_fixed_panels();
        app.filing_index_path = None;
        send(&mut app, AiModelMessage::Open);
        send(&mut app, AiModelMessage::Download);
        let card = app.ai_model_for_test().unwrap();
        assert_eq!(card.error(), Some(&AiDownloadError::NoDataFolder));
        assert!(!card.is_downloading());
    }
}
