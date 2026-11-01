//! Everything the settings pages send.

use std::path::PathBuf;

use iced::mouse::ScrollDelta;
use ncrs_config::config::{
    AiProviderChoice, AiProviderName, Config, FilingWriteError, OwnDetail, ProgramLine, ServerEntry,
};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::keymap::Binding;
use ncrs_config::palette::{ColorRole, ThemePreset};
use ncrs_config::SettingsFeature;
use ncrs_fs::OpenKind;
use ncrs_i18n::Language;
use ncrs_pipeline::Load;

use crate::ai_model::DownloadError as AiDownloadError;
use crate::ocr_models::DownloadError;
use crate::open_vsx::FetchError;
use crate::theme_fetch::{Fetched, Installed};

/// A setting the settings tab changes: applied at once and written to the
/// config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Preset(ThemePreset),
    Language(Language),
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
}

/// A text field of the settings tab. What is typed stays a draft until
/// Enter, which checks it and writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingField {
    Program(OpenKind),
    Color(ColorRole),
    AiThreads,
    AiEndpoint,
    AiModel,
    /// Where an entry for one of the own details is typed.
    Own(OwnDetail),
}

/// Everything the settings tab sends.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsMessage {
    /// The key and the command list: opens the tab, or closes it when it is up.
    Toggle,
    /// Switches the tags after the names in the file lists on or off.
    ToggleFileListTags,
    /// Opens the tags page of the Ablage.
    ChooseFileListTags,
    Close,
    /// Folds the technical rows of a page open or shut.
    ToggleAdvanced,
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
    /// The button that opens the card to fetch a color theme.
    OpenThemeFetch,
    OpenKeyBindings,
    OpenOcrModels,
    /// The button that adds a saved server.
    OpenServerAdd,
    /// The button of the saved server with this address.
    OpenServer(PathBuf),
    /// The button that opens the status page of the Ablage.
    OpenFilingStatus,
    /// The button at this position of the protection of the index.
    Protection(usize),
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
    /// The card after the download could not turn the AI on: opens the setting.
    ShowAiSetting,
    /// The download of the generation has ended. Dropped when the card was
    /// closed or opened again meanwhile.
    Downloaded {
        generation: u64,
        result: Result<(), AiDownloadError>,
    },
}
