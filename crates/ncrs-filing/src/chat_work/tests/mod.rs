use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::NaiveDate;
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_index::{ContentHash, DocumentFields, Index, KindCatalog, Transfer};
use ncrs_intake::{
    hash_path, location_structure, ConditionProblem, PdfUnavailable, PlainText, PlanFile,
    RuleProblem, TextError,
};
use tempfile::TempDir;

use super::*;

mod continuous;
mod diagnosis;
mod eval;
mod groups;
mod live;
mod model_scope;
mod picks;
mod reading;
mod recent_runs;
mod rules_snapshot;
mod runs;
mod trial;

struct Setup {
    dir: TempDir,
    access: ChatAccess,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let trash = JournaledTrash::new(dir.path().join("trash.toml"), FolderBackend(bin));
        let access = ChatAccess::new(
            Arc::new(Mutex::new(Some(index))),
            Arc::new(TextChain::new().with_source(PlainText)),
            Arc::new(trash),
        );
        Self { dir, access }
    }

    fn write(&self, relative: &str, content: &str) -> PathBuf {
        // Joined part by part: a `/` kept inside a Windows path would fall
        // outside the index's subtree range, which separates with `\`.
        let path = relative
            .split('/')
            .fold(self.dir.path().to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn read(&self, folder: &Path) -> Result<Arc<FolderReading>, ReadError> {
        self.read_with_progress(folder, Arc::default())
    }

    fn read_with_progress(
        &self,
        folder: &Path,
        progress: Arc<ReadProgress>,
    ) -> Result<Arc<FolderReading>, ReadError> {
        match self.access.run(ChatWork::Read {
            folder: folder.to_path_buf(),
            progress,
        }) {
            ChatResult::Read(result) => result,
            other => Err(ReadError::Io(format!("{other:?}"))),
        }
    }
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn plan_file(path: &str, seed: u8, sender: Option<&str>, when: NaiveDate) -> PlanFile {
    PlanFile {
        relative_path: PathBuf::from(path),
        hash: ContentHash([seed; 32]),
        fields: DocumentFields {
            sender: sender.map(str::to_owned),
            kind: Some("invoice".to_owned()),
            date: Some(when),
            ..DocumentFields::default()
        },
        text: None,
        recorded_at: i64::from(seed),
    }
}

fn reading(root: &str, mut files: Vec<PlanFile>) -> FolderReading {
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let structure = location_structure(&files, &KindCatalog::built_in());
    let mut hashes = HashMap::new();
    for file in &files {
        hashes
            .entry(file.hash)
            .or_insert_with(|| file.relative_path.clone());
    }
    FolderReading {
        ai: Default::default(),
        kinds: Default::default(),
        root: PathBuf::from(root),
        files,
        photos: 0,
        without_text: 0,
        structure,
        duplicates: Vec::new(),
        hashes,
    }
}
