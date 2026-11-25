//! The documents of a location as the index knows them, and the structure they
//! are laid out in. Reads only: the files of the location are not touched.

use std::path::{Path, PathBuf};

use ncrs_index::{Index, KindCatalog};

use crate::duplicate_copies::PlanFile;
use crate::error::IntakeError;
use crate::structure::{learn_structure, LocationStructure, StructureDocument};

/// The structure the documents of a location are laid out in, learned from
/// their fields as they are now.
pub fn location_structure(files: &[PlanFile], kinds: &KindCatalog) -> LocationStructure {
    let documents: Vec<StructureDocument> = files
        .iter()
        .map(|file| StructureDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
            sender: file.fields.sender.clone(),
            kind: file.fields.kind.clone(),
        })
        .collect();
    learn_structure(&documents, kinds)
}

/// Where releases 1.1.x moved the copies of a duplicate to, at the root of a
/// location. What lies there is not part of the archive: it is neither laid
/// out by a structure nor planned again.
const LEGACY_DUPLICATES_FOLDER: &str = "duplicates";

fn is_in_legacy_duplicates_folder(relative: &Path) -> bool {
    relative.starts_with(LEGACY_DUPLICATES_FOLDER) && relative.components().count() > 1
}

/// Every document below `root` with its fields and text, and each of its
/// known copies as a file of its own.
pub fn location_documents(index: &Index, root: &Path) -> Result<Vec<PlanFile>, IntakeError> {
    let mut files = Vec::new();
    for (id, path) in index.documents_under(root)? {
        let (Some(document), Ok(relative)) = (index.document(id)?, path.strip_prefix(root)) else {
            continue;
        };
        let text = index.document_text(id)?;
        let shared = |relative_path: PathBuf, recorded_at: i64| PlanFile {
            relative_path,
            hash: document.hash,
            fields: document.fields.clone(),
            text: text.clone(),
            recorded_at,
        };
        if !is_in_legacy_duplicates_folder(relative) {
            files.push(shared(
                relative.to_path_buf(),
                document.received_at.timestamp(),
            ));
        }
        for (copy, seen_at) in index.duplicate_copies(id)? {
            if let Ok(relative_copy) = copy.strip_prefix(root) {
                if !is_in_legacy_duplicates_folder(relative_copy) {
                    files.push(shared(relative_copy.to_path_buf(), seen_at.timestamp()));
                }
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{ContentHash, DocumentId, DocumentSource, FileStamp, NewDocument};
    use tempfile::TempDir;

    use super::*;

    const ROOT: &str = "/archive";

    fn stamp() -> FileStamp {
        FileStamp {
            size: 1,
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    fn add(index: &mut Index, seed: u8, relative: &str) -> DocumentId {
        index
            .insert_document(&NewDocument {
                hash: ContentHash([seed; 32]),
                path: Path::new(ROOT).join(relative),
                source: DocumentSource::Archive,
                received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: Some(stamp()),
            })
            .unwrap()
    }

    #[test]
    fn files_a_release_1_1_plan_moved_to_the_duplicates_folder_are_left_out() {
        let dir = TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let invoice = "Insurance/2024/2024-04-01_invoice.pdf";
        let first = add(&mut index, 1, invoice);
        for copy in [
            "duplicates/Insurance/2024/2024-04-01_invoice.pdf",
            "Insurance/copy.pdf",
        ] {
            index
                .record_duplicate_path(first, &Path::new(ROOT).join(copy), stamp())
                .unwrap();
        }
        add(&mut index, 2, "duplicates/Downloads/old.pdf");

        let files = location_documents(&index, Path::new(ROOT)).unwrap();

        let paths: Vec<PathBuf> = files.into_iter().map(|file| file.relative_path).collect();
        assert_eq!(
            paths,
            [PathBuf::from(invoice), PathBuf::from("Insurance/copy.pdf")]
        );
    }
}
