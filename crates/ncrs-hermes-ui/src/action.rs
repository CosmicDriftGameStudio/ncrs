use std::path::PathBuf;

use ncrs_config::file_list_tags::{FileListTags, TagExceptions};
use ncrs_filing::review_work::AcceptSummary;
use ncrs_index::{QuerySpec, SavedQueryId};
use ncrs_intake::LocationMode;
use ncrs_jobs::Job;
use tokio::sync::mpsc::UnboundedSender;

use crate::hermes_setup::FolderSlot;
use crate::message::{HermesPage, Message};

/// The setup card a folder dialog or a folder access belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCard {
    Filing(LocationMode),
    Hermes(FolderSlot),
}

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
    /// Clears the error and the notice in the status bar.
    ClearStatus,
    ShowNotice(String),
    ShowError(String),
    RequestHermesCounts,
    /// Hermes reads these files again.
    ReadPathsAgain(Vec<PathBuf>),
    /// Closes the overview and the status page, which a page of this crate
    /// replaces.
    CloseOverviewAndStatus,
    /// Goes to a page that the app still owns, from the page that is up.
    GoToHermesPage(HermesPage),
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
    /// The documents of a group were filed from the quick-fix menu.
    QuickFixFiled(AcceptSummary),
}
