//! What the flows of the file operations keep between two messages.

use std::ffi::OsString;
use std::path::PathBuf;

use ncrs_core::{PanelSide, TransferKind};
use ncrs_fs::archive::ViewCopies;
use ncrs_fs::transfer::OnConflict;
use ncrs_fs::TrashedItem;

use crate::clipboard_paste::PastedContent;
use crate::message::ConflictChoice;
use crate::prompt::Prompt;
use crate::transfer::TransferDialog;
use crate::trash::TrashView;

/// What a transfer does, and to which rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub kind: TransferKind,
    /// The rows the user tagged, or the single row under the cursor when nothing
    /// is tagged. Resolved once at F5, because by the time a conflict dialog is
    /// up the selection may have moved.
    pub sources: Vec<PathBuf>,
    pub target: PathBuf,
    /// The name each source gets in the target, by position; empty when every
    /// source keeps its own. Differs only where sources from several folders
    /// share a name.
    pub names: Vec<OsString>,
}

impl Transfer {
    /// The name the source at `index` gets in the target, when it is not its own.
    pub fn name_in_target(&self, index: usize) -> Option<OsString> {
        let name = self.names.get(index).filter(|name| !name.is_empty())?;
        let source = self.sources.get(index)?;
        (source.file_name() != Some(name.as_os_str())).then(|| name.clone())
    }
}

/// One file waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingConflict {
    pub transfer: Transfer,
    /// Which of `sources` is in the way.
    pub index: usize,
}

/// The archive file F3 is unpacking, and the folder it goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracting {
    pub archive: PathBuf,
    pub slot: PathBuf,
}

/// A pack, a copy into an archive or a delete from one, while its job runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveWrite {
    /// The panel the entries came from, which is cleared and reloaded at the
    /// end even if the user switches panels meanwhile.
    pub side: PanelSide,
    pub what: ArchiveWriteKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveWriteKind {
    /// A new archive of this name in `folder`, by default the other panel's.
    Pack {
        name: String,
        folder: PathBuf,
    },
    CopyIn,
    Delete,
}

/// What a delete works on, resolved once when F8 is pressed: by the time the
/// user has answered the dialog the cursor may have moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub sources: Vec<PathBuf>,
    /// The permanent-delete key (Shift+F8 by default): no trash.
    pub permanent: bool,
    /// The panel the entries came from, which is cleared and reloaded at the
    /// end even if the user switches panels meanwhile.
    pub side: PanelSide,
}

/// A put-back from the trash, resolved when Enter is pressed: by the time a
/// name turns out to be taken the cursor may have moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restoration {
    pub items: Vec<TrashedItem>,
    pub restored: usize,
    pub skipped: usize,
    /// An "all" answer to the dialog, applied to the rows after it.
    pub rule: Option<OnConflict>,
}

/// The state of the transfer, delete, restore and archive flows and the count that tells a
/// late result from a current one. Every operation of the app bumps `generation` when it starts or
/// ends, so a result that arrives after that is dropped.
#[derive(Debug, Default)]
pub struct Ops {
    /// The open modal prompt, if any.
    pub prompt: Option<Prompt>,
    /// Panel the prompt belongs to, so a panel switch while it is open cannot
    /// create a directory in the wrong place.
    pub prompt_side: Option<PanelSide>,
    /// Id of the running prompt operation, so a late result cannot land on a
    /// prompt the user already dismissed.
    pub prompt_request_id: u64,
    /// The delete confirmation dialog, while it is up.
    pub delete_dialog: Option<Deletion>,
    /// The delete that is running.
    pub deleting: Option<Deletion>,
    /// What the latest F8 run moved to the trash, for the restore key.
    pub last_deletion: Vec<PathBuf>,
    /// A restore from the trash, while it runs.
    pub restoring: Option<Restoration>,
    /// The restore paused on a name that is taken: the index of its row.
    pub restore_conflict: Option<usize>,
    /// What a transfer works on and where to. Kept so a conflict can resume
    /// without asking the user to mark the rows again.
    pub transfer: Option<Transfer>,
    /// A transfer paused on a name that is already taken.
    pub pending_conflict: Option<PendingConflict>,
    /// Whether "for all files" is ticked in the conflict dialog. Lives here so
    /// the tick survives the dialog being rebuilt every frame.
    pub conflict_all: bool,
    /// The conflict rule from a previous "for all" answer, applied to the rest
    /// without asking again.
    pub conflict_rule: Option<OnConflict>,
    /// Whether the progress mask of the running transfer is up. "In the
    /// background" closes it; the status bar brings it back.
    pub transfer_mask_visible: bool,
    /// How many rows (top-level entries) of the running transfer are done.
    pub transfer_rows_done: usize,
    /// The folders F3 unpacks archive files into. `None` when there is no
    /// cache folder to put them in.
    pub view_copies: Option<ViewCopies>,
    /// The file F3 is unpacking, while its job runs.
    pub extracting: Option<Extracting>,
    /// The pack, copy into an archive or delete from one that is running.
    pub archive_write: Option<ArchiveWrite>,
    /// The trash view, in place of the panel on `trash_side`.
    /// Files from another app are held over the window.
    pub drop_hover: bool,
    /// The files of the drop still arriving.
    pub dropped_files: Vec<PathBuf>,
    /// Counts the dropped files, so the settle timer knows the burst is over.
    pub drop_generation: u64,
    /// The picture or text of a paste while its file name is being asked for.
    pub pending_paste: Option<PastedContent>,
    /// Where a pasted picture or text is written before it is copied into the
    /// panel. Kept until the next paste so the running copy keeps its source.
    pub paste_staging: Option<tempfile::TempDir>,
    /// The copy or move confirmation, while it is up.
    pub transfer_dialog: Option<TransferDialog>,
    /// Counts the confirmations opened, so a result of the background scan can
    /// tell if its dialog is still the one on screen.
    pub transfer_dialogs_opened: u64,
    pub trash_view: Option<TrashView>,
    pub trash_side: PanelSide,
    /// Counts the listings of the trash, so a late one is dropped.
    pub trash_generation: u64,
    pub generation: u64,
}

impl Ops {
    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Whether a name that is taken waits for an answer, in a transfer or in
    /// a restore from the trash.
    pub fn conflict_is_open(&self) -> bool {
        self.pending_conflict.is_some() || self.restore_conflict.is_some()
    }

    /// The file the open conflict dialog asks about.
    pub fn conflict_name(&self) -> String {
        let path = match (self.pending_conflict.as_ref(), self.restore_conflict) {
            (Some(pending), _) => pending.transfer.sources.get(pending.index),
            (None, Some(index)) => self
                .restoring
                .as_ref()
                .and_then(|restoring| restoring.items.get(index))
                .map(|item| &item.original),
            (None, None) => None,
        };
        let renamed = self
            .pending_conflict
            .as_ref()
            .and_then(|pending| pending.transfer.name_in_target(pending.index));
        renamed
            .as_deref()
            .or_else(|| path?.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// What the dialog's button means: the tick belonged to the question just
    /// answered, so it goes with it.
    pub fn conflict_answer(&mut self, clicked: ConflictChoice) -> ConflictChoice {
        let choice = if self.conflict_all {
            clicked.for_all()
        } else {
            clicked
        };
        self.conflict_all = false;
        choice
    }

    /// Whether the progress mask is up over a running transfer.
    pub fn progress_mask_is_open(&self) -> bool {
        self.transfer_mask_visible && self.transfer.is_some()
    }
}
