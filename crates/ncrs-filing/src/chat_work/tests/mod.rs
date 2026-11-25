use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::NaiveDate;
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_index::{BatchKind, ContentHash, DocumentFields, Index, Transfer};
use ncrs_intake::{hash_path, PdfUnavailable, PlainText, PlanFile, TextError};
use tempfile::TempDir;

use super::*;
use crate::execute::test_runs::execute_run;
use crate::execute::{trash_run, ExecutionItem, RunPlaces, TrashItem};

mod diagnosis;
mod reading;
mod recent_runs;
mod runs;
mod snapshots;

struct Setup {
    dir: TempDir,
    access: ChatAccess,
}

impl Setup {
    fn new() -> Self {
        Self::with_chain(TextChain::new().with_source(PlainText))
    }

    fn with_chain(chain: TextChain) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let trash = JournaledTrash::new(dir.path().join("trash.toml"), FolderBackend(bin));
        let access = ChatAccess::new(
            Arc::new(Mutex::new(Some(index))),
            Arc::new(chain),
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

    /// Carries files of `inbox` to their places below `archive`, as one run.
    fn file(&self, name: &str, transfer: Transfer, files: &[(&str, &str)]) -> BatchId {
        let items: Vec<_> = files
            .iter()
            .map(|(file, to)| self.item(&format!("inbox/{file}"), file, to))
            .collect();
        self.execute(name, transfer, "inbox", &items, false)
    }

    /// A file written as `from` that a run carries to `to` below the archive.
    fn item(&self, from: &str, content: &str, to: &str) -> ExecutionItem {
        let path = self.write(from, content);
        ExecutionItem {
            expected_hash: hash_path(&path).unwrap(),
            from: path,
            to: to.split('/').fold(self.archive(), |path, part| path.join(part)),
            document_id: None,
            duplicate: false,
        }
    }

    fn execute(
        &self,
        name: &str,
        transfer: Transfer,
        source: &str,
        items: &[ExecutionItem],
        inside_target: bool,
    ) -> BatchId {
        fs::create_dir_all(self.archive()).unwrap();
        let report = self
            .access
            .with_index(|index| {
                execute_run(
                    index,
                    BatchKind::Hermes,
                    name,
                    transfer,
                    RunPlaces {
                        source: &self.dir.path().join(source),
                        target: &self.archive(),
                        inside_target,
                    },
                    items,
                )
                .map_err(|err| err.to_string())
            })
            .unwrap();
        assert_eq!(report.skipped(), 0);
        report.batch
    }

    /// Trashes a copy of `archive/x.txt` that lies in `inbox`.
    fn trash_copy(&self) -> PathBuf {
        let keeper = self.write("archive/x.txt", "same");
        let copy = self.write("inbox/x.txt", "same");
        let item = TrashItem {
            path: copy.clone(),
            expected_hash: hash_path(&keeper).unwrap(),
            keeper,
        };
        let root = self.dir.path().join("inbox");
        let report = self
            .access
            .with_index(|index| {
                trash_run(index, "Duplicates", &root, &[item], &*self.access.trash)
                    .map_err(|err| err.to_string())
            })
            .unwrap();
        assert_eq!(report.trashed, 1);
        assert!(!copy.exists());
        copy
    }

    fn archive(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    fn last_run(&self) -> Option<RunSummary> {
        match self.access.run(ChatWork::LastRun) {
            ChatResult::LastRun(result) => result.unwrap(),
            other => panic!("{other:?}"),
        }
    }

    fn recent_runs(&self, limit: usize) -> Vec<RecentRun> {
        self.access
            .with_index(|index| runs_of_index(index, limit))
            .unwrap()
    }

    /// Puts a run back and says how many files came back.
    fn undo(&self, batch: BatchId) -> usize {
        match self.access.run(ChatWork::Undo { batch }) {
            ChatResult::Undone(result) => result.unwrap().undone,
            other => panic!("{other:?}"),
        }
    }
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}
