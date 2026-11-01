use std::path::PathBuf;

use ncrs_config::file_list_tags::{FileListTags, TagExceptions};
use ncrs_filing::replan_work::{ReplanItem, ReplanScope};
use ncrs_filing::review_work::AcceptSummary;
use ncrs_index::{QuerySpec, SavedQueryId};
use ncrs_intake::LocationMode;

use crate::hermes_setup::FolderSlot;
use crate::message::HermesPage;

/// The setup card a folder dialog or a folder access belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCard {
    Filing(LocationMode),
    Hermes(FolderSlot),
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
    ExcludeFromHermes(Vec<PathBuf>),
    /// The documents of the previewed replan were confirmed: one job files them.
    Refile {
        scope: ReplanScope,
        items: Vec<ReplanItem>,
    },
    /// The documents of a group were filed from the quick-fix menu.
    QuickFixFiled(AcceptSummary),
}
