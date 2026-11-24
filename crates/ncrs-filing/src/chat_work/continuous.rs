//! Filing new files by itself: the sure places of the filing tab are carried
//! out as a run of their own, everything else stays for the tab. The model's
//! own pick counts as a sure place here too, since the run can be undone.

use std::path::{Path, PathBuf};

use ncrs_ai::ChatModel;
use ncrs_index::{DocumentStatus, Transfer};

use super::live::{plan_listed, run_plan, LiveFile, LivePlan, LiveRun, LiveState};
use super::trial::PlacedBy;
use super::{runs, ChatAccess};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContinuousReport {
    pub filed: usize,
    /// The listed files that were not filed: unsure, without a folder, gone
    /// or already in the target.
    pub left: usize,
}

fn is_sure_place(file: &LiveFile) -> bool {
    matches!(file.state, LiveState::Filed { unsure: false, .. }) || is_model_pick(file)
}

fn is_model_pick(file: &LiveFile) -> bool {
    matches!(file.state, LiveState::Filed { .. }) && file.placed_by == Some(PlacedBy::Model)
}

/// The documents under `roots` that the index holds as new and without an
/// open review, as (root, path): what a restart leaves lying there. Files
/// whose place was unsure are planned again once per app start.
pub fn open_in(access: &ChatAccess, roots: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    access.with_index(|index| {
        let mut found = Vec::new();
        for id in index
            .documents_with_status(DocumentStatus::New)
            .map_err(|err| err.to_string())?
        {
            let Some(document) = index.document(id).map_err(|err| err.to_string())? else {
                continue;
            };
            let Some(root) = roots.iter().find(|root| document.path.starts_with(root)) else {
                continue;
            };
            let reviewed = index
                .open_review_of(id)
                .map_err(|err| err.to_string())?
                .is_some();
            if !reviewed {
                found.push((root.clone(), document.path));
            }
        }
        Ok(found)
    })
}

/// Plans `files` (relative to `source`) like the filing tab does and files
/// the ones with a sure place into `target` as the run `run_name`, which the
/// history lists and can undo. Tags nothing.
pub fn file_new(
    access: &ChatAccess,
    source: &Path,
    target: &Path,
    files: &[PathBuf],
    transfer: Transfer,
    model: &dyn ChatModel,
    run_name: String,
) -> Result<ContinuousReport, String> {
    if files.is_empty() {
        return Ok(ContinuousReport::default());
    }
    let run = LiveRun::default();
    let Some(planned) = plan_listed(access, source, target, files, model, &run)? else {
        return Ok(ContinuousReport {
            filed: 0,
            left: files.len(),
        });
    };
    let sure = LivePlan {
        files: planned
            .files
            .iter()
            .filter(|file| is_sure_place(file))
            .cloned()
            .collect(),
        ..planned
    };
    if sure.files.is_empty() {
        return Ok(ContinuousReport {
            filed: 0,
            left: files.len(),
        });
    }
    let report = runs::file(access, &run_plan(&sure, transfer, run_name))?;
    Ok(ContinuousReport {
        filed: report.done,
        left: files.len().saturating_sub(report.done),
    })
}
