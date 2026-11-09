//! The batch rename: opening the dialog, reading what the tokens need in the
//! background, running the renames as one job and taking the last batch back.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use iced::widget::operation;
use iced::Task;
use ncrs_ai::{propose_name_spec, NameBlock, NameSpec};
use ncrs_core::{FileEntry, PanelSide};
use ncrs_fs as fs;
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::RemotePath;
use ncrs_i18n::{fill_template, Msg};
use ncrs_index::{Amount, SearchIndex};
use ncrs_jobs::{Job, JobKind};
use ncrs_ui_kit::focus::release_focus;

use super::pattern::{plan, split_name, EntryFacts, NameRules, Step};
use super::state::{
    digits_only, ApplyFailure, BatchRename, BatchRenameMessage, Extras, Opening, VISIBLE_ROWS,
};
use super::view::PATTERN_FIELD_ID;
use crate::action::{Action, StartJob};
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, idle, Outcome};
use crate::message::OpsMessage;
use crate::state::{BatchRun, Ops};
use crate::transfer_flow::child_path;

/// What the rename key found: the tagged entries of the active panel.
#[derive(Debug)]
pub struct BatchRenameRequest {
    pub side: PanelSide,
    pub folder: PathBuf,
    /// The tagged entries, in the order of the panel.
    pub entries: Vec<FileEntry>,
    /// Every name of the panel's folder.
    pub folder_names: Vec<String>,
}

fn rules_for(folder: &Path) -> NameRules {
    let on_server = RemotePath::parse(folder).is_some();
    NameRules {
        windows: cfg!(windows) && !on_server,
        fold_case: !on_server && cfg!(any(target_os = "macos", target_os = "windows")),
    }
}

fn facts_of(entry: &FileEntry) -> EntryFacts {
    let name = entry.name.to_string_lossy();
    let (stem, extension) = split_name(&name, entry.is_dir);
    EntryFacts {
        stem,
        extension,
        modified: entry
            .modified
            .map(|time| chrono::DateTime::<chrono::Local>::from(time).date_naive()),
        ..EntryFacts::default()
    }
}

fn send(message: BatchRenameMessage) -> OpsMessage {
    OpsMessage::BatchRename(message)
}

/// The rename key with two or more tagged entries. Something else holding the
/// keyboard or the queue means nothing happens.
pub fn open_batch_rename(ops: &mut Ops, request: BatchRenameRequest, ctx: &FlowCtx<'_>) -> Outcome {
    if ctx.start_blocked || ops.batch_rename.is_some() || request.entries.len() < 2 {
        return idle();
    }
    ops.batch_dialogs_opened = ops.batch_dialogs_opened.wrapping_add(1);
    let id = ops.batch_dialogs_opened;
    let on_server = RemotePath::parse(&request.folder).is_some();
    let paths: Vec<PathBuf> = request
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    let opening = Opening {
        id,
        side: request.side,
        rules: rules_for(&request.folder),
        entries: request
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), facts_of(entry)))
            .collect(),
        folder: request.folder,
        folder_names: request.folder_names,
    };
    let names = opening.folder_names.clone();
    ops.batch_rename = Some(BatchRename::new(opening));

    let mut tasks = vec![focus_pattern()];
    if !on_server {
        let index = ctx.index.clone();
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || read_extras(&paths, index.as_deref()))
                    .await
                    .unwrap_or_default()
            },
            move |extras| send(BatchRenameMessage::ExtrasLoaded { dialog: id, extras }),
        ));
    }
    if let Some(model) = ctx.model.clone() {
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let spec = propose_name_spec(model.as_ref(), &names).ok().flatten()?;
                    pattern_of(&spec)
                })
                .await
                .unwrap_or_default()
            },
            move |pattern| {
                send(BatchRenameMessage::Proposed {
                    dialog: id,
                    pattern,
                })
            },
        ));
    }
    (Task::batch(tasks), vec![Action::ClearStatus])
}

fn focus_pattern() -> Task<OpsMessage> {
    operation::focus(PATTERN_FIELD_ID).chain(operation::move_cursor_to_end(PATTERN_FIELD_ID))
}

/// The pattern a proposed name spec stands for: its blocks joined by the
/// separator, and the extension.
pub fn pattern_of(spec: &NameSpec) -> Option<String> {
    let blocks: Vec<&str> = spec
        .blocks
        .iter()
        .map(|block| match block {
            NameBlock::Year => "{year}",
            NameBlock::Month => "{month}",
            NameBlock::Day => "{day}",
            NameBlock::Sender => "{sender}",
            NameBlock::Kind => "{kind}",
        })
        .collect();
    (!blocks.is_empty())
        .then(|| format!("{}{{extension}}", blocks.join(&spec.separator.to_string())))
}

/// What the tokens need beyond the listing, for local `paths`: the day a
/// picture was taken and the fields the index has read.
fn read_extras(paths: &[PathBuf], index: Option<&dyn SearchIndex>) -> Vec<Extras> {
    let documents: HashMap<PathBuf, ncrs_index::DocumentFields> = index
        .and_then(|source| source.document_fields(paths).ok())
        .unwrap_or_default()
        .into_iter()
        .collect();
    paths
        .iter()
        .map(|path| {
            let fields = documents.get(path);
            Extras {
                taken: date_taken(path),
                sender: fields.and_then(|found| found.sender.clone()),
                kind: fields.and_then(|found| found.kind.clone()),
                document_date: fields.and_then(|found| found.date),
                amount: fields
                    .and_then(|found| found.amount.as_ref())
                    .map(amount_text),
            }
        })
        .collect()
}

pub fn amount_text(amount: &Amount) -> String {
    let sign = if amount.minor_units < 0 { "-" } else { "" };
    let minor = amount.minor_units.unsigned_abs();
    format!(
        "{sign}{}.{:02} {}",
        minor.div_euclid(100),
        minor.rem_euclid(100),
        amount.currency.as_str()
    )
}

fn date_taken(path: &Path) -> Option<NaiveDate> {
    let file = std::fs::File::open(path).ok()?;
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::BufReader::new(file))
        .ok()?;
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    let text = field.display_value().to_string();
    NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()
}

/// A message of the dialog, or the undo key.
pub fn update_batch_rename(
    ops: &mut Ops,
    message: BatchRenameMessage,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    match message {
        BatchRenameMessage::Undo => undo_last_batch(ops, ctx),
        BatchRenameMessage::Applied { generation, result } => applied(ops, generation, result, ctx),
        BatchRenameMessage::Cancel => {
            ops.batch_rename = None;
            (release_focus(), Vec::new())
        }
        BatchRenameMessage::Confirm => confirm(ops, ctx),
        other => match ops.batch_rename.as_mut() {
            Some(dialog) => edit(dialog, other),
            None => idle(),
        },
    }
}

fn edit(dialog: &mut BatchRename, message: BatchRenameMessage) -> Outcome {
    let mut task = Task::none();
    match message {
        BatchRenameMessage::PatternInput(text) => dialog.pattern = text,
        BatchRenameMessage::StartInput(text) => dialog.start = digits_only(&text),
        BatchRenameMessage::DigitsInput(text) => dialog.digits = digits_only(&text),
        BatchRenameMessage::FindInput(text) => dialog.find = text,
        BatchRenameMessage::ReplaceInput(text) => dialog.replace = text,
        BatchRenameMessage::Insert(token) => {
            dialog.pattern.push_str(token.text());
            task = focus_pattern();
        }
        BatchRenameMessage::SetCase(case) => dialog.case = case,
        BatchRenameMessage::ToggleRegex => dialog.regex = !dialog.regex,
        BatchRenameMessage::UseHermes => {
            if let Some(pattern) = dialog.suggestion.clone() {
                dialog.pattern = pattern;
                task = focus_pattern();
            }
        }
        BatchRenameMessage::Scrolled(delta) => {
            let mut unused_cursor = 0;
            dialog.window.scroll(
                delta,
                dialog.preview.rows.len(),
                VISIBLE_ROWS,
                &mut unused_cursor,
            );
            return idle();
        }
        BatchRenameMessage::ExtrasLoaded { dialog: id, extras } => {
            if id == dialog.id {
                dialog.set_extras(extras);
            }
            return idle();
        }
        BatchRenameMessage::Proposed {
            dialog: id,
            pattern,
        } => {
            if id == dialog.id {
                dialog.suggestion = pattern;
            }
            return idle();
        }
        BatchRenameMessage::Confirm
        | BatchRenameMessage::Cancel
        | BatchRenameMessage::Applied { .. }
        | BatchRenameMessage::Undo => return idle(),
    }
    dialog.refresh();
    (task, Vec::new())
}

/// Enter or the button: run the renames, unless a clash is open.
fn confirm(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    if ops
        .batch_rename
        .as_ref()
        .is_some_and(|dialog| !dialog.preview.can_apply())
    {
        return idle();
    }
    let Some(dialog) = ops.batch_rename.take() else {
        return idle();
    };
    let pairs = dialog.preview.pairs();
    if pairs.is_empty() {
        return (release_focus(), Vec::new());
    }
    let steps = plan(&pairs, &dialog.folder_names, dialog.rules);
    let done: Vec<(PathBuf, PathBuf)> = pairs
        .iter()
        .map(|(old, new)| {
            (
                child_path(&dialog.folder, OsStr::new(old)),
                child_path(&dialog.folder, OsStr::new(new)),
            )
        })
        .collect();
    let run = BatchRun {
        pairs: done,
        undo: false,
        side: dialog.side,
    };
    let (task, asked) = start_run(ops, run, dialog.folder, steps, ctx);
    (Task::batch([release_focus(), task]), asked)
}

/// The undo key: the names of the last batch back.
fn undo_last_batch(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    if ctx.start_blocked || ops.batch_rename.is_some() || ops.last_batch.is_empty() {
        return idle();
    }
    let Some(folder) = ops
        .last_batch
        .first()
        .and_then(|(_, new)| new.parent())
        .map(Path::to_path_buf)
    else {
        return idle();
    };
    let name_of = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let back: Vec<(String, String)> = ops
        .last_batch
        .iter()
        .map(|(old, new)| (name_of(new), name_of(old)))
        .collect();
    let steps = plan(&back, &[], rules_for(&folder));
    let run = BatchRun {
        pairs: ops.last_batch.clone(),
        undo: true,
        side: ctx.source_side,
    };
    start_run(ops, run, folder, steps, ctx)
}

fn start_run(
    ops: &mut Ops,
    run: BatchRun,
    folder: PathBuf,
    steps: Vec<Step>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    ops.bump_generation();
    let generation = ops.generation;
    let job = Job {
        kind: JobKind::Rename,
        path: folder.clone(),
        total: Some(steps.len()),
    };
    let pool = Arc::clone(ctx.remote);
    let check = run.undo.then(|| run.pairs.clone());
    ops.batch_running = Some(run);
    let build = Box::new(move || {
        Task::perform(run_steps(pool, folder, steps, check), move |result| {
            send(BatchRenameMessage::Applied { generation, result })
        })
    });
    actions(vec![
        Action::ClearStatus,
        Action::StartJob(StartJob { job, build }),
    ])
}

/// Renames the entries step by step. A failure puts back what was done, so
/// the folder is as it was. With `check` (taking a batch back) the entries
/// are first looked for, local ones only: there is nothing to change when one
/// is gone or its old name is taken.
pub async fn run_steps(
    pool: Arc<Pool>,
    folder: PathBuf,
    steps: Vec<Step>,
    check: Option<Vec<(PathBuf, PathBuf)>>,
) -> Result<(), ApplyFailure> {
    if let Some(pairs) = check.filter(|_| RemotePath::parse(&folder).is_none()) {
        check_before_undo(&pairs)?;
    }
    let mut done: Vec<&Step> = Vec::new();
    for step in &steps {
        let from = child_path(&folder, OsStr::new(&step.from));
        if let Err(error) = fs::rename(Arc::clone(&pool), from, step.to.clone()).await {
            let rollback_failed = !put_back(&pool, &folder, &done).await;
            return Err(ApplyFailure::Failed {
                name: step.from.clone(),
                reason: error.to_string(),
                rollback_failed,
            });
        }
        done.push(step);
    }
    Ok(())
}

/// Whether every step could be taken back.
async fn put_back(pool: &Arc<Pool>, folder: &Path, done: &[&Step]) -> bool {
    let mut all = true;
    for step in done.iter().rev() {
        let from = child_path(folder, OsStr::new(&step.to));
        all &= fs::rename(Arc::clone(pool), from, step.from.clone())
            .await
            .is_ok();
    }
    all
}

fn check_before_undo(pairs: &[(PathBuf, PathBuf)]) -> Result<(), ApplyFailure> {
    let name_of = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    for (old, new) in pairs {
        if std::fs::symlink_metadata(new).is_err() {
            return Err(ApplyFailure::Gone(name_of(new)));
        }
        let held_by_batch = pairs.iter().any(|(_, other)| other == old);
        if !held_by_batch && std::fs::symlink_metadata(old).is_ok() {
            return Err(ApplyFailure::Taken(name_of(old)));
        }
    }
    Ok(())
}

fn applied(
    ops: &mut Ops,
    generation: u64,
    result: Result<(), ApplyFailure>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    let Some(run) = ops.batch_running.take() else {
        return idle();
    };
    ops.bump_generation();
    let mut asked = vec![Action::FinishJob, Action::ClearJobProgress];
    let count = run.pairs.len().to_string();
    match result {
        Ok(()) if run.undo => {
            ops.last_batch.clear();
            asked.push(Action::ShowNotice(fill_template(
                ctx.lang.text(Msg::BatchRenameUndone),
                &[("{count}", &count)],
            )));
        }
        Ok(()) => {
            ops.last_batch = run.pairs;
            asked.push(Action::ShowNotice(fill_template(
                ctx.lang.text(Msg::BatchRenameDone),
                &[("{count}", &count)],
            )));
        }
        Err(failure) => asked.push(Action::ShowError(failure_text(&failure, ctx))),
    }
    asked.extend([
        Action::ClearSelection(run.side),
        Action::ReloadPanels,
        Action::RestartSearch,
    ]);
    actions(asked)
}

/// The job was aborted: what was renamed stays, the panels are read again.
pub fn abort_batch_rename(ops: &mut Ops) -> Option<Outcome> {
    ops.batch_running.take()?;
    ops.bump_generation();
    Some(actions(vec![Action::ReloadPanels, Action::RestartSearch]))
}

pub fn failure_text(failure: &ApplyFailure, ctx: &FlowCtx<'_>) -> String {
    match failure {
        ApplyFailure::Gone(name) => {
            fill_template(ctx.lang.text(Msg::BatchRenameUndoGone), &[("{name}", name)])
        }
        ApplyFailure::Taken(name) => fill_template(
            ctx.lang.text(Msg::BatchRenameUndoTaken),
            &[("{name}", name)],
        ),
        ApplyFailure::Failed {
            name,
            reason,
            rollback_failed,
        } => fill_template(
            ctx.lang.text(if *rollback_failed {
                Msg::BatchRenameRollbackFailed
            } else {
                Msg::BatchRenameFailed
            }),
            &[("{name}", name), ("{reason}", reason)],
        ),
    }
}
