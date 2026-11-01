//! What other parts of the app read from the index without owning it.

use super::*;

/// What the status page shows about the index.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexFigures {
    pub path: PathBuf,
    pub file_bytes: Option<u64>,
    pub encrypted: bool,
    pub statistics: IndexStatistics,
}

/// Reads the figures of the index. Meant for a blocking thread: the intake may
/// hold the index for as long as it hashes a large file.
pub struct IndexFiguresReader {
    db_path: PathBuf,
    slot: IndexSlot,
}

impl fmt::Debug for IndexFiguresReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexFiguresReader")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl IndexFiguresReader {
    pub fn read(self) -> Result<IndexFigures, String> {
        let file_bytes = std::fs::metadata(&self.db_path)
            .ok()
            .map(|metadata| metadata.len());
        let (encrypted, statistics) = {
            let guard = lock(&self.slot);
            let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
            let statistics = index.statistics().map_err(|err| err.to_string())?;
            (index.is_encrypted(), statistics)
        };
        Ok(IndexFigures {
            path: self.db_path,
            file_bytes,
            encrypted,
            statistics,
        })
    }
}

/// Reads the counts of the automatic filing, on a blocking thread like
/// [`IndexFiguresReader`].
pub struct HermesCountsReader {
    slot: IndexSlot,
}

impl fmt::Debug for HermesCountsReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesCountsReader").finish_non_exhaustive()
    }
}

impl HermesCountsReader {
    pub fn read(self) -> Result<HermesCounts, String> {
        let guard = lock(&self.slot);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        index.hermes_counts().map_err(|err| err.to_string())
    }
}

/// Reads what one panel's Hermes column needs, on a blocking thread like
/// [`HermesCountsReader`].
pub struct ColumnReader {
    slot: IndexSlot,
}

impl fmt::Debug for ColumnReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColumnReader").finish_non_exhaustive()
    }
}

impl ColumnReader {
    pub fn read(self, request: &ColumnRequest) -> Result<ColumnReading, String> {
        let guard = lock(&self.slot);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        read_column(index, request).map_err(|err| err.to_string())
    }
}

pub use ncrs_core::{FoundDocument, TextSource};

/// The index does not store how a text was read, so this follows the file
/// type: a scanned PDF, which was read by OCR, counts as `Pdf`.
pub(in crate::filing) fn text_source_of_file(path: &Path) -> TextSource {
    if ncrs_intake::is_image_file(path) {
        TextSource::Ocr
    } else if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        TextSource::Pdf
    } else {
        TextSource::Text
    }
}

/// Looks documents up by their text, on a blocking thread like
/// [`ColumnReader`].
pub struct DocumentFinder {
    slot: IndexSlot,
}

impl fmt::Debug for DocumentFinder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentFinder").finish_non_exhaustive()
    }
}

impl DocumentFinder {
    /// The documents whose text has all `words`, the last one as a beginning,
    /// best match first. A document whose file is gone is left out.
    pub fn find(&self, words: &str, limit: usize) -> Result<Vec<FoundDocument>, String> {
        let guard = lock(&self.slot);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let ids = index
            .search_text_prefix(words, limit)
            .map_err(|err| err.to_string())?;
        let mut found = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(document) = index.document(id).map_err(|err| err.to_string())? else {
                continue;
            };
            if document.missing_since.is_none() {
                found.push(FoundDocument {
                    read_by: text_source_of_file(&document.path),
                    path: document.path,
                });
            }
        }
        Ok(found)
    }
}

impl FilingModule {
    /// Only looks at the phase and never at the index, so it is safe on the UI
    /// thread; `read` then runs elsewhere.
    pub fn index_figures_reader(&self) -> Option<IndexFiguresReader> {
        match self.phase {
            Phase::Ready => Some(IndexFiguresReader {
                db_path: self.db_path.clone(),
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// Like [`Self::index_figures_reader`]: looks at the phase only.
    pub fn hermes_counts_reader(&self) -> Option<HermesCountsReader> {
        match self.phase {
            Phase::Ready => Some(HermesCountsReader {
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// Like [`Self::index_figures_reader`]: looks at the phase only.
    pub fn column_reader(&self) -> Option<ColumnReader> {
        match self.phase {
            Phase::Ready => Some(ColumnReader {
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }

    /// Like [`Self::column_reader`]: looks at the phase only.
    pub fn document_finder(&self) -> Option<DocumentFinder> {
        match self.phase {
            Phase::Ready => Some(DocumentFinder {
                slot: Arc::clone(&self.index),
            }),
            _ => None,
        }
    }
}
