//! The inbox files whose content the archive already holds. They are no
//! documents (a content hash belongs to one document), so they are read from
//! the repeats the intake notes, and answered with the same journaled trash
//! run as a duplicate in a taken place.

use std::path::{Path, PathBuf};

use ncrs_index::{DocumentId, DocumentStatus, FileStamp};

use super::answers::NOT_TRASHED;
use super::{AcceptSummary, ReviewAccess};

/// An inbox file with the content of a filed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlreadyFiled {
    pub path: PathBuf,
    pub keeper: DocumentId,
    /// The file of the filed document.
    pub keeper_path: PathBuf,
}

fn unchanged(path: &Path, recorded: Option<FileStamp>) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    recorded
        == Some(FileStamp {
            size: meta.len(),
            mtime,
        })
}

impl ReviewAccess {
    /// The inbox files found to repeat a filed document that are still there,
    /// unchanged and not left alone.
    pub fn load_already_filed(&self) -> Result<Vec<AlreadyFiled>, String> {
        self.with_index(|index| {
            let mut found = Vec::new();
            for copy in index.filed_inbox_paths()? {
                if !unchanged(&copy.path, copy.stamp) || index.is_excluded(&copy.path)? {
                    continue;
                }
                let Some(keeper) = index.document(copy.document)? else {
                    continue;
                };
                if keeper.missing_since.is_some() || keeper.status != DocumentStatus::Filed {
                    continue;
                }
                found.push(AlreadyFiled {
                    path: copy.path,
                    keeper: copy.document,
                    keeper_path: keeper.path,
                });
            }
            Ok::<_, ncrs_index::IndexError>(found)
        })
    }

    /// The files go to the trash, the filed documents stay. Each file is
    /// checked again, so one that differs from its document stays.
    pub fn delete_already_filed(&self, items: &[AlreadyFiled]) -> AcceptSummary {
        self.trash_duplicates(
            items,
            |item, _kinds| self.duplicate_to_trash(&item.path, item.keeper_path.clone()),
            |item, summary| {
                if item.path.exists() {
                    summary.fail(NOT_TRASHED.to_owned());
                    return;
                }
                match self.with_index(|index| index.remove_filed_inbox_path(&item.path)) {
                    Ok(_) => summary.trashed += 1,
                    Err(detail) => summary.fail(detail),
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{DocumentSource, Index, NewDocument};
    use ncrs_intake::hash_path;
    use tempfile::TempDir;

    use super::*;

    const CONTENT: &str = "the same scan twice";

    struct Found {
        _dir: TempDir,
        access: ReviewAccess,
        copy: PathBuf,
        keeper_path: PathBuf,
        bin: PathBuf,
    }

    fn stamp_of(path: &Path) -> FileStamp {
        let meta = fs::metadata(path).unwrap();
        FileStamp {
            size: meta.len(),
            mtime: meta.modified().unwrap(),
        }
    }

    /// A filed archive document and an inbox file with its content, noted as
    /// a repeat the way the intake does.
    fn found() -> Found {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let archive = dir.path().join("archive");
        let bin = dir.path().join("bin");
        for folder in [&inbox, &archive, &bin] {
            fs::create_dir_all(folder).unwrap();
        }
        let keeper_path = archive.join("Invoice.pdf");
        let copy = inbox.join("Scan again.pdf");
        fs::write(&keeper_path, CONTENT).unwrap();
        fs::write(&copy, CONTENT).unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let keeper = index
            .insert_document(&NewDocument {
                hash: hash_path(&keeper_path).unwrap(),
                path: keeper_path.clone(),
                source: DocumentSource::Archive,
                received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: Some(stamp_of(&keeper_path)),
            })
            .unwrap();
        index.mark_document_filed(keeper).unwrap();
        index
            .set_document_status(keeper, DocumentStatus::Filed)
            .unwrap();
        index
            .record_filed_inbox_path(keeper, &copy, stamp_of(&copy))
            .unwrap();
        let trash: Arc<dyn ncrs_fs::Trash> = Arc::new(ncrs_fs::JournaledTrash::new(
            dir.path().join("trash.toml"),
            ncrs_fs::FolderBackend(bin.clone()),
        ));
        let access = ReviewAccess::new(Arc::new(Mutex::new(Some(index))), archive)
            .with_location_roots(vec![inbox])
            .with_trash(trash);
        Found {
            _dir: dir,
            access,
            copy,
            keeper_path,
            bin,
        }
    }

    #[test]
    fn a_repeat_in_the_inbox_is_a_question_with_the_archive_file_beside_it() {
        let found = found();
        let loaded = found.access.load_already_filed().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].path, found.copy);
        assert_eq!(loaded[0].keeper_path, found.keeper_path);
        assert!(
            found.access.load().unwrap().is_empty(),
            "no document review"
        );
    }

    #[test]
    fn a_changed_or_ignored_or_gone_repeat_is_no_question() {
        let found = found();
        fs::write(&found.copy, "edited meanwhile, so longer").unwrap();
        assert!(found.access.load_already_filed().unwrap().is_empty());

        let ignored = self::found();
        ignored
            .access
            .exclude(std::slice::from_ref(&ignored.copy))
            .unwrap();
        assert!(ignored.access.load_already_filed().unwrap().is_empty());

        let gone = self::found();
        fs::remove_file(&gone.copy).unwrap();
        assert!(gone.access.load_already_filed().unwrap().is_empty());
    }

    #[test]
    fn deleting_the_repeat_moves_it_to_the_trash_and_ends_the_question() {
        let found = found();
        let items = found.access.load_already_filed().unwrap();

        let summary = found.access.delete_already_filed(&items);

        assert_eq!((summary.trashed, summary.failed), (1, 0));
        assert!(!found.copy.exists());
        assert_eq!(fs::read_to_string(&found.keeper_path).unwrap(), CONTENT);
        assert_eq!(
            fs::read_to_string(found.bin.join("Scan again.pdf")).unwrap(),
            CONTENT
        );
        assert!(found.access.load_already_filed().unwrap().is_empty());
    }

    #[test]
    fn a_repeat_whose_content_differs_from_the_archive_file_stays() {
        let found = found();
        let items = found.access.load_already_filed().unwrap();
        fs::write(&found.keeper_path, "other content").unwrap();

        let summary = found.access.delete_already_filed(&items);

        assert_eq!((summary.trashed, summary.failed), (0, 1));
        assert!(found.copy.exists());
    }
}
