use rusqlite::params;

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{DocumentId, DocumentSource, DocumentStatus, HermesCounts, StatusCounts};

const WITHOUT_TEXT: &str = "(t.rowid IS NULL OR trim(t.body) = '')";

impl Index {
    /// Whether text was ever stored for the document, an empty one included.
    pub fn has_document_text_row(&self, id: DocumentId) -> Result<bool, IndexError> {
        let found = self.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM document_text WHERE rowid = ?1)",
            [id.get()],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    /// Counts for the display of the automatic filing. Documents that are
    /// missing are left out. Cheaper than `statistics`: no page statistics.
    pub fn hermes_counts(&self) -> Result<HermesCounts, IndexError> {
        let open_reviews_without_text = self.count(
            &format!(
                "SELECT COUNT(*) FROM review_entry r
                 LEFT JOIN document_text t ON t.rowid = r.document_id
                 WHERE r.resolved_at IS NULL AND {WITHOUT_TEXT}"
            ),
            [],
        )?;
        Ok(HermesCounts {
            inbox: self.status_counts(DocumentSource::Inbox)?,
            archive: self.status_counts(DocumentSource::Archive)?,
            open_reviews: self.count(
                "SELECT COUNT(*) FROM review_entry WHERE resolved_at IS NULL",
                [],
            )?,
            open_reviews_without_text,
            confirmed_rules: self.count("SELECT COUNT(*) FROM rule WHERE confirmed = 1", [])?,
            archive_documents: self.count(
                "SELECT COUNT(*) FROM document WHERE source = 'archive' AND missing_since IS NULL",
                [],
            )?,
            archive_documents_with_text: self.count(
                &format!(
                    "SELECT COUNT(*) FROM document d
                     LEFT JOIN document_text t ON t.rowid = d.id
                     WHERE d.source = 'archive' AND d.missing_since IS NULL
                       AND NOT {WITHOUT_TEXT}"
                ),
                [],
            )?,
        })
    }

    fn status_counts(&self, source: DocumentSource) -> Result<StatusCounts, IndexError> {
        let of_status = |status: DocumentStatus| {
            self.count(
                "SELECT COUNT(*) FROM document
                 WHERE source = ?1 AND status = ?2 AND missing_since IS NULL",
                params![source.as_str(), status.as_str()],
            )
        };
        Ok(StatusCounts {
            new: of_status(DocumentStatus::New)?,
            planned: of_status(DocumentStatus::Planned)?,
            in_review: of_status(DocumentStatus::InReview)?,
            filed: of_status(DocumentStatus::Filed)?,
        })
    }

    fn count(&self, sql: &str, parameters: impl rusqlite::Params) -> Result<u64, IndexError> {
        Ok(self
            .connection()
            .query_row(sql, parameters, |row| row.get::<_, i64>(0))?
            .unsigned_abs())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::ReviewReason;
    use crate::test_support::{new_document, plain_index, rule_spec};
    use crate::NewDocument;

    fn archive_document(seed: u8) -> NewDocument {
        NewDocument {
            path: PathBuf::from(format!("archive/doc-{seed}.pdf")),
            source: DocumentSource::Archive,
            ..new_document(seed)
        }
    }

    #[test]
    fn a_text_row_counts_even_when_the_text_is_empty() {
        let (_dir, mut index) = plain_index();
        let none = index.insert_document(&new_document(1)).unwrap();
        let empty = index.insert_document(&new_document(2)).unwrap();
        index.set_document_text(empty, "").unwrap();
        assert!(!index.has_document_text_row(none).unwrap());
        assert!(index.has_document_text_row(empty).unwrap());
    }

    #[test]
    fn an_empty_index_counts_nothing() {
        let (_dir, index) = plain_index();
        assert_eq!(index.hermes_counts().unwrap(), HermesCounts::default());
    }

    #[test]
    fn the_counts_separate_inbox_and_archive_and_text_states() {
        let (_dir, mut index) = plain_index();
        let waiting = index.insert_document(&new_document(1)).unwrap();
        let textless = index.insert_document(&new_document(2)).unwrap();
        let read = index.insert_document(&new_document(3)).unwrap();
        index.set_document_text(read, "Invoice").unwrap();
        index
            .add_review(waiting, ReviewReason::NoMatchingRule)
            .unwrap();
        index
            .add_review(textless, ReviewReason::NoMatchingRule)
            .unwrap();
        index.set_document_text(textless, "").unwrap();
        let filed = index.insert_document(&new_document(4)).unwrap();
        index
            .set_document_status(filed, DocumentStatus::Filed)
            .unwrap();
        index
            .add_review(read, ReviewReason::NoMatchingRule)
            .unwrap();

        let archived_text = index.insert_document(&archive_document(5)).unwrap();
        index.set_document_text(archived_text, "Rent").unwrap();
        let archived_empty = index.insert_document(&archive_document(6)).unwrap();
        index.set_document_text(archived_empty, "").unwrap();
        index.insert_document(&archive_document(7)).unwrap();
        let gone = index.insert_document(&archive_document(8)).unwrap();
        index.mark_document_missing(gone).unwrap();

        index
            .create_rule(&crate::RuleSpec {
                confirmed: true,
                ..rule_spec()
            })
            .unwrap();
        index.create_rule(&rule_spec()).unwrap();

        let counts = index.hermes_counts().unwrap();
        assert_eq!(
            counts.inbox,
            StatusCounts {
                new: 0,
                planned: 0,
                in_review: 3,
                filed: 1
            }
        );
        assert_eq!(
            counts.archive,
            StatusCounts {
                new: 3,
                ..StatusCounts::default()
            }
        );
        assert_eq!(counts.open_reviews, 3);
        assert_eq!(counts.open_reviews_without_text, 2);
        assert_eq!(counts.confirmed_rules, 1);
        assert_eq!(counts.archive_documents, 3);
        assert_eq!(counts.archive_documents_with_text, 1);
    }
}
