//! What the file operations ask the app to do, which they cannot do
//! themselves. The app runs it right after `update`.

use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_fs::{RestoreError, TrashedItem};
use ncrs_jobs::Job;
use tokio::sync::mpsc::UnboundedSender;

use crate::message::OpsMessage;

/// What the copy and move dialog asks of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferAction {
    /// The typed target changed: count its taken names again.
    CountConflicts,
    /// The dialog was closed without a transfer: give the keyboard focus back.
    Closed,
    /// Enter: check the target and start the transfer.
    Confirm,
    /// Close the progress mask; the job goes on.
    HideProgress,
    /// Open the progress mask again if a job runs.
    ShowProgress,
}

/// What the trash view asks of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum TrashAction {
    /// Show the trash in place of the active panel.
    Open,
    Close,
    /// Put these entries back.
    Restore(Vec<TrashedItem>),
    RestoreRowDone {
        result: Result<(), RestoreError>,
        index: usize,
        generation: u64,
    },
    RestoreLast,
    LastListed {
        generation: u64,
        result: Result<Vec<TrashedItem>, String>,
    },
    OpenInFinder,
}

/// The task of a job, made only when the queue starts the job: the work spawns
/// the moment it is built.
pub type JobBuilder = Box<dyn FnOnce() -> iced::Task<OpsMessage>>;

/// A job for the queue and the work that runs it.
pub struct StartJob {
    pub job: Job,
    pub build: JobBuilder,
}

impl std::fmt::Debug for StartJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartJob").field("job", &self.job).finish()
    }
}

/// The task of a job that reports its progress, made only when the queue
/// starts the job. It gets the sender of the app's progress channel.
pub type ProgressJobBuilder =
    Box<dyn FnOnce(UnboundedSender<ncrs_fs::transfer::Tick>) -> iced::Task<OpsMessage>>;

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

/// What a file handed to the viewer is a copy of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewOrigin {
    Archive,
    Server,
}

/// What a flow of the file operations asks of the app. The app runs the
/// actions in order, right after `update`; an action that follows another sees
/// what the first changed.
#[derive(Debug)]
pub enum Action {
    /// Clear the error and the notice of the status bar.
    ClearStatus,
    ShowNotice(String),
    ShowError(String),
    /// Move the keyboard focus to this button of the open dialog.
    FocusDialogButton(usize),
    /// Put the job into the queue and start it if nothing runs.
    StartJob(StartJob),
    /// As `StartJob`, for work that sends its progress through the app's
    /// channel: the app makes the channel first, then queues the job.
    StartJobWithProgress(StartProgressJob),
    /// The running job is over: the next one may start.
    FinishJob,
    SetJobProgress(ncrs_fs::transfer::Tick),
    ClearJobProgress,
    ClearSelection(PanelSide),
    /// Read both panels again.
    ReloadPanels,
    /// Read both panels again, the one the operation acted on first.
    ReloadSourceThenTarget,
    /// Read both panels again; the one that shows `folder` selects `name`.
    ReloadPanelsSelecting {
        folder: PathBuf,
        name: String,
    },
    /// One more row of the running job is done.
    CountJobDone,
    ResetJobDone,
    /// Open the copy made for the viewer in the viewer.
    LaunchViewCopy {
        path: PathBuf,
        origin: ViewOrigin,
    },
    RestartSearch,
    /// Read the trash again.
    ListTrash,
    /// A name that is taken asks a question: nothing may stay open over it.
    CloseQuickOpenAndPalette,
    /// An error that, pressed, opens the trash in the Finder.
    ShowErrorWithFinderPress(String),
    /// Shows the trash in place of the panel on this side.
    OpenTrash(PanelSide),
    /// Shows the trash folder in the Finder, where the app cannot list it.
    OpenTrashInFinder,
}
