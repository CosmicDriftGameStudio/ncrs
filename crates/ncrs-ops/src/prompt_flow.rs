//! What the open prompt shows while its operation runs and after it failed.

use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;
use ncrs_fs::CreateDirError;
use ncrs_i18n::{Language, Msg};

use crate::archive_flow::PackRequest;
use crate::prompt::PromptKind;
use crate::state::Ops;

impl Ops {
    /// Whether the create that reported is the one the prompt still waits for.
    /// A prompt dismissed or retried while the operation ran has another id.
    pub fn is_current_prompt_request(&self, request_id: u64) -> bool {
        request_id == self.prompt_request_id
    }

    /// The create of the name prompt has reported: closes the prompt or shows
    /// why it failed. Returns the new file to open when a file was created.
    /// `created` is the path the queued reload selects.
    pub fn finish_prompt(
        &mut self,
        kind: PromptKind,
        result: Result<(), CreateDirError>,
        created: Option<PathBuf>,
        lang: Language,
    ) -> Option<PathBuf> {
        match result {
            Ok(()) => {
                self.prompt = None;
                self.prompt_side = None;
                created.filter(|_| kind == PromptKind::CreateFile)
            }
            Err(err) => {
                self.set_prompt_busy(false);
                let text = self.prompt_error(lang, &err);
                self.set_prompt_error_text(&text);
                None
            }
        }
    }

    /// Shows a validation failure in the open prompt.
    pub fn set_prompt_error(&mut self, reason: &'static str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_validation_error(reason);
        }
    }

    /// Wording for a failed create, in the active language. Deciding here
    /// rather than in the filesystem layer keeps `fs` free of user-facing text,
    /// and the two failures need different advice: a taken name means pick
    /// another, a missing parent means the panel has gone stale.
    pub fn prompt_error(&self, lang: Language, err: &CreateDirError) -> String {
        use std::io::ErrorKind;
        let name = self.prompt_name();
        match err.io_error().map(|e| e.kind()) {
            Some(ErrorKind::AlreadyExists) => {
                lang.text(Msg::ErrorMkdirTaken).replace("{name}", &name)
            }
            Some(ErrorKind::NotFound) => lang.text(Msg::ErrorParentMissing).to_string(),
            _ => lang
                .text(Msg::ErrorMkdirFailed)
                .replace("{name}", &name)
                .replace("{reason}", &err.to_string()),
        }
    }

    /// The name currently in the prompt, or an empty string.
    pub fn prompt_name(&self) -> String {
        self.prompt
            .as_ref()
            .map(|p| p.name().to_string())
            .unwrap_or_default()
    }

    /// Shows a filesystem failure in the open prompt.
    pub fn set_prompt_error_text(&mut self, reason: &str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_error(reason.to_string());
        }
    }

    pub fn set_prompt_busy(&mut self, busy: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_busy(busy);
        }
    }

    /// The prompt is a pack or a paste with a name that passed validation:
    /// neither creates an entry by itself, each starts its own job.
    pub fn valid_prompt_has_its_own_job(&self) -> bool {
        self.prompt.as_ref().is_some_and(|prompt| {
            matches!(prompt.kind(), PromptKind::Pack | PromptKind::Paste)
                && prompt.validate().is_ok()
        })
    }

    pub fn toggle_keep_folders(&mut self) {
        if let Some(prompt) = self.prompt.as_mut().filter(|prompt| !prompt.busy()) {
            prompt.toggle_keep_folders();
        }
    }
}

impl Ops {
    /// Enter in the pack prompt, once its name passed `Prompt::validate`: the
    /// prompt closes and what it held is the archive to write. `fallback_side`
    /// is the target panel when the prompt does not remember one.
    pub fn take_pack_request(&mut self, fallback_side: PanelSide) -> Option<PackRequest> {
        let prompt = self.prompt.take()?;
        let target_side = self.prompt_side.take().unwrap_or(fallback_side);
        Some(PackRequest {
            archive: prompt.archive_path(),
            sources: prompt.sources().to_vec(),
            keep_under: prompt.kept_folders_under().map(Path::to_path_buf),
            target_side,
        })
    }
}
