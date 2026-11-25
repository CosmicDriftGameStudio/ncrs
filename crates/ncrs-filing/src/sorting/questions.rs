//! The questions the watched sources leave. They live in the index; which
//! source one belongs to is told from where its file lies.

use std::path::{Path, PathBuf};

use ncrs_fs::search::PathFilter;
use ncrs_fs::transfer::Progress;
use ncrs_index::{DocumentId, ReviewReason, TargetId};
use ncrs_intake::SortSource;

use super::access::{text, SortingAccess};
use super::plan_run::{
    filter_of, plan_watched, source_root, PlanFiles, PlanRequest, RowPlace, RowState,
};
use super::read_unknown::ReadUnknown;
use super::run::{close_question, run, RunEntry, RunError, RunRequest, SortReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub document: DocumentId,
    pub path: PathBuf,
    pub kind: QuestionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionKind {
    NoText,
    /// The places the rules name now; none when no rule names one any more.
    SeveralTargets(Vec<RowPlace>),
}

/// The open questions about files that are still in the source. A source that
/// cannot be reached has none for now.
pub fn open_questions(
    access: &SortingAccess,
    source: &SortSource,
) -> Result<Vec<Question>, String> {
    let open = asked(access, source)?;
    let several: Vec<PathBuf> = open
        .iter()
        .filter(|found| found.reason == ReviewReason::SeveralTargets)
        .map(|found| found.path.clone())
        .collect();
    let rows = if several.is_empty() {
        Vec::new()
    } else {
        let request = PlanRequest {
            source: source.clone(),
            files: PlanFiles::Listed(several),
            read: ReadUnknown::No,
        };
        plan_watched(access, &request).map_err(text)?.rows
    };
    let places_of = |path: &Path| match rows.iter().find(|row| row.path == path) {
        Some(row) => match &row.state {
            RowState::Several(places) => places.clone(),
            RowState::Place(place) => vec![place.clone()],
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    Ok(open
        .into_iter()
        .map(|found| Question {
            kind: if found.reason == ReviewReason::NoText {
                QuestionKind::NoText
            } else {
                QuestionKind::SeveralTargets(places_of(&found.path))
            },
            document: found.document,
            path: found.path,
        })
        .collect())
}

/// How many `open_questions` there are; decides nothing anew.
pub fn question_count(access: &SortingAccess, source: &SortSource) -> Result<usize, String> {
    Ok(asked(access, source)?.len())
}

/// The file stays where it is and is not asked about again.
pub fn leave(access: &SortingAccess, document: DocumentId) -> Result<(), String> {
    access.with_index(|index| close_question(index, document))
}

/// Files the document of a question into `places`; the question is closed only
/// when every place worked. `name` is what the history calls the run.
pub fn answer<P: Progress>(
    access: &SortingAccess,
    source: &SortSource,
    document: DocumentId,
    places: Vec<(TargetId, PathBuf)>,
    name: &str,
    progress: &mut P,
) -> Result<SortReport, RunError> {
    let found = access
        .with_index(|index| index.document(document).map_err(text))
        .map_err(RunError::Index)?
        .ok_or_else(|| RunError::Index("the document is gone".to_owned()))?;
    let request = RunRequest {
        name: name.to_owned(),
        source: source.clone(),
        entries: vec![RunEntry {
            path: found.path,
            hash: found.hash,
            document: Some(document),
            places,
        }],
        transfer: source.transfer,
    };
    run(access, &request, progress)
}

struct Asked {
    document: DocumentId,
    path: PathBuf,
    reason: ReviewReason,
}

fn asked(access: &SortingAccess, source: &SortSource) -> Result<Vec<Asked>, String> {
    let Ok(root) = source_root(source) else {
        return Ok(Vec::new());
    };
    let filter = filter_of(source, &root)?;
    let nested = access.targets_below(&root);
    let mut open = access.with_index(|index| {
        let mut open = Vec::new();
        for entry in index.open_reviews().map_err(text)? {
            if !matches!(
                entry.reason,
                ReviewReason::NoText | ReviewReason::SeveralTargets
            ) {
                continue;
            }
            if let Some(document) = index.document(entry.document_id).map_err(text)? {
                open.push(Asked {
                    document: document.id,
                    path: document.path,
                    reason: entry.reason,
                });
            }
        }
        Ok(open)
    })?;
    open.retain(|found| belongs(&filter, &nested, &found.path));
    Ok(open)
}

fn belongs(filter: &PathFilter, nested: &[PathBuf], path: &Path) -> bool {
    filter.admits_file(path)
        && !nested.iter().any(|folder| path.starts_with(folder))
        && path.is_file()
}
