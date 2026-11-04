use rusqlite::{params, OptionalExtension as _};

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

    /// The version of the field detection that last read the document; 0 when
    /// none has.
    pub fn fields_read_version(&self, id: DocumentId) -> Result<u32, IndexError> {
        self.connection()
            .query_row(
                "SELECT fields_read_version FROM document WHERE id = ?1",
                [id.get()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(IndexError::DocumentNotFound(id))
    }

    pub fn set_fields_read_version(
        &mut self,
        id: DocumentId,
        version: u32,
    ) -> Result<(), IndexError> {
        let changed = self.connection().execute(
            "UPDATE document SET fields_read_version = ?2 WHERE id = ?1",
            params![id.get(), version],
        )?;
        if changed == 0 {
            return Err(IndexError::DocumentNotFound(id));
        }
        Ok(())
    }

    /// Whether the AI was asked for the fields of the document.
    pub fn ai_asked(&self, id: DocumentId) -> Result<bool, IndexError> {
        self.connection()
            .query_row(
                "SELECT ai_asked FROM document WHERE id = ?1",
                [id.get()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(IndexError::DocumentNotFound(id))
    }

    pub fn set_ai_asked(&mut self, id: DocumentId) -> Result<(), IndexError> {
        let changed = self
            .connection()
            .execute("UPDATE document SET ai_asked = 1 WHERE id = ?1", [id.get()])?;
        if changed == 0 {
            return Err(IndexError::DocumentNotFound(id));
        }
        Ok(())
    }

    /// Counts for the display of the automatic filing. Documents that are
    /// missing are left out. Cheaper than `statistics`: no page statistics.
    pub fn hermes_counts(&self) -> Result<HermesCounts, IndexError> {
        let open_reviews_without_text = self.count(
            &format!(
                "SELECT COUNT(*) FROM review_entry r
                 JOIN document d ON d.id = r.document_id
                 LEFT JOIN document_text t ON t.rowid = r.document_id
                 WHERE r.resolved_at IS NULL AND d.missing_since IS NULL
                   AND {WITHOUT_TEXT}"
            ),
            [],
        )?;
        Ok(HermesCounts {
            inbox: self.status_counts(DocumentSource::Inbox)?,
            archive: self.status_counts(DocumentSource::Archive)?,
            open_reviews: self.count(
                "SELECT COUNT(*) FROM review_entry r
                 JOIN document d ON d.id = r.document_id
                 WHERE r.resolved_at IS NULL AND d.missing_since IS NULL",
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

    /// How many archive documents the AI understood: they have a text, and a
    /// sender or kind is set, or it was asked without result.
    pub fn archive_documents_understood(&self) -> Result<u64, IndexError> {
        self.count(
            "SELECT COUNT(*) FROM document d JOIN document_text t ON t.rowid = d.id
             WHERE d.source = 'archive' AND d.missing_since IS NULL
               AND trim(t.body) <> ''
               AND (d.sender_id IS NOT NULL OR d.kind_id IS NOT NULL OR d.ai_asked = 1)",
            [],
        )
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
    use crate::test_support::{native, new_document, plain_index, rule_spec};
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
    fn the_ai_asked_flag_starts_unset_and_is_stored() {
        let (_dir, mut index) = plain_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        assert!(!index.ai_asked(id).unwrap());
        index.set_ai_asked(id).unwrap();
        assert!(index.ai_asked(id).unwrap());
        let missing = DocumentId::new(id.get() + 100);
        assert!(matches!(
            index.set_ai_asked(missing),
            Err(IndexError::DocumentNotFound(_))
        ));
    }

    #[test]
    fn ai_coverage_counts_documents_with_a_text_and_those_the_ai_was_asked_about() {
        let (_dir, mut index) = plain_index();
        let mut ids = Vec::new();
        for seed in 1..=3 {
            let document = NewDocument {
                path: native(&format!("/arch/doc-{seed}.pdf")),
                ..new_document(seed)
            };
            ids.push(index.insert_document(&document).unwrap());
        }
        index.set_document_text(ids[0], "Rechnung").unwrap();
        index.set_document_text(ids[1], "Brief").unwrap();
        index.set_document_text(ids[2], "  ").unwrap();
        index.set_ai_asked(ids[0]).unwrap();
        let folder = native("/arch");
        assert_eq!(index.ai_coverage_beneath(&folder).unwrap(), (2, 1));
        index.set_ai_asked(ids[1]).unwrap();
        assert_eq!(index.ai_coverage_beneath(&folder).unwrap(), (2, 2));
    }

    #[test]
    fn understood_archive_documents_have_a_text_and_were_asked_about() {
        let (_dir, mut index) = plain_index();
        let asked = index.insert_document(&archive_document(1)).unwrap();
        index.set_document_text(asked, "Rent").unwrap();
        index.set_ai_asked(asked).unwrap();
        let only_read = index.insert_document(&archive_document(2)).unwrap();
        index.set_document_text(only_read, "Letter").unwrap();
        let asked_without_text = index.insert_document(&archive_document(3)).unwrap();
        index.set_ai_asked(asked_without_text).unwrap();
        let inbox = index.insert_document(&new_document(4)).unwrap();
        index.set_document_text(inbox, "Bill").unwrap();
        index.set_ai_asked(inbox).unwrap();
        assert_eq!(index.archive_documents_understood().unwrap(), 1);
        assert_eq!(
            index.hermes_counts().unwrap().archive_documents_with_text,
            2
        );
    }

    #[test]
    fn the_fields_read_version_starts_at_zero_and_is_stored() {
        let (_dir, mut index) = plain_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        assert_eq!(index.fields_read_version(id).unwrap(), 0);
        index.set_fields_read_version(id, 3).unwrap();
        assert_eq!(index.fields_read_version(id).unwrap(), 3);
        let missing = DocumentId::new(id.get() + 100);
        assert!(matches!(
            index.fields_read_version(missing),
            Err(IndexError::DocumentNotFound(_))
        ));
    }

    #[test]
    fn an_empty_index_counts_nothing() {
        let (_dir, index) = plain_index();
        assert_eq!(index.hermes_counts().unwrap(), HermesCounts::default());
    }

    #[test]
    fn reviews_of_missing_documents_are_not_counted() {
        let (_dir, mut index) = plain_index();
        let gone = index.insert_document(&new_document(1)).unwrap();
        let here = index.insert_document(&new_document(2)).unwrap();
        index
            .add_review(gone, ReviewReason::NoMatchingRule)
            .unwrap();
        index
            .add_review(here, ReviewReason::NoMatchingRule)
            .unwrap();
        index.mark_document_missing(gone).unwrap();
        let counts = index.hermes_counts().unwrap();
        assert_eq!(counts.open_reviews, 1);
        assert_eq!(counts.open_reviews_without_text, 1);
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
