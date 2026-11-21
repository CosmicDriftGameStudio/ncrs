//! The model's folder picks for a whole read folder, made ahead of the chat.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ncrs_ai::{AiError, ChatModel};

use super::{
    reading::read_folder, trial::trial, AnsweredGroups, ChatAccess, FolderReading, ModelScope,
    PlanFailure, ReadProgress, TrialPlan,
};

/// What `left` of [`remember_folder_picks`] holds before the first round has
/// counted the files that still wait for the model.
pub const LEFT_UNKNOWN: usize = usize::MAX;

/// Has the model pick a folder below `target` for every document of `source`
/// and stores the picks in the index, so a trial later finds them. Ends when
/// nothing is left to ask; a round that left as many files as the one before
/// is an error (the model failed or was closed). `left` gets the number of
/// files still waiting for the model after every round.
pub fn remember_folder_picks(
    access: &ChatAccess,
    source_path: &Path,
    target_path: &Path,
    model: &dyn ChatModel,
    stop: &AtomicBool,
    left: &AtomicUsize,
) -> Result<(), String> {
    left.store(LEFT_UNKNOWN, Ordering::Relaxed);
    let progress = ReadProgress::default();
    let source = read_folder(access, source_path, &progress, Some(stop))
        .map_err(|err| format!("{err:?}"))?;
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    let target = read_folder(access, target_path, &progress, Some(stop))
        .map_err(|err| format!("{err:?}"))?;
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    remember_from_readings(access, &source, &target, model, Some(stop), left)
}

pub(in crate::chat_work) fn remember_from_readings(
    access: &ChatAccess,
    source: &FolderReading,
    target: &FolderReading,
    model: &dyn ChatModel,
    stop: Option<&AtomicBool>,
    left: &AtomicUsize,
) -> Result<(), String> {
    ask_until_done(
        access,
        source,
        target,
        model,
        &ModelScope::LastPage,
        stop,
        &mut |plan| left.store(plan.model_left.len(), Ordering::Relaxed),
    )
    .map_err(|failure| failure.to_string())
}

// ponytail: the background pass goes from the end because the chat's rest
// pages go from the start, so both never ask the same files. Each round
// re-runs the whole trial, which is cheap against a model call.
/// Asks the model round by round until no file waits for it. `on_plan` sees
/// the plan after every round; a set `stop` ends the loop before the next one.
pub(in crate::chat_work) fn ask_until_done(
    access: &ChatAccess,
    source: &FolderReading,
    target: &FolderReading,
    model: &dyn ChatModel,
    scope: &ModelScope,
    stop: Option<&AtomicBool>,
    on_plan: &mut dyn FnMut(&TrialPlan),
) -> Result<(), PlanFailure> {
    let mut previous_left: Option<usize> = None;
    loop {
        if stop.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Ok(());
        }
        let plan = trial(
            access,
            source,
            target,
            &AnsweredGroups::default(),
            None,
            Some(model),
            scope,
        )?;
        on_plan(&plan);
        match plan.model_failure {
            // A model that cannot choose folders is the same as no model: the
            // files it would have decided stay as they are.
            Some(AiError::Unsupported) => return Ok(()),
            Some(failure) => return Err(PlanFailure::Model(failure)),
            None => {}
        }
        let left = plan.model_left.len();
        if left == 0 {
            return Ok(());
        }
        if previous_left.is_some_and(|before| left >= before) {
            return Err(PlanFailure::Other("the model made no progress".to_owned()));
        }
        previous_left = Some(left);
    }
}
