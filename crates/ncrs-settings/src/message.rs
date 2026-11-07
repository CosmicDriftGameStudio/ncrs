//! Everything the settings pages send.

use std::path::PathBuf;

use iced::mouse::ScrollDelta;
use ncrs_config::config::{
    AiProviderChoice, AiProviderName, Config, FilingLocation, FilingWriteError, OwnDetail,
    ProgramLine, ServerEntry,
};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::keymap::Binding;
use ncrs_config::palette::{ColorRole, ThemePreset};
use ncrs_config::system_monitor::SystemSettings;
use ncrs_config::SettingsFeature;
use ncrs_fs::OpenKind;
use ncrs_i18n::Language;
use ncrs_pipeline::Load;

use crate::ai_model::DownloadError as AiDownloadError;
use crate::facts::{AreaFacts, LoadedFacts, ModelKind};
use crate::folders::AddAs;
use crate::ocr_models::DownloadError;
use crate::open_vsx::FetchError;
use crate::theme_fetch::{Fetched, Installed};

/// A setting the settings tab changes: applied at once and written to the
/// config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Preset(ThemePreset),
    Language(Language),
    /// The `check_for_updates` switch.
    CheckForUpdates(bool),
    HermesEnabled(bool),
    Load(Load),
    AiEnabled(bool),
    /// `None` hands the file to the system default again.
    Program {
        kind: OpenKind,
        line: Option<ProgramLine>,
    },
    /// The theme file as `[theme] vscode` names it, `None` drops it.
    VscodeTheme(Option<String>),
    /// `#rrggbb` or `#rgb`, `None` leaves the role to the color scheme.
    Color {
        role: ColorRole,
        color: Option<String>,
    },
    AiProvider(AiProviderChoice),
    /// The whole list of saved SFTP servers, as it is to be written.
    Servers(Vec<ServerEntry>),
    /// One whole list of the user's own names, accounts or tax numbers.
    OwnDetails {
        detail: OwnDetail,
        entries: Vec<String>,
    },
    /// Which tags follow a file name in the file lists.
    FileListTags(FileListTags),
    /// The whole `[system]` section.
    System(SystemSettings),
    /// The whole list of read folders, as it is to be written. Hermes and the
    /// AI stay as they are.
    Locations(Vec<FilingLocation>),
}

/// A text field of the settings tab. What is typed stays a draft until
/// Enter, which checks it and writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingField {
    Program(OpenKind),
    Color(ColorRole),
    AiEndpoint,
    AiModel,
    /// Where an entry for one of the own details is typed.
    Own(OwnDetail),
}

/// An entry of the terminal list on the programs page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalChoice {
    /// The first terminal found.
    Automatic,
    /// The terminal at this position of the list found on this computer.
    Detected(usize),
    /// A command typed in the field below the list.
    Own,
}

/// Everything the settings tab sends.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsMessage {
    /// The key and the command list: opens the tab, or closes it when it is up.
    Toggle,
    /// Switches the tags after the names in the file lists on or off.
    ToggleFileListTags,
    /// Shows the tags page, where the tags for the file lists are chosen.
    ChooseFileListTags,
    Close,
    Select(SettingsFeature),
    SearchChanged(String),
    Change(SettingChange),
    Edit {
        field: SettingField,
        text: String,
    },
    Submit(SettingField),
    /// Opens the system file dialog for the program behind F3 or F4.
    PickProgram(OpenKind),
    ProgramPicked {
        kind: OpenKind,
        path: Option<PathBuf>,
    },
    PickThemeFile,
    ThemeFilePicked(Option<PathBuf>),
    ChooseProvider(AiProviderName),
    ChooseTerminal(TerminalChoice),
    /// The button that opens the card to fetch a color theme.
    OpenThemeFetch,
    OpenKeyBindings,
    OpenOcrModels,
    /// The button that adds a saved server.
    OpenServerAdd,
    /// The button of the saved server with this address.
    OpenServer(PathBuf),
    /// The button at this position of the protection of the index.
    Protection(usize),
    /// "+ Ordner wählen …": opens the system folder dialog.
    AddFolder(AddAs),
    FolderPicked {
        kind: AddAs,
        path: Option<PathBuf>,
    },
    /// The access to the picked folder is kept (sandbox), or not.
    FolderGranted {
        kind: AddAs,
        path: PathBuf,
        result: Result<(), String>,
    },
    RemoveFolder(PathBuf),
    /// Opens the dialog of this read folder, or closes it.
    EditFolder(Option<PathBuf>),
    /// The folder with this path takes the values of the location.
    ReplaceFolder(FilingLocation),
    /// The "+" next to the field of an own detail.
    AddOwnDetail(OwnDetail),
    ToggleAllAreas,
    ToggleAllRuns,
    /// Opens the dialog of the own server, or closes it.
    ServerDialog(bool),
    /// Opens the chat (F2), at the change of this area when one is named.
    OpenChat(Option<AreaFacts>),
    TogglePause,
    /// Puts the run of this journal batch back.
    UndoRun(i64),
    /// A run was put back, or the reason it could not be.
    RunUndone(Result<(), String>),
    /// The app read the facts the Index and Hermes pages show.
    FactsLoaded(LoadedFacts),
    OpenAiModel,
    /// Asks before this model is removed from the computer.
    AskRemoveModel(ModelKind),
    CancelRemoveModel,
    ConfirmRemoveModel,
    ModelRemoved {
        kind: ModelKind,
        result: Result<(), String>,
    },
    /// The config as read back after the write of `change`.
    Written {
        change: SettingChange,
        result: Result<Box<Config>, FilingWriteError>,
    },
}

/// Everything the keyboard shortcuts dialog sends.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyBindingsMessage {
    /// The key, the command list and the settings tab open the dialog.
    Open,
    Close,
    SearchChanged(String),
    /// A character typed while the list has the keys: it goes to the search
    /// field, which takes the keys again.
    Typed(String),
    /// A chord pressed in the search field: finds what it is bound to.
    SearchByKey(Binding),
    /// The arrows: a row up or down.
    Move(isize),
    /// The arrows: the key cap left or right; past the last one is the slot
    /// that adds a key.
    MoveKey(isize),
    /// A click on a row, or on one of its key caps.
    Select {
        row: usize,
        key: Option<usize>,
    },
    Scrolled(ScrollDelta),
    /// Enter: records a new key for the focused cap, or a further one on the
    /// slot that adds a key.
    Record,
    /// Records a further key for the selected action.
    RecordAdditional,
    /// The key pressed while recording.
    Recorded(Binding),
    /// Escape while recording.
    CancelRecording,
    /// Delete or Backspace: removes the focused key.
    RemoveKey,
    /// Back to the keys the action ships with.
    Reset,
    /// Asks before every action goes back to its built-in keys.
    ResetAll,
    /// Takes the key from the action it is bound to, or confirms a reset of
    /// everything.
    Confirm,
    /// Leaves the clash or the question without changing anything.
    Dismiss,
    /// The config as read back after a write.
    Written(Result<Box<Config>, FilingWriteError>),
}

/// Everything the card that fetches a color theme sends.
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeFetchMessage {
    /// Opens the card, or gives its field the focus again when it is up.
    Open,
    Close,
    TextChanged(String),
    /// Enter: fetches the typed extension, or installs the theme under the
    /// cursor, whichever the card is waiting for.
    Activate,
    Move(isize),
    /// A click on the theme at this position of the list: installs it.
    Pick(usize),
    /// The download of the generation is done. Dropped when the card was
    /// closed or opened again meanwhile.
    Fetched {
        generation: u64,
        result: Result<Fetched, FetchError>,
    },
    /// The installation of the generation is done.
    Installed {
        generation: u64,
        result: Result<Box<Installed>, FetchError>,
    },
}

/// Everything the card that asks for the text recognition models sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrModelsMessage {
    Open,
    Close,
    Download,
    /// The 250 ms look at the byte counter of the running download.
    Progress,
    /// The download of the generation is done. Dropped when the card was
    /// closed or opened again meanwhile.
    Downloaded {
        generation: u64,
        result: Result<(), DownloadError>,
    },
}

/// Everything the card that asks for the AI model sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiModelMessage {
    Open,
    Close,
    /// Download, or continue where an earlier download stopped.
    Download,
    Stop,
    /// The 250 ms look at the byte counter of the running download: nothing to
    /// change, the redraw is the point.
    Progress,
    /// Writes the AI switch on and closes the card.
    TurnOn,
    /// The download of the generation has ended. Dropped when the card was
    /// closed or opened again meanwhile.
    Downloaded {
        generation: u64,
        result: Result<(), AiDownloadError>,
    },
}

/// Everything the settings send to `update`: the messages of its pages.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Settings(SettingsMessage),
    KeyBindings(KeyBindingsMessage),
    ThemeFetch(ThemeFetchMessage),
    OcrModels(OcrModelsMessage),
    AiModel(AiModelMessage),
}
