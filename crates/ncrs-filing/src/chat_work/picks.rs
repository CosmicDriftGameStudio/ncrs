//! The model's folder picks for a whole read folder, made ahead of the chat.

use std::path::Path;

use ncrs_ai::ChatModel;

use super::{
    reading::read_folder, trial::trial, AnsweredGroups, ChatAccess, FolderReading, ModelScope,
    ReadProgress,
};

/// Has the model pick a folder below `target` for every document of `source`
/// and stores the picks in the index, so a trial later finds them. Ends when
/// nothing is left to ask; a round that left as many files as the one before
/// is an error (the model failed or was closed).
pub fn remember_folder_picks(
    access: &ChatAccess,
    source: &Path,
    target: &Path,
    model: &dyn ChatModel,
) -> Result<(), String> {
    let progress = ReadProgress::default();
    let source = read_folder(access, source, &progress).map_err(|err| format!("{err:?}"))?;
    let target = read_folder(access, target, &progress).map_err(|err| format!("{err:?}"))?;
    remember_from_readings(access, &source, &target, model)
}

// ponytail: it goes from the end because the chat's rest pages go from the
// start, so both never ask the same files. Each round re-runs the whole
// trial, which is cheap against a model call.
pub(super) fn remember_from_readings(
    access: &ChatAccess,
    source: &FolderReading,
    target: &FolderReading,
    model: &dyn ChatModel,
) -> Result<(), String> {
    let mut previous_left: Option<usize> = None;
    loop {
        let plan = trial(
            access,
            source,
            target,
            &AnsweredGroups::default(),
            None,
            Some(model),
            &ModelScope::LastPage,
        )?;
        let left = plan.model_left.len();
        if left == 0 {
            return Ok(());
        }
        if previous_left.is_some_and(|before| left >= before) {
            return Err("the model made no progress".to_owned());
        }
        previous_left = Some(left);
    }
}
