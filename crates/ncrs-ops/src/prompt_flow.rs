//! What the open prompt shows while its operation runs and after it failed.

use std::path::Path;

use iced::Task;
use ncrs_fs::CreateDirError;
use ncrs_i18n::{Language, Msg};

use crate::action::Action;
use crate::archive_flow::PackRequest;
use crate::ctx::FlowCtx;
use crate::delete_flow::{idle, Outcome};
use crate::message::OpsMessage;
use crate::prompt::{PromptKind, PromptProblem};
use crate::state::Ops;

impl Ops {
    /// Whether the create that reported is the one the prompt still waits for.
    /// A prompt dismissed or retried while the operation ran has another id.
    pub fn is_current_prompt_request(&self, request_id: u64) -> bool {
        request_id == self.prompt_request_id
    }

    /// Shows a validation failure in the open prompt.
    pub fn set_prompt_error(&mut self, problem: PromptProblem) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_validation_error(problem);
        }
    }

    /// Wording for a failed create, in the active language. Deciding here
    /// rather than in the filesystem layer keeps `fs` free of user-facing text,
    /// and the two failures need different advice: a taken name means pick
    /// another, a missing parent means the panel has gone stale.
    pub fn prompt_error(&self, lang: Language, err: &CreateDirError) -> String {
        use std::io::ErrorKind;
        let name = self.prompt_name();
        if matches!(err, CreateDirError::NameNotAllowed { .. }) {
            return lang
                .text(Msg::ErrorMkdirNameNotAllowed)
                .replace("{name}", &name);
        }
        match err.io_error().map(|e| e.kind()) {
            Some(ErrorKind::AlreadyExists) => {
                lang.text(Msg::ErrorMkdirTaken).replace("{name}", &name)
            }
            Some(ErrorKind::NotFound) => lang.text(Msg::ErrorParentMissing).to_string(),
            kind => {
                let reason = kind.map_or_else(
                    || err.to_string(),
                    |known| ncrs_ui_kit::io_reason::io_reason_text(known, &err.to_string(), lang),
                );
                lang.text(Msg::ErrorMkdirFailed)
                    .replace("{name}", &name)
                    .replace("{reason}", &reason)
            }
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
            (matches!(prompt.kind(), PromptKind::Pack | PromptKind::Paste) || prompt.is_pattern())
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
    /// prompt closes and what it held is the archive to write.
    pub fn take_pack_request(&mut self) -> Option<PackRequest> {
        let prompt = self.prompt.take()?;
        let target_side = prompt.side()?;
        Some(PackRequest {
            archive: prompt.archive_path(),
            sources: prompt.sources().to_vec(),
            keep_under: prompt.kept_folders_under().map(Path::to_path_buf),
            target_side,
        })
    }
}

/// Enter in the name prompt of a new folder or file: checks the name and
/// starts the create. The reload of the panels waits for the create's result:
/// run beside it, a read that finishes first would not see the new entry.
pub fn submit_create_prompt(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    let Some(prompt) = ops.prompt.as_ref() else {
        return idle();
    };
    if let Err(reason) = prompt.validate() {
        ops.set_prompt_error(reason);
        return idle();
    }
    let kind = prompt.kind();
    let Some(side) = prompt.side() else {
        return idle();
    };
    let target = ctx.folder_of(side).join(prompt.name().trim());
    let request_id = ops.prompt_request_id.wrapping_add(1);
    ops.prompt_request_id = request_id;
    ops.set_prompt_busy(true);
    ops.pending_create = Some(target.clone());
    let finished = move |result| OpsMessage::PromptFinished {
        kind,
        request_id,
        result,
    };
    let task = match kind {
        PromptKind::CreateDir => Task::perform(ncrs_fs::create_dir(target), finished),
        PromptKind::CreateFile => Task::perform(ncrs_fs::create_file(target), finished),
        // A pack and a paste have their own jobs.
        PromptKind::Pack
        | PromptKind::Paste
        | PromptKind::TagByPattern
        | PromptKind::UntagByPattern => Task::none(),
    };
    (task, Vec::new())
}

/// The create of the name prompt has reported: closes the prompt or shows why
/// it failed, and reads the panels again. On failure the read still runs: the
/// name may belong to something else that appeared meanwhile.
pub fn prompt_finished(
    ops: &mut Ops,
    kind: PromptKind,
    request_id: u64,
    result: Result<(), CreateDirError>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if !ops.is_current_prompt_request(request_id) {
        return idle();
    }
    let created = ops.pending_create.take();
    let mut actions = Vec::new();
    match result {
        Ok(()) => {
            ops.prompt = None;
            if let Some(path) = created.as_ref().filter(|_| kind == PromptKind::CreateFile) {
                actions.push(Action::EditFile(path.clone()));
            }
        }
        Err(err) => {
            ops.set_prompt_busy(false);
            let text = ops.prompt_error(ctx.lang, &err);
            ops.set_prompt_error_text(&text);
        }
    }
    if let Some((folder, name)) = created.as_deref().and_then(|path| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some((path.parent()?.to_path_buf(), name))
    }) {
        actions.push(Action::ReloadPanelsSelecting { folder, name });
    }
    (Task::none(), actions)
}
