//! Root application: state, `update` (the only place state changes) and
//! `view` (pure composition of UI components).
use std::path::{Path, PathBuf};
use std::rc::Rc;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use iced::futures;
use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::widget::{
    column, container, mouse_area, operation, row, Column, Container, Space, Stack,
};
use iced::{event, mouse, window, Element, Length, Point, Size, Subscription, Task, Theme};

use crate::about::{AboutCard, REPOSITORY_URL};
use crate::ai_model::{
    download_ai_model, model_is_installed, AiModelCard, DownloadError as AiDownloadError,
    DownloadJob, InstallOutcome,
};
use crate::clipboard_paste::{self, FileListWriter, PastedContent};
use crate::command_line::{self, CommandLine, CommandLineAction, CommandLineCtx};
#[cfg(test)]
use crate::config::FilingWriteError;
use crate::config::ServerEntry;
use crate::config::{
    self, Config, ConfigError, ContrastWarning, Filing, KindWarning, LanguageWarning, VscodeWarning,
};
use crate::context_action_message::ContextActionMessage as _;
#[cfg(test)]
use crate::context_menu::ContextAction;
use crate::context_menu::{ContextMenu, HermesAction};
use crate::dialog::{Prompt, PromptKind};
use crate::execute;
use crate::file_list_tags::{FileListTags, TagExceptions};
#[cfg(test)]
use crate::filing::Phase;
use crate::filing::{DocumentFinder, FilingMessage, FilingModule};
#[cfg(test)]
use crate::filing_pieces::TargetEdit;
use crate::filing_setup;
#[cfg(test)]
use crate::filing_setup::{FilingSetup, SetupNote};
use crate::filing_status::SNAPSHOT_EVERY;
use crate::filing_work::FilingSummary;
use crate::fs;
use crate::fs::remote::credentials::KeyringPasswords;
use crate::fs::remote::credentials::PasswordStore;
use crate::fs::remote::session::Pool;
use crate::fs::remote::{KnownHostsFiles, RemotePath, ServerId};
#[cfg(test)]
use crate::fs::CreateDirError;
use crate::hermes_column::{role_of, ColumnView, FolderRole, HermesFileState};
#[cfg(test)]
use crate::hermes_overview::{ModuleState, Protection};
use crate::hermes_overview::{OverviewFacts, OverviewState};
#[cfg(test)]
use crate::hermes_setup::{HermesSetup, SetupFocus, SetupOffer};
use crate::hermes_status::HermesStatus;
use crate::i18n::{Language, Msg};
use crate::input_state::InputState;
use crate::jobs::{self, JobEvent};
use crate::key_bindings::KeyBindings;
use crate::keymap::{self, KeyAction, KeyScope, Keymap};
use crate::keymap::{KeyActionMessage as _, KeymapMessages as _};
#[cfg(any(target_os = "macos", test))]
use crate::menu::Availability;
use crate::messages::{
    AboutMessage, AiModelMessage, ClipboardKind, CommandLineMessage, CommandPaletteMessage,
    ConflictChoice, FilingSetupMessage, FilingStatusMessage, GrantMessage, HermesFileMessage,
    HermesFocus, HermesOverviewMessage, HermesPage, HermesSetupMessage, HitListMessage,
    KeyBindingsMessage, Message, OcrModelsMessage, PanelSide, QuickFixMessage, QuickOpenMessage,
    RemoteMessage, ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage,
    RulesMessage, SavedQueriesMessage, SearchMessage, SettingChange, SettingsMessage, TagsMessage,
    ThemeFetchMessage, TitleBarMessage, TransferKind, TrashMessage,
};
#[cfg(target_os = "macos")]
use crate::native_menu;
#[cfg(test)]
use crate::ocr_models::OcrModelsCard;
use crate::ocr_models::{download_models, DownloadError};
use crate::open_vsx::{self, ExtensionId, FetchError, FetchedExtension};
use crate::palette::{Palette, ThemePreset};
#[cfg(test)]
use crate::quick_fix::QuickFix;
use crate::quick_open::{
    self, QuickOpen, QuickOpenAction, QuickOpenCtx, WalkEvent, QUICK_OPEN_FILE_LIMIT,
};
#[cfg(test)]
use crate::reorganisation_plan::ReorganisationView;
use crate::reorganisation_work::{ExecutionSummary, PlannedItem};
use crate::replan_work::ReplanSummary;
use crate::review_list::ReviewList;
#[cfg(test)]
use crate::review_work::ReviewItem;
#[cfg(test)]
use crate::rule_suggestions::RuleSuggestions;
use crate::rules_page::RulesPage;
use crate::saved_queries::Draft;
#[cfg(test)]
use crate::saved_queries::SavedQueries;
#[cfg(test)]
use crate::search_filters::SearchFilters;
#[cfg(test)]
use crate::search_view::SearchField;
use crate::search_view::{stays_on_file_system, SearchView};
use crate::session_state::{self, PanelRecord, SessionState, WindowSize, WriteOrder};
use crate::system_dialogs::FilePick;
#[cfg(test)]
use crate::tags_page::TagsPage;
#[cfg(test)]
use crate::theme_fetch::{Fetched, Stage, ThemeFetch};
use crate::theme_fetch::{Installed, Pick};
#[cfg(test)]
use crate::transfer_dialog::TransferDialog;
use crate::transfer_dialog::TransferDialogMessage;
#[cfg(test)]
use crate::transfer_dialog::{self, Policy};
#[cfg(test)]
use crate::trash_view::TrashView;
use crate::ui::about as about_ui;
use crate::ui::ai_model as ai_model_ui;
use crate::ui::command_line as command_line_ui;
use crate::ui::context_menu as context_menu_view;
use crate::ui::delete as delete_dialog_view;
use crate::ui::filing as filing_ui;
use crate::ui::filing_setup as filing_setup_ui;
use crate::ui::grant as grant_ui;
use crate::ui::hermes_flow;
use crate::ui::hermes_overview as hermes_overview_ui;
use crate::ui::hermes_setup as hermes_setup_ui;
use crate::ui::key_bindings as key_bindings_ui;
use crate::ui::ocr_models as ocr_models_ui;
use crate::ui::quick_fix as quick_fix_view;
use crate::ui::quick_open as quick_open_ui;
use crate::ui::remote as remote_ui;
use crate::ui::settings as settings_ui;
use crate::ui::theme_fetch as theme_fetch_ui;
use crate::ui::titlebar;
use crate::ui::transfer as transfer_ui;
use crate::ui::trash as trash_ui;
#[cfg(test)]
use crate::ui::volumes::FavouriteRow;
use crate::ui::volumes::{self as volumes_view, ServerRow};
use crate::ui::{conflict, dialog, fkeys, layout, panel, statusbar, theme, PanelState};
use ncrs_files::context_menu_flow::{HermesCursor, HermesFacts, MenuFacts, MenuNav, MenuOpenCtx};
#[cfg(test)]
use ncrs_files::hit_list::HitList;
use ncrs_files::hit_list_flow::HitListAsk;
use ncrs_files::keys::{ContextMenuKey, VolumeMenuKey};
use ncrs_files::load_flow::{LoadStart, ReloadNow};
use ncrs_files::open_plan::{open_refused_text, CopyOf, OpenPlan, OpenSource};
#[cfg(test)]
use ncrs_files::volumes::VolumeMenu;
use ncrs_files::{PanelMessage, Panels, PendingReload};
use ncrs_hermes_ui::{SavedFilingApplies, SetupCard};
pub use ncrs_i18n::fill_template;
use ncrs_index::{KeyStore, KeyringKeyStore, KindSkip, Kinds, QuerySpec, SavedQueryId};
use ncrs_intake::LocationMode;
use ncrs_ops::keys::{
    Bound as OpsBound, ConflictKey, DialogKey, TransferOverlayKey, TrashBound, TrashKey,
};
use ncrs_ops::prompt_view::PromptMessages;
use ncrs_ops::PanelRows;
#[cfg(test)]
use ncrs_ops::{restore_one, Restoration};
#[cfg(test)]
use ncrs_ops::{Deletion, Extracting, PendingConflict};
pub use ncrs_ops::{RowFailure, Transfer};
use ncrs_palette::command_list::{
    self, view as command_palette_ui, CommandPalette, CommandPaletteAction, CommandPaletteCtx,
    HermesCommands, RECENT_LIMIT,
};
#[cfg(test)]
use ncrs_pipeline::Load;

use ncrs_palette::keys::{CommandLineBound, CommandLineKey};
use ncrs_search::{search_events, SearchKey, SearchKeyFacts};
use ncrs_settings::keys::{SettingsKey, ThemeFetchKey};
#[cfg(test)]
use ncrs_settings::theme_fetch::install::download_theme;
#[cfg(test)]
use ncrs_settings::theme_file::{copy_picked_theme_file, theme_file_entry, IncludeProblem};
use ncrs_settings::Settings;
use ncrs_ui_kit::focus::release_focus;
#[cfg(test)]
use std::ffi::OsString;
use tokio::sync::mpsc;

/// How often the bar of a model download reads the byte counter.
const DOWNLOAD_BAR_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// What a list of search hits has no use for: it is not a folder, so nothing is
/// made, pasted or measured in it.
fn is_not_offered_in_hit_list(message: &Message) -> bool {
    matches!(
        message,
        Message::CreateDirPrompt
            | Message::CreateFilePrompt
            | Message::Paste
            | Message::ComputeFolderSizes
            | Message::PathFieldInput(_)
    )
}

/// What a panel inside an archive refuses: everything that would change or
/// hand over the files in it except deleting and copying into it (both
/// rewrite the archive), anything that takes the panel's path or selection
/// into the config or to Hermes, and an archive inside an archive. Viewing is
/// allowed: it works on a copy, see `open_external`.
/// Keys, the command list and the context menu all end up here; the lists of
/// what may run are `allowed_in_archive` on the three action enums.
fn archive_refusal(panel: &PanelState, message: &Message) -> Option<Msg> {
    panel.archive.as_ref()?;
    match message {
        Message::PackPrompt => Some(Msg::StatusPackUnavailable),
        Message::Transfer(TransferKind::Move)
        | Message::OpenExternal(fs::OpenKind::Edit)
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::ComputeFolderSizes
        | Message::OpenSearch
        | Message::AddFavourite
        | Message::RemoveFavourite
        | Message::QuickFix(QuickFixMessage::Open)
        | Message::FilingSetup(FilingSetupMessage::AddPanelFolder(_))
        | Message::HermesFile(
            HermesFileMessage::HandToInbox
            | HermesFileMessage::ProcessAgain
            | HermesFileMessage::ReprocessFolder
            | HermesFileMessage::SetRole(_)
            | HermesFileMessage::ToggleExclusion
            | HermesFileMessage::ShowInInspection,
        )
        | Message::QuickOpen(QuickOpenMessage::Open) => Some(Msg::StatusArchiveReadOnly),
        Message::Panel(PanelMessage::OpenSelected)
            if panel.selected_entry().is_some_and(|entry| {
                !entry.is_dir && !entry.is_parent && fs::archive::is_archive_name(&entry.name)
            }) =>
        {
            Some(Msg::StatusArchiveNested)
        }
        _ => None,
    }
}

/// What a panel on an SFTP server refuses: what would change something there,
/// the command line, which runs on this computer, and whatever would take the
/// folder into the config or to Hermes. Browsing, viewing and copying are what
/// it does; the lists of what may run are `allowed_on_server` on the three
/// action enums.
fn remote_refusal(panel: &PanelState, message: &Message) -> Option<Msg> {
    panel.remote()?;
    match message {
        Message::CommandLine(CommandLineMessage::Focus) => Some(Msg::StatusRemoteNoShell),
        Message::PackPrompt => Some(Msg::StatusPackUnavailable),
        Message::Transfer(TransferKind::Move)
        | Message::OpenExternal(fs::OpenKind::Edit)
        | Message::Delete { .. }
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::ComputeFolderSizes
        | Message::OpenSearch
        | Message::AddFavourite
        | Message::RemoveFavourite
        | Message::QuickFix(QuickFixMessage::Open)
        | Message::FilingSetup(FilingSetupMessage::AddPanelFolder(_))
        | Message::HermesFile(
            HermesFileMessage::HandToInbox
            | HermesFileMessage::ProcessAgain
            | HermesFileMessage::ReprocessFolder
            | HermesFileMessage::SetRole(_)
            | HermesFileMessage::ToggleExclusion
            | HermesFileMessage::ShowInInspection,
        )
        | Message::QuickOpen(QuickOpenMessage::Open) => Some(Msg::StatusRemoteReadOnly),
        _ => None,
    }
}

/// Where a shell command of this panel runs. A path inside an archive is no
/// directory a process can start in, so it runs in the folder the archive
/// lies in.
fn shell_directory(panel: &PanelState) -> PathBuf {
    panel
        .archive
        .as_deref()
        .and_then(archive_file_place)
        .map_or_else(|| panel.path.clone(), |(folder, _)| folder.to_path_buf())
}

/// The folder an archive lies in and its file name.
fn archive_file_place(archive: &Path) -> Option<(&Path, String)> {
    Some((archive.parent()?, archive.file_name()?.to_str()?.to_owned()))
}

fn quick_open_batch(generation: u64, events: Vec<fs::search::SearchEvent>) -> Message {
    let walk_events = events
        .into_iter()
        .filter_map(|event| match event {
            fs::search::SearchEvent::File { path, .. } => Some(WalkEvent::File { path }),
            fs::search::SearchEvent::Progress { .. } => None,
            fs::search::SearchEvent::Finished(totals) => Some(WalkEvent::Finished {
                truncated: totals.truncated,
            }),
        })
        .collect();
    Message::QuickOpen(QuickOpenMessage::Events {
        generation,
        events: walk_events,
    })
}

/// How long a confirmation stays in the status bar.
/// per file, all in one burst, so the burst becomes one transfer.
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

/// Turns the transfer's channel into a stream `Subscription::run_with` can use.
///
/// tokio's Mutex rather than std's, because the guard is held across an await.
/// There is a single consumer, so the lock is a way of moving the receiver
/// rather than of sharing it. `recv().await` registers the waker with the
/// channel, so an empty channel costs nothing until a tick is sent; waking
/// the task by hand on empty would spin it for as long as the sender lives,
/// and the app keeps one.
fn receiver_stream(
    shared: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>,
) -> impl futures::Stream<Item = fs::transfer::Tick> + 'static {
    futures::stream::unfold(shared, |channel| async move {
        let tick = channel.lock().await.recv().await?;
        Some((tick, channel))
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

/// The user asks for something only Hermes can do, as opposed to the answers
/// to what it was asked before.
fn asks_for_hermes(message: &Message) -> bool {
    matches!(
        message,
        Message::HermesFile(
            HermesFileMessage::HandToInbox
                | HermesFileMessage::ProcessAgain
                | HermesFileMessage::ReprocessFolder
                | HermesFileMessage::SetRole(_)
                | HermesFileMessage::ToggleExclusion
                | HermesFileMessage::ShowInInspection
        ) | Message::QuickFix(QuickFixMessage::Open)
            | Message::RuleSuggestions(RuleSuggestionsMessage::Open)
            | Message::Rules(RulesMessage::Open)
            | Message::ReviewList(ReviewListMessage::Open)
            | Message::SavedQueries(SavedQueriesMessage::Open)
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Open)
            | Message::UndoReorganisation
    )
}

/// Other things the user does end the editing of the path or the command line:
/// the text field loses the focus to them without telling the app.
fn leaves_bar_fields(message: &Message) -> bool {
    matches!(
        message,
        Message::Panel(PanelMessage::RowClicked { .. })
            | Message::Panel(PanelMessage::RowActivated { .. })
            | Message::Transfer(_)
            | Message::OpenSearch
            | Message::QuickOpen(QuickOpenMessage::Open)
            | Message::CommandPalette(CommandPaletteMessage::Toggle)
            | Message::KeyBindings(KeyBindingsMessage::Open)
            | Message::ThemeFetch(ThemeFetchMessage::Open)
            | Message::RuleSuggestions(RuleSuggestionsMessage::Open)
            | Message::Rules(RulesMessage::Open)
            | Message::ReviewList(ReviewListMessage::Open)
            | Message::SavedQueries(SavedQueriesMessage::Open)
            | Message::HermesPage(_)
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Open)
            | Message::OcrModels(OcrModelsMessage::Open)
            | Message::About(AboutMessage::Open)
            | Message::AiModel(AiModelMessage::Open)
            | Message::Grant(GrantMessage::Open)
            | Message::Remote(RemoteMessage::OpenConnect)
            | Message::Settings(_)
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
            | Message::Paste
            | Message::CopyFilesToClipboard
            | Message::Panel(PanelMessage::SwitchPanel)
            | Message::Panel(PanelMessage::ActivatePanel(_))
            | Message::Panel(PanelMessage::SortBy { .. })
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

/// The walk of one folder, off the UI thread; nothing when the run is over.
fn folder_size_task(walk: Option<ncrs_files::folder_size::FolderWalk>) -> Task<Message> {
    let Some(ncrs_files::folder_size::FolderWalk { run, folder, stop }) = walk else {
        return Task::none();
    };
    let walked = folder.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || fs::measure_folder(&walked, &stop)),
        move |done| Message::FolderSizeMeasured {
            run,
            folder: folder.clone(),
            size: done.ok().flatten(),
        },
    )
}

/// Runs one job and reports what happens as messages.
///
/// Separate from `App` so the queue can hand it to `Task::abortable` without
/// borrowing the app. The real work of F5/F6/F8 runs per row in `App`; a job
/// that reaches the queue's `start_next` here has nothing left to do.
fn run_job(_job: jobs::Job) -> Task<Message> {
    Task::done(Message::JobFinished(JobEvent::Done))
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

/// What a click on the "access blocked" error sends. Only macOS has a privacy
/// settings page to open, and only there does the message exist. In the
/// sandbox the click asks for access to the blocked folder instead, since Full
/// Disk Access does not reach into the sandbox.
fn blocked_access_press(sandbox_active: bool, blocked: &Path) -> Option<Message> {
    if sandbox_active {
        return Some(Message::Grant(GrantMessage::OpenFor(blocked.to_path_buf())));
    }
    #[cfg(target_os = "macos")]
    return Some(Message::OpenPrivacySettings);
    #[cfg(not(target_os = "macos"))]
    None
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
    UnknownLanguage(LanguageWarning),
    /// The file is in use, but some `[[kinds]]` entries or words were left out.
    Kinds(KindWarning),
}

pub struct App {
    /// Both panels, which one is active, the type-ahead and the drag between them.
    panels: Panels,
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
    /// Long-running file operations, one at a time.
    jobs: jobs::Queue,
    /// Folder sizes computed this session, by directory and then entry name.
    /// Kept in memory only, so coming back to a folder shows them again.
    /// The question whether to remove the Hermes index and begin again.
    start_over_dialog: bool,
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
    /// What a press on `job_error` sends. Only read while `job_error` is set, and
    /// every `Some` error goes through a setter that replaces it.
    job_error_press: Option<Message>,
    /// A neutral confirmation, such as "Path copied". Goes the way of `job_error`.
    notice: Option<String>,
    /// Counts the notices shown, so a timer can tell if it is still the latest.
    notice_generation: u64,
    /// How many items the running job has finished, for the progress bar.
    job_done: usize,
    /// The state of the transfer, delete, restore and archive flows and the
    /// generation that tells a late result from the current one.
    ops: ncrs_ops::Ops,
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
    /// Folders the user bookmarked, in the order they were added.
    command_line: CommandLine,
    /// What a leading `~` in a typed path stands for.
    home: PathBuf,
    /// The time type-ahead measures against. Injected so tests control it.
    clock: Arc<dyn Fn() -> std::time::Instant + Send + Sync>,
    /// The window's size, for keeping the context menu inside it.
    window_size: Size,
    /// Where the session is remembered; `None` writes nothing.
    state_path: Option<PathBuf>,
    /// The snapshot last scheduled for writing, so a message that changes
    /// nothing worth remembering schedules nothing.
    session_scheduled: SessionState,
    /// Identifies the newest scheduled write; an older timer finds it moved on.
    session_save_generation: u64,
    /// Shared with the background writes so none lands after the one at quit.
    session_writes: Arc<WriteOrder>,
    /// The pointer and the Option tap, fed by the raw event subscription.
    input: Arc<InputCell>,
    /// Where the drive menu gets its list. Injected so tests offer their own
    /// directories instead of the machine's disks.
    list_volumes: Arc<dyn Fn() -> Vec<fs::Volume> + Send + Sync>,
    /// The search: its view, which is kept when it is closed, and its run.
    search: ncrs_search::Search,
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
    /// The time the status page measures how long a worker has been busy by.
    wall_clock: Arc<dyn ncrs_pipeline::Clock>,
    /// Where the filing setup writes. `None` when the system names no place.
    config_path: Option<PathBuf>,
    /// Which tags the file panels show after a file name.
    file_list_tags: FileListTags,
    /// The document kinds the config asks for, on top of the built-in ones.
    kinds: Kinds,
    /// The settings tab, while it is open.
    settings: Settings,
    /// The pages of Hermes that its crate owns and the quick-fix menu.
    hermes: ncrs_hermes_ui::Hermes,
    /// The preset and the VS Code theme file the config names, as last loaded
    /// or written.
    theme_preset: ThemePreset,
    vscode_theme: Option<String>,
    /// What a new filing module opens and keeps its key in; set by
    /// `with_filing`. Without a store no module is started.
    /// Moves the dots of the picture on the setup card.
    hermes_flow_step: u32,
    /// Looks up a folder the setup adds, on a blocking thread. Injected so
    /// tests add their fixed folders without a disk.
    resolve_folder: fn(&Path) -> Result<PathBuf, String>,
    /// Must be called from `update`: on macOS rfd sets up the panel as soon as
    /// the dialog is asked for, and AppKit allows that only on the main thread,
    /// which is the one that runs `update`.
    pick_folder: fn(&Path) -> Task<Option<PathBuf>>,
    /// The file dialog of the settings tab; same rule as `pick_folder`.
    pick_file: fn(FilePick) -> Task<Option<PathBuf>>,
    /// Reads the system clipboard for a paste, off the UI thread. Injected so
    /// tests paste fixed content and never touch the real clipboard.
    read_clipboard: fn() -> Task<Option<PastedContent>>,
    /// Puts files on the system clipboard. A closure rather than a function
    /// pointer: the real one keeps the clipboard open between calls.
    write_clipboard_files: clipboard_paste::ClipboardFileWriter,
    /// The tags the file list shows against their group, from the index.
    tag_exceptions: TagExceptions,
    /// Whether the arrow keys act on the sidebar or the page of the Hermes card.
    hermes_focus: HermesFocus,
    /// Downloads an extension, on a blocking thread. Injected so tests need
    /// no network.
    fetch_extension: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>,
    /// Writes the picked theme and switches it on, on a blocking thread.
    /// Injected so tests write no files.
    install_theme: fn(&Pick, &Path) -> Result<Box<Installed>, FetchError>,
    /// The about card, while it is up.
    about: Option<AboutCard>,
    /// Finds the file with the third-party licenses of this installation.
    /// Injected so tests do not look at the machine they run on.
    find_third_party_licenses: fn() -> Option<PathBuf>,
    /// The version the about card shows.
    app_version: &'static str,
    /// Downloads the OCR models into the folder, on a blocking thread.
    /// Injected so tests need no network.
    download_ocr_models: fn(&Path, &AtomicU64) -> Result<(), DownloadError>,
    /// The dialog that asks for access to the home folder, while it is up.
    grant_dialog: bool,
    /// The App Sandbox side: whether it applies, where grants are kept, and
    /// the checks that touch the system. Injected so tests need no sandbox.
    sandbox: fs::SandboxAccess,
    /// The connections to SFTP servers.
    remote: Arc<Pool>,
    /// Where the passwords the user asked to remember are kept.
    passwords: Arc<dyn PasswordStore>,
    /// The dialog that connects to a server, while it is up, and the count of
    /// its attempts.
    remote_ui: ncrs_remote::Remote,
    /// The servers of `[[servers]]`, as last loaded or written.
    saved_servers: Vec<ServerEntry>,
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
    /// The copy or move confirmation is up: Enter confirms, Tab switches.
    pub transfer_dialog: bool,
    /// The progress mask of a transfer is up: Escape stops the job.
    pub transfer_progress: bool,
    /// The path field is being edited: its keys are the text field's.
    pub path_editing: bool,
    /// The command line has the focus: its field types, Up and Down walk the history.
    pub command_line: bool,
    /// The search view is up, so the cursor keys move in its results.
    pub search: bool,
    /// The trash view is up, so the cursor keys move in its rows and Enter
    /// restores.
    pub trash: bool,
    /// The active panel lists search hits, so Escape goes back to the search.
    pub hit_list: bool,
    /// Quick Open is up, so the up and down keys move in its hits.
    pub quick_open: bool,
    /// The command palette is up, so the up and down keys move in its entries.
    pub command_palette: bool,
    /// The keyboard shortcuts dialog is up: its keys edit the bindings.
    pub key_bindings: bool,
    /// The dialog waits for a key to record: every key but Escape is that key.
    pub key_bindings_recording: bool,
    /// The filing status page is up, so its keys act on the pipeline.
    pub filing_status: bool,
    /// The Hermes overview is up: it has a text field, so only Escape and
    /// its own key close it.
    pub hermes_overview: bool,
    /// The filing setup is up, so Escape closes it.
    pub filing_setup: bool,
    /// The first-time setup of Hermes is up: keys answer it like a button dialog.
    pub hermes_setup: bool,
    /// A filing card that "Not now" may leave is up, so Escape leaves it.
    pub filing_card: bool,
    /// The theme download card is up, so Enter and the arrows are its.
    pub theme_fetch: bool,
    /// The rule suggestions view is up, so Enter, the arrows and its letters are its.
    pub rule_suggestions: bool,
    /// The rules page is up, so Enter, the arrows, Space and Delete are its.
    pub rules: bool,
    /// Its form is up, so the text fields take the keys.
    pub rules_form: bool,
    /// Its delete question is up, so Enter answers it.
    pub rules_confirming_delete: bool,
    /// The review list is up, so Enter, the arrows and its letters are its.
    pub review_list: bool,
    /// Its form is up, so the text fields take the letters.
    pub review_list_editing: bool,
    /// The review list shows the preview of re-planned documents.
    pub review_list_replanning: bool,
    /// The saved queries page is up, so Enter, the arrows and Tab are its.
    pub saved_queries: bool,
    /// The tags page is up, so the arrows, Space and Enter are its.
    pub tags: bool,
    /// The overview's pattern field has the focus, so the letters are the field's.
    pub hermes_overview_editing: bool,
    /// A Hermes page is up and the arrow keys act on its sidebar.
    pub hermes_sidebar: bool,
    /// The reorganisation plan view is up, so its keys are the card's.
    pub reorganisation_plan: bool,
    /// The about card is up, so keys answer it like a button dialog.
    pub about: bool,
    /// The OCR models card is up, so keys answer it like a button dialog.
    pub ocr_models: bool,
    /// The AI model card is up, so keys answer it like a button dialog.
    pub ai_model: bool,
    /// The dialog that asks for access to the home folder is up.
    pub grant_dialog: bool,
    /// The dialog that connects to a server is up, so the text fields take the
    /// typing and Tab moves between them.
    pub connect_dialog: bool,
    /// The settings tab is up, so the panel keys are inert and Escape closes it.
    pub settings: bool,
    /// The delete dialog is up, so keys answer it.
    pub delete_dialog: DeleteKeys,
    /// The question whether to remove the Hermes index is up: keys answer it.
    pub start_over_dialog: bool,
    /// The drive menu is up, so keys move its highlight.
    pub volume_menu: bool,
    /// The context menu is up, so keys move its highlight.
    pub context_menu: bool,
    /// Its Hermes submenu is up too: Escape and Left go back to the menu.
    pub context_submenu: bool,
    /// The quick-fix menu is up, so keys move its highlight.
    pub quick_fix: bool,
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
            transfer_dialog: false,
            transfer_progress: false,
            path_editing: false,
            command_line: false,
            search: false,
            trash: false,
            hit_list: false,
            quick_open: false,
            command_palette: false,
            key_bindings: false,
            key_bindings_recording: false,
            filing_status: false,
            hermes_overview: false,
            filing_setup: false,
            hermes_setup: false,
            filing_card: false,
            theme_fetch: false,
            rule_suggestions: false,
            rules: false,
            rules_form: false,
            rules_confirming_delete: false,
            review_list: false,
            review_list_editing: false,
            review_list_replanning: false,
            saved_queries: false,
            tags: false,
            hermes_overview_editing: false,
            hermes_sidebar: false,
            reorganisation_plan: false,
            about: false,
            ocr_models: false,
            ai_model: false,
            grant_dialog: false,
            connect_dialog: false,
            settings: false,
            delete_dialog: DeleteKeys::default(),
            start_over_dialog: false,
            volume_menu: false,
            context_menu: false,
            context_submenu: false,
            quick_fix: false,
            busy: false,
            typed: Arc::default(),
        }
    }
}

/// What Escape sends while one of the small button dialogs is up: the about
/// card, the OCR models card, the AI model card or the folder grant dialog.
fn card_close_message(prompt: &PromptKeyState) -> Option<Message> {
    if let Some(hermes) = ncrs_hermes_ui::keys::hermes_card_close_message(&hermes_key_facts(prompt))
    {
        Some(hermes.into())
    } else if prompt.about {
        Some(Message::About(AboutMessage::Close))
    } else if prompt.ocr_models {
        Some(Message::OcrModels(OcrModelsMessage::Close))
    } else if prompt.ai_model {
        Some(Message::AiModel(AiModelMessage::Close))
    } else if prompt.grant_dialog {
        Some(Message::Grant(GrantMessage::Later))
    } else {
        None
    }
}

/// What the keys of the Hermes pages read of the prompt state.
fn hermes_key_facts(prompt: &PromptKeyState) -> ncrs_hermes_ui::keys::KeyFacts {
    ncrs_hermes_ui::keys::KeyFacts {
        hermes_sidebar: prompt.hermes_sidebar,
        review_list_editing: prompt.review_list_editing,
        review_list_replanning: prompt.review_list_replanning,
        rules_form: prompt.rules_form,
        rules_confirming_delete: prompt.rules_confirming_delete,
        hermes_overview_editing: prompt.hermes_overview_editing,
        start_over_dialog: prompt.start_over_dialog,
        hermes_setup: prompt.hermes_setup,
    }
}

type PageKeyFn = fn(
    &ncrs_hermes_ui::keys::KeyFacts,
    &ncrs_hermes_ui::keys::KeyPress<'_>,
    &dyn Fn() -> ncrs_hermes_ui::keys::Bound,
) -> Option<ncrs_hermes_ui::keys::PageKey>;

/// What a key does on a page of Hermes. The keymap is asked only for a key
/// the page does not know.
fn hermes_page_key(
    page: PageKeyFn,
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    use ncrs_hermes_ui::keys::{Bound, KeyPress, PageKey};
    let press = KeyPress {
        key,
        modifiers: key_modifiers,
        not_typed_by_text_field: is_not_typed_by_text_field(key, key_modifiers),
    };
    let bound = || match bound_message(&prompt.keymap, key, key_modifiers, scope) {
        Some(Message::Quit) => Bound::Quit,
        Some(Message::FilingStatus(FilingStatusMessage::Toggle)) => Bound::StatusToggle,
        _ => Bound::Other,
    };
    page(&hermes_key_facts(prompt), &press, &bound).map(|pressed| match pressed {
        PageKey::Message(message) => message.into(),
        PageKey::Focus(focus) => Message::HermesFocus(focus),
        PageKey::Step(step) => Message::HermesStep(step),
        PageKey::Quit => Message::Quit,
    })
}

fn card_dialog_key(close: Message, key: Key, key_modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(close),
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
fn conflict_key(
    prompt: &PromptKeyState,
    key: Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let aborts =
        || bound_message(&prompt.keymap, &key, key_modifiers, scope) == Some(Message::AbortJob);
    ncrs_ops::keys::conflict_key(&key, key_modifiers, aborts).map(|pressed| match pressed {
        ConflictKey::Answer(choice) => Message::TransferConflict(choice),
        ConflictKey::ToggleAll => Message::ToggleConflictAll,
        ConflictKey::Dialog(dialog) => dialog_message(dialog),
    })
}

fn transfer_overlay_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let bound = || match bound_message(&prompt.keymap, key, key_modifiers, scope) {
        Some(Message::Quit) => OpsBound::Quit,
        Some(Message::AbortJob) => OpsBound::AbortJob,
        _ => OpsBound::Other,
    };
    ncrs_ops::keys::transfer_overlay_key(
        prompt.transfer_dialog,
        key,
        is_not_typed_by_text_field(key, key_modifiers),
        bound,
    )
    .map(|pressed| match pressed {
        TransferOverlayKey::Dialog(message) => Message::TransferDialog(message),
        TransferOverlayKey::AbortJob => Message::AbortJob,
        TransferOverlayKey::Quit => Message::Quit,
    })
}

fn context_menu_key(prompt: &PromptKeyState, key: &Key) -> Option<Message> {
    ncrs_files::keys::context_menu_key(prompt.context_submenu, key).map(|pressed| match pressed {
        ContextMenuKey::CloseSub => Message::ContextMenuCloseSub,
        ContextMenuKey::Close => Message::ContextMenuClose,
        ContextMenuKey::OpenSub => Message::ContextMenuOpenSub,
        ContextMenuKey::Activate => Message::ContextMenuActivate,
        ContextMenuKey::Move(delta) => Message::ContextMenuMove(delta),
        ContextMenuKey::First => Message::ContextMenuFirst,
        ContextMenuKey::Last => Message::ContextMenuLast,
    })
}

/// The keys of the connect dialog: its fields take the typing and Enter, Tab
/// goes from one to the next, Escape closes it. Of the bindings only quitting
/// reaches past it.
fn connect_dialog_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::Remote(RemoteMessage::Cancel)),
        Key::Named(Named::Enter) => Some(Message::Remote(RemoteMessage::Submit)),
        Key::Named(Named::Tab) => Some(Message::Remote(RemoteMessage::FocusField {
            backwards: key_modifiers.shift(),
        })),
        _ => bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| *message == Message::Quit)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers)),
    }
}

fn theme_fetch_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let quits = || {
        bound_message(&prompt.keymap, key, key_modifiers, scope)
            .is_some_and(|message| message == Message::Quit)
            && is_not_typed_by_text_field(key, key_modifiers)
    };
    Some(
        match ncrs_settings::keys::theme_fetch_key(key, key_modifiers, quits)? {
            ThemeFetchKey::Message(message) => Message::ThemeFetch(message),
            ThemeFetchKey::Quit => Message::Quit,
        },
    )
}

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
        return ncrs_files::keys::volume_menu_key(&key).map(|pressed| match pressed {
            VolumeMenuKey::Close => Message::VolumeMenuClose,
            VolumeMenuKey::Activate => Message::VolumeMenuActivate,
            VolumeMenuKey::Move(delta) => Message::VolumeMenuMove(delta),
            VolumeMenuKey::First => Message::VolumeMenuFirst,
            VolumeMenuKey::Last => Message::VolumeMenuLast,
            VolumeMenuKey::Remove => Message::VolumeMenuRemove,
        });
    }
    if prompt.quick_fix {
        return ncrs_hermes_ui::keys::quick_fix_key(&key).map(Message::from);
    }
    if prompt.context_menu {
        return context_menu_key(prompt, &key);
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

    if prompt.key_bindings {
        return key_bindings_key(prompt, &key, key_modifiers);
    }

    // The fields take the typing and Enter; Tab goes from one to the next. Of
    // the bindings only quitting reaches past the dialog.
    if prompt.connect_dialog {
        return connect_dialog_key(prompt, &key, key_modifiers, scope);
    }

    // Cancel has the focus when a download card opens, so Enter alone never
    // downloads.
    if let Some(close) = card_close_message(prompt) {
        return card_dialog_key(close, key, key_modifiers);
    }

    if prompt.conflict_open {
        return conflict_key(prompt, key, key_modifiers, scope);
    }

    // The target field takes the typing and Enter on its own; Escape is
    // `InputState`'s there and here. Tab switches between copy and move, as the
    // footer says; of the bindings only quitting reaches past the dialog.
    if prompt.transfer_dialog || prompt.transfer_progress {
        return transfer_overlay_key(prompt, &key, key_modifiers, scope);
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
        return hermes_page_key(
            ncrs_hermes_ui::keys::reorganisation_plan_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.rules {
        return hermes_page_key(
            ncrs_hermes_ui::keys::rules_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.rule_suggestions {
        return hermes_page_key(
            ncrs_hermes_ui::keys::rule_suggestions_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.review_list {
        return hermes_page_key(
            ncrs_hermes_ui::keys::review_list_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.saved_queries {
        return hermes_page_key(
            ncrs_hermes_ui::keys::saved_queries_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.tags {
        return hermes_page_key(
            ncrs_hermes_ui::keys::tags_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    // Like the command palette: the field takes the typing, the card the
    // rest. Enter reaches the routing when the list has the keys, not the field.
    if prompt.theme_fetch {
        return theme_fetch_key(prompt, &key, key_modifiers, scope);
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
        return hermes_page_key(
            ncrs_hermes_ui::keys::filing_status_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.hermes_overview {
        return hermes_page_key(
            ncrs_hermes_ui::keys::hermes_overview_key,
            prompt,
            &key,
            key_modifiers,
            scope,
        );
    }

    if prompt.settings {
        return settings_key(prompt, &key, key_modifiers, scope);
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

    if prompt.command_line {
        return command_line_key(prompt, &key, key_modifiers, scope);
    }

    if prompt.trash {
        return trash_key(prompt, &key, key_modifiers);
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

/// Keys while the command line has the focus. The field types characters and
/// digits, so of the bindings only quitting and the key that focused the line
/// reach through, and only as keys the field does not type. Escape is
/// `InputState`'s; it arrives here only when the field lost its own focus.
fn command_line_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let bound = || {
        let Some(message) = bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
        else {
            return CommandLineBound::Other;
        };
        if message == Message::Quit {
            CommandLineBound::Quit
        } else if message == KeyAction::CommandLine.message() {
            CommandLineBound::CommandLine
        } else {
            CommandLineBound::Other
        }
    };
    Some(
        match ncrs_palette::keys::command_line_key(key, key_modifiers, bound)? {
            CommandLineKey::Message(message) => Message::CommandLine(message),
            CommandLineKey::Quit => Message::Quit,
        },
    )
}

/// The keyboard shortcuts dialog: see `ncrs_settings::keys::key_bindings_key`.
fn key_bindings_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
) -> Option<Message> {
    ncrs_settings::keys::key_bindings_key(prompt.key_bindings_recording, key, key_modifiers)
        .map(Message::KeyBindings)
}

/// The tab shows no panel, so the panel keys do nothing. Of the bindings the
/// command list, the key that closes the tab and quitting reach through. A
/// card opened from the tab has the Escape first.
fn settings_key(
    prompt: &PromptKeyState,
    key: &Key,
    key_modifiers: Modifiers,
    scope: KeyScope,
) -> Option<Message> {
    let bound = || {
        bound_message(&prompt.keymap, key, key_modifiers, scope)
            .filter(|message| {
                *message == Message::Quit
                    || *message == KeyAction::CommandPalette.message()
                    || *message == KeyAction::Settings.message()
                    || *message == KeyAction::KeyBindings.message()
            })
            .filter(|_| is_not_typed_by_text_field(key, key_modifiers))
    };
    match ncrs_settings::keys::settings_key(key, || bound().is_some())? {
        SettingsKey::Close if prompt.filing_setup => {
            Some(Message::FilingSetup(FilingSetupMessage::Close))
        }
        SettingsKey::Close => Some(Message::Settings(SettingsMessage::Close)),
        SettingsKey::Bound => bound(),
    }
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
    } else if prompt.hit_list {
        Some(Message::HitList(HitListMessage::BackToSearch))
    } else {
        None
    }
}

/// Keys while the search view is up: see `ncrs_search::search_key`.
fn search_key(prompt: &PromptKeyState, key: &Key, key_modifiers: Modifiers) -> Option<Message> {
    let bound = bound_message(&prompt.keymap, key, key_modifiers, KeyScope::Search);
    let facts = SearchKeyFacts {
        tag_or_toggle: bound.as_ref().and_then(tag_or_toggle),
        bound_untyped: bound.is_some() && is_not_typed_by_text_field(key, key_modifiers),
        type_ahead: match type_ahead_message(key, key_modifiers) {
            Some(Message::Panel(PanelMessage::TypeAhead(typed))) => Some(typed),
            _ => None,
        },
    };
    match ncrs_search::search_key(key, key_modifiers, facts)? {
        SearchKey::Message(message) => Some(Message::Search(message)),
        SearchKey::Bound => bound,
    }
}

/// Keys while the trash view is up: it owns the cursor keys, Enter puts back,
/// and the tagging keys tag its rows. Of the other bindings only the few that
/// leave the view reach through; everything else, F5 and F8 included, does
/// nothing here.
fn trash_key(prompt: &PromptKeyState, key: &Key, key_modifiers: Modifiers) -> Option<Message> {
    let bound = bound_message(&prompt.keymap, key, key_modifiers, KeyScope::Panel);
    let kind = match bound.as_ref() {
        Some(message) => {
            if let Some(tagging) = trash_tagging(message) {
                TrashBound::Tagging(tagging)
            } else if *message == Message::Quit {
                TrashBound::Quit
            } else if *message == Message::Panel(PanelMessage::SwitchPanel) {
                TrashBound::SwitchPanel
            } else if *message == KeyAction::CommandPalette.message() {
                TrashBound::CommandPalette
            } else if *message == KeyAction::RestoreLastDeletion.message() {
                TrashBound::RestoreLast
            } else if *message == KeyAction::OpenTrashInFinder.message() {
                TrashBound::OpenInFinder
            } else {
                TrashBound::Other
            }
        }
        None => TrashBound::Other,
    };
    let pressed = ncrs_ops::keys::trash_key(prompt.job_running, key, key_modifiers, kind)?;
    match pressed {
        TrashKey::Message(message) => Some(Message::Trash(message)),
        TrashKey::AbortJob => Some(Message::AbortJob),
        TrashKey::Bound(_) => bound,
    }
}

/// The trash view's version of a bound tagging message.
fn trash_tagging(message: &Message) -> Option<TrashMessage> {
    match message {
        Message::Panel(PanelMessage::ToggleTag) => Some(TrashMessage::ToggleTag),
        Message::Panel(PanelMessage::TagMove(delta)) => Some(TrashMessage::TagMove(*delta)),
        Message::Panel(PanelMessage::TagAll) => Some(TrashMessage::TagAll),
        Message::Panel(PanelMessage::ClearTags) => Some(TrashMessage::ClearTags),
        _ => None,
    }
}

/// The search's version of a bound tagging or toggle message.
fn tag_or_toggle(message: &Message) -> Option<SearchMessage> {
    match message {
        Message::Panel(PanelMessage::ToggleTag) => Some(SearchMessage::ToggleTag),
        Message::Panel(PanelMessage::TagMove(delta)) => Some(SearchMessage::TagMove(*delta)),
        Message::Panel(PanelMessage::TagAll) => Some(SearchMessage::TagAll),
        Message::Panel(PanelMessage::ClearTags) => Some(SearchMessage::ClearTags),
        Message::Search(toggle) if toggle.is_toggle() => Some(toggle.clone()),
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
        Message::Search(toggle) if toggle.is_toggle() => Some(toggle),
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

fn type_ahead_message(key: &Key, modifiers: Modifiers) -> Option<Message> {
    ncrs_files::keys::type_ahead_message(key, modifiers).map(Message::Panel)
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

fn dialog_focus_key(key: Key, modifiers: Modifiers) -> Option<Message> {
    ncrs_ops::keys::dialog_focus_key(&key, modifiers).map(dialog_message)
}

fn dialog_message(pressed: DialogKey) -> Message {
    match pressed {
        DialogKey::Activate => Message::DialogActivate,
        DialogKey::Focus(step) => Message::DialogFocus(step),
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
        Message::Panel(PanelMessage::MoveSelection(_))
            | Message::Panel(PanelMessage::PageUp)
            | Message::Panel(PanelMessage::PageDown)
            | Message::Panel(PanelMessage::SelectFirst)
            | Message::Panel(PanelMessage::SelectLast)
            | Message::Panel(PanelMessage::ToggleTag)
            | Message::Panel(PanelMessage::TagMove(_))
            | Message::PromptInput(_)
            | Message::CommandLine(
                CommandLineMessage::HistoryPrevious | CommandLineMessage::HistoryNext,
            )
            | Message::VolumeMenuMove(_)
            | Message::VolumeMenuFirst
            | Message::VolumeMenuLast
            | Message::ContextMenuMove(_)
            | Message::ContextMenuFirst
            | Message::ContextMenuLast
            | Message::QuickFix(QuickFixMessage::Move(_))
            | Message::QuickOpen(QuickOpenMessage::Move(_))
            | Message::CommandPalette(CommandPaletteMessage::Move(_))
            | Message::KeyBindings(KeyBindingsMessage::Move(_) | KeyBindingsMessage::MoveKey(_))
            | Message::ThemeFetch(ThemeFetchMessage::Move(_))
            | Message::RuleSuggestions(RuleSuggestionsMessage::Move(_))
            | Message::Rules(RulesMessage::Move(_))
            | Message::ReviewList(ReviewListMessage::Move(_))
            | Message::SavedQueries(SavedQueriesMessage::Move(_))
            | Message::Tags(TagsMessage::Move(_))
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Move(_))
            | Message::HermesStep(_)
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

fn panel_facts(panels: &Panels, side: PanelSide) -> ncrs_ops::PanelFacts<'_> {
    let panel = panels.panel(side);
    ncrs_ops::PanelFacts {
        side,
        path: &panel.path,
        is_virtual: panel.is_virtual(),
        hit_list: panel.hit_list.as_ref().map(|list| ncrs_ops::HitListFacts {
            scope: &list.scope,
            archive_name: ncrs_files::hit_list::hit_archive_name(&list.query),
        }),
        entries: &panel.entries,
    }
}

fn selection_facts(
    panels: &Panels,
    source_side: PanelSide,
    sources: Vec<PathBuf>,
) -> ncrs_ops::SelectionFacts<'_> {
    ncrs_ops::SelectionFacts {
        source: panel_facts(panels, source_side),
        other: panel_facts(panels, source_side.other()),
        sources,
    }
}

fn rows_of(panel: &PanelState) -> PanelRows<'_> {
    PanelRows {
        path: &panel.path,
        entries: &panel.entries,
        selection: &panel.selection,
        cursor: panel.selected_entry(),
    }
}

/// How long the state waits after the last change before it is written. A
/// resize drag or a quick walk through folders writes once.
const SESSION_SAVE_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// Writes the session state. No error reaches the UI: the state is a
/// convenience, and a failed write must not interrupt the user.
fn write_session_state(order: &WriteOrder, generation: u64, path: &Path, state: &SessionState) {
    if let Err(err) = order.save(generation, path, state) {
        eprintln!("ncrs: {}: {err}", path.display());
    }
}

/// Where the app finds its config and index and what holds the index key.
#[derive(Clone)]
pub struct Environment {
    pub config_path: Option<PathBuf>,
    pub index_path: Option<PathBuf>,
    pub state_path: Option<PathBuf>,
    /// The record of what ncrs trashed, where the system gives no list.
    pub journal_path: Option<PathBuf>,
    /// The cache folder F3 unpacks archive files into.
    pub view_copies_dir: Option<PathBuf>,
    pub key_store: Arc<dyn KeyStore + Send + Sync>,
    /// Where the passwords of SFTP servers are kept, when the user asks for it.
    pub passwords: Arc<dyn PasswordStore>,
    /// The user's OpenSSH `known_hosts`, which is only read.
    pub ssh_known_hosts: Option<PathBuf>,
    /// The host keys the user confirmed in ncrs.
    pub own_known_hosts: Option<PathBuf>,
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("config_path", &self.config_path)
            .field("index_path", &self.index_path)
            .field("state_path", &self.state_path)
            .field("journal_path", &self.journal_path)
            .field("view_copies_dir", &self.view_copies_dir)
            .field("ssh_known_hosts", &self.ssh_known_hosts)
            .field("own_known_hosts", &self.own_known_hosts)
            .finish_non_exhaustive()
    }
}

impl Environment {
    /// The config, index and keychain of the person running ncrs.
    pub fn of_the_user() -> Self {
        Self {
            config_path: config::default_path(),
            index_path: config::default_index_path(),
            state_path: config::default_state_path(),
            journal_path: config::default_journal_path(),
            view_copies_dir: config::default_view_copies_dir(),
            key_store: Arc::new(user_key_store()),
            passwords: Arc::new(KeyringPasswords::new()),
            ssh_known_hosts: Some(config::default_ssh_known_hosts_path()),
            own_known_hosts: config::default_known_hosts_path(),
        }
    }

    /// No config file, no index, a key store in memory: nothing outside the
    /// test.
    #[cfg(test)]
    pub fn isolated() -> Self {
        Self {
            config_path: None,
            index_path: None,
            state_path: None,
            journal_path: None,
            view_copies_dir: None,
            key_store: Arc::new(ncrs_index::InMemoryKeyStore::empty()),
            passwords: Arc::new(fs::remote::credentials::InMemoryPasswords::default()),
            ssh_known_hosts: None,
            own_known_hosts: None,
        }
    }
}

/// The keychain service of the App Store build. Its own name keeps that build
/// from reading, or replacing, the key an installed ncrs keeps under "ncrs":
/// the two have different indexes, and a key that does not open one is lost
/// for it.
const APP_STORE_KEY_SERVICE: &str = "ncrs-appstore";

/// The one place the app builds the system keychain store.
fn user_key_store() -> KeyringKeyStore {
    config::forbid_user_machine_in_tests("the system keychain");
    if fs::sandbox_active() {
        return KeyringKeyStore::with_service(APP_STORE_KEY_SERVICE);
    }
    config::portable_data_dir()
        .map_or_else(KeyringKeyStore::new, |dir| KeyringKeyStore::portable(&dir))
}

impl App {
    /// Initial state plus tasks that load both panels. The panels open where
    /// `saved` left them; without a saved folder the left panel starts in the
    /// working directory and the right in `$HOME`.
    pub fn new(environment: Environment, saved: SessionState) -> (Self, Task<Message>) {
        let (app, loading) = Self::restored(environment, saved);
        (app, then_install_native_menu(loading))
    }

    /// `new` with the places it reads and writes given instead of looked up,
    /// and nothing remembered from an earlier run.
    #[cfg(test)]
    pub fn with_environment(environment: Environment) -> (Self, Task<Message>) {
        Self::restored(environment, SessionState::default())
    }

    /// Starts where `saved` left off: its folders (or their nearest existing
    /// parent), sort, active panel and window size.
    pub fn restored(environment: Environment, saved: SessionState) -> (Self, Task<Message>) {
        let Environment {
            config_path,
            index_path,
            state_path,
            journal_path,
            view_copies_dir,
            key_store,
            passwords,
            ssh_known_hosts,
            own_known_hosts,
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
        let home = fs::home_dir();
        let (mut panels, panel_loads) = Self::starting_at(
            saved.left.start_directory().unwrap_or_else(fs::start_dir),
            saved
                .right
                .start_directory()
                .unwrap_or_else(|| home.clone()),
            home,
            [saved.left.start_cursor(), saved.right.start_cursor()],
        );
        panels.config_path = config_path;
        panels.state_path = state_path;
        panels.trash = fs::system_trash(journal_path);
        panels.sandbox = fs::SandboxAccess::detect(index_path.as_deref());
        panels.remote = Arc::new(Pool::over_tcp(KnownHostsFiles {
            ssh: ssh_known_hosts,
            own: own_known_hosts,
        }));
        panels.passwords = passwords;
        panels.open_grant_dialog_if_needed();
        panels.ops.view_copies = view_copies_dir
            .as_deref()
            .and_then(|dir| fs::archive::ViewCopies::open(dir).ok());
        panels.apply_session_state(&saved);
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
        self.hermes.index_path = db_path;
        self.hermes.key_store = Some(ncrs_hermes_ui::KeyStoreHandle(store));
        let task = self.start_filing_module(filing);
        if filing.enabled && filing.locations.is_empty() {
            self.open_hermes_setup();
        }
        (self, task)
    }

    /// Replaces nothing: the caller has dropped any module and pipeline before.
    fn start_filing_module(&mut self, filing: &Filing) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::start_filing_module(hermes, filing, ctx)
        })
    }

    fn open_hermes_setup(&mut self) {
        drop(self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::open_hermes_setup));
    }

    fn update_filing(&mut self, message: FilingMessage) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_filing(hermes, message, ctx)
        })
    }

    fn update_filing_setup(&mut self, message: FilingSetupMessage) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_filing_setup(hermes, message, ctx)
        })
    }

    fn update_hermes_setup(&mut self, message: HermesSetupMessage) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_hermes_setup(hermes, message, ctx)
        })
    }

    fn activate_hermes_focus(&mut self) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::activate_hermes_focus)
    }

    fn apply_saved_filing(&mut self, filing: Filing) -> (SavedFilingApplies, Task<Message>) {
        let (applies, outcome) = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::apply_saved_filing(hermes, filing, ctx)
        });
        (applies, self.run_hermes_outcome(outcome))
    }

    fn apply_ai_change(&mut self, filing: Filing) -> (SavedFilingApplies, Task<Message>) {
        let (applies, outcome) = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::apply_ai_change(hermes, filing, ctx)
        });
        (applies, self.run_hermes_outcome(outcome))
    }

    #[cfg(test)]
    fn advance_hermes_plan(&mut self) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::advance_hermes_plan)
    }

    fn filing_restarted(&mut self, generation: u64) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::filing_restarted(hermes, generation, ctx)
        })
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
                self.theme_preset = config.theme_preset();
                self.vscode_theme = config.vscode_theme_path().map(str::to_owned);
                self.open_programs = config.open;
                self.saved_servers = config.servers;
                self.file_list_tags = config.file_list_tags;
                self.kinds = Kinds::new(config.kinds, self.lang.kind_language());
                self.keymap = config.keymap;
                self.refresh_function_keys();
                self.palette = config.palette;
                self.report_look_warnings(config.contrast_warning, config.vscode_warning);
                self.report_language_warning(config.language_warning);
                self.report_kind_warning(config.kind_warning);
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

    fn report_kind_warning(&mut self, found: Option<KindWarning>) {
        if let Some(warning) = found {
            self.report(ConfigProblem::Kinds(warning));
        }
    }

    fn report_language_warning(&mut self, found: Option<LanguageWarning>) {
        if let Some(warning) = found {
            self.report(ConfigProblem::UnknownLanguage(warning));
        }
    }

    /// F9. A config error still up is worded again in the new language.
    fn switch_language(&mut self) {
        self.set_language(self.lang.other());
    }

    fn set_language(&mut self, lang: Language) {
        self.lang = lang;
        if let Some(module) = &mut self.hermes.filing {
            module.set_kind_language(lang.kind_language());
        }
        self.kinds = self.kinds.in_language(lang.kind_language());
        let kinds = self.kinds.clone();
        if let Some(list) = &mut self.hermes.review_list {
            list.set_kinds(kinds.clone());
        }
        if let Some(view) = &mut self.hermes.saved_queries {
            view.set_kinds(kinds.clone());
        }
        if let Some(page) = &mut self.hermes.tags {
            page.set_kinds(kinds);
        }
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
            ConfigProblem::UnknownLanguage(warning) => {
                let path = warning.path.display().to_string();
                let allowed = Language::ALL
                    .map(|language| format!("\"{}\"", language.code()))
                    .join(", ");
                let values = [
                    ("{path}", path.as_str()),
                    ("{code}", warning.code.as_str()),
                    ("{allowed}", allowed.as_str()),
                ];
                match warning.line {
                    Some(number) => {
                        let line = number.to_string();
                        let mut with_line = values.to_vec();
                        with_line.push(("{line}", line.as_str()));
                        fill_template(self.lang.text(Msg::ErrorUnknownLanguageAtLine), &with_line)
                    }
                    None => fill_template(self.lang.text(Msg::ErrorUnknownLanguage), &values),
                }
            }
            ConfigProblem::Kinds(warning) => {
                let path = warning.path.display().to_string();
                let skipped = warning
                    .skips
                    .iter()
                    .map(|skip| match skip {
                        KindSkip::EmptyId => self.lang.text(Msg::ErrorKindsNoId).to_owned(),
                        KindSkip::Word { id, word } => fill_template(
                            self.lang.text(Msg::ErrorKindsWordTaken),
                            &[("{word}", word), ("{id}", id)],
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                fill_template(
                    self.lang.text(Msg::ErrorKinds),
                    &[("{path}", &path), ("{skipped}", &skipped)],
                )
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
    #[cfg(test)]
    pub fn starting_in(start: PathBuf, home: PathBuf) -> (Self, Task<Message>) {
        let right = home.clone();
        Self::starting_at(start, right, home, [None, None])
    }

    /// `starting_in` with the right panel's folder of its own; `home` stays
    /// what `~` means.
    /// `cursors` name the entries the first listings put the cursor on.
    fn starting_at(
        left: PathBuf,
        right: PathBuf,
        home: PathBuf,
        cursors: [Option<String>; 2],
    ) -> (Self, Task<Message>) {
        let [left_cursor, right_cursor] = cursors;
        let mut app = Self {
            panels: Panels::new(
                PanelState::new(left.clone()),
                PanelState::new(right.clone()),
                PanelSide::Left,
            ),
            visible_rows: layout::visible_rows(layout::INITIAL_WINDOW_SIZE),
            lang: Language::default(),
            function_keys: Keymap::built_in().function_keys(Language::default(), KeyScope::Panel),
            search_function_keys: Keymap::built_in()
                .function_keys(Language::default(), KeyScope::Search),
            keymap: Keymap::built_in(),
            jobs: jobs::Queue::new(),
            start_over_dialog: false,
            dialog_focus: 0,
            modifiers: Modifiers::default(),
            trash: fs::system_trash(None),
            launcher: Arc::new(fs::SystemLauncher),
            open_programs: config::OpenPrograms::default(),
            config_error: None,
            config_problem: None,
            palette: Palette::default(),
            job_error: None,
            job_error_press: None,
            notice: None,
            notice_generation: 0,
            job_done: 0,
            ops: ncrs_ops::Ops::default(),
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            command_line: CommandLine::default(),
            home,
            clock: Arc::new(std::time::Instant::now),
            window_size: layout::INITIAL_WINDOW_SIZE,
            state_path: None,
            session_scheduled: SessionState::default(),
            session_save_generation: 0,
            session_writes: Arc::default(),
            input: Arc::default(),
            list_volumes: Arc::new(fs::list_volumes),
            search: ncrs_search::Search::new(layout::INITIAL_WINDOW_SIZE),
            quick_open: None,
            quick_open_walk_generation: 0,
            quick_open_walk: None,
            command_palette: None,
            recent_commands: Vec::new(),
            wall_clock: Arc::new(ncrs_pipeline::SystemClock),
            config_path: None,
            file_list_tags: FileListTags::default(),
            kinds: Kinds::default(),
            hermes_flow_step: 0,
            settings: Settings::default(),
            hermes: ncrs_hermes_ui::Hermes::default(),
            theme_preset: ThemePreset::DEFAULT,
            vscode_theme: None,
            resolve_folder: filing_setup::resolve_folder,
            pick_folder: crate::system_dialogs::pick_folder_with_system_dialog,
            pick_file: crate::system_dialogs::pick_file_with_system_dialog,
            read_clipboard: clipboard_paste::read_clipboard_off_the_ui_thread,
            write_clipboard_files: {
                let mut writer = FileListWriter::default();
                Box::new(move |files| writer.write(files))
            },
            tag_exceptions: TagExceptions::new(),
            hermes_focus: HermesFocus::default(),
            fetch_extension: open_vsx::fetch_extension,
            install_theme: ncrs_settings::theme_fetch::install::install_picked_theme,
            about: None,
            find_third_party_licenses: crate::about::installed_third_party_licenses,
            app_version: env!("CARGO_PKG_VERSION"),
            download_ocr_models: download_models,
            download_ai_model,
            grant_dialog: false,
            sandbox: fs::SandboxAccess::detect(None),
            remote: Arc::new(Pool::over_tcp(KnownHostsFiles::default())),
            passwords: Arc::new(KeyringPasswords::new()),
            remote_ui: ncrs_remote::Remote::default(),
            saved_servers: Vec::new(),
        };

        let tasks = Task::batch([
            app.load(PanelSide::Left, left, left_cursor),
            app.load(PanelSide::Right, right, right_cursor),
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
            if self.hermes.pipeline.is_some()
                || self.hermes.filing_status.page_open()
                || self.hermes.overview.is_some()
            {
                iced::time::every(SNAPSHOT_EVERY)
                    .map(|_| Message::FilingStatus(FilingStatusMessage::Tick))
            } else {
                Subscription::none()
            },
            // The bar of a running model download reads a counter the download
            // thread fills; nothing else wakes the view.
            if self.hermes_flow_is_visible() {
                iced::time::every(hermes_flow::STEP_INTERVAL).map(|_| Message::HermesFlowTick)
            } else {
                Subscription::none()
            },
            if self.settings.ai_model_download_runs() {
                iced::time::every(DOWNLOAD_BAR_EVERY)
                    .map(|_| Message::AiModel(AiModelMessage::Progress))
            } else {
                Subscription::none()
            },
            if self.settings.ocr_models_download_runs() {
                iced::time::every(DOWNLOAD_BAR_EVERY)
                    .map(|_| Message::OcrModels(OcrModelsMessage::Progress))
            } else {
                Subscription::none()
            },
            // Mouse, focus and modifier events of every status: widgets capture
            // the clicks and the pointer moves the menu has to know about.
            event::listen_with(|event, status, _window| Some((event, status)))
                .with(Arc::clone(&self.input))
                .filter_map(|(input, (event, status))| input.lock().handle(&event, status)),
            window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
            window::events().filter_map(|(_id, event)| match event {
                window::Event::Focused => Some(Message::WindowFocused),
                window::Event::FileHovered(_) => Some(Message::FilesHovered),
                window::Event::FilesHoveredLeft => Some(Message::FilesHoverLeft),
                window::Event::FileDropped(path) => Some(Message::FileDropped(path)),
                _ => None,
            }),
            // The close button and Alt+F4 end the app through Quit, which saves
            // the session first; the window does not close by itself.
            window::close_requests().map(|_id| Message::Quit),
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
        let task = self.update_message(message);
        let back = self.return_to_overview_when_its_card_is_gone();
        Task::batch([task, back])
    }

    fn return_to_overview_when_its_card_is_gone(&mut self) -> Task<Message> {
        if !self.hermes.return_to_overview
            || self.hermes.filing_setup.is_some()
            || self.hermes.hermes_setup.is_some()
            || self
                .hermes
                .filing
                .as_ref()
                .is_some_and(FilingModule::has_card)
        {
            return Task::none();
        }
        self.hermes.return_to_overview = false;
        self.open_hermes_overview()
    }

    fn update_message(&mut self, message: Message) -> Task<Message> {
        if let Message::Panel(panel_message) = &message {
            if panel_message.resets_type_ahead() {
                self.panels.type_ahead.reset();
            }
        }
        if let Message::NoticeExpired(generation) = message {
            if generation == self.notice_generation {
                self.notice = None;
            }
            return Task::none();
        }
        if leaves_bar_fields(&message) {
            self.panels.set_path_field(None);
            self.command_line.leave();
        }
        if let Message::SaveSessionState(generation) = message {
            return match self.due_session_state(generation) {
                Some((path, state)) => {
                    let order = Arc::clone(&self.session_writes);
                    Task::future(async move {
                        // A failed write was already reported to stderr.
                        drop(
                            tokio::task::spawn_blocking(move || {
                                write_session_state(&order, generation, &path, &state);
                            })
                            .await,
                        );
                    })
                    .discard()
                }
                None => Task::none(),
            };
        }
        let had_search_keys = self.search_has_keys();
        let applied = self.apply_unless_refused(message);
        let task = Task::batch([
            applied,
            self.with_hermes(Vec::new(), ncrs_hermes_ui::request_hermes_column)
                .map(Message::from),
            self.schedule_session_save(),
        ]);
        // A dialog that opens while a line is being typed gets the keys, not the field.
        if self.command_line.is_focused() && self.command_line_is_blocked() {
            self.command_line.leave();
            self.publish_key_state();
            return Task::batch([task, release_focus()]);
        }
        // The text field keeps typing into the query until it is told to let
        // go, so leaving the search's panel releases it.
        if had_search_keys && !self.search_has_keys() {
            return Task::batch([task, release_focus()]);
        }
        task
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            // A raw key press. Routed here rather than in the subscription,
            // because that one cannot see whether a prompt is open.
            // --- copy / move ---
            Message::Transfer(kind) => self.open_transfer_dialog(kind),
            Message::TransferDialog(dialog_message) => self.update_transfer_dialog(dialog_message),

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
                self.ops.conflict_all = !self.ops.conflict_all;
                Task::none()
            }

            // The user answered a conflict. The queue's pending job carries
            // what was asked, so the answer lands on the right file.
            Message::TransferConflict(choice) => {
                self.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, choice, ctx))
            }

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

            // Stop the running job. The abort key (Ctrl+C on a Mac unless
            // rebound) and Escape both, because a long copy that cannot be
            // stopped is the case a queue was supposed to fix.
            //
            // A delete in flight finishes its current entry even so: it runs in
            // `spawn_blocking`, which cannot be cancelled, so the panel may
            // still list that entry for a moment after the abort.
            Message::AbortJob => self.abort_job(),
            Message::Trash(trash_message) => self.update_trash(trash_message),
            Message::HitList(hit_message) => self.update_hit_list(hit_message),

            // One row of a transfer finished. Continue with the next, or ask
            // about a name that is in the way.
            Message::TransferRowDone {
                result,
                index,
                generation,
            } => self.with_ops(|ops, ctx| {
                ncrs_ops::transfer_row_done(ops, result, index, generation, ctx)
            }),

            Message::OpenExternal(kind) => self.open_external(kind),

            Message::ViewExtracted {
                generation,
                slot,
                result,
            } => self
                .with_ops(|ops, ctx| ncrs_ops::view_extracted(ops, generation, slot, result, ctx)),

            #[cfg(target_os = "macos")]
            Message::OpenPrivacySettings => {
                self.open_privacy_settings();
                Task::none()
            }

            // --- drive menu ---
            Message::VolumeMenu(side) => {
                let overlay_is_open = self.overlay_is_open();
                let facts = self.volume_facts_source();
                self.panels
                    .open_volume_menu_on(side, false, overlay_is_open, facts);
                Task::none()
            }
            Message::VolumeMenuMove(delta) => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.move_by(delta);
                }
                Task::none()
            }
            Message::VolumeMenuFirst => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.selected = 0;
                }
                Task::none()
            }
            Message::VolumeMenuLast => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.selected = menu.last_row();
                }
                Task::none()
            }
            Message::VolumeMenuRemove => {
                let saved = self.saved_server_ids();
                let actions = self.panels.remove_highlighted_row(&saved);
                self.run_panel_actions(actions)
            }
            Message::AddFavourite => {
                let action = self.panels.add_favourite(self.lang);
                self.run_panel_actions(vec![action])
            }
            Message::RemoveFavourite => {
                let action = self.panels.remove_favourite(self.lang);
                self.run_panel_actions(vec![action])
            }
            Message::OpenFavourites => {
                let overlay_is_open = self.overlay_is_open();
                let facts = self.volume_facts_source();
                let actions = self
                    .panels
                    .open_favourites(self.lang, overlay_is_open, facts);
                self.run_panel_actions(actions)
            }
            Message::VolumeMenuActivate => {
                let actions = self.panels.go_to_selected_volume(self.lang);
                self.run_panel_actions(actions)
            }
            Message::VolumeMenuClick(index) => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.selected = index;
                }
                let actions = self.panels.go_to_selected_volume(self.lang);
                self.run_panel_actions(actions)
            }
            Message::VolumeMenuClose => {
                self.panels.volume_menu = None;
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
            | Message::ContextMenuOpenSub
            | Message::ContextMenuCloseSub
            | Message::ContextMenuSubHover(_)
            | Message::ContextMenuSubClick(_)
            | Message::ContextMenuClose) => self.update_context_menu(menu_message),

            Message::HermesFile(file_message) => self.update_hermes_file(file_message),

            Message::QuickFix(fix_message) => self.update_quick_fix(fix_message),

            // --- path field ---
            path_message @ (Message::PathFieldInput(_)
            | Message::PathFieldSubmit
            | Message::PathFieldCancel
            | Message::PathChecked { .. }
            | Message::ClickedOutside) => self.update_path_field(path_message),

            Message::CommandLine(line_message) => self.update_command_line(line_message),

            // --- delete ---
            Message::Delete { permanent } => self.open_delete_dialog(permanent),
            Message::CopyToClipboard(kind) => self.copy_to_clipboard(kind),
            Message::Paste => self.paste_from_clipboard(),
            Message::ClipboardRead(content) => self.paste_content(content),
            Message::CopyFilesToClipboard => self.copy_files_to_clipboard(),
            Message::DeleteCancel => {
                self.ops.delete_dialog = None;
                Task::none()
            }
            Message::DeleteConfirm => self.confirm_delete(),
            Message::DeleteRowDone {
                result,
                index,
                generation,
            } => self.with_ops(|ops, ctx| {
                ncrs_ops::delete_row_done(ops, result, index, generation, ctx)
            }),

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
                self.open_prompt(Prompt::create_dir(parent))
            }
            Message::CreateFilePrompt => {
                let parent = self.active_panel().path.clone();
                self.open_prompt(Prompt::create_file(parent))
            }
            Message::PackPrompt => self.open_pack_prompt(),
            Message::ArchiveWritten { result, generation } => {
                self.with_ops(|ops, ctx| ncrs_ops::archive_written(ops, generation, result, ctx))
            }
            Message::PromptInput(text) => {
                if let Some(prompt) = self.ops.prompt.as_mut() {
                    prompt.set_name(text);
                }
                Task::none()
            }
            Message::PromptToggleKeepFolders => self.toggle_keep_folders(),
            // Its own arm, not an early return: starting the job changes what the
            // keys mean, and the routing state is published after the match.
            Message::PromptSubmit if self.ops.valid_prompt_has_its_own_job() => {
                self.start_prompt_job()
            }
            Message::PromptSubmit => {
                let Some(prompt) = self.ops.prompt.as_ref() else {
                    return Task::none();
                };
                if let Err(reason) = prompt.validate() {
                    self.ops.set_prompt_error(reason);
                    return Task::none();
                }
                let kind = prompt.kind();
                let side = self.ops.prompt_side.unwrap_or(self.panels.active);
                let parent = self.panel(side).path.clone();
                let new_name = prompt.name().trim().to_string();
                let request_id = self.ops.prompt_request_id.wrapping_add(1);
                self.ops.prompt_request_id = request_id;
                self.ops.set_prompt_busy(true);

                // The new entry's name, carried through the task so the
                // reload can select it.
                let target = parent.join(&new_name);
                let finished = move |result| Message::PromptFinished {
                    prompt: kind,
                    request_id,
                    result,
                };
                let create = match kind {
                    PromptKind::CreateDir => Task::perform(fs::create_dir(target), finished),
                    PromptKind::CreateFile => Task::perform(fs::create_file(target), finished),
                    // Taken out above: a pack and a paste have their own jobs.
                    PromptKind::Pack | PromptKind::Paste => Task::none(),
                };

                // The reload belongs *after* the create, not beside it.
                // Running both at once races: a read that finishes first sees a
                // directory that does not contain the new entry yet, and the
                // selection then points at whatever is there instead.
                self.panels.pending_reload = Some(PendingReload {
                    side,
                    name: new_name,
                });
                create
            }
            Message::PromptCancel => {
                self.ops.prompt = None;
                self.ops.pending_paste = None;
                self.ops.prompt_side = None;
                Task::none()
            }
            Message::PromptFinished {
                prompt: kind,
                request_id,
                result,
            } => self.finish_prompt(kind, request_id, result),

            Message::Panel(panel_message) => self.update_panel(panel_message),
            Message::TitleBar(title_bar) => update_title_bar(title_bar),
            Message::ComputeFolderSizes => {
                let walk = self.panels.start_folder_size_run();
                folder_size_task(walk)
            }
            Message::FolderSizeMeasured { run, folder, size } => {
                let walk = self
                    .panels
                    .folder_size_measured(run, folder, size, self.visible_rows);
                folder_size_task(walk)
            }
            Message::SwitchLanguage => {
                self.switch_language();
                Task::none()
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Message::DragOutStarted(started) => self.drag_out_started(started),

            Message::WindowResized(size) => {
                self.apply_window_size(size);
                Task::none()
            }
            Message::WindowFocused => self.reload_local_panels(),
            Message::FilesHovered => {
                self.ops.drop_hover = true;
                Task::none()
            }
            Message::FilesHoverLeft => {
                self.ops.drop_hover = false;
                Task::none()
            }
            Message::FileDropped(path) => {
                self.ops.queue_dropped_file(path).map(Message::DropSettled)
            }
            Message::DropSettled(generation) => match self.ops.settled_drop(generation) {
                Some(paths) => self.copy_external_files_into_active_panel(paths),
                None => Task::none(),
            },
            Message::OpenSearch => self.open_search(),
            Message::Search(search) => self.update_search(search),
            Message::QuickOpen(quick_open) => self.update_quick_open(quick_open),
            Message::CommandPalette(command_palette) => {
                self.update_command_palette(command_palette)
            }
            Message::KeyBindings(shortcuts) => self.update_key_bindings(shortcuts),
            Message::ThemeFetch(fetch_message) => self.update_theme_fetch(fetch_message),
            Message::RuleSuggestions(suggestions) => self.update_rule_suggestions(suggestions),
            Message::Rules(rules) => self.update_rules(rules),
            Message::ReviewList(review) => {
                let update = self.update_review_list(review);
                Task::batch([update, self.request_review_preview()])
            }
            Message::SavedQueries(queries) => self.update_saved_queries(queries),
            Message::Tags(tags) => self.update_tags(tags),
            Message::HermesPage(page) => self.show_hermes_page(page),
            Message::HermesFocus(focus) => self.focus_hermes(focus),
            Message::HermesStep(step) => self.step_hermes_page(step),
            Message::ReorganisationPlan(plan) => self.update_reorganisation(plan),
            // #73 hangs the executor here; until then the selection goes nowhere.
            Message::ExecuteReorganisation(selection) => self.run_reorganisation(selection),
            Message::ReorganisationExecuted(result) => self.reorganisation_executed(result),
            Message::UndoReorganisation => self.undo_reorganisation(),
            Message::DocumentsFiled(result) => self.documents_filed(result),
            Message::DocumentsRefiled(result) => self.documents_refiled(result),
            Message::ReorganisationUndone(result) => self.reorganisation_undone(result),
            Message::About(about_message) => self.update_about(about_message),
            Message::OcrModels(models_message) => self.update_ocr_models(models_message),
            Message::AiModel(model_message) => self.update_ai_model(model_message),
            Message::Grant(grant_message) => self.update_grant(grant_message),
            Message::Remote(remote_message) => {
                let outcome = self.with_connect_feature(|remote, ctx| {
                    ncrs_remote::update(remote, remote_message, ctx)
                });
                self.run_remote_outcome(outcome)
            }
            Message::Settings(settings_message) => self.update_settings(settings_message),
            Message::Filing(filing_message) => self.update_filing(filing_message),
            Message::FilingStatus(page_message) => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::update_filing_status(hermes, page_message, ctx)
                }),
            Message::HermesColumn(column_message) => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::update_hermes_column(hermes, column_message, ctx)
                }),
            Message::FilingSetup(setup_message) => self.update_filing_setup(setup_message),
            Message::HermesSetup(setup_message) => self.update_hermes_setup(setup_message),
            Message::HermesOverview(overview_message) => {
                self.update_hermes_overview(overview_message)
            }
            Message::HermesFlowTick => {
                self.hermes_flow_step = self.hermes_flow_step.wrapping_add(1);
                Task::none()
            }
            #[cfg(any(target_os = "macos", test))]
            Message::RunCommand(action) => self.run_command(action),
            #[cfg(target_os = "macos")]
            Message::InstallNativeMenu => {
                native_menu::install(self.lang, &self.keymap, self.menu_availability());
                Task::none()
            }
            Message::Quit => self.quit(),
            // Handled in `update`, which owns the timer's generation check.
            Message::NoticeExpired(_) | Message::SaveSessionState(_) => Task::none(),
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

    /// The inquiries under the cursor or among the marked files of the active
    /// panel.
    fn inquiry_paths(&self) -> Vec<PathBuf> {
        self.action_sources()
            .into_iter()
            .filter(|path| {
                self.hermes.hermes_column.state_of(path) == Some(HermesFileState::Inquiry)
            })
            .collect()
    }

    /// Both panels read their folder again and keep the cursor on its file.
    fn reload_panels(&mut self) -> Task<Message> {
        let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
            let panel = self.panel(each);
            let path = panel.path.clone();
            let cursor = panel
                .selected_entry()
                .map(|entry| entry.name.to_string_lossy().into_owned());
            self.load(each, path, cursor)
        });
        Task::batch(tasks)
    }

    fn update_context_menu(&mut self, message: Message) -> Task<Message> {
        let nav = match message {
            Message::ContextMenuAt { side, index } => {
                let pointer = self.input.lock().pointer;
                self.open_context_menu(side, index, Some(pointer));
                return Task::none();
            }
            Message::ContextMenuKey => {
                let side = self.panels.active;
                let index = self.panel(side).selected;
                self.open_context_menu(side, index, None);
                return Task::none();
            }
            Message::ContextMenuActivate => return self.run_context_menu_entry(),
            Message::ContextMenuClick(index) => return self.click_context_menu(index, false),
            Message::ContextMenuSubClick(index) => return self.click_context_menu(index, true),
            Message::ContextMenuMove(delta) => MenuNav::Move(delta),
            Message::ContextMenuFirst => MenuNav::First,
            Message::ContextMenuLast => MenuNav::Last,
            Message::ContextMenuHover(index) => MenuNav::Hover(index),
            Message::ContextMenuOpenSub => MenuNav::OpenSub,
            Message::ContextMenuCloseSub => MenuNav::CloseSub,
            Message::ContextMenuSubHover(index) => MenuNav::SubHover(index),
            Message::ContextMenuClose => MenuNav::Close,
            _ => return Task::none(),
        };
        self.panels.navigate_context_menu(nav);
        Task::none()
    }

    fn update_path_field(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PathFieldInput(typed) => {
                self.command_line.leave();
                self.panels.set_path_field(Some(typed));
                Task::none()
            }
            Message::PathFieldSubmit => {
                let remote = &self.remote;
                let session_home = |server: &fs::remote::ServerId| {
                    remote.session(server).map(|s| s.home().to_owned())
                };
                let ctx = ncrs_files::path_field::TypedPathCtx {
                    lang: self.lang,
                    home: &self.home,
                    session_home: &session_home,
                };
                let (check, actions) = self.panels.go_to_typed_path(&ctx);
                let located = check.map(|checked| Message::PathChecked {
                    request_id: checked.request_id,
                    side: checked.side,
                    target: checked.target,
                    result: checked.result,
                });
                Task::batch([located, self.run_panel_actions(actions)])
            }
            Message::PathChecked {
                request_id,
                side,
                target,
                result,
            } => {
                let checked = ncrs_files::path_field::PathChecked {
                    request_id,
                    side,
                    target,
                    result,
                };
                let actions = self.panels.finish_path_check(self.lang, checked);
                self.run_panel_actions(actions)
            }
            Message::PathFieldCancel => {
                self.panels.set_path_field(None);
                release_focus()
            }
            Message::ClickedOutside => {
                self.panels.context_menu = None;
                self.panels.set_path_field(None);
                self.command_line.leave();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn update_command_line(&mut self, message: CommandLineMessage) -> Task<Message> {
        let ctx = CommandLineCtx {
            blocked: self.command_line_is_blocked(),
            lang: self.lang,
            remote: self.active_panel().remote().is_some(),
            active_side: self.panels.active,
            active_path: self.active_panel().path.clone(),
            home: &self.home,
            shell_directory: shell_directory(self.active_panel()),
        };
        let (inner, action) = command_line::update(&mut self.command_line, message, &ctx);
        let task = inner.map(Message::CommandLine);
        match action {
            None => task,
            Some(CommandLineAction::ClosePathField) => {
                self.panels.set_path_field(None);
                task
            }
            Some(CommandLineAction::ShowError(text)) => {
                self.set_status_error(&text);
                task
            }
            Some(CommandLineAction::OpenDirectory { side, destination }) => {
                self.job_error = None;
                self.notice = None;
                Task::batch([
                    self.load(side, destination.directory, destination.select),
                    task,
                ])
            }
            Some(CommandLineAction::ShowPathProblem { problem, target }) => {
                let reason = ncrs_files::path_field::problem_text(self.lang, &problem, &target);
                self.set_status_error(&reason);
                task
            }
            Some(CommandLineAction::ShellFinished { report }) => self.finish_shell_command(report),
        }
    }

    /// Settings and the filing setup replace the panel area; an overlay covers it.
    fn command_line_is_blocked(&self) -> bool {
        self.overlay_is_open() || self.settings.page.is_some() || self.hermes.filing_setup.is_some()
    }

    /// A shell command ended: its report goes to the status bar and both panels
    /// are re-read, as the command may have changed what they show.
    fn finish_shell_command(&mut self, report: Result<String, String>) -> Task<Message> {
        let reloads = [PanelSide::Left, PanelSide::Right].map(|each| {
            let path = self.panel(each).path.clone();
            self.load(each, path, None)
        });
        let notice = match report {
            Ok(text) => {
                self.job_error = None;
                self.show_notice(text)
            }
            Err(text) => {
                self.set_status_error(&text);
                Task::none()
            }
        };
        Task::batch(reloads.into_iter().chain([notice]))
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
        input.command_line = state.command_line;
        input.search_open = search_open;
        input.job_running = job_running;
        input.quick_open = self.quick_open.is_some();
        input.command_palette = self.command_palette.is_some();
        input.key_bindings = self
            .settings
            .key_bindings
            .as_ref()
            .is_some_and(|shortcuts| !shortcuts.is_recording());
        input.theme_fetch = self.settings.theme_fetch.is_some();
        input.rule_suggestions = self.hermes.rule_suggestions.is_some();
        input.rules = self.hermes.rules.is_some();
        input.review_list = self.hermes.review_list.is_some();
        input.saved_queries = self.hermes.saved_queries.is_some();
        input.tags = self.hermes.tags.is_some();
        input.hermes_sidebar = state.hermes_sidebar;
        input.transfer_dialog = self.ops.transfer_dialog.is_some();
        input.reorganisation_plan = self.hermes.reorganisation.is_some();
        input.settings = self.settings.page.is_some()
            && self.hermes.filing_setup.is_none()
            && !self.blocking_overlay_is_open();
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
            let hermes_cell: Option<ncrs_files::HermesCellOf<'_>> =
                self.hermes.is_active().then(|| {
                    let column = ColumnView::new(
                        &self.hermes.hermes_column,
                        &self.hermes.filing_config.locations,
                        self.hermes.hermes_spinner_angle,
                        &self.file_list_tags,
                        &self.tag_exceptions,
                        self.window_size.width / 2.0,
                        &self.kinds,
                    );
                    let (lang, palette) = (self.lang, self.palette);
                    Box::new(move |entry: &fs::FileEntry| {
                        panel::hermes_cell_of(entry, column, lang, &palette)
                    }) as ncrs_files::HermesCellOf<'_>
                });
            ncrs_files::view(
                self.panel(side),
                ncrs_files::ViewCtx {
                    side,
                    lang: self.lang,
                    palette: self.palette,
                    is_active: self.panels.active == side,
                    visible_rows: self.visible_rows,
                    hermes_cell,
                },
            )
            .map(Message::Panel)
        };

        let side_view_plain = |side: PanelSide| match self.search.view.as_ref() {
            _ if self.ops.trash_view.is_some() && side == self.ops.trash_side => {
                self.trash_panel(side)
            }
            Some(search) if self.search.open && side == self.search.side => {
                let keymap = &self.keymap;
                ncrs_search::view(
                    search,
                    self.search.rows,
                    ncrs_search::ViewCtx {
                        lang: self.lang,
                        palette: &self.palette,
                        is_active: self.panels.active == side,
                        shortcuts: self.toggle_shortcuts(),
                        toggle_for_press: Rc::new(move |key, physical_key, modifiers| {
                            search_toggle_for_press(keymap, key, physical_key, modifiers)
                        }),
                    },
                )
                .map(Message::Search)
            }
            _ => panel_view(side),
        };

        let side_view = |side: PanelSide| {
            let plain = side_view_plain(side);
            let view: Element<'_, Message> = if self.is_drop_target(side) {
                Stack::with_children([
                    plain,
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(theme::outlined_in(self.palette.accent))
                        .into(),
                ])
                .into()
            } else {
                plain
            };
            // The cursor goes on a wrapper, not on the overlay: a Stack layer
            // with its own cursor lifts the pointer off the layers below, so
            // the panel would report the pointer as gone and end the hover.
            // The wrapper stays in place without a drag so the panel keeps its
            // widget state when a drag starts.
            let wrapper = mouse_area(view);
            Element::from(match self.drag_cursor(side) {
                Some(interaction) => wrapper.interaction(interaction),
                None => wrapper,
            })
        };

        let panels = row![
            side_view(PanelSide::Left),
            theme::vertical_rule(self.palette.border),
            side_view(PanelSide::Right),
        ]
        .height(Length::Fill);

        let below_title_bar =
            if let Some(state) = self.settings.page.as_ref() {
                self.settings_area(state)
            } else if let Some(area) = self.ablage_area() {
                area
            } else {
                container(column![
                    panels,
                    command_line_ui::view(command_line_ui::Props {
                        prompt: self.active_panel().path.display().to_string(),
                        typed: self.command_line.typed(),
                        focused: self.command_line.is_focused(),
                        focusable: !self.command_line_is_blocked(),
                        running: self.command_line.running(),
                        key_label: self
                            .keymap
                            .shortcut_label(&KeyAction::CommandLine.message()),
                        lang: self.lang,
                        palette: &self.palette,
                    })
                    .map(Message::CommandLine),
                    fkeys::view(&self.function_key_bar(), &self.palette,),
                    statusbar::view(
                        self.active_panel(),
                        self.lang,
                        &self.palette,
                        statusbar::Activity {
                            side: self.panels.active,
                            theme_name: self.theme_name(),
                            job: self.job_status(),
                            job_press: self.ops.transfer.is_some().then_some(
                                Message::TransferDialog(TransferDialogMessage::ShowProgress,)
                            ),
                            filing: self.filing_badge(),
                            on_blocked_press: blocked_access_press(
                                self.sandbox.active,
                                &self.active_panel().path,
                            ),
                            on_failure_press: self
                                .job_error
                                .is_some()
                                .then(|| self.job_error_press.clone())
                                .flatten(),
                            blocked_by_sandbox: cfg!(target_os = "macos") && self.sandbox.active,
                            notice: self.notice.as_deref(),
                            hermes_line: self.hermes_line(),
                        },
                        self.status_error(),
                        self.search_status(),
                        statusbar::PathBar {
                            editing: self.panels.path_field.as_deref(),
                            on_input: Message::PathFieldInput,
                            on_submit: Message::PathFieldSubmit,
                            on_copy: Message::CopyToClipboard(ClipboardKind::Directory),
                            hit_line: self.hit_line(),
                        },
                    ),
                ])
                .width(Length::Fill)
                .height(Length::Fill)
            };

        let window = container(column![self.title_bar_view(), below_title_bar])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::root(&self.palette));

        // The cards sit on the same scrim as the modals; the panels keep their
        // keys. Every modal below stacks on this base, so it stays above them.
        let module_card = self
            .hermes
            .filing
            .as_ref()
            .filter(|module| module.has_card())
            .and_then(|module| filing_ui::view(module, self.window_size, self.lang, &self.palette))
            .map(|card| card.map(Message::from));
        let setup_card = self.hermes.filing_setup.as_ref().map(|setup| {
            let context = filing_setup_ui::SetupContext {
                current_folder: &self.active_panel().path,
                home: &self.home,
                index_is_plain: self
                    .hermes
                    .filing
                    .as_ref()
                    .is_some_and(FilingModule::index_is_plain),
                lang: self.lang,
                window: self.window_size,
            };
            filing_setup_ui::view(setup, &context, &self.palette).map(Message::from)
        });
        let hermes_card = self.hermes_setup_view();
        let cards: Vec<Element<'_, Message>> = setup_card
            .into_iter()
            .chain(hermes_card)
            .chain(module_card)
            .collect();
        let base: Element<'_, Message> = if cards.is_empty() {
            window.into()
        } else {
            let card = Column::with_children(cards)
                .spacing(theme::spacing::LINE_GAP)
                .align_x(iced::Alignment::Center);
            Stack::with_children([window.into(), dialog::scrim(card.into())]).into()
        };
        let root = if cfg!(target_os = "macos") {
            base
        } else {
            titlebar::with_resize_edges(base)
        };

        if let Some(menu) = self.hermes.quick_fix.as_ref() {
            let click_catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::QuickFix(QuickFixMessage::Close));
            return Stack::with_children([
                root,
                click_catcher.into(),
                quick_fix_view::view(menu, self.lang, &self.palette).map(Message::from),
            ])
            .into();
        }

        if let Some(menu) = self.panels.context_menu.as_ref() {
            // A left press anywhere beside the menu closes it, the path field
            // and the column titles included: those take their own clicks and
            // would never report one. A right press passes through, so another
            // row's menu replaces this one.
            // A Ctrl+click on a Mac is a right click and passes through too.
            let mut click_catcher =
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill));
            if !ncrs_files::ctx::is_context_click(self.modifiers) {
                click_catcher = click_catcher.on_press(Message::ContextMenuClose);
            }
            return Stack::with_children([
                root,
                click_catcher.into(),
                context_menu_view::view(
                    menu,
                    self.lang,
                    &context_menu_view::EntryKeys {
                        context: &|action| self.keymap.shortcut_label(&action.message()),
                        hermes: &|action| self.keymap.shortcut_label(&action.message()),
                    },
                    &self.palette,
                )
                .map(Message::from),
            ])
            .into();
        }

        if let Some(menu) = self.panels.volume_menu.as_ref() {
            let menu_element = volumes_view::view(
                menu.side,
                volumes_view::MenuRows {
                    volumes: &menu.volumes,
                    favourites: &menu.favourites,
                    servers: &menu.servers,
                    trash: menu.trash,
                },
                menu.selected,
                self.window_size,
                self.lang,
                &self.palette,
                volumes_view::MenuMessages {
                    on_click: Box::new(Message::VolumeMenuClick),
                    on_close: Message::VolumeMenuClose,
                },
            );
            return Stack::with_children([root, dialog::scrim(menu_element)]).into();
        }

        if let Some(deletion) = self.ops.delete_dialog.as_ref() {
            let dialog_element = delete_dialog_view::view(
                &ncrs_ops::delete_subject(self.lang, deletion),
                deletion.permanent,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        if let Some(card) = self.about.as_ref() {
            let card_element = about_ui::view(
                card,
                self.app_version,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        if let Some(card) = self.settings.ocr_models.as_ref() {
            let card_element = ocr_models_ui::view(
                card,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([
                root,
                dialog::scrim(card_element.map(Message::OcrModels)),
            ])
            .into();
        }

        if let Some(card) = self
            .settings
            .ai_model
            .as_ref()
            .filter(|_| !self.settings.ai_card_hidden)
        {
            let card_element = ai_model_ui::view(
                card,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element.map(Message::AiModel))])
                .into();
        }

        if self.start_over_dialog {
            let question = hermes_overview_ui::start_over_dialog(
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(question)]).into();
        }

        if let Some(dialog) = self.remote_ui.dialog.as_ref() {
            let dialog_element =
                remote_ui::view(dialog, self.window_size, self.lang, &self.palette)
                    .map(Message::from);
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        if self.grant_dialog {
            let dialog_element = grant_ui::view(
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        // The conflict dialog comes first, before the prompt. A conflict is
        // asked *after* the prompt is gone — the user submitted, and the name
        // turned out to be taken — so behind the prompt's early return the
        // dialog was never drawn. That was a real bug, and the snapshot test is
        // what found it.
        if self.ops.conflict_is_open() {
            let name = self.ops.conflict_name();
            let conflict_element = conflict::view(
                &name,
                self.lang,
                &self.palette,
                self.ops.conflict_all,
                self.dialog_focus,
                self.window_size,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(conflict_element)]).into();
        }

        if let Some(confirm) = self.ops.transfer_dialog.as_ref() {
            let confirm_element =
                transfer_ui::confirm(confirm, self.window_size, self.lang, &self.palette)
                    .map(Message::from);
            return Stack::with_children([root, dialog::scrim(confirm_element)]).into();
        }

        if let Some(running) = self.ops.progress_mask(self.job_progress) {
            let progress_element =
                transfer_ui::progress(&running, self.window_size, self.lang, &self.palette)
                    .map(Message::from);
            return Stack::with_children([root, dialog::scrim(progress_element)]).into();
        }

        if let Some(quick_open) = self.quick_open.as_ref() {
            let box_element =
                quick_open_ui::view(quick_open, self.window_size, self.lang, &self.palette)
                    .map(Message::QuickOpen);
            return Stack::with_children([root, dialog::scrim(box_element)]).into();
        }

        if let Some(shortcuts) = self.settings.key_bindings.as_ref() {
            let dialog_element =
                key_bindings_ui::view(shortcuts, self.window_size, self.lang, &self.palette);
            return Stack::with_children([
                root,
                dialog::scrim(dialog_element.map(Message::KeyBindings)),
            ])
            .into();
        }

        if let Some(command_palette) = self.command_palette.as_ref() {
            let box_element = command_palette_ui::view(
                command_palette,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::CommandPalette);
            return Stack::with_children([root, dialog::scrim(box_element)]).into();
        }

        if let Some(card) = self.settings.theme_fetch.as_ref() {
            let card_element =
                theme_fetch_ui::view(card, self.window_size, self.lang, &self.palette);
            return Stack::with_children([
                root,
                dialog::scrim(card_element.map(Message::ThemeFetch)),
            ])
            .into();
        }

        // Then the create-directory prompt, for F7.
        let Some(prompt) = self.ops.prompt.as_ref() else {
            return root;
        };
        let overlay = dialog::view(
            prompt,
            self.window_size,
            self.lang,
            &self.palette,
            PromptMessages {
                on_input: Box::new(Message::PromptInput),
                on_submit: Message::PromptSubmit,
                on_cancel: Message::PromptCancel,
                on_toggle_keep_folders: Message::PromptToggleKeepFolders,
            },
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
        self.panels.panel(side)
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelState {
        self.panels.panel_mut(side)
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panels.active_panel()
    }

    #[allow(dead_code)] // Used by upcoming two-panel operations (copy/move).
    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.panels.active.other())
    }

    /// The error the status line shows: a job's, else the config's.
    pub fn status_error(&self) -> Option<&str> {
        self.job_error.as_deref().or(self.config_error.as_deref())
    }

    /// What the bar's path field says while the active panel is a hit list:
    /// the query, where it ran and how many files it left, with the way back.
    /// The keys the bar shows: the search's while it has the keys, else the
    /// panels'. A hit list is no folder, so F7 (new folder) is left out.
    fn function_key_bar(&self) -> [Option<&'static str>; keymap::FUNCTION_KEY_COUNT] {
        if self.search_has_keys() {
            return self.search_function_keys;
        }
        let mut keys = self.function_keys;
        if self.active_panel().is_hit_list() {
            if let Some(slot) = keys.get_mut(keymap::NEW_FOLDER_SLOT) {
                *slot = None;
            }
        }
        keys
    }

    fn hit_line(&self) -> Option<statusbar::HitLine> {
        let panel = self.active_panel();
        let list = panel.hit_list.as_ref()?;
        let count = panel.entries.len();
        let scope = titlebar::tab_label(&list.scope, &self.home, false, usize::MAX);
        Some(statusbar::HitLine {
            text: ncrs_files::hit_list_flow::hit_line_text(self.lang, list, count, &scope),
            back_key: "Esc".to_owned(),
            back_label: self.lang.text(Msg::HitListBackHint).to_owned(),
        })
    }

    /// What the status bar shows instead of the panel's entry while the search
    /// has the keys: the cursor's file below the root.
    /// Empty without a cursor, so the panel's entry does not pose as the
    /// search's.
    fn search_status(&self) -> Option<String> {
        self.search.status(self.search_has_keys())
    }

    /// The state the key routing needs, cheap to clone.
    fn prompt_key_state(&self) -> PromptKeyState {
        let job_running = self.jobs.is_busy();
        let config_error = self.config_error.is_some();
        let volume_menu = self.panels.volume_menu.is_some();
        let context_menu = self.panels.context_menu.is_some();
        let context_submenu = self
            .panels
            .context_menu
            .as_ref()
            .is_some_and(ContextMenu::submenu_is_open);
        let quick_fix = self.hermes.quick_fix.is_some();
        let path_editing = self.panels.path_field.is_some();
        let command_line = self.command_line.is_focused();
        let search = self.search_has_keys();
        let trash = self.trash_has_keys();
        let hit_list = self.active_panel().is_hit_list();
        let quick_open = self.quick_open.is_some();
        let command_palette = self.command_palette.is_some();
        let key_bindings = self.settings.key_bindings.is_some();
        let key_bindings_recording = self
            .settings
            .key_bindings
            .as_ref()
            .is_some_and(KeyBindings::is_recording);
        let filing_status = self.hermes.filing_status.page_open();
        let hermes_overview = self.hermes.overview.is_some();
        let hermes_overview_editing = self
            .hermes
            .overview
            .as_ref()
            .is_some_and(OverviewState::is_editing);
        let filing_setup = self.hermes.filing_setup.is_some();
        let hermes_setup = self.hermes.hermes_setup.is_some();
        let filing_card = self
            .hermes
            .filing
            .as_ref()
            .is_some_and(FilingModule::card_can_be_dismissed);
        let theme_fetch = self.settings.theme_fetch.is_some();
        let rule_suggestions = self.hermes.rule_suggestions.is_some();
        let rules = self.hermes.rules.is_some();
        let rules_form = self
            .hermes
            .rules
            .as_ref()
            .is_some_and(RulesPage::is_editing);
        let rules_confirming_delete = self
            .hermes
            .rules
            .as_ref()
            .is_some_and(RulesPage::is_confirming_delete);
        let review_list = self.hermes.review_list.is_some();
        let review_list_editing = self
            .hermes
            .review_list
            .as_ref()
            .is_some_and(ReviewList::is_editing);
        let review_list_replanning = self
            .hermes
            .review_list
            .as_ref()
            .is_some_and(|view| view.replan().is_some());
        let saved_queries = self.hermes.saved_queries.is_some();
        let tags = self.hermes.tags.is_some();
        let hermes_sidebar =
            self.hermes_focus == HermesFocus::Sidebar && self.open_hermes_page().is_some();
        let reorganisation_plan = self.hermes.reorganisation.is_some();
        let about = self.about.is_some();
        let ocr_models = self.settings.ocr_models.is_some();
        let ai_model = self.settings.ai_card_is_shown();
        let grant_dialog = self.grant_dialog;
        let connect_dialog = self.remote_ui.dialog.is_some();
        let settings = self.settings.page.is_some();
        let transfer_dialog = self.ops.transfer_dialog.is_some();
        let transfer_progress = self.ops.progress_mask_is_open();
        let start_over_dialog = self.start_over_dialog;
        let delete_dialog = match &self.ops.delete_dialog {
            None => DeleteKeys::Closed,
            Some(deletion) if deletion.permanent => DeleteKeys::Permanent,
            Some(_) => DeleteKeys::Trash,
        };
        match &self.ops.prompt {
            None => PromptKeyState {
                keymap: Arc::clone(&self.keymap),
                job_running,
                config_error,
                delete_dialog,
                start_over_dialog,
                volume_menu,
                context_menu,
                context_submenu,
                quick_fix,
                path_editing,
                command_line,
                search,
                trash,
                hit_list,
                quick_open,
                command_palette,
                key_bindings,
                key_bindings_recording,
                filing_status,
                hermes_overview,
                hermes_overview_editing,
                filing_setup,
                hermes_setup,
                filing_card,
                theme_fetch,
                rule_suggestions,
                rules,
                rules_form,
                rules_confirming_delete,
                review_list,
                review_list_editing,
                review_list_replanning,
                saved_queries,
                tags,
                hermes_sidebar,
                reorganisation_plan,
                about,
                ocr_models,
                ai_model,
                grant_dialog,
                connect_dialog,
                settings,
                // A conflict is asked while no prompt is open — the prompt is
                // gone by then, the transfer is what is running. Without this
                // the dialog's keys would fall through to the panels.
                conflict_open: self.ops.conflict_is_open(),
                transfer_dialog,
                transfer_progress,
                ..PromptKeyState::default()
            },
            Some(prompt) => PromptKeyState {
                keymap: Arc::clone(&self.keymap),
                open: true,
                job_running,
                config_error,
                delete_dialog,
                start_over_dialog,
                volume_menu,
                context_menu,
                context_submenu,
                quick_fix,
                path_editing,
                command_line,
                search,
                trash,
                hit_list,
                quick_open,
                command_palette,
                key_bindings,
                key_bindings_recording,
                filing_status,
                hermes_overview,
                hermes_overview_editing,
                filing_setup,
                hermes_setup,
                filing_card,
                theme_fetch,
                rule_suggestions,
                rules,
                rules_form,
                rules_confirming_delete,
                review_list,
                review_list_editing,
                review_list_replanning,
                saved_queries,
                tags,
                hermes_sidebar,
                reorganisation_plan,
                about,
                ocr_models,
                ai_model,
                grant_dialog,
                connect_dialog,
                settings,
                conflict_open: self.ops.conflict_is_open(),
                transfer_dialog,
                transfer_progress,
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
        let from_results = self
            .open_search_view()
            .filter(|_| kind != ClipboardKind::Directory)
            .map(|search| ncrs_ops::clipboard_text_of_files(&search.action_files(), kind));
        let Some(copied) = from_results
            .unwrap_or_else(|| ncrs_ops::clipboard_text(rows_of(self.active_panel()), kind))
        else {
            return Task::none();
        };
        self.job_error = None;
        let shown = self.show_notice(ncrs_ops::copied_notice(self.lang, kind, copied.count));
        Task::batch([iced::clipboard::write(copied.text), shown])
    }

    /// The dots move only while the picture is on screen.
    fn hermes_flow_is_visible(&self) -> bool {
        self.hermes.hermes_setup.is_some() || self.hermes.overview.is_some()
    }

    fn hermes_setup_view(&self) -> Option<Element<'_, Message>> {
        let setup = self.hermes.hermes_setup.as_ref()?;
        let card = hermes_setup_ui::SetupView {
            setup,
            focus: self
                .hermes
                .setup_focus_order()
                .get(self.dialog_focus)
                .copied(),
            busy: self.hermes.setup_is_busy(),
            creating_index: self
                .hermes
                .filing
                .as_ref()
                .is_some_and(FilingModule::is_creating_index),
            creation_error: self
                .hermes
                .filing
                .as_ref()
                .and_then(FilingModule::index_creation_error),
            home: &self.home,
            flow_step: self.hermes_flow_step,
            lang: self.lang,
            window: self.window_size,
        };
        Some(hermes_setup_ui::view(&card, &self.palette).map(Message::from))
    }

    /// Shows `text` in the status bar for [`NOTICE_DURATION`]. The timer carries
    /// the generation, so it cannot clear a notice that came after it.
    /// F7 and Shift+F4: the name prompt for the active panel's folder.
    fn open_prompt(&mut self, prompt: Prompt) -> Task<Message> {
        let side = self.panels.active;
        self.ops.open_prompt(prompt, side)
    }

    fn show_notice(&mut self, text: String) -> Task<Message> {
        self.notice_generation = self.notice_generation.wrapping_add(1);
        self.notice = Some(text);
        let generation = self.notice_generation;
        Task::perform(
            async { tokio::time::sleep(NOTICE_DURATION).await },
            move |()| Message::NoticeExpired(generation),
        )
    }

    fn dialog_button_count(&self) -> usize {
        if self.hermes.hermes_setup.is_some() {
            self.hermes.setup_focus_order().len()
        } else if self.about.is_some() {
            3
        } else if self.settings.model_card_has_one_button() {
            1
        } else if self.ops.delete_dialog.is_some()
            || self.start_over_dialog
            || self.settings.ocr_models.is_some()
            || self.settings.ai_card_is_shown()
            || self.grant_dialog
        {
            2
        } else if self.ops.conflict_is_open() {
            3
        } else {
            0
        }
    }

    /// Enter in a button dialog: whatever the focused button does.
    fn activate_dialog_button(&mut self) -> Task<Message> {
        if self.hermes.hermes_setup.is_some() {
            return self.activate_hermes_focus();
        }
        if self.start_over_dialog {
            return if self.dialog_focus == hermes_overview_ui::START_OVER_CONFIRM_FOCUS {
                self.update_hermes_overview(HermesOverviewMessage::ConfirmStartOver)
            } else {
                self.start_over_dialog = false;
                Task::none()
            };
        }
        if self.ops.delete_dialog.is_some() {
            return if self.dialog_focus == delete_dialog_view::CONFIRM_FOCUS {
                self.confirm_delete()
            } else {
                self.ops.delete_dialog = None;
                Task::none()
            };
        }
        if self.about.is_some() {
            return match self.dialog_focus {
                about_ui::REPOSITORY_FOCUS => self.update_about(AboutMessage::OpenRepository),
                about_ui::LICENSES_FOCUS => self.update_about(AboutMessage::OpenLicenses),
                _ => self.update_about(AboutMessage::Close),
            };
        }
        if self.settings.model_card_has_one_button() {
            return if self.settings.ocr_models.is_some() {
                self.update_ocr_models(OcrModelsMessage::Close)
            } else {
                self.update_ai_model(AiModelMessage::Close)
            };
        }
        if self
            .settings
            .ai_model
            .as_ref()
            .is_some_and(AiModelCard::is_finished)
        {
            return if self.dialog_focus == ai_model_ui::DOWNLOAD_FOCUS {
                self.update_ai_model(AiModelMessage::ShowAiSetting)
            } else {
                self.update_ai_model(AiModelMessage::Close)
            };
        }
        if self.settings.ocr_models.is_some() {
            return if self.dialog_focus == ocr_models_ui::DOWNLOAD_FOCUS {
                self.update_ocr_models(OcrModelsMessage::Download)
            } else {
                self.update_ocr_models(OcrModelsMessage::Close)
            };
        }
        if self.settings.ai_card_is_shown() {
            return if self.dialog_focus == ai_model_ui::DOWNLOAD_FOCUS {
                self.press_ai_model_button()
            } else {
                self.update_ai_model(AiModelMessage::Close)
            };
        }
        if self.grant_dialog {
            return if self.dialog_focus == grant_ui::GRANT_FOCUS {
                self.update_grant(GrantMessage::Choose)
            } else {
                self.update_grant(GrantMessage::Later)
            };
        }
        if self.ops.conflict_is_open() {
            let choice = match self.dialog_focus {
                conflict::OVERWRITE_FOCUS => ConflictChoice::ThisOverwrite,
                conflict::KEEP_FOCUS => ConflictChoice::ThisKeep,
                _ => ConflictChoice::Cancel,
            };
            return self.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, choice, ctx));
        }
        Task::none()
    }

    /// A prompt, dialog or menu is on top of the panels.
    fn overlay_is_open(&self) -> bool {
        self.blocking_overlay_is_open() || self.panels.context_menu.is_some()
    }

    /// Everything on top of the panels except the context menu, which a new
    /// right click may replace.
    fn blocking_overlay_is_open(&self) -> bool {
        self.ablage_tab_is_open() || self.overlay_apart_from_overview_is_open()
    }

    /// The page of filing that stands in for the panels: the overview or the
    /// status.
    fn ablage_tab_is_open(&self) -> bool {
        self.hermes.filing_status.page_open() || self.hermes.page_is_open()
    }

    /// Closes the page of filing without a word, for a page that replaces it.
    fn close_ablage_tab(&mut self) -> Task<Message> {
        self.hermes.filing_status.close_page();
        let left_overview = self.leave_hermes_overview();
        self.hermes.close_pages();
        left_overview
    }

    /// What the overview itself may open on top of it: the model cards.
    fn overlay_apart_from_overview_is_open(&self) -> bool {
        self.overlay_apart_from_overview_and_hermes_is_open()
            || self.hermes.quick_fix.is_some()
            || self.hermes.hermes_setup.is_some()
    }

    fn overlay_apart_from_overview_and_hermes_is_open(&self) -> bool {
        self.ops.prompt.is_some()
            || self.ops.conflict_is_open()
            || self.ops.transfer_dialog.is_some()
            || self.ops.progress_mask_is_open()
            || self.ops.delete_dialog.is_some()
            || self.start_over_dialog
            || self.panels.volume_menu.is_some()
            || self.quick_open.is_some()
            || self.command_palette.is_some()
            || self.settings.key_bindings.is_some()
            || self.settings.theme_fetch.is_some()
            || self.about.is_some()
            || self.settings.ocr_models.is_some()
            || self.settings.ai_card_is_shown()
            || self.grant_dialog
            || self.remote_ui.dialog.is_some()
    }

    fn context_menu_facts(&self) -> MenuFacts {
        let panel = self.active_panel();
        MenuFacts {
            search_cursor: self
                .open_search_view()
                .map(|search| search.cursor_target().is_some()),
            has_sources: !self.action_sources().is_empty(),
            can_copy_path: ncrs_ops::clipboard_text(rows_of(panel), ClipboardKind::Path).is_some(),
            can_copy_name: ncrs_ops::clipboard_text(rows_of(panel), ClipboardKind::Name).is_some(),
            hermes: self.hermes_facts(),
        }
    }

    fn menu_open_ctx(&self) -> MenuOpenCtx {
        MenuOpenCtx {
            blocking_overlay_is_open: self.blocking_overlay_is_open(),
            search_side: self.search.open.then_some(self.search.side),
            visible_rows: self.visible_rows,
            window: self.window_size,
        }
    }

    /// The menu of the search results at the pointer, for the file the cursor
    /// is on or the tagged ones.
    fn open_search_context_menu(&mut self) {
        if self.blocking_overlay_is_open() || !self.search_has_keys() {
            return;
        }
        self.job_error = None;
        self.notice = None;
        let pointer = self.input.lock().pointer;
        let facts = self.context_menu_facts();
        self.panels
            .show_search_context_menu(pointer, self.window_size, &facts);
    }

    /// Opens the context menu on row `index` of `side`, at `anchor` or, without
    /// one, below that row.
    fn open_context_menu(&mut self, side: PanelSide, index: usize, anchor: Option<Point>) {
        let ctx = self.menu_open_ctx();
        let Some(place_at) = self.panels.point_at_menu_row(side, index, anchor, &ctx) else {
            return;
        };
        self.job_error = None;
        self.notice = None;
        let facts = self.context_menu_facts();
        self.panels
            .show_context_menu(side, place_at, self.window_size, &facts);
    }

    fn run_context_menu_entry(&mut self) -> Task<Message> {
        let from_search = self.search_has_keys();
        let actions = self.panels.run_context_menu_entry(from_search);
        self.run_panel_actions(actions)
    }

    fn click_context_menu(&mut self, index: usize, in_submenu: bool) -> Task<Message> {
        let from_search = self.search_has_keys();
        let actions = self
            .panels
            .click_context_menu(index, in_submenu, from_search);
        self.run_panel_actions(actions)
    }

    /// The file or folder under the cursor of the active panel, for the Hermes
    /// commands: not the `..` row.
    fn hermes_cursor_entry(&self) -> Option<&fs::FileEntry> {
        self.active_panel()
            .selected_entry()
            .filter(|entry| !entry.is_parent)
    }

    fn cursor_is_excluded_from_hermes(&self) -> bool {
        self.hermes_cursor_entry().is_some_and(|entry| {
            if entry.is_dir {
                self.hermes
                    .hermes_column
                    .summary_of(&entry.path)
                    .is_some_and(|summary| summary.excluded)
            } else {
                self.hermes.hermes_column.state_of(&entry.path) == Some(HermesFileState::Excluded)
            }
        })
    }

    fn inbox_location(&self) -> Option<&Path> {
        self.hermes
            .filing_config
            .locations
            .iter()
            .find(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.as_path())
    }

    /// What the Hermes entry of the context menu checks for the cursor row.
    fn hermes_facts(&self) -> HermesFacts {
        let locations = &self.hermes.filing_config.locations;
        let cursor = self.hermes_cursor_entry().map(|entry| {
            let role_free = |role| role_of(&entry.path, locations) != Some(role);
            HermesCursor {
                is_dir: entry.is_dir,
                excluded: self.cursor_is_excluded_from_hermes(),
                can_be_handed_to_inbox: self
                    .inbox_location()
                    .is_some_and(|inbox| !entry.path.starts_with(inbox)),
                lies_in_a_filing_location: locations
                    .iter()
                    .any(|location| entry.path.starts_with(&location.path)),
                inbox_role_is_free: role_free(FolderRole::Inbox),
                filing_location_role_is_free: role_free(FolderRole::FilingLocation),
                archive_role_is_free: role_free(FolderRole::Archive),
            }
        });
        HermesFacts {
            ready: self.hermes.is_ready(),
            active: self.hermes.is_active(),
            has_inquiry_paths: !self.inquiry_paths().is_empty(),
            has_live_pipeline: self.hermes.live_pipeline().is_some(),
            cursor,
        }
    }

    fn hermes_action_enabled(&self, action: HermesAction) -> bool {
        self.hermes_facts().action_enabled(action)
    }

    fn update_hermes_file(&mut self, message: HermesFileMessage) -> Task<Message> {
        match message {
            HermesFileMessage::RoleSaved(result) => {
                return self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::hermes_role_saved(hermes, result, ctx)
                })
            }
            HermesFileMessage::Included { path, result } => {
                return self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::hermes_included(hermes, &path, result, ctx)
                })
            }
            HermesFileMessage::HandToInbox
            | HermesFileMessage::ProcessAgain
            | HermesFileMessage::ReprocessFolder
            | HermesFileMessage::SetRole(_)
            | HermesFileMessage::ToggleExclusion
            | HermesFileMessage::ShowInInspection => {}
        }
        let available = if matches!(message, HermesFileMessage::SetRole(_)) {
            self.hermes.is_ready()
        } else {
            self.hermes.is_active()
        };
        if self.overlay_is_open() || self.search.open || !available {
            return Task::none();
        }
        let cursor = self
            .hermes_cursor_entry()
            .map(|entry| (entry.path.clone(), entry.is_dir));
        if message == HermesFileMessage::ReprocessFolder {
            let folder = match cursor {
                Some((path, true)) => path,
                Some((_, false)) | None => self.active_panel().path.clone(),
            };
            return self.reprocess_folder(folder);
        }
        let Some((path, is_dir)) = cursor else {
            return Task::none();
        };
        match message {
            HermesFileMessage::HandToInbox => {
                let Some(inbox) = self.inbox_location().map(Path::to_path_buf) else {
                    let text = self.lang.text(Msg::StatusHermesNoInbox).to_owned();
                    return self.show_notice(text);
                };
                self.open_transfer_dialog_to(TransferKind::Move, Some(inbox))
            }
            HermesFileMessage::ProcessAgain => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::process_again(hermes, &path, ctx)
            }),
            HermesFileMessage::SetRole(role) if is_dir => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::set_folder_role(hermes, &path, role, ctx)
                }),
            HermesFileMessage::ToggleExclusion => {
                let excluded = self.cursor_is_excluded_from_hermes();
                self.run_hermes_flow(Vec::new(), |hermes, _| {
                    ncrs_hermes_ui::toggle_hermes_exclusion(hermes, path, excluded)
                })
            }
            HermesFileMessage::ShowInInspection => {
                let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::show_in_inspection(hermes, &path, ctx)
                });
                self.run_hermes_outcome(outcome)
            }
            HermesFileMessage::SetRole(_)
            | HermesFileMessage::ReprocessFolder
            | HermesFileMessage::RoleSaved(_)
            | HermesFileMessage::Included { .. } => Task::none(),
        }
    }

    /// The create of the name prompt has reported: close the prompt or show
    /// why it failed, and reload the folder.
    fn finish_prompt(
        &mut self,
        kind: PromptKind,
        request_id: u64,
        result: Result<(), fs::CreateDirError>,
    ) -> Task<Message> {
        if !self.ops.is_current_prompt_request(request_id) {
            return Task::none();
        }
        // The queued reload runs here, after the create has reported, never
        // beside it. On success it selects the new entry; on failure it still
        // re-reads, because the name may belong to something else that
        // appeared meanwhile.
        let pending = self.panels.take_pending_reload();
        let created = pending.as_ref().map(ReloadNow::created);
        let reload = pending.map(|due| self.load(due.side, due.path, Some(due.name)));
        let lang = self.lang;
        if let Some(path) = self.ops.finish_prompt(kind, result, created, lang) {
            self.launch_external(fs::OpenKind::Edit, path, None, CopyOf::Nothing);
        }
        reload.unwrap_or_else(Task::none)
    }

    /// Writes the session state here and now, then ends the app: the runtime
    /// stops right after, so no task would run.
    fn quit(&mut self) -> Task<Message> {
        if let Some(path) = &self.state_path {
            write_session_state(
                &self.session_writes,
                WriteOrder::FINAL,
                path,
                &self.session_state(),
            );
        }
        if let Some(copies) = self.ops.view_copies.as_mut() {
            copies.close();
        }
        self.remote.close_all();
        iced::exit()
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

    /// F8 / Shift+F8: ask before deleting.
    fn open_delete_dialog(&mut self, permanent: bool) -> Task<Message> {
        let side = self.source_side();
        let request = ncrs_ops::DeleteRequest {
            permanent,
            sources: self.action_sources(),
            side,
            in_archive: self.panel(side).archive.is_some(),
        };
        self.with_ops(|ops, ctx| ncrs_ops::open_delete(ops, request, ctx))
    }

    fn confirm_delete(&mut self) -> Task<Message> {
        self.with_ops(ncrs_ops::confirm_delete)
    }

    fn end_delete(&mut self) -> Task<Message> {
        self.with_ops(ncrs_ops::end_delete)
    }

    fn end_restore(&mut self) -> Task<Message> {
        self.with_ops(|ops, ctx| ncrs_ops::end_restore(ops, ctx, false))
    }

    /// Runs `step` on the file operation flows and then what they ask of the app.
    fn with_ops(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>) -> ncrs_ops::Outcome,
    ) -> Task<Message> {
        let outcome = self.with_ops_ctx(step);
        self.run_ops_outcome(outcome)
    }

    /// Runs `step` on the flows' state, taken out of the app for it since the
    /// context borrows the rest.
    fn with_ops_ctx<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>) -> R,
    ) -> R {
        self.with_ops_and_panels_ctx(|ops, flow, _| step(ops, flow))
    }

    /// As [`Self::with_ops_ctx`], with the panels to read from.
    fn with_ops_and_panels_ctx<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>, &Panels) -> R,
    ) -> R {
        let start_blocked =
            self.jobs.is_busy() || self.ops.prompt.is_some() || self.ops.conflict_is_open();
        let transfer_dialog_open = self.ops.transfer_dialog.is_some();
        let permanent_delete_key = self
            .keymap
            .shortcut_label(&Message::Delete { permanent: true });
        let restore_last_key = self
            .keymap
            .shortcut_label(&KeyAction::RestoreLastDeletion.message());
        let mut ops = std::mem::take(&mut self.ops);
        let outcome = step(
            &mut ops,
            &ncrs_ops::FlowCtx {
                lang: self.lang,
                trash: &self.trash,
                jobs_busy: self.jobs.is_busy(),
                start_blocked,
                transfer_dialog_open,
                has_job_error: self.job_error.is_some(),
                sandbox: &self.sandbox,
                source_side: self.source_side(),
                left_archive: self.panel(PanelSide::Left).archive.as_deref(),
                right_archive: self.panel(PanelSide::Right).archive.as_deref(),
                left_server: self.panel(PanelSide::Left).remote().map(|s| s.root_path()),
                right_server: self.panel(PanelSide::Right).remote().map(|s| s.root_path()),
                remote: &self.remote,
                permanent_delete_key,
                restore_last_key,
            },
            &self.panels,
        );
        self.ops = ops;
        outcome
    }

    /// As [`Self::with_ops`], with the panels to read from.
    fn with_ops_and_panels(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>, &Panels) -> ncrs_ops::Outcome,
    ) -> Task<Message> {
        let outcome = self.with_ops_and_panels_ctx(step);
        self.run_ops_outcome(outcome)
    }

    fn run_ops_actions(&mut self, actions: Vec<ncrs_ops::Action>) -> Task<Message> {
        self.run_ops_outcome((Task::none(), actions))
    }

    fn run_ops_outcome(&mut self, (task, actions): ncrs_ops::Outcome) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_ops_action(action));
        }
        Task::batch(tasks)
    }

    fn run_ops_action(&mut self, action: ncrs_ops::Action) -> Task<Message> {
        use ncrs_ops::Action as Ask;
        match action {
            Ask::ClearStatus => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Ask::FocusDialogButton(button) => {
                self.dialog_focus = button;
                Task::none()
            }
            Ask::StartJob(start) => self.start_job(start.job, start.build),
            Ask::StartJobWithProgress(start) => {
                self.start_job_with_progress(start.job, start.build)
            }
            Ask::FinishJob => {
                self.jobs.finish();
                Task::none()
            }
            Ask::SetJobProgress(tick) => {
                self.job_progress = Some(tick);
                Task::none()
            }
            Ask::ClearJobProgress => {
                self.job_progress = None;
                Task::none()
            }
            Ask::ClearSelection(side) => {
                self.panel_mut(side).selection.clear();
                Task::none()
            }
            Ask::ReloadSourceThenTarget => {
                let source_side = self.source_side();
                let sides = [source_side, source_side.other()];
                Task::batch(sides.map(|each| {
                    let path = self.panel(each).path.clone();
                    self.load(each, path, None)
                }))
            }
            Ask::ReloadPanelsSelecting { folder, name } => {
                let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
                    let path = self.panel(each).path.clone();
                    let select = (path == folder).then(|| name.clone());
                    self.load(each, path, select)
                });
                Task::batch(tasks)
            }
            Ask::CountJobDone => {
                self.job_done += 1;
                Task::none()
            }
            Ask::ResetJobDone => {
                self.job_done = 0;
                Task::none()
            }
            Ask::LaunchViewCopy { path, origin } => {
                let from = match origin {
                    ncrs_ops::ViewOrigin::Archive => CopyOf::Archive,
                    ncrs_ops::ViewOrigin::Server => CopyOf::Server,
                };
                self.launch_external(fs::OpenKind::View, path, None, from);
                Task::none()
            }
            Ask::ReloadPanels => {
                let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
                    let path = self.panel(each).path.clone();
                    self.load(each, path, None)
                });
                Task::batch(tasks)
            }
            Ask::RestartSearch => self.restart_search(),
            Ask::ListTrash => self.list_trash(),
            Ask::CloseQuickOpenAndPalette => {
                Task::batch([self.close_quick_open(), self.close_command_palette()])
            }
            Ask::OpenTrash(side) => self.open_trash_on(side),
            Ask::OpenTrashInFinder => self.open_trash_in_finder(),
            Ask::ShowErrorWithFinderPress(text) => {
                self.set_status_error_with_press(&text, Message::Trash(TrashMessage::OpenInFinder));
                Task::none()
            }
        }
    }

    // --- the trash ---

    fn update_trash(&mut self, message: TrashMessage) -> Task<Message> {
        let rows = self.trash_rows();
        self.with_ops(|ops, flow| ncrs_ops::update_trash_flow(ops, message, rows, flow))
    }

    /// Rows of the trash view that fit the window.
    fn trash_rows(&self) -> usize {
        layout::trash_visible_rows(self.window_size)
    }

    /// Shows the trash in place of the panel on `side` and reads it. Where the
    /// app cannot read the trash, the Finder opens it instead.
    fn open_trash_on(&mut self, side: PanelSide) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        let reach = self.trash.reach();
        if reach == fs::TrashReach::FinderOnly {
            return self.open_trash_in_finder();
        }
        self.panels.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let closed_search = if self.search.open && self.search.side == side {
            self.update_search(SearchMessage::Close)
        } else {
            Task::none()
        };
        self.panels.active = side;
        let listing = self.ops.show_trash(side, reach, &self.trash);
        Task::batch([closed_search, listing.map(Message::Trash)])
    }

    /// Reads the trash on the blocking pool; the answer carries the generation,
    /// so a late one finds the view closed or read again.
    fn list_trash(&mut self) -> Task<Message> {
        self.ops.list_trash(&self.trash).map(Message::Trash)
    }

    fn trash_panel(&self, side: PanelSide) -> Element<'_, Message> {
        let Some(view) = self.ops.trash_view.as_ref() else {
            return Space::new().into();
        };
        trash_ui::view(trash_ui::Props {
            view,
            visible_rows: self.trash_rows(),
            is_active: self.panels.active == side,
            panel_width: self.window_size.width / 2.0,
            lang: self.lang,
            palette: &self.palette,
            finder_key: self
                .keymap
                .shortcut_label(&KeyAction::OpenTrashInFinder.message()),
        })
        .map(Message::from)
    }

    /// Shows the trash folder in the Finder, where the app cannot list it.
    #[cfg(target_os = "macos")]
    fn open_trash_in_finder(&mut self) -> Task<Message> {
        if let Err(reason) = self.launcher.launch(&fs::trash_in_finder_command()) {
            let text = fill_template(
                self.lang.text(Msg::TrashFinderFailed),
                &[("{reason}", &reason)],
            );
            self.set_status_error(&text);
        }
        Task::none()
    }

    /// Only the Finder has a trash the app cannot list; no other platform
    /// offers the action.
    #[cfg(not(target_os = "macos"))]
    fn open_trash_in_finder(&mut self) -> Task<Message> {
        Task::none()
    }

    /// F3 / F4: the row under the cursor goes to the external program. Tags
    /// are ignored, as in Norton Commander; `..` and directories have nothing
    /// to view or edit. A file inside an archive is unpacked first, and only
    /// viewing works: the copy is thrown away, so an edit would be lost.
    fn open_external(&mut self, kind: fs::OpenKind) -> Task<Message> {
        let source = match self.open_search_view() {
            Some(search) => {
                OpenSource::Search(search.cursor_target().filter(|(path, _)| !path.is_dir()))
            }
            None => OpenSource::ActivePanel,
        };
        match self.panels.plan_open(kind, source) {
            OpenPlan::Nothing => Task::none(),
            OpenPlan::Refused(msg) => {
                let text = self.lang.text(msg).to_owned();
                self.set_status_error(&text);
                Task::none()
            }
            OpenPlan::ViewFromServer(remote) => self.view_from_server(remote),
            OpenPlan::ViewFromArchive {
                source: packed,
                archive,
            } => self.view_from_archive(packed, archive),
            OpenPlan::Launch { path, line } => {
                self.launch_external(kind, path, line, CopyOf::Nothing);
                Task::none()
            }
        }
    }

    /// Starts unpacking `source`, a file in `archive`, for the viewer. The
    /// viewer starts when the copy is there, see `view_extracted`.
    fn view_from_archive(&mut self, source: PathBuf, archive: PathBuf) -> Task<Message> {
        let shown = source.clone();
        self.with_ops(move |ops, ctx| {
            ncrs_ops::start_view_copy(
                ops,
                archive,
                shown,
                jobs::JobKind::Extract,
                move |slot, tx| {
                    let mut progress = fs::transfer::Reporting::new(tx);
                    fs::archive::extract_for_view(&source, slot, &mut progress)
                },
                ctx,
            )
        })
    }

    /// Starts fetching `source`, a file on a server, for the viewer. The copies
    /// of one server are kept together under its root address, as those of an
    /// archive are under the archive.
    fn view_from_server(&mut self, source: RemotePath) -> Task<Message> {
        let pool = Arc::clone(&self.remote);
        let (key, shown) = (source.server().root_path(), source.to_path_buf());
        self.with_ops(move |ops, ctx| {
            ncrs_ops::start_view_copy(
                ops,
                key,
                shown,
                jobs::JobKind::Copy,
                move |slot, tx| {
                    let mut progress = fs::transfer::Reporting::new(tx);
                    fs::remote::transfer::download_for_view(&pool, &source, slot, &mut progress)
                },
                ctx,
            )
        })
    }

    fn reload_local_panels(&mut self) -> Task<Message> {
        let folders = self.panels.local_folders();
        Task::batch(
            folders
                .into_iter()
                .map(|(side, path)| self.load(side, path, None)),
        )
    }

    fn open_pack_prompt(&mut self) -> Task<Message> {
        let (sources, source_side) = (self.action_sources(), self.source_side());
        self.with_ops_and_panels(|ops, ctx, panels| {
            let facts = selection_facts(panels, source_side, sources);
            ncrs_ops::open_pack_prompt(ops, facts, ctx)
        })
    }

    fn toggle_keep_folders(&mut self) -> Task<Message> {
        self.ops.toggle_keep_folders();
        Task::none()
    }

    /// Stops the running job, see `Message::AbortJob`.
    fn abort_job(&mut self) -> Task<Message> {
        if !self.jobs.abort_running() {
            return Task::none();
        }
        self.drop_progress_channel();
        if self.ops.extracting.is_some() {
            ncrs_ops::end_extraction(&mut self.ops);
            Task::none()
        } else if let Some(outcome) = ncrs_ops::abort_archive_write(&mut self.ops) {
            self.run_ops_outcome(outcome)
        } else if self.ops.deleting.is_some() {
            self.end_delete()
        } else if self.ops.restoring.is_some() {
            self.end_restore()
        } else {
            let outcome = ncrs_ops::end_transfer(&mut self.ops);
            self.run_ops_outcome(outcome)
        }
    }

    /// Both halves of the progress channel go together: a sender kept without
    /// its receiver makes every later transfer fail on a closed channel.
    /// Dropping the receiver is also what stops a blocking copy, whose next
    /// tick cannot be sent.
    fn drop_progress_channel(&mut self) {
        self.job_done = 0;
        self.job_progress = None;
        self.progress_rx = None;
        self.progress_tx = None;
    }

    /// A panel has left `archive`: the copies made for the viewer go, unless
    /// the other panel is still in it. A running unpacking stops first.
    fn drop_view_copies_of(&mut self, archive: &Path) {
        if self.panels.shows(archive) {
            return;
        }
        if self
            .ops
            .extracting
            .as_ref()
            .is_some_and(|extracting| extracting.archive == archive)
        {
            self.jobs.abort_running();
            self.drop_progress_channel();
            ncrs_ops::end_extraction(&mut self.ops);
        }
        if let Some(copies) = self.ops.view_copies.as_mut() {
            copies.release(archive);
        }
    }

    /// Hands `path` to the program for `kind`, at `line` when the program
    /// takes one. `from` says whether `path` is a temporary copy made for
    /// viewing, and of what.
    fn launch_external(
        &mut self,
        kind: fs::OpenKind,
        path: PathBuf,
        line: Option<u64>,
        from: CopyOf,
    ) {
        self.job_error = None;
        self.notice = None;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let executable = fs::could_execute(&path);
        let Some(command) = fs::open_command(&self.open_programs, kind, &path, executable, line)
        else {
            let label = |opener| self.keymap.shortcut_label(&Message::OpenExternal(opener));
            let message = open_refused_text(
                self.lang,
                &name,
                from,
                label(fs::OpenKind::View),
                label(fs::OpenKind::Edit),
            );
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

    /// F5 / F6: resolve what to transfer and ask before anything starts.
    ///
    /// The rows are resolved here rather than at the confirmation because a
    /// conflict dialog pauses the operation, and by then the selection may
    /// have moved. With a transfer already running the key brings its progress
    /// back instead: one transfer at a time owns the conflict state.
    fn open_transfer_dialog(&mut self, kind: TransferKind) -> Task<Message> {
        self.open_transfer_dialog_to(kind, None)
    }

    fn open_transfer_dialog_to(
        &mut self,
        kind: TransferKind,
        preset: Option<PathBuf>,
    ) -> Task<Message> {
        let (sources, source_side) = (self.action_sources(), self.source_side());
        self.with_ops_and_panels(|ops, ctx, panels| {
            let facts = selection_facts(panels, source_side, sources);
            ncrs_ops::open_transfer_dialog(ops, kind, preset, facts, ctx)
        })
    }

    fn update_transfer_dialog(&mut self, message: TransferDialogMessage) -> Task<Message> {
        self.with_ops(|ops, ctx| ncrs_ops::update_transfer_dialog_flow(ops, message, ctx))
    }

    fn copy_external_files_into_active_panel(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        let active = self.panels.active;
        self.with_ops_and_panels(|ops, ctx, panels| {
            ncrs_ops::copy_external_files(ops, paths, panel_facts(panels, active), ctx)
        })
    }

    #[cfg(test)]
    fn prompt_error(&self, err: &CreateDirError) -> String {
        self.ops.prompt_error(self.lang, err)
    }

    fn start_prompt_job(&mut self) -> Task<Message> {
        let active = self.panels.active;
        self.with_ops_and_panels(|ops, ctx, panels| {
            ncrs_ops::start_prompt_job(ops, panel_facts(panels, active), ctx)
        })
    }

    /// Cmd+V / Ctrl+V: reads the system clipboard, then `paste_content` decides.
    fn paste_from_clipboard(&mut self) -> Task<Message> {
        if self
            .ops
            .transfer_into_panel_is_blocked(self.ops.transfer_dialog.is_some())
        {
            let text = self.lang.text(Msg::StatusPasteBusy).to_owned();
            return self.show_notice(text);
        }
        (self.read_clipboard)().map(Message::ClipboardRead)
    }

    fn paste_content(&mut self, content: Option<PastedContent>) -> Task<Message> {
        let active = self.panels.active;
        self.with_ops_and_panels(|ops, ctx, panels| {
            ncrs_ops::paste_content(ops, content, panel_facts(panels, active), ctx)
        })
    }

    /// Files from another app held over the window outline the active panel;
    /// a row dragged over the other panel outlines that one.
    fn is_drop_target(&self, side: PanelSide) -> bool {
        (self.ops.drop_hover && side == self.panels.active) || self.panels.drag_hovers_side(side)
    }

    /// A cursor only where a drag from a panel is over a panel; files held over
    /// the window from another app keep the OS cursor.
    fn drag_cursor(&self, side: PanelSide) -> Option<mouse::Interaction> {
        self.panels.drag_cursor(side, self.modifiers)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn drag_out_started(&mut self, started: Result<(), String>) -> Task<Message> {
        let failure = ncrs_ops::drag_out::drag_out_failure(self.lang, started);
        self.run_ops_actions(failure.into_iter().collect())
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn end_panel_drag(&mut self) -> Option<crate::panel_drag::PanelDrag> {
        self.input.lock().panel_drag_armed = false;
        self.panels.drag.take()
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn start_drag_out(&mut self) -> Task<Message> {
        self.end_panel_drag();
        let sources = self.action_sources();
        let all_on_local_disk = sources
            .iter()
            .all(|path| ncrs_files::panel::is_file_on_local_disk(path));
        let (started, actions) = ncrs_ops::drag_out::start_drag_out(
            self.lang,
            sources,
            all_on_local_disk,
            ncrs_ops::drag_out::DragIcon {
                size: crate::WINDOW_ICON_SIZE,
                rgba: crate::WINDOW_ICON_RGBA,
            },
        );
        Task::batch([
            started.map(Message::DragOutStarted),
            self.run_ops_actions(actions),
        ])
    }

    fn copy_files_to_clipboard(&mut self) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let sources = self.action_sources();
        let all_on_local_disk = sources
            .iter()
            .all(|path| ncrs_files::panel::is_file_on_local_disk(path));
        let actions = ncrs_ops::copy_files_to_clipboard(
            self.lang,
            &sources,
            all_on_local_disk,
            &mut *self.write_clipboard_files,
        );
        self.run_ops_actions(actions)
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

    fn run_reorganisation(&mut self, selection: Vec<PlannedItem>) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::run_reorganisation(hermes, selection, ctx)
        })
    }

    fn documents_filed(&mut self, result: Result<FilingSummary, String>) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |_, ctx| {
            ncrs_hermes_ui::documents_filed(result, ctx)
        })
    }

    fn reorganisation_executed(
        &mut self,
        result: Result<ExecutionSummary, String>,
    ) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::reorganisation_executed(hermes, result, ctx)
        })
    }

    fn undo_reorganisation(&mut self) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::undo_reorganisation(hermes, ctx)
        })
    }

    fn reorganisation_undone(
        &mut self,
        result: Result<Option<execute::UndoReport>, String>,
    ) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::reorganisation_undone(hermes, result, ctx)
        })
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
            abortable: self.ops.prompt.is_none() && !self.ops.conflict_is_open(),
        })
    }

    /// Records why a job failed. The status bar reads it; nothing else does, so
    /// it is a string rather than a typed error — the layer that knew the kind
    /// already wrote the wording.
    fn set_status_error(&mut self, reason: &str) {
        self.job_error = Some(reason.to_string());
        self.job_error_press = None;
        self.notice = None;
    }

    /// An error the user can press, for instance to open the trash in the Finder.
    fn set_status_error_with_press(&mut self, reason: &str, press: Message) {
        self.set_status_error(reason);
        self.job_error_press = Some(press);
    }

    /// A panel inside an archive can be read and copied from, nothing else.
    fn apply_unless_refused(&mut self, message: Message) -> Task<Message> {
        if asks_for_hermes(&message) {
            if self.hermes.is_restarting() {
                let text = self.lang.text(Msg::StatusHermesRestarting).to_owned();
                return self.show_notice(text);
            }
            if self.hermes.is_off() {
                return self.explain_hermes_is_off();
            }
        }
        let panel = self.panel(self.source_side());
        if panel.is_hit_list() && is_not_offered_in_hit_list(&message) {
            return Task::none();
        }
        match archive_refusal(panel, &message).or_else(|| remote_refusal(panel, &message)) {
            Some(refusal) => {
                let text = self.lang.text(refusal).to_string();
                self.set_status_error(&text);
                Task::none()
            }
            None => self.apply(message),
        }
    }

    fn toggle_shortcuts(&self) -> ncrs_search::ToggleShortcuts {
        let label = |toggle| self.keymap.shortcut_label(&Message::Search(toggle));
        ncrs_search::ToggleShortcuts {
            case: label(SearchMessage::ToggleCase),
            whole_word: label(SearchMessage::ToggleWholeWord),
            regex: label(SearchMessage::ToggleRegex),
        }
    }

    /// The search view, while it is up.
    fn open_search_view(&self) -> Option<&SearchView> {
        self.search.open_view(self.search_has_keys())
    }

    /// The search view is up and its panel is the active one: the keys and the
    /// file actions are the search's. After Tab or a click on the other panel
    /// they are that panel's again, while the search stays on screen.
    /// The trash view is up and its panel is the active one: its keys are
    /// the cursor's.
    fn trash_has_keys(&self) -> bool {
        self.ops.trash_view.is_some() && self.panels.active == self.ops.trash_side
    }

    fn search_has_keys(&self) -> bool {
        self.search.open && self.panels.active == self.search.side
    }

    /// The panel that file actions take their files from: the one the search
    /// view stands in for, else the active one.
    fn source_side(&self) -> PanelSide {
        if self.search_has_keys() {
            self.search.side
        } else if self.trash_has_keys() {
            self.ops.trash_side
        } else {
            self.panels.active
        }
    }

    fn apply_window_size(&mut self, size: Size) {
        self.window_size = size;
        self.resize_command_palette(size);
        self.visible_rows = layout::visible_rows(size);
        self.search.resize(size);
        self.panels.left.ensure_visible(self.visible_rows);
        self.panels.right.ensure_visible(self.visible_rows);
    }

    /// Puts what an earlier run remembered on top of the fresh start. The first
    /// listing of each panel arrives sorted the default way and is re-sorted by
    /// `apply_sorted_listing`, which compares its `sorted_by` with the panel's.
    fn apply_session_state(&mut self, saved: &SessionState) {
        self.panels.left.sort = saved.left.sort_key();
        self.panels.right.sort = saved.right.sort_key();
        self.panels.left.history = saved.left.history();
        self.panels.right.history = saved.right.history();
        // Not checked for existence: a favourite on a drive that is not
        // plugged in now is still wanted when it is.
        self.panels.favourites = saved.favourites.iter().map(PathBuf::from).collect();
        if let Some(side) = saved.active {
            self.panels.active = side;
        }
        if let Some(size) = saved.window_size() {
            self.apply_window_size(size);
        }
        // Starting is not a change: nothing is written until something differs.
        self.session_scheduled = self.session_state();
    }

    /// What would be remembered if ncrs quit now.
    fn session_state(&self) -> SessionState {
        let record = |panel: &PanelState| {
            // A restart must not unpack a large tar.zst before the panel shows
            // anything, and the archive may be gone or replaced by then, so a
            // panel inside one is remembered as standing on the archive file.
            let (path, cursor) = match panel.archive.as_deref().and_then(archive_file_place) {
                Some((folder, name)) => (session_state::record_path(folder), Some(name)),
                // A server is never written to the file; the panel comes back
                // in the home folder.
                None if panel.remote().is_some() => (session_state::record_path(&self.home), None),
                None => (
                    session_state::record_path(
                        panel
                            .hit_list
                            .as_ref()
                            .map_or(&panel.path, |list| &list.return_to),
                    ),
                    panel
                        .selected_entry()
                        .filter(|entry| !entry.is_parent)
                        .and_then(|entry| entry.name.to_str().map(str::to_owned)),
                ),
            };
            PanelRecord {
                path,
                sort: panel.sort.column,
                descending: panel.sort.descending,
                back: session_state::record_history(panel.history.back()),
                forward: session_state::record_history(panel.history.forward()),
                cursor,
            }
        };
        let current = WindowSize {
            width: self.window_size.width,
            height: self.window_size.height,
        };
        // A minimised window reports no usable size; the last good one stays.
        let window = if current.restorable().is_some() {
            Some(current)
        } else {
            self.session_scheduled.window
        };
        SessionState {
            version: session_state::STATE_VERSION,
            active: Some(self.panels.active),
            favourites: session_state::record_paths(&self.panels.favourites),
            window,
            left: record(&self.panels.left),
            right: record(&self.panels.right),
        }
    }

    /// Plans a write when the remembered state differs from the last one
    /// planned. Debounced: the timer carries the generation, so a resize drag
    /// or a quick walk through folders writes once, after it settles.
    fn schedule_session_save(&mut self) -> Task<Message> {
        if self.state_path.is_none() {
            return Task::none();
        }
        let snapshot = self.session_state();
        if snapshot == self.session_scheduled {
            return Task::none();
        }
        self.session_scheduled = snapshot;
        self.session_save_generation = self.session_save_generation.wrapping_add(1);
        let generation = self.session_save_generation;
        Task::perform(
            async { tokio::time::sleep(SESSION_SAVE_DELAY).await },
            move |()| Message::SaveSessionState(generation),
        )
    }

    /// The write a timer is entitled to: only the newest generation, and only
    /// with a state path.
    fn due_session_state(&self, generation: u64) -> Option<(PathBuf, SessionState)> {
        if generation != self.session_save_generation {
            return None;
        }
        Some((self.state_path.clone()?, self.session_scheduled.clone()))
    }

    /// What the search reads from the app.
    fn search_ctx(&self) -> ncrs_search::Ctx {
        ncrs_search::Ctx {
            window_size: self.window_size,
            side_is_remote: self.panel(self.search.side).remote().is_some(),
            has_keys: self.search_has_keys(),
            index: self
                .hermes
                .filing
                .as_ref()
                .and_then(FilingModule::query_access)
                .map(|access| Arc::new(access) as Arc<dyn ncrs_search::SearchIndex>),
        }
    }

    /// The rows of the search after its filters changed what they offer.
    fn refresh_search_rows(&mut self) {
        self.search.resize(self.window_size);
    }

    fn focus_search_field(&self) -> Task<Message> {
        self.search.focus_field()
    }

    /// Shows the search view in place of the active panel, below that panel's
    /// directory. A view kept from before keeps its fields and options, and
    /// searches anew, so no result is older than the files. Pressed while it is
    /// up, the key moves to the next field.
    fn open_search(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        if self.search.open {
            if self.search_has_keys() {
                self.search.focus_next_field();
            } else {
                self.panels.active = self.search.side;
            }
            return self.focus_search_field();
        }
        self.panels.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let side = self.panels.active;
        if self.ops.trash_side == side {
            self.ops.close_trash();
        }
        let root = self.panel(side).path.clone();
        let indexed = self.panel(side).remote().is_none()
            && self
                .hermes
                .filing
                .as_ref()
                .and_then(FilingModule::query_access)
                .is_some();
        let kinds = Arc::clone(&self.kinds.catalog);
        self.search.show(side, root, indexed, kinds);
        self.refresh_search_rows();
        let search = self.restart_search();
        Task::batch([self.focus_search_field(), search])
    }

    /// The search opened with the words typed in the box that asked.
    fn open_search_with_words(&mut self, text: String) -> Task<Message> {
        let opened = self.open_search();
        self.search.set_query(text);
        Task::batch([opened, self.restart_search()])
    }

    /// Searches again now, for when the files changed under the results.
    fn restart_search(&mut self) -> Task<Message> {
        let ctx = self.search_ctx();
        self.search.restart(&ctx).map(Message::Search)
    }

    /// Lists the hits of the search in the panel on `target`.
    fn open_hit_list(&mut self, target: PanelSide) -> Task<Message> {
        let Some(view) = self.open_search_view() else {
            return Task::none();
        };
        if view.results.is_empty() {
            return Task::none();
        }
        let hits = ncrs_files::hit_list_flow::Hits {
            paths: view.results.iter().map(|file| file.path.clone()).collect(),
            query: view.query.trim().to_owned(),
            scope: view.root.clone(),
        };
        let origin = self.search.side;
        self.panels.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let closed = if target == origin {
            self.close_search()
        } else {
            if self.ops.trash_view.is_some() && self.ops.trash_side == target {
                self.ops.close_trash();
            }
            Task::none()
        };
        let read = self.panels.list_hits(target, hits);
        Task::batch([closed, read.map(Message::HitList)])
    }

    fn refresh_hit_list(&mut self, side: PanelSide) -> Task<Message> {
        self.panels.refresh_hit_list(side).map(Message::HitList)
    }

    fn update_hit_list(&mut self, message: HitListMessage) -> Task<Message> {
        match self.panels.update_hit_list(message, self.visible_rows) {
            None => Task::none(),
            Some(HitListAsk::BackToSearch) => {
                if self.jobs.is_busy() {
                    return Task::none();
                }
                let side = self.panels.active;
                self.leave_hit_list(side, true)
            }
            Some(HitListAsk::Leave(side)) => self.leave_hit_list(side, false),
            Some(HitListAsk::ShowOnOtherSide) => self.show_hit_on_other_side(),
        }
    }

    /// Drops the list of `side` and shows the folder it came from. With
    /// `search` the search comes back, with the fields it was left with.
    fn leave_hit_list(&mut self, side: PanelSide, search: bool) -> Task<Message> {
        let Some(return_to) = self.panels.leave_hit_list(side) else {
            return Task::none();
        };
        let load = self.load(side, return_to, None);
        if !search || self.search.view.is_none() {
            return load;
        }
        if self.search.open {
            self.panels.active = self.search.side;
            return load;
        }
        self.search.open = true;
        self.panels.active = self.search.side;
        self.refresh_search_rows();
        let restarted = self.restart_search();
        Task::batch([load, restarted, self.focus_search_field()])
    }

    /// Shows the file under the cursor in its folder on the other side. The
    /// list stays.
    fn show_hit_on_other_side(&mut self) -> Task<Message> {
        let side = self.panels.active;
        let target = match self.open_search_view() {
            Some(search) => search.cursor_target().map(|(path, _)| path),
            None => self
                .panel(side)
                .selected_entry()
                .map(|entry| entry.path.clone()),
        };
        let Some((folder, name)) = target
            .as_deref()
            .and_then(ncrs_files::hit_list_flow::folder_and_name)
        else {
            return Task::none();
        };
        let other = side.other();
        let closed = if self.search.open && self.search.side == other {
            self.close_search()
        } else {
            Task::none()
        };
        if self.ops.trash_view.is_some() && self.ops.trash_side == other {
            self.ops.close_trash();
        }
        self.panels.leave_hit_list_for_hit(other);
        let load = self.load(other, folder, Some(name));
        Task::batch([closed, load])
    }

    fn close_search(&mut self) -> Task<Message> {
        self.search.close().map(Message::Search)
    }

    /// The report page after the search handed its criteria over.
    fn hand_search_to_reports(
        &mut self,
        unsaved: Option<Draft>,
        report: Option<SavedQueryId>,
    ) -> Task<Message> {
        let closed = self.close_search();
        let opened = self.open_saved_queries();
        if let Some(view) = self.hermes.saved_queries.as_mut() {
            if let Some(draft) = unsaved {
                view.prefill(draft);
            }
            if let Some(id) = report {
                view.select_when_loaded(id);
            }
        }
        Task::batch([closed, opened])
    }

    /// The search opened on `spec`, in place of the reports page. With a
    /// `report` the search changes that one.
    fn open_search_for(&mut self, spec: &QuerySpec, report: Option<SavedQueryId>) -> Task<Message> {
        self.hermes.saved_queries = None;
        let opened = self.open_search();
        self.search.start_from(spec, report);
        self.refresh_search_rows();
        Task::batch([opened, self.restart_search()])
    }

    fn update_panel(&mut self, message: PanelMessage) -> Task<Message> {
        let overlay_is_open = self.overlay_is_open();
        let context_menu_is_open = self.panels.context_menu.is_some();
        let pointer = self.input.lock().pointer;
        let ctx = ncrs_files::Ctx {
            visible_rows: self.visible_rows,
            window_size: self.window_size,
            modifiers: self.modifiers,
            is_context_click: ncrs_files::ctx::is_context_click(self.modifiers),
            pointer,
            overlay_is_open,
            context_menu_is_open,
            clock: &*self.clock,
            nearest_existing_directory: session_state::nearest_existing_directory,
        };
        let (task, actions) = ncrs_files::update(&mut self.panels, message, &ctx);
        let mapped = task.map(Message::Panel);
        let acted = self.run_panel_actions(actions);
        Task::batch([mapped, acted])
    }

    fn run_panel_actions(&mut self, actions: Vec<ncrs_files::Action>) -> Task<Message> {
        let acted: Vec<_> = actions
            .into_iter()
            .map(|action| self.run_panel_action(action))
            .collect();
        Task::batch(acted)
    }

    /// What the drive menu lists, read when it opens.
    fn volume_facts_source(&self) -> impl FnOnce() -> ncrs_files::VolumeFacts {
        let list_volumes = Arc::clone(&self.list_volumes);
        let remote = Arc::clone(&self.remote);
        let trash = Arc::clone(&self.trash);
        let saved_servers: Vec<ServerRow> = self
            .saved_servers
            .iter()
            .map(|entry| ServerRow {
                label: entry.label(),
                url: entry.url(),
                server: entry.server_id(),
                connected: false,
            })
            .collect();
        move || ncrs_files::VolumeFacts {
            volumes: list_volumes(),
            saved_servers,
            connected: remote.connected(),
            trash: ncrs_ops::trash_menu_row(trash.reach()),
        }
    }

    fn saved_server_ids(&self) -> Vec<fs::remote::ServerId> {
        self.saved_servers
            .iter()
            .map(ServerEntry::server_id)
            .collect()
    }

    /// What the panels asked for because it reaches outside the panels.
    fn run_panel_action(&mut self, action: ncrs_files::Action) -> Task<Message> {
        use ncrs_files::Action;
        match action {
            Action::Load { side, path, select } => self.load(side, path, select),
            Action::MarkHistoryStep(side) => {
                self.panel_mut(side).mark_history_step();
                Task::none()
            }
            Action::ClearMessages => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Action::OpenContextMenu {
                side,
                index,
                pointer,
            } => {
                self.open_context_menu(side, index, Some(pointer));
                Task::none()
            }
            Action::ContextMenuAt { side, index } => {
                self.update(Message::ContextMenuAt { side, index })
            }
            Action::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Action::RunContextEntry(entry) => self.update(entry.message()),
            Action::RunHermesEntry(entry) => self.update(entry.message()),
            Action::ActivateSearchResult => self.update(Message::Search(SearchMessage::Activate)),
            Action::CloseMenuOfReloaded(side) => {
                self.panels.close_menu_of_reloaded(side);
                Task::none()
            }
            Action::DropViewCopies(place) => {
                self.drop_view_copies_of(&place);
                Task::none()
            }
            Action::StopFolderSizeRunIfLeft(side) => {
                self.panels.stop_folder_size_run_if_left(side);
                Task::none()
            }
            Action::DragArmed(armed) => {
                self.input.lock().panel_drag_armed = armed;
                Task::none()
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Action::StartDragOut => self.start_drag_out(),
            Action::OpenTransferDialog(kind) => self.open_transfer_dialog_to(kind, None),
            Action::OpenSelected => self.update(Message::Panel(PanelMessage::OpenSelected)),
            Action::CloseSettings => self.close_settings(),
            Action::ReleaseFocus => release_focus(),
            Action::OpenConnectDialog(url) => {
                let outcome = self.with_connect_feature(|remote, ctx| {
                    ncrs_remote::open_connect_dialog_for(remote, ctx, &url)
                });
                self.run_remote_outcome(outcome)
            }
            Action::ShowNotice(text) => self.show_notice(text),
            Action::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Action::OpenTrash(side) => self.open_trash_on(side),
            Action::Disconnect(server) => {
                Task::done(Message::Remote(RemoteMessage::Disconnect(server)))
            }
        }
    }

    fn update_search(&mut self, message: SearchMessage) -> Task<Message> {
        if !self.search.open {
            return Task::none();
        }
        let ctx = self.search_ctx();
        // Using the view makes its panel the active one, unless the keys it
        // asks for are not its own.
        if ncrs_search::activates_panel(&message)
            && !(ncrs_search::needs_keys(&message) && !ctx.has_keys)
        {
            self.panels.active = self.search.side;
        }
        let (task, asked) = ncrs_search::update(&mut self.search, message, &ctx);
        let mapped = task.map(Message::Search);
        match asked {
            Some(action) => Task::batch([mapped, self.run_search_action(action)]),
            None => mapped,
        }
    }

    /// What the search asked for because it reaches outside the search.
    fn run_search_action(&mut self, action: ncrs_search::Action) -> Task<Message> {
        match action {
            ncrs_search::Action::Close => self.close_search(),
            ncrs_search::Action::OpenHitList(side) => self.open_hit_list(side),
            ncrs_search::Action::ActivateCursor => self.activate_search_cursor(),
            ncrs_search::Action::OpenContextMenu => {
                self.open_search_context_menu();
                Task::none()
            }
            ncrs_search::Action::HandToReports { unsaved, report } => {
                let draft = unsaved.map(|spec| Draft {
                    name: String::new(),
                    spec,
                });
                self.hand_search_to_reports(draft, report)
            }
        }
    }

    /// Enter on a result: the file is shown in the other panel.
    fn activate_search_cursor(&mut self) -> Task<Message> {
        let Some((directory, name)) = self.search.cursor_location(self.search_has_keys()) else {
            return Task::none();
        };
        self.load(self.search.side.other(), directory, Some(name))
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
        self.panels.context_menu = None;
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
        let documents_enabled = self.document_finder().is_some();
        self.quick_open = Some(QuickOpen::new(root, generation).with_documents(documents_enabled));
        Task::batch([focus, task])
    }

    fn reprocess_folder(&mut self, folder: PathBuf) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::reprocess_folder(hermes, folder, ctx)
        })
    }

    fn documents_refiled(&mut self, result: Result<ReplanSummary, String>) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::documents_refiled(hermes, result, ctx)
        })
    }

    /// The tags page, from the settings or the command list: in place of the
    /// page of the Ablage tab that is up, if any.
    fn show_tags_page(&mut self) -> Task<Message> {
        let left = self.leave_settings_page();
        let opened = if self.open_hermes_page().is_some() {
            self.show_hermes_page(HermesPage::Tags)
        } else {
            self.open_tags()
        };
        Task::batch([left, opened])
    }

    fn hermes_view_ctx(&self) -> ncrs_hermes_ui::ViewCtx<'_> {
        ncrs_hermes_ui::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            home: &self.home,
            window_size: self.window_size,
            hermes_focus: self.hermes_focus,
            file_list_tags: &self.file_list_tags,
            jobs_busy: self.jobs.is_busy(),
            ai_download: self
                .settings
                .ai_model
                .as_ref()
                .filter(|card| card.is_downloading())
                .and_then(AiModelCard::progress),
            system_reads_scans: crate::ocr_models::system_reads_scans(),
        }
    }

    fn overview_facts(&self, ocr_models_present: bool) -> OverviewFacts {
        ncrs_hermes_ui::overview_facts(&self.hermes, &self.hermes_view_ctx(), ocr_models_present)
    }

    /// The overview or the status below the title bar, with the status bar of
    /// filing under it.
    fn ablage_area(&self) -> Option<Container<'_, Message>> {
        let page = ncrs_hermes_ui::ablage_area(&self.hermes, &self.hermes_view_ctx())?;
        Some(
            container(page.map(Message::from))
                .width(Length::Fill)
                .height(Length::Fill),
        )
    }

    fn hermes_status(&self) -> Option<HermesStatus> {
        ncrs_hermes_ui::hermes_status(&self.hermes)
    }

    fn hermes_chip(&self) -> Option<String> {
        ncrs_hermes_ui::hermes_chip(&self.hermes, self.lang)
    }

    fn filing_badge(&self) -> Option<statusbar::FilingBadge<Message>> {
        ncrs_hermes_ui::filing_badge(&self.hermes, self.lang).map(|badge| statusbar::FilingBadge {
            text: badge.text,
            failing: badge.failing,
            on_press: Message::FilingStatus(FilingStatusMessage::Open),
        })
    }

    fn hermes_line(&self) -> Option<String> {
        let cursor = self.active_panel().selected_entry()?;
        ncrs_hermes_ui::hermes_line(&self.hermes, Some(&cursor.path), self.lang)
    }

    fn open_hermes_overview(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        let left_settings = self.leave_settings_page();
        if self.settings.page.is_some() {
            return left_settings;
        }
        let opened = self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::open_overview);
        Task::batch([left_settings, opened])
    }

    fn leave_hermes_overview(&mut self) -> Task<Message> {
        ncrs_hermes_ui::leave_overview(&mut self.hermes).map(Message::from)
    }

    fn update_hermes_overview(&mut self, message: HermesOverviewMessage) -> Task<Message> {
        let before = match &message {
            HermesOverviewMessage::EnterRecoveryKey => {
                self.update_filing(FilingMessage::AskKeyAgain)
            }
            HermesOverviewMessage::ChangeFolders => {
                self.update_filing_setup(FilingSetupMessage::Toggle)
            }
            HermesOverviewMessage::SwitchHermesOn => {
                self.update_settings(SettingsMessage::Change(SettingChange::HermesEnabled(true)))
            }
            HermesOverviewMessage::PatternWritten(Ok(config)) => {
                self.hermes.filing_config.default_pattern = config.filing.default_pattern.clone();
                Task::none()
            }
            _ => Task::none(),
        };
        let write_failure = match &message {
            HermesOverviewMessage::PatternWritten(Err(error)) => Some(
                ncrs_settings::write::write_failure(error, &self.settings_ctx()),
            ),
            _ => None,
        };
        let after = self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_hermes_overview(hermes, message, ctx, write_failure)
        });
        Task::batch([before, after])
    }

    /// The Hermes card that is up, if any.
    fn open_hermes_page(&self) -> Option<HermesPage> {
        self.hermes
            .filing_status
            .page_open()
            .then_some(HermesPage::Inspection)
            .or_else(|| self.hermes.open_page())
    }

    /// A click in the sidebar: the card that is up closes without its Escape
    /// steps, the chosen page opens. Nothing happens when it is the open one,
    /// or when no Hermes card is up.
    fn go_to_hermes_page(&mut self, page: HermesPage) -> Task<Message> {
        if page != HermesPage::Inspection {
            return self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::go_to_hermes_page(hermes, page, ctx)
            });
        }
        match self.open_hermes_page() {
            None => return Task::none(),
            Some(open) if open == page => return Task::none(),
            Some(_) => {}
        }
        self.hermes.close_pages();
        self.hermes.filing_status.close_page();
        let open = self.open_filing_status();
        Task::batch([release_focus(), open])
    }

    fn settings_view<'a>(
        &'a self,
        state: &'a settings_ui::SettingsState,
    ) -> settings_ui::SettingsView<'a> {
        settings_ui::SettingsView {
            state,
            preset: self.theme_preset,
            vscode_theme: self.vscode_theme.as_deref(),
            filing: &self.hermes.filing_config,
            servers: &self.saved_servers,
            home: &self.home,
            keys_changed: KeyAction::ALL
                .iter()
                .filter(|&&action| !self.keymap.has_built_in_keys(action))
                .count(),
            protection: Some(self.protection_panel(&self.overview_facts(state.ocr_models_present))),
            lang: self.lang,
            palette: &self.palette,
        }
    }

    /// The protection of the index as the settings page shows it.
    fn protection_panel(&self, facts: &OverviewFacts) -> settings_ui::ProtectionPanel {
        let text = hermes_overview_ui::protection_text(facts, self.lang);
        settings_ui::ProtectionPanel {
            state: text.state.unwrap_or_default().to_owned(),
            good: text.good,
            sentence: text.sentence.to_owned(),
            failure: text.failure,
            buttons: hermes_overview_ui::protection_actions(facts)
                .into_iter()
                .map(|action| settings_ui::ProtectionButton {
                    label: action.label,
                    enabled: action.enabled,
                    primary: action.primary,
                })
                .collect(),
        }
    }

    fn settings_ctx(&self) -> ncrs_settings::Ctx<'_> {
        ncrs_settings::Ctx {
            lang: self.lang,
            keymap: &self.keymap,
            config_path: self.config_path.as_deref(),
            home: &self.home,
            sandbox_active: self.sandbox.active,
            blocking_overlay_open: self.blocking_overlay_is_open(),
            overlay_apart_from_overview_open: self.overlay_apart_from_overview_is_open(),
            overview_open: self.hermes.overview.is_some(),
            ocr_models_dir: self
                .hermes
                .index_path
                .as_deref()
                .and_then(crate::filing::ocr_models_dir),
            ai_models_dir: self
                .hermes
                .index_path
                .as_deref()
                .and_then(crate::filing::ai_models_dir),
            file_list_tags: &self.file_list_tags,
            ai_is_on: self.hermes.filing_config.ai != crate::config::AiSetting::Off,
            services: ncrs_settings::Services {
                fetch_extension: self.fetch_extension,
                install_theme: self.install_theme,
                download_ocr_models: self.download_ocr_models,
                download_ai_model: self.download_ai_model,
            },
        }
    }

    /// Runs `step` on the settings. The state is taken out for it, since the
    /// context borrows the rest of the app; the overlay facts are read before,
    /// while the settings' own dialogs still count.
    fn with_settings<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_settings::Settings, &ncrs_settings::Ctx<'_>) -> R,
    ) -> R {
        let blocking_overlay_open = self.blocking_overlay_is_open();
        let overlay_apart_from_overview_open = self.overlay_apart_from_overview_is_open();
        let mut settings = std::mem::take(&mut self.settings);
        let outcome = step(
            &mut settings,
            &ncrs_settings::Ctx {
                blocking_overlay_open,
                overlay_apart_from_overview_open,
                ..self.settings_ctx()
            },
        );
        self.settings = settings;
        outcome
    }

    fn run_settings(&mut self, message: ncrs_settings::Message) -> Task<Message> {
        let (task, actions) =
            self.with_settings(|settings, ctx| ncrs_settings::update(settings, message, ctx));
        self.run_settings_actions(task, actions)
    }

    fn run_settings_actions(
        &mut self,
        task: Task<ncrs_settings::Message>,
        actions: Vec<ncrs_settings::Action>,
    ) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from_settings)];
        for action in actions {
            tasks.push(self.run_settings_action(action));
        }
        Task::batch(tasks)
    }

    fn run_settings_action(&mut self, action: ncrs_settings::Action) -> Task<Message> {
        use ncrs_settings::Action as Ask;
        match action {
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::SetDialogFocus(focus) => {
                self.dialog_focus = focus;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::LeaveAblagePage => {
                let left_ablage = self.close_ablage_tab();
                self.panels.context_menu = None;
                self.panels.set_path_field(None);
                self.notice = None;
                left_ablage
            }
            Ask::PickProgram { kind, apps_only } => (self.pick_file)(FilePick::Program {
                apps_only,
            })
            .map(move |path| Message::Settings(SettingsMessage::ProgramPicked { kind, path })),
            Ask::PickThemeFile => (self.pick_file)(FilePick::ThemeFile)
                .map(|path| Message::Settings(SettingsMessage::ThemeFilePicked(path))),
            Ask::ShowTagsPage => self.show_tags_page(),
            Ask::OpenServerAdd => self.update(Message::Remote(RemoteMessage::OpenAdd)),
            Ask::OpenServer(url) => self.update(Message::Remote(RemoteMessage::OpenServer(url))),
            Ask::OpenFilingStatus => self.update(Message::FilingStatus(FilingStatusMessage::Open)),
            Ask::Protection(position) => {
                let facts = self.overview_facts(
                    self.settings
                        .page
                        .as_ref()
                        .is_some_and(|state| state.ocr_models_present),
                );
                match hermes_overview_ui::protection_actions(&facts)
                    .into_iter()
                    .nth(position)
                {
                    Some(button) => self.update(Message::HermesOverview(button.message)),
                    None => Task::none(),
                }
            }
            Ask::Apply { change, config } => self.apply_setting(change, *config),
            Ask::ApplyLook(config) => {
                self.apply_look(*config);
                Task::none()
            }
            Ask::KeymapChanged(keymap) => {
                self.keymap = keymap;
                self.refresh_function_keys();
                Task::none()
            }
            Ask::OpenOverview => self.open_hermes_overview(),
            Ask::OcrModelsInstalled => {
                if let Some(overview) = self.hermes.overview.as_mut() {
                    overview.set_ocr_models_present(true);
                }
                Task::none()
            }
            Ask::RefreshAiModel => {
                if let Some(module) = self.hermes.filing.as_ref() {
                    module.refresh_ai_model();
                }
                Task::none()
            }
        }
    }

    fn hermes_ctx(
        &self,
        hermes: &ncrs_hermes_ui::Hermes,
        inquiry_paths: Vec<PathBuf>,
    ) -> ncrs_hermes_ui::Ctx<'_> {
        let side = self.panels.active;
        let panel = self.panel(side);
        ncrs_hermes_ui::Ctx {
            lang: self.lang,
            kinds: &self.kinds,
            status_page_open: hermes.filing_status.page_open(),
            overlay_apart_from_ablage_tab_open: self
                .overlay_apart_from_overview_and_hermes_is_open(),
            start_over_dialog_open: self.start_over_dialog,
            hermes_is_restarting: hermes.is_restarting(),
            hermes_has_open_reviews: hermes
                .filing_status
                .hermes_counts()
                .is_some_and(|counts| counts.open_reviews > 0),
            file_list_tags: &self.file_list_tags,
            dialog_focus: self.dialog_focus,
            filing_status_shortcut: self
                .keymap
                .shortcut_label(&Message::FilingStatus(FilingStatusMessage::Toggle)),
            model_is_installed,
            active_panel_path: &panel.path,
            other_panel_path: &self.panel(side.other()).path,
            context_menu_open: self.panels.context_menu.is_some(),
            search_open: self.search.open,
            hermes_active: hermes.is_active(),
            hermes_counts_missing: hermes.filing_status.hermes_counts().is_none(),
            jobs_busy: self.jobs.is_busy(),
            window_size: self.window_size,
            config_path: self.config_path.as_deref(),
            home: &self.home,
            resolve_folder: self.resolve_folder,
            quick_fix_anchor: layout::cursor_row_anchor(
                side == PanelSide::Right,
                panel.selected.saturating_sub(panel.scroll_offset),
                panel.row_height(),
                self.window_size,
            ),
            inquiry_paths,
            now: self.wall_clock.now(),
            panels: [
                self.panel_facts(PanelSide::Left),
                self.panel_facts(PanelSide::Right),
            ],
            cursor_entry: Some(self.active_panel())
                .filter(|active| !active.is_virtual())
                .and_then(PanelState::selected_entry)
                .map(|entry| entry.path.as_path()),
        }
    }

    fn panel_facts(&self, side: PanelSide) -> ncrs_hermes_ui::ctx::PanelFacts<'_> {
        let panel = self.panel(side);
        ncrs_hermes_ui::ctx::PanelFacts {
            side,
            directory: &panel.path,
            request_id: panel.request_id,
            readable: !panel.loading && !panel.is_virtual(),
            visible: panel
                .entries
                .iter()
                .skip(panel.scroll_offset)
                .take(panel.rows_in_view(self.visible_rows))
                .map(|entry| ncrs_hermes_ui::ctx::VisibleEntry {
                    path: &entry.path,
                    is_dir: entry.is_dir,
                    is_parent: entry.is_parent,
                })
                .collect(),
        }
    }

    /// Runs `step` on the Hermes pages. The state is taken out for it, since
    /// the context borrows the rest of the app. The context reads nothing of
    /// the pages themselves: the update looks at those as they are while it
    /// runs.
    fn with_hermes<R>(
        &mut self,
        inquiry_paths: Vec<PathBuf>,
        step: impl FnOnce(&mut ncrs_hermes_ui::Hermes, &ncrs_hermes_ui::Ctx<'_>) -> R,
    ) -> R {
        let mut hermes = std::mem::take(&mut self.hermes);
        let ctx = self.hermes_ctx(&hermes, inquiry_paths);
        let outcome = step(&mut hermes, &ctx);
        self.hermes = hermes;
        outcome
    }

    /// Runs a flow of the Hermes pages and then the actions it asked for.
    fn run_hermes_flow(
        &mut self,
        inquiry_paths: Vec<PathBuf>,
        step: impl FnOnce(
            &mut ncrs_hermes_ui::Hermes,
            &ncrs_hermes_ui::Ctx<'_>,
        ) -> ncrs_hermes_ui::Outcome,
    ) -> Task<Message> {
        let outcome = self.with_hermes(inquiry_paths, step);
        self.run_hermes_outcome(outcome)
    }

    /// Puts the job into the queue and starts it if nothing runs.
    fn start_job<M>(
        &mut self,
        job: jobs::Job,
        build: impl FnOnce() -> Task<M> + 'static,
    ) -> Task<Message>
    where
        Message: From<M>,
        M: Send + 'static,
    {
        self.jobs.enqueue(job);
        self.jobs
            .start_next(move |_| build().map(Message::from))
            .unwrap_or_else(Task::none)
    }

    /// As `start_job`, for work that sends its progress through the channel,
    /// which is made first.
    fn start_job_with_progress<M>(
        &mut self,
        job: jobs::Job,
        build: impl FnOnce(mpsc::UnboundedSender<fs::transfer::Tick>) -> Task<M> + 'static,
    ) -> Task<Message>
    where
        Message: From<M>,
        M: Send + 'static,
    {
        let progress_tx = self.progress_sender();
        self.jobs.enqueue(job);
        self.jobs
            .start_next(move |_| build(progress_tx).map(Message::from))
            .unwrap_or_else(Task::none)
    }

    fn run_hermes_outcome(&mut self, (task, actions): ncrs_hermes_ui::Outcome) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_hermes_action(action));
        }
        Task::batch(tasks)
    }

    fn run_hermes_action(&mut self, action: ncrs_hermes_ui::Action) -> Task<Message> {
        use ncrs_hermes_ui::Action as Ask;
        match action {
            Ask::OpenHermesOverview => self.open_hermes_overview(),
            Ask::OpenFilingStatus => self.open_filing_status(),
            Ask::RevealFile(path) => self.reveal_file(&path),
            Ask::FilingRestarted(generation) => self.filing_restarted(generation),
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::ClearStatus => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::ShowError(detail) => {
                self.set_status_error(&detail);
                Task::none()
            }
            Ask::RequestHermesCounts => self
                .with_hermes(Vec::new(), |hermes, _| {
                    ncrs_hermes_ui::request_hermes_counts(hermes)
                })
                .map(Message::from),
            Ask::ReadPathsAgain(paths) => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::read_paths_again(hermes, paths, ctx)
            }),
            Ask::CloseStatusPage => {
                self.hermes.filing_status.close_page();
                Task::none()
            }
            Ask::GoToHermesPage(page) => self.go_to_hermes_page(page),
            Ask::SetHermesFocus(focus) => {
                self.hermes_focus = focus;
                Task::none()
            }
            Ask::RefreshFilingSnapshot => {
                self.hermes.refresh_filing_snapshot(self.wall_clock.now());
                Task::none()
            }
            Ask::ReadOcrModelsPresent => {
                let present = self.ocr_models_present();
                if let Some(overview) = self.hermes.overview.as_mut() {
                    overview.set_ocr_models_present(present);
                }
                Task::none()
            }
            Ask::ChangeProtection(begin) => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::change_protection(hermes, begin, ctx)
            }),
            Ask::OpenStartOverDialog(focus) => {
                self.dialog_focus = focus;
                self.start_over_dialog = true;
                Task::none()
            }
            Ask::CloseStartOverDialog => {
                self.start_over_dialog = false;
                Task::none()
            }
            Ask::SetAiEnabled(on) => {
                self.update_settings(SettingsMessage::Change(SettingChange::AiEnabled(on)))
            }
            Ask::SetLoad(load) => {
                self.update_settings(SettingsMessage::Change(SettingChange::Load(load)))
            }
            Ask::OpenOcrModels => self.update_ocr_models(OcrModelsMessage::Open),
            Ask::OpenAndDownloadAiModel => {
                let open = self.update_ai_model(AiModelMessage::Open);
                let download = self.update_ai_model(AiModelMessage::Download);
                Task::batch([open, download])
            }
            Ask::ShowHermesSetting(row) => {
                self.show_setting(crate::messages::SettingsFeature::Hermes, Some(row))
            }
            Ask::OpenHermesSetup => {
                self.open_hermes_setup();
                Task::none()
            }
            Ask::RestartHermesModule => {
                self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::restart_hermes_module)
            }
            Ask::SetTagExceptions(exceptions) => {
                self.tag_exceptions = exceptions;
                Task::none()
            }
            Ask::ChangeFileListTags(tags) => {
                self.update_settings(SettingsMessage::Change(SettingChange::FileListTags(tags)))
            }
            Ask::OpenSearchFor { spec, report } => self.open_search_for(&spec, report),
            Ask::PickExportFolder(start_in) => (self.pick_folder)(&start_in)
                .map(|folder| Message::SavedQueries(SavedQueriesMessage::FolderPicked(folder))),
            Ask::PickFolder(card) => {
                let start_in = self.active_panel().path.clone();
                (self.pick_folder)(&start_in).map(move |folder| match card {
                    SetupCard::Filing(mode) => {
                        Message::FilingSetup(FilingSetupMessage::FolderPicked { mode, folder })
                    }
                    SetupCard::Hermes(slot) => {
                        Message::HermesSetup(HermesSetupMessage::FolderPicked { slot, folder })
                    }
                })
            }
            Ask::GrantFolder { card, folder } => {
                self.grant_chosen_folder(&folder, move |granted_folder, result| match card {
                    SetupCard::Filing(_) => Message::FilingSetup(FilingSetupMessage::Granted {
                        folder: granted_folder,
                        result,
                    }),
                    SetupCard::Hermes(_) => Message::HermesSetup(HermesSetupMessage::Granted {
                        folder: granted_folder,
                        result,
                    }),
                })
            }
            Ask::StartJobWithProgress(start) => {
                self.start_job_with_progress(start.job, start.build)
            }
            Ask::FinishJob => {
                self.jobs.finish();
                Task::none()
            }
            Ask::SetJobProgress(tick) => {
                self.job_progress = Some(tick);
                Task::none()
            }
            Ask::ClearJobProgress => {
                self.job_progress = None;
                Task::none()
            }
            Ask::ReloadPanels => self.reload_panels(),
            Ask::SelectErrorOf(path) => {
                self.hermes.filing_status.select_error_of(&path);
                Task::none()
            }
            Ask::SetDialogFocus(focus) => {
                self.dialog_focus = focus;
                Task::none()
            }
            Ask::OpenAiModelCard => {
                let opened = self.update_ai_model(AiModelMessage::Open);
                self.hermes.ai_model_asked = self.settings.ai_model.is_some();
                opened
            }
        }
    }

    fn update_review_list(&mut self, message: ReviewListMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_review_list(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn update_quick_fix(&mut self, message: QuickFixMessage) -> Task<Message> {
        let inquiry_paths = if matches!(message, QuickFixMessage::Open) {
            self.inquiry_paths()
        } else {
            Vec::new()
        };
        let outcome = self.with_hermes(inquiry_paths, |hermes, ctx| {
            ncrs_hermes_ui::update_quick_fix(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn update_reorganisation(&mut self, message: ReorganisationPlanMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_reorganisation(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn update_rule_suggestions(&mut self, message: RuleSuggestionsMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_rule_suggestions(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn update_rules(&mut self, message: RulesMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_rules(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn update_saved_queries(&mut self, message: SavedQueriesMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_saved_queries(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn update_tags(&mut self, message: TagsMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_tags(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    fn open_saved_queries(&mut self) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), ncrs_hermes_ui::open_saved_queries);
        self.run_hermes_outcome(outcome)
    }

    fn open_tags(&mut self) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), ncrs_hermes_ui::open_tags);
        self.run_hermes_outcome(outcome)
    }

    fn request_review_preview(&mut self) -> Task<Message> {
        ncrs_hermes_ui::request_review_preview(&mut self.hermes).map(Message::from)
    }

    fn update_settings(&mut self, message: SettingsMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::Settings(message))
    }

    fn update_key_bindings(&mut self, message: KeyBindingsMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::KeyBindings(message))
    }

    fn update_theme_fetch(&mut self, message: ThemeFetchMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::ThemeFetch(message))
    }

    fn update_ocr_models(&mut self, message: OcrModelsMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::OcrModels(message))
    }

    fn update_ai_model(&mut self, message: AiModelMessage) -> Task<Message> {
        self.run_settings(ncrs_settings::Message::AiModel(message))
    }

    fn press_ai_model_button(&mut self) -> Task<Message> {
        let (task, actions) = self.with_settings(ncrs_settings::press_ai_model_button);
        self.run_settings_actions(task, actions)
    }

    fn ocr_models_present(&self) -> bool {
        ncrs_settings::ocr_models_present(self.settings_ctx().ocr_models_dir.as_deref())
    }

    fn close_settings(&mut self) -> Task<Message> {
        ncrs_settings::close_settings(&mut self.settings).map(Message::from_settings)
    }

    /// Writes the values typed on the page and closes it.
    fn leave_settings_page(&mut self) -> Task<Message> {
        self.update_settings(SettingsMessage::Close)
    }

    fn settings_area<'a>(
        &'a self,
        state: &'a settings_ui::SettingsState,
    ) -> Container<'a, Message> {
        let page = settings_ui::view(&self.settings_view(state)).map(Message::Settings);
        let bar = settings_ui::status_bar(&settings_ui::StatusBar {
            changed: state.changed,
            failure: self.settings.failure.as_deref(),
            notice: self.notice.as_deref(),
            lang: self.lang,
            palette: &self.palette,
        })
        .map(Message::Settings);
        container(column![page, bar])
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// Takes over what the file now says about `change`.
    fn apply_setting(&mut self, change: SettingChange, config: Config) -> Task<Message> {
        match change {
            SettingChange::Preset(_)
            | SettingChange::VscodeTheme(_)
            | SettingChange::Color { .. } => {
                self.apply_look(config);
                Task::none()
            }
            SettingChange::Program { .. } => {
                self.open_programs = config.open;
                Task::none()
            }
            SettingChange::Language(lang) => {
                self.set_language(lang);
                Task::none()
            }
            SettingChange::Load(load) => {
                self.hermes.filing_config.load = load;
                self.hermes.set_pipeline_load(load, self.wall_clock.now());
                Task::none()
            }
            SettingChange::Servers(_) => self.take_over_servers(config.servers),
            SettingChange::FileListTags(_) => {
                self.file_list_tags = config.file_list_tags;
                Task::none()
            }
            SettingChange::AiEnabled(_) | SettingChange::AiProvider(_) => {
                let (applies, task) = self.apply_ai_change(config.filing);
                let outcome = if matches!(applies, SavedFilingApplies::Now) {
                    InstallOutcome::TurnedOn
                } else {
                    InstallOutcome::TurnedOnAtNextStart
                };
                if let Some(card) = self.settings.ai_model.as_mut() {
                    card.set_outcome(outcome);
                }
                if matches!(applies, SavedFilingApplies::AtNextStart) {
                    let notice = self.lang.text(Msg::SettingsSavedNextStart).to_owned();
                    Task::batch([task, self.show_notice(notice)])
                } else {
                    task
                }
            }
            SettingChange::OwnDetails { .. } => {
                let (applies, task) = self.apply_ai_change(config.filing);
                if matches!(applies, SavedFilingApplies::AtNextStart) {
                    let notice = self.lang.text(Msg::SettingsSavedNextStart).to_owned();
                    Task::batch([task, self.show_notice(notice)])
                } else {
                    task
                }
            }
            SettingChange::HermesEnabled(_) => {
                let (applies, task) = self.apply_saved_filing(config.filing);
                if matches!(applies, SavedFilingApplies::AtNextStart) {
                    let notice = self.lang.text(Msg::SettingsSavedNextStart).to_owned();
                    Task::batch([task, self.show_notice(notice)])
                } else {
                    task
                }
            }
        }
    }

    /// The list of saved servers is what the file now says. A server that left
    /// it takes its password in the keychain along: nothing is kept for a
    /// server the user removed.
    fn take_over_servers(&mut self, servers: Vec<ServerEntry>) -> Task<Message> {
        let removed: Vec<ServerId> = self
            .saved_servers
            .iter()
            .map(ServerEntry::server_id)
            .filter(|gone| !servers.iter().any(|kept| kept.server_id() == *gone))
            .collect();
        self.saved_servers = servers;
        let forgets = removed.into_iter().map(|server| {
            let passwords = Arc::clone(&self.passwords);
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || passwords.forget(&server))
                        .await
                        .unwrap_or_else(|failure| Err(failure.to_string()))
                },
                |result| Message::Remote(RemoteMessage::PasswordStored(result)),
            )
        });
        Task::batch(forgets)
    }

    /// Takes over the look `config` describes; the warnings about the look
    /// before it no longer apply.
    fn apply_look(&mut self, config: Config) {
        self.theme_preset = config.theme_preset();
        self.vscode_theme = config.vscode_theme_path().map(str::to_owned);
        self.palette = config.palette;
        self.kinds = Kinds::new(config.kinds, self.lang.kind_language());
        let catalog = Arc::clone(&self.kinds.catalog);
        if let Some(module) = &mut self.hermes.filing {
            module.set_kinds(catalog);
        }
        if matches!(
            self.config_problem,
            Some(
                ConfigProblem::LowContrast(_)
                    | ConfigProblem::VscodeTheme(_)
                    | ConfigProblem::UnknownLanguage(_)
                    | ConfigProblem::Kinds(_)
            )
        ) {
            self.config_problem = None;
            self.config_error = None;
        }
        self.report_look_warnings(config.contrast_warning, config.vscode_warning);
        self.report_language_warning(config.language_warning);
        self.report_kind_warning(config.kind_warning);
    }

    fn update_about(&mut self, message: AboutMessage) -> Task<Message> {
        match message {
            AboutMessage::Open => {
                if self.blocking_overlay_is_open() {
                    return Task::none();
                }
                self.panels.context_menu = None;
                self.dialog_focus = about_ui::CLOSE_FOCUS;
                self.about = Some(AboutCard::new());
            }
            AboutMessage::Close => self.about = None,
            AboutMessage::OpenLicenses => match (self.find_third_party_licenses)() {
                Some(path) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.launch_from_about(path.clone().into_os_string(), &name);
                }
                None => {
                    if let Some(card) = self.about.as_mut() {
                        card.report_licenses_missing();
                    }
                }
            },
            AboutMessage::OpenRepository => {
                self.launch_from_about(REPOSITORY_URL.into(), REPOSITORY_URL);
            }
        }
        Task::none()
    }

    /// A launch that fails closes the card, so the error in the status bar is
    /// not hidden behind it.
    fn launch_from_about(&mut self, target: std::ffi::OsString, name: &str) {
        let command = fs::system_open_command(target);
        if let Err(reason) = self.launcher.launch(&command) {
            self.about = None;
            let failure = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", name)
                .replace("{reason}", &reason);
            self.set_status_error(&failure);
        }
    }

    fn update_grant(&mut self, message: GrantMessage) -> Task<Message> {
        match message {
            GrantMessage::Open => {
                if !self.sandbox.active {
                    let text = self.lang.text(Msg::GrantNotNeeded).to_owned();
                    return self.show_notice(text);
                }
                if self.blocking_overlay_is_open() {
                    return Task::none();
                }
                self.panels.context_menu = None;
                self.pick_grant_folder()
            }
            GrantMessage::OpenFor(blocked) => {
                if self.blocking_overlay_is_open() {
                    return Task::none();
                }
                self.panels.context_menu = None;
                (self.pick_folder)(&fs::grant_start_dir_for(&blocked))
                    .map(|folder| Message::Grant(GrantMessage::Picked(folder)))
            }
            GrantMessage::Choose => self.pick_grant_folder(),
            GrantMessage::Later => {
                self.grant_dialog = false;
                Task::none()
            }
            GrantMessage::Picked(None) => Task::none(),
            GrantMessage::Picked(Some(folder)) => self.grant_picked_folder(folder),
            GrantMessage::Saved { folder, result } => self.grant_saved(&folder, result),
        }
    }

    /// Opens the system folder dialog in the real home, where the user is most
    /// likely to look. Called from `update`, like every use of `pick_folder`.
    fn pick_grant_folder(&mut self) -> Task<Message> {
        (self.pick_folder)(&fs::grant_start_dir())
            .map(|folder| Message::Grant(GrantMessage::Picked(folder)))
    }

    /// Keeps the access the user gave by choosing a folder in a setup card
    /// for the next start, like the grant dialog does, but without its dialog.
    /// Nothing when the app is not sandboxed. `message` gets the folder and
    /// whether saving worked.
    fn grant_chosen_folder(
        &self,
        folder: &Path,
        message: impl Fn(PathBuf, Result<(), String>) -> Message + Send + 'static,
    ) -> Task<Message> {
        let Some(dir) = self.sandbox.grants_dir.clone() else {
            return Task::none();
        };
        let grant = self.sandbox.grant;
        let saved = folder.to_path_buf();
        let chosen = saved.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || grant(&dir, &saved))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            move |result| message(chosen.clone(), result),
        )
    }

    /// Saves the access the user just gave, while the open panel's access is
    /// fresh, and shows both panels again: the folder may be one they could not
    /// read.
    fn grant_picked_folder(&mut self, folder: PathBuf) -> Task<Message> {
        // The dialog goes either way: behind it the status bar is dimmed, and
        // a failure is read there. "Grant folder access…" tries again.
        self.grant_dialog = false;
        let Some(dir) = self.sandbox.grants_dir.clone() else {
            return self.grant_saved(
                &folder,
                Err("there is no data folder to keep it in".to_owned()),
            );
        };
        // Right away, while the access the panel gave is fresh, but not on the
        // thread that draws: making a bookmark and writing its file can take a
        // moment on a slow volume.
        let grant = self.sandbox.grant;
        let saved = folder.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || grant(&dir, &saved))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            move |result| {
                Message::Grant(GrantMessage::Saved {
                    folder: folder.clone(),
                    result,
                })
            },
        )
    }

    /// The grant of `folder` is saved, or not: both panels load again when it
    /// is, since the folder may be one they could not read.
    fn grant_saved(&mut self, folder: &Path, saved: Result<(), String>) -> Task<Message> {
        let name = folder.to_string_lossy();
        let report = match saved {
            Ok(()) => Ok(fill_template(
                self.lang.text(Msg::GrantAdded),
                &[("{folder}", &name)],
            )),
            Err(reason) => Err(fill_template(
                self.lang.text(Msg::GrantFailed),
                &[("{folder}", &name), ("{reason}", &reason)],
            )),
        };
        match report {
            Ok(text) => {
                self.job_error = None;
                let reloads = [PanelSide::Left, PanelSide::Right].map(|each| {
                    let path = self.panel(each).path.clone();
                    self.load(each, path, None)
                });
                let notice = self.show_notice(text);
                Task::batch(reloads.into_iter().chain([notice]))
            }
            Err(text) => {
                self.set_status_error(&text);
                Task::none()
            }
        }
    }

    /// Asks for the home folder when the sandbox is on and home cannot be
    /// listed. Asked again at every start while that stays so.
    fn open_grant_dialog_if_needed(&mut self) {
        if self.sandbox.needs_home_grant() {
            self.dialog_focus = grant_ui::GRANT_FOCUS;
            self.grant_dialog = true;
        }
    }

    /// Cmd/Ctrl+Shift+P: lists the actions that work where the keys are now.
    fn open_command_palette(&mut self) -> Task<Message> {
        self.update_command_palette(CommandPaletteMessage::Toggle)
    }

    /// Which Hermes commands on a file the command list offers.
    fn command_palette_hermes(&self) -> HermesCommands {
        HermesCommands::from_facts(self.hermes.is_ready(), self.hermes.is_active(), || {
            self.hermes_action_enabled(HermesAction::ProcessAgain)
        })
    }

    /// The settings pages and settings the command list finds by a query.
    fn command_palette_settings_targets(&self) -> Vec<ncrs_config::SettingsTarget> {
        let fresh;
        let state = match &self.settings.page {
            Some(open) => open,
            None => {
                fresh = ncrs_settings::fresh_settings_state(&self.settings_ctx());
                &fresh
            }
        };
        settings_ui::search_targets(&self.settings_view(state))
    }

    fn resize_command_palette(&mut self, window: Size) {
        command_list::resize(&mut self.command_palette, window);
    }

    fn close_command_palette(&mut self) -> Task<Message> {
        command_list::close(&mut self.command_palette).map(Message::CommandPalette)
    }

    fn update_command_palette(&mut self, message: CommandPaletteMessage) -> Task<Message> {
        let opening =
            matches!(message, CommandPaletteMessage::Toggle) && self.command_palette.is_none();
        let blocked = opening && self.blocking_overlay_is_open();
        let (hermes, settings_targets) = if opening && !blocked {
            (
                self.command_palette_hermes(),
                self.command_palette_settings_targets(),
            )
        } else {
            (HermesCommands::All, Vec::new())
        };
        let scope = if self.search_has_keys() {
            KeyScope::Search
        } else {
            KeyScope::Panel
        };
        let ctx = CommandPaletteCtx {
            lang: self.lang,
            keymap: &self.keymap,
            recent: &self.recent_commands,
            window: self.window_size,
            blocked,
            scope,
            hermes,
            settings_targets,
        };
        let (inner, action) = command_list::update(&mut self.command_palette, message, &ctx);
        let task = inner.map(Message::CommandPalette);
        match action {
            None => task,
            Some(CommandPaletteAction::CloseContextMenu) => {
                self.panels.context_menu = None;
                task
            }
            // The closing task comes first and is chained, not batched: a
            // prompt the action opens takes the focus after the list gave it
            // up.
            Some(CommandPaletteAction::Run(command)) => task.chain(self.run_command(command)),
            Some(CommandPaletteAction::ShowSetting { feature, row }) => {
                Task::batch([task, self.show_setting(feature, row)])
            }
        }
    }

    /// Opens the settings tab, unless something blocks it, on `feature`, with
    /// `row` in view.
    fn show_setting(
        &mut self,
        feature: crate::messages::SettingsFeature,
        row: Option<String>,
    ) -> Task<Message> {
        let mut actions = Vec::new();
        let task = self.with_settings(|settings, ctx| {
            ncrs_settings::show_setting(settings, ctx, feature, row, &mut actions)
        });
        self.run_settings_actions(task, actions)
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
        quick_open::close(&mut self.quick_open)
    }

    fn update_quick_open(&mut self, message: QuickOpenMessage) -> Task<Message> {
        let ctx = QuickOpenCtx {
            documents_available: self.document_finder().is_some(),
        };
        let (inner, action) = quick_open::update(&mut self.quick_open, message, &ctx);
        let task = inner.map(Message::QuickOpen);
        match action {
            None => task,
            Some(QuickOpenAction::Open) => self.open_quick_open(),
            Some(QuickOpenAction::Close) => self.close_quick_open(),
            Some(QuickOpenAction::SwitchToCommands(query)) => {
                self.switch_quick_open_to_commands(query)
            }
            Some(QuickOpenAction::FindDocuments(wanted)) => {
                let documents = self.document_finder().map_or_else(Task::none, |finder| {
                    quick_open::find_documents(wanted, move |words, limit| {
                        finder.find(words, limit)
                    })
                    .map(Message::QuickOpen)
                });
                Task::batch([task, documents])
            }
            Some(QuickOpenAction::WalkFinished) => {
                self.quick_open_walk = None;
                task
            }
            // Enter on the last row.
            Some(QuickOpenAction::OpenSearch(text)) => {
                let closed = self.close_quick_open();
                Task::batch([closed, self.open_search_with_words(text)])
            }
            // Enter on a hit: the active panel shows the file's directory with
            // the cursor on the file.
            Some(QuickOpenAction::Reveal(path)) => {
                let closed = self.close_quick_open();
                let revealed = self.reveal_file(&path);
                Task::batch([closed, revealed])
            }
        }
    }

    /// A `>` typed first: the box gives way to the command list, with the rest
    /// of the text as its query.
    fn switch_quick_open_to_commands(&mut self, query: String) -> Task<Message> {
        let closed = self.close_quick_open();
        let opened = self.open_command_palette();
        if let Some(palette) = self.command_palette.as_mut() {
            palette.set_query(query);
        }
        Task::batch([closed, opened])
    }

    /// The active panel shows the file's directory with the cursor on the file.
    /// Only navigates: the file is never opened or run.
    fn reveal_file(&mut self, path: &Path) -> Task<Message> {
        let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
            return Task::none();
        };
        let directory = parent.to_path_buf();
        let name = file.to_string_lossy().into_owned();
        self.load(self.panels.active, directory, Some(name))
    }

    fn title_bar_view(&self) -> Element<'_, Message> {
        let tab = |side: PanelSide| {
            let title = match self.search.view.as_ref() {
                _ if self.panel(side).is_hit_list() => {
                    let query = self
                        .panel(side)
                        .hit_list
                        .as_ref()
                        .map_or("", |list| list.query.as_str());
                    titlebar::TabTitle::HitList(fill_template(
                        self.lang.text(Msg::HitListTab),
                        &[("{query}", query)],
                    ))
                }
                Some(search) if self.search.open && side == self.search.side => {
                    titlebar::TabTitle::Search(ncrs_search::title(search, self.lang))
                }
                _ => match self.ops.trash_view.as_ref() {
                    Some(view) if side == self.ops.trash_side => titlebar::TabTitle::Trash(
                        trash_ui::heading(view.reach, self.lang).to_owned(),
                    ),
                    _ => titlebar::TabTitle::Path {
                        path: &self.panel(side).path,
                        loading: self.panel(side).loading,
                    },
                },
            };
            titlebar::Tab {
                side,
                title,
                active: self.panels.active == side && !self.page_tab_is_open(),
            }
        };
        titlebar::view(&titlebar::TitleBar {
            tabs: [tab(PanelSide::Left), tab(PanelSide::Right)],
            page_tab: self.page_tab(),
            home: &self.home,
            hermes: self.hermes_chip(),
            hermes_tone: self
                .hermes_status()
                .map_or(titlebar::HermesTone::Fine, |status| status.tone()),
            lab_shortcut: self
                .keymap
                .shortcut_label(&KeyAction::CommandPalette.message()),
            lang: self.lang,
            palette: &self.palette,
        })
    }

    fn page_tab_is_open(&self) -> bool {
        self.settings.page.is_some() || self.ablage_tab_is_open()
    }

    fn page_tab(&self) -> Option<titlebar::PageTab> {
        if self.settings.page.is_some() {
            Some(titlebar::PageTab::Settings)
        } else {
            self.open_hermes_page()
                .map(|page| titlebar::PageTab::Filing {
                    close: Box::new(Self::close_message_of(page)),
                })
        }
    }

    /// What Escape sends on the top level of the page of filing.
    fn close_message_of(page: HermesPage) -> Message {
        match page {
            HermesPage::Overview => Message::HermesOverview(HermesOverviewMessage::Close),
            HermesPage::Inspection => Message::FilingStatus(FilingStatusMessage::Close),
            HermesPage::Review => Message::ReviewList(ReviewListMessage::Close),
            HermesPage::SavedQueries => Message::SavedQueries(SavedQueriesMessage::Close),
            HermesPage::Rules => Message::Rules(RulesMessage::Close),
            HermesPage::RuleSuggestions => Message::RuleSuggestions(RuleSuggestionsMessage::Close),
            HermesPage::Tags => Message::Tags(TagsMessage::Close),
            HermesPage::Reorganisation => {
                Message::ReorganisationPlan(ReorganisationPlanMessage::Close)
            }
        }
    }

    /// The name of the colour theme when the palette is one of the presets.
    fn theme_name(&self) -> Option<String> {
        ThemePreset::ALL
            .into_iter()
            .find(|preset| preset.palette() == self.palette)
            .map(|preset| {
                let name = preset.name();
                let mut letters = name.chars();
                letters.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(letters).collect()
                })
            })
    }

    #[cfg(test)]
    pub(crate) fn hermes_chip_for_test(&self) -> Option<String> {
        self.hermes_chip()
    }

    fn show_hermes_page(&mut self, page: HermesPage) -> Task<Message> {
        self.hermes_focus = HermesFocus::Content;
        self.go_to_hermes_page(page)
    }

    fn focus_hermes(&mut self, focus: HermesFocus) -> Task<Message> {
        if self.open_hermes_page().is_some() {
            self.hermes_focus = focus;
        }
        Task::none()
    }

    fn step_hermes_page(&mut self, step: isize) -> Task<Message> {
        let next = self
            .open_hermes_page()
            .and_then(|page| crate::ui::hermes::neighbour(page, step));
        next.map_or_else(Task::none, |page| self.go_to_hermes_page(page))
    }

    /// The column shows only while the filing module is ready and has places
    /// to look at; otherwise the panels are drawn as they always were.
    /// The full-text lookup of Hermes, when its module is ready. Without an
    /// index it answers with an error, which the box shows as no section.
    fn document_finder(&self) -> Option<DocumentFinder> {
        self.hermes
            .filing
            .as_ref()
            .and_then(FilingModule::document_finder)
    }

    /// Sends the user to the overview, where Hermes is switched on, with the
    /// reason in the status bar.
    fn explain_hermes_is_off(&mut self) -> Task<Message> {
        let shown = self.open_hermes_overview();
        if self.hermes.overview.is_some() {
            let reason = self.lang.text(Msg::StatusHermesIsOff).to_owned();
            self.set_status_error(&reason);
        }
        shown
    }

    fn open_filing_status(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        let left_settings = self.leave_settings_page();
        if self.settings.page.is_some() {
            return left_settings;
        }
        let opened = self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::open_filing_status_page);
        Task::batch([left_settings, opened])
    }

    /// Starts an async directory read for `side`.
    fn load(&mut self, side: PanelSide, path: PathBuf, select: Option<String>) -> Task<Message> {
        match self.panels.start_load(side, path, select.clone()) {
            LoadStart::RefreshHitList => self.refresh_hit_list(side),
            LoadStart::Remote(remote) => self.load_remote(side, remote, select),
            LoadStart::Local(read) => read.map(Message::Panel),
        }
    }

    /// Opens a folder on a server in `side`. With a connection the listing is
    /// read; without one the keychain is asked for the password first, so a
    /// server whose password was remembered reconnects without a question.
    fn load_remote(
        &mut self,
        side: PanelSide,
        remote: RemotePath,
        select: Option<String>,
    ) -> Task<Message> {
        let sort = self.panel(side).sort;
        let request_id = self.panel_mut(side).begin_load();
        if self.remote.is_connected(remote.server()) {
            return remote_listing(
                Arc::clone(&self.remote),
                side,
                request_id,
                remote,
                sort,
                select,
            );
        }
        let passwords = Arc::clone(&self.passwords);
        let server = remote.server().clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || passwords.get(&server))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            move |result| {
                Message::Remote(RemoteMessage::PasswordLoaded {
                    side,
                    request_id,
                    remote: remote.clone(),
                    select: select.clone(),
                    result,
                })
            },
        )
    }

    /// Runs `step` on the connect feature. The state is taken out for it, since
    /// the context borrows the rest of the app; the overlay fact is read before,
    /// while the connect dialog still counts.
    fn with_connect_feature<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_remote::Remote, &ncrs_remote::Ctx<'_>) -> R,
    ) -> R {
        let blocking_overlay_open = self.blocking_overlay_is_open();
        let mut remote_ui = std::mem::take(&mut self.remote_ui);
        let outcome = step(
            &mut remote_ui,
            &ncrs_remote::Ctx {
                lang: self.lang,
                pool: &self.remote,
                passwords: &self.passwords,
                blocking_overlay_open,
                active_side: self.panels.active,
                active_panel_server: self.active_panel().remote(),
                left_request_id: self.panel(PanelSide::Left).request_id,
                right_request_id: self.panel(PanelSide::Right).request_id,
            },
        );
        self.remote_ui = remote_ui;
        outcome
    }

    fn run_remote_outcome(&mut self, (task, actions): ncrs_remote::Outcome) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_remote_action(action));
        }
        Task::batch(tasks)
    }

    fn run_remote_action(&mut self, action: ncrs_remote::Action) -> Task<Message> {
        use ncrs_remote::Action as Ask;
        match action {
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::ClearJobError => {
                self.job_error = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Ask::OpenServer(url) => {
                self.job_error = None;
                self.notice = None;
                let side = self.panels.active;
                self.load(side, url, None)
            }
            Ask::LoadFolder { side, path, select } => self.load(side, path, select),
            Ask::SaveServer { server, typed_path } => {
                self.save_server(&server, typed_path.as_deref())
            }
            Ask::StopWaiting { side, request_id } => {
                let panel = self.panel_mut(side);
                if panel.request_id == request_id {
                    panel.loading = false;
                }
                Task::none()
            }
            Ask::LeaveServer(server) => {
                let mut tasks = Vec::new();
                for side in [PanelSide::Left, PanelSide::Right] {
                    if self.panel(side).remote().as_ref() == Some(&server) {
                        let home = self.home.clone();
                        tasks.push(self.load(side, home, None));
                    }
                }
                Task::batch(tasks)
            }
            Ask::ListFolder {
                side,
                request_id,
                remote,
                select,
            } => {
                let sort = self.panel(side).sort;
                remote_listing(
                    Arc::clone(&self.remote),
                    side,
                    request_id,
                    remote,
                    sort,
                    select,
                )
            }
            Ask::ReportDisconnected {
                side,
                request_id,
                remote,
            } => {
                let server = remote.server().clone();
                Task::done(Message::Panel(PanelMessage::DirectoryLoaded {
                    side,
                    request_id,
                    path: remote.to_path_buf(),
                    entries: remote
                        .parent()
                        .map(|above| fs::FileEntry::parent(&above.to_path_buf()))
                        .into_iter()
                        .collect(),
                    error: Some(fs::ReadError::RemoteDisconnected { server }),
                    sorted_by: self.panel(side).sort,
                    select: None,
                    archive: None,
                }))
            }
        }
    }

    /// Puts the server into `[[servers]]`, or updates its start folder when it
    /// is there. Never the password.
    fn save_server(&mut self, server: &ServerId, typed_path: Option<&str>) -> Task<Message> {
        if self.config_path.is_none() {
            let text = self.lang.text(Msg::FilingSetupNoConfigPath).to_owned();
            self.set_status_error(&text);
            return Task::none();
        }
        let path = typed_path
            .filter(|text| text.starts_with('/'))
            .map(str::to_owned);
        let mut servers = self.saved_servers.clone();
        match servers
            .iter_mut()
            .find(|entry| entry.server_id() == *server)
        {
            Some(existing) => {
                if path.is_some() {
                    existing.path = path;
                }
            }
            None => servers.push(ServerEntry {
                name: None,
                host: server.host.clone(),
                port: server.port,
                user: server.user.clone(),
                path,
            }),
        }
        self.update_settings(SettingsMessage::Change(SettingChange::Servers(servers)))
    }
}

/// Reads a folder on a server and reports it as a panel's listing.
fn remote_listing(
    pool: Arc<Pool>,
    side: PanelSide,
    request_id: u64,
    remote: RemotePath,
    sort: fs::SortKey,
    select: Option<String>,
) -> Task<Message> {
    Task::perform(
        async move { fs::remote::list::list(&pool, remote, sort).await },
        move |listing| {
            Message::Panel(PanelMessage::DirectoryLoaded {
                side,
                request_id,
                path: listing.path,
                entries: listing.entries,
                error: listing.error,
                sorted_by: listing.sorted_by,
                select: select.clone(),
                archive: listing.archive,
            })
        },
    )
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub use tests::support::fixed::fixed_result_entry;
