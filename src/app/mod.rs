//! Root application: state, `update` (the only place state changes) and
//! `view` (pure composition of UI components).
use std::path::{Path, PathBuf};
use std::rc::Rc;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use iced::futures;
use iced::keyboard::{self, key::Named, Key, Modifiers};
use iced::widget::{container, mouse_area, operation, row, Column, Container, Space, Stack};
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

mod bars;
mod context_menu;
mod dialogs;
mod hermes;
mod hermes_pages;
mod keys;
mod ops;
mod palette;
mod panels;
mod remote;
mod search;
mod session;
mod settings;
mod startup;
mod subscription;
mod update;
mod view;

use self::dialogs::*;
pub use self::keys::*;
use self::ops::*;
use self::panels::*;
use self::remote::*;
use self::session::*;
use self::settings::*;
pub use self::startup::*;
#[cfg(test)]
use self::subscription::receiver_stream;
#[cfg(test)]
use self::update::{archive_refusal, remote_refusal};

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

#[cfg(test)]
mod tests;
#[cfg(test)]
pub use tests::support::fixed::fixed_result_entry;
