//! Files that come from outside the panels: dropped from another app, or
//! pasted from the clipboard. And the prompts that ask for a name.

use std::path::PathBuf;
use std::time::Duration;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_i18n::Language;

use crate::clipboard_paste::{pasted_name, PastedContent, PastedKind};
use crate::prompt::Prompt;
use crate::prompt_view::FIELD_ID;
use crate::state::Ops;
use crate::transfer::view::Progress;

/// How long after the last dropped file the drop counts as complete.
const DROP_SETTLE: Duration = Duration::from_millis(100);

impl Ops {
    /// Opens `prompt` for the panel on `side`. The field has to be focused or
    /// the prompt swallows every keystroke: `TextInput` only takes keys while
    /// focused, and giving it an Id does not focus it.
    pub fn open_prompt<T: Send + 'static>(&mut self, prompt: Prompt, side: PanelSide) -> Task<T> {
        self.prompt = Some(prompt);
        self.prompt_side = Some(side);
        iced::widget::operation::focus(FIELD_ID)
    }

    /// A dropped file arrived. The task answers with the count that was
    /// current then, once the burst is over.
    pub fn queue_dropped_file(&mut self, path: PathBuf) -> Task<u64> {
        self.drop_hover = false;
        self.dropped_files.push(path);
        self.drop_generation = self.drop_generation.wrapping_add(1);
        let generation = self.drop_generation;
        Task::perform(async { tokio::time::sleep(DROP_SETTLE).await }, move |()| {
            generation
        })
    }

    /// The files of a drop that is over, if `generation` is still the latest.
    pub fn settled_drop(&mut self, generation: u64) -> Option<Vec<PathBuf>> {
        if generation != self.drop_generation || self.dropped_files.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.dropped_files))
    }

    /// A job runs or a dialog is open, so nothing may start a transfer into
    /// the active panel.
    pub fn transfer_into_panel_is_blocked(&self, transfer_dialog_open: bool) -> bool {
        self.transfer.is_some()
            || self.extracting.is_some()
            || self.prompt.is_some()
            || self.conflict_is_open()
            || self.delete_dialog.is_some()
            || transfer_dialog_open
            || self.archive_write.is_some()
    }

    /// Asks for the name of the file a pasted picture or text becomes in the
    /// folder `parent`, which `taken` knows the entries of.
    pub fn ask_for_pasted_file_name<T: Send + 'static>(
        &mut self,
        pasted: PastedContent,
        kind: PastedKind,
        lang: Language,
        (parent, side): (PathBuf, PanelSide),
        taken: impl Fn(&str) -> bool,
    ) -> Task<T> {
        let name = pasted_name(kind, chrono::Local::now().naive_local(), lang, taken);
        self.pending_paste = Some(pasted);
        self.open_prompt(Prompt::paste(parent, name), side)
    }

    /// The name of a pasted picture or text is confirmed: writes it into a
    /// private folder and closes the prompt. The file is the one to copy; a
    /// failed write stays in the prompt.
    pub fn stage_paste(&mut self) -> Option<PathBuf> {
        let (prompt, bytes) = (
            self.prompt.as_ref()?,
            self.pending_paste
                .as_ref()
                .and_then(PastedContent::file_bytes)?,
        );
        let name = prompt.name().trim().to_owned();
        let written = tempfile::Builder::new()
            .prefix("ncrs-paste-")
            .tempdir()
            .and_then(|folder| {
                let file = folder.path().join(&name);
                std::fs::write(&file, bytes)?;
                Ok((folder, file))
            });
        let (folder, file) = match written {
            Ok(staging) => staging,
            Err(error) => {
                self.set_prompt_error_text(&error.to_string());
                return None;
            }
        };
        self.prompt = None;
        self.prompt_side = None;
        self.pending_paste = None;
        self.paste_staging = Some(folder);
        Some(file)
    }

    /// What the progress mask shows: how far, counted in rows when there are
    /// several (the tick only knows the row it is in) and in the tick's items
    /// when a single entry, maybe a whole folder, is the transfer.
    pub fn progress_mask(&self, latest: Option<ncrs_fs::transfer::Tick>) -> Option<Progress<'_>> {
        let transfer = self
            .transfer
            .as_ref()
            .filter(|_| self.transfer_mask_visible)?;
        let (done, total) = if transfer.sources.len() > 1 {
            (self.transfer_rows_done, transfer.sources.len())
        } else {
            latest.map_or((0, 0), |tick| (tick.done, tick.total))
        };
        Some(Progress {
            kind: transfer.kind,
            target: &transfer.target,
            done,
            total,
        })
    }
}
