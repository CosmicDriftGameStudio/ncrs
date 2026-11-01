use std::path::PathBuf;

use ncrs_config::file_list_tags::{FileListTags, TagExceptions};
use ncrs_filing::filing::{FilingModule, Work};
use ncrs_index::{QuerySpec, SavedQueryId};
use ncrs_intake::LocationMode;
use ncrs_jobs::Job;
use ncrs_pipeline::Load;
use tokio::sync::mpsc::UnboundedSender;

use crate::hermes_setup::FolderSlot;
use crate::message::{HermesFocus, HermesPage, Message};

/// The setup card a folder dialog or a folder access belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCard {
    Filing(LocationMode),
    Hermes(FolderSlot),
}

/// The step of the module that starts a change of the protection of the index.
pub type ProtectionStep = fn(&mut FilingModule) -> Option<Work>;

/// The task of a job that reports its progress, made only when the queue
/// starts the job. It gets the sender of the app's progress channel.
pub type ProgressJobBuilder =
    Box<dyn FnOnce(UnboundedSender<ncrs_fs::transfer::Tick>) -> iced::Task<Message>>;

/// A job for the queue and the work that runs it and reports progress.
pub struct StartProgressJob {
    pub job: Job,
    pub build: ProgressJobBuilder,
}

impl std::fmt::Debug for StartProgressJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartProgressJob")
            .field("job", &self.job)
            .finish()
    }
}

/// What the Hermes pages ask of the app, which they cannot do themselves. The
/// app runs the actions in order, right after `update`.
#[derive(Debug)]
pub enum Action {
    CloseContextMenu,
    /// Opens the overview, if nothing covers it.
    OpenHermesOverview,
    /// Opens the status page, if nothing covers it.
    OpenFilingStatus,
    /// Shows the file in the active panel.
    RevealFile(PathBuf),
    /// The module of the previous settings is gone: the one of this
    /// generation starts now.
    FilingRestarted(u64),
    /// Opens the card of the AI model, and notes whether the user was asked.
    OpenAiModelCard,
    /// Moves the focus of the dialog or card that is up.
    SetDialogFocus(usize),
    /// Selects the error of the file on the status page.
    SelectErrorOf(PathBuf),
    /// Both panels read their folder again, keeping the cursor.
    ReloadPanels,
    /// Clears the error and the notice in the status bar.
    ClearStatus,
    ShowNotice(String),
    ShowError(String),
    RequestHermesCounts,
    /// Hermes reads these files again.
    ReadPathsAgain(Vec<PathBuf>),
    /// Closes the status page, which a page of this crate replaces.
    CloseStatusPage,
    /// Goes to the status page, which the app still owns, from the page that
    /// is up.
    GoToHermesPage(HermesPage),
    SetHermesFocus(HermesFocus),
    /// Takes in the latest snapshot of the pipeline and of the module.
    RefreshFilingSnapshot,
    /// The overview asks whether the OCR models are installed.
    ReadOcrModelsPresent,
    /// Stops the pipeline and runs the job that encrypts or removes the index.
    ChangeProtection(ProtectionStep),
    /// Opens the question whether to begin again, with this button focused.
    OpenStartOverDialog(usize),
    CloseStartOverDialog,
    /// The setting of the AI changes.
    SetAiEnabled(bool),
    /// The setting of the load changes.
    SetLoad(Load),
    OpenOcrModels,
    /// Opens the card of the AI model and starts the download.
    OpenAndDownloadAiModel,
    /// Shows the settings of Hermes at the row with this title.
    ShowHermesSetting(String),
    /// Opens the card that sets Hermes up the first time.
    OpenHermesSetup,
    /// Builds the module again from the config it started with.
    RestartHermesModule,
    /// The tags the file list shows against their group, as the index has them.
    SetTagExceptions(TagExceptions),
    /// The setting of the tags in the file list changes.
    ChangeFileListTags(FileListTags),
    /// Opens the search on `spec`, in place of the reports page; with a
    /// report, the search changes that one.
    OpenSearchFor {
        spec: QuerySpec,
        report: Option<SavedQueryId>,
    },
    /// Asks for the folder the reports are exported to, starting at the path.
    PickExportFolder(PathBuf),
    /// Opens the system folder dialog for a setup card, starting at the
    /// folder of the active panel; the answer is the card's `FolderPicked`.
    PickFolder(SetupCard),
    /// Keeps the access to the folder picked for a setup card (sandbox only);
    /// the answer is the card's `Granted`.
    GrantFolder {
        card: SetupCard,
        folder: PathBuf,
    },
    /// Put the job into the queue and start it if nothing runs; the work
    /// sends its progress through the app's channel, which the app makes
    /// first.
    StartJobWithProgress(StartProgressJob),
    /// The running job is over: the next one may start.
    FinishJob,
    SetJobProgress(ncrs_fs::transfer::Tick),
    ClearJobProgress,
}
