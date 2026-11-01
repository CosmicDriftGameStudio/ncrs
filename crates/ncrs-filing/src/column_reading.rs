//! What one panel needs from the index about the files and folders it lists:
//! the request, the reading and the states of one folder. Plain data; the
//! Hermes column in the app keeps what comes back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ncrs_index::{Index, IndexError, PathState, StatusCounts};

/// What the index holds about the files directly in one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryStates {
    pub files: HashMap<PathBuf, PathState>,
    /// Excluded paths below the folder.
    pub excluded_below: Vec<PathBuf>,
    /// The folder itself is excluded, or one around it.
    pub excluded_itself: bool,
}

impl DirectoryStates {
    pub fn is_excluded(&self, path: &Path) -> bool {
        self.excluded_itself
            || self
                .excluded_below
                .iter()
                .any(|excluded| path.starts_with(excluded))
    }
}

/// How much of a folder's counts one reading holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnStage {
    /// The states of the listed files and the direct counts of the subfolders.
    Listing,
    /// The counts of everything below each subfolder; slower on a deep tree.
    Deep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRequest {
    /// The listing generation the request was made for; the answer is dropped
    /// when the panel has moved on.
    pub generation: u64,
    pub directory: PathBuf,
    pub subfolders: Vec<PathBuf>,
    pub stage: ColumnStage,
    /// [`HermesColumn::epoch`] when the request was made: a reading is dropped
    /// when something was invalidated since.
    pub epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderReading {
    pub path: PathBuf,
    pub excluded: bool,
    pub direct: Option<StatusCounts>,
    pub deep: Option<StatusCounts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnReading {
    pub generation: u64,
    pub epoch: u64,
    pub directory: PathBuf,
    pub stage: ColumnStage,
    /// Only in the `Listing` stage.
    pub states: Option<DirectoryStates>,
    pub folders: Vec<FolderReading>,
}

/// Everything one panel needs from the index, in one go.
pub fn read_column(index: &Index, request: &ColumnRequest) -> Result<ColumnReading, IndexError> {
    let (states, folders) = match request.stage {
        ColumnStage::Listing => (
            Some(read_directory(index, &request.directory)?),
            read_folders(index, &request.subfolders, false)?,
        ),
        ColumnStage::Deep => (None, read_folders(index, &request.subfolders, true)?),
    };
    Ok(ColumnReading {
        generation: request.generation,
        epoch: request.epoch,
        directory: request.directory.clone(),
        stage: request.stage,
        states,
        folders,
    })
}

fn read_directory(index: &Index, directory: &Path) -> Result<DirectoryStates, IndexError> {
    Ok(DirectoryStates {
        files: index
            .document_states_in(directory)?
            .into_iter()
            .map(|state| (state.path.clone(), state))
            .collect(),
        excluded_below: index.excluded_paths_under(directory)?,
        excluded_itself: index.is_excluded(directory)?,
    })
}

fn read_folders(
    index: &Index,
    folders: &[PathBuf],
    deep: bool,
) -> Result<Vec<FolderReading>, IndexError> {
    let (direct_stage, deep_stage) = if deep {
        let both = index.status_counts_under(folders)?;
        (
            vec![None; folders.len()],
            both.into_iter().map(|counts| Some(counts.deep)).collect(),
        )
    } else {
        (
            index
                .direct_status_counts_under(folders)?
                .into_iter()
                .map(Some)
                .collect(),
            vec![None; folders.len()],
        )
    };
    folders
        .iter()
        .zip(direct_stage)
        .zip(deep_stage)
        .map(|((path, direct_counts), deep_counts)| {
            Ok(FolderReading {
                path: path.clone(),
                excluded: index.is_excluded(path)?,
                direct: direct_counts,
                deep: deep_counts,
            })
        })
        .collect()
}

#[cfg(any(test, feature = "test-support"))]
impl DirectoryStates {
    pub fn sample(files: Vec<PathState>, excluded_below: Vec<PathBuf>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|state| (state.path.clone(), state))
                .collect(),
            excluded_below,
            excluded_itself: false,
        }
    }
}
