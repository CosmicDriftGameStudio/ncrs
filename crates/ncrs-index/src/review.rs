use chrono::Utc;
use rusqlite::{params, OptionalExtension as _};

use crate::documents::require_document;
use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    timestamp_from_column, DocumentId, DocumentStatus, Field, ReviewEntry, ReviewEntryId,
    ReviewReason,
};

impl Index {
    /// Moves the document to `InReview` in the same transaction.
    pub fn add_review(
        &mut self,
        document: DocumentId,
        why: ReviewReason,
    ) -> Result<ReviewEntryId, IndexError> {
        let tx = self.conn_mut().transaction()?;
        require_document(&tx, document)?;
        let already_open = tx
            .query_row(
                "SELECT id FROM review_entry WHERE document_id = ?1 AND resolved_at IS NULL",
                [document.get()],
                |row| row.get(0).map(ReviewEntryId::new),
            )
            .optional()?;
        if let Some(open) = already_open {
            return Err(IndexError::ReviewAlreadyOpen(open));
        }
        let (reason, field) = why.as_columns();
        tx.execute(
            "INSERT INTO review_entry (document_id, reason, field, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                document.get(),
                reason,
                field.map(Field::as_str),
                Utc::now().timestamp(),
            ],
        )?;
        let id = ReviewEntryId::new(tx.last_insert_rowid());
        tx.execute(
            "UPDATE document SET status = ?2 WHERE id = ?1",
            params![document.get(), DocumentStatus::InReview.as_str()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// The document's status is left to the caller.
    pub fn resolve_review(&self, id: ReviewEntryId) -> Result<(), IndexError> {
        let changed = self.connection().execute(
            "UPDATE review_entry SET resolved_at = ?2 WHERE id = ?1 AND resolved_at IS NULL",
            params![id.get(), Utc::now().timestamp()],
        )?;
        if changed == 0 {
            return Err(IndexError::ReviewNotFound(id));
        }
        Ok(())
    }

    /// Oldest first.
    pub fn open_reviews(&self) -> Result<Vec<ReviewEntry>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT id, document_id, reason, field, created_at, resolved_at
             FROM review_entry WHERE resolved_at IS NULL ORDER BY created_at, id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(id, document_id, reason, field, created_at, resolved_at)| {
                    Ok(ReviewEntry {
                        id: ReviewEntryId::new(id),
                        document_id: DocumentId::new(document_id),
                        reason: ReviewReason::parse(&reason, field.as_deref())?,
                        created_at: timestamp_from_column("review_entry.created_at", created_at)?,
                        resolved_at: resolved_at
                            .map(|seconds| {
                                timestamp_from_column("review_entry.resolved_at", seconds)
                            })
                            .transpose()?,
                    })
                },
            )
            .collect()
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
    use super::*;
    use crate::test_support::{encrypted_index, new_document};

    #[test]
    fn review_opens_resolves_and_blocks_a_second_open_entry() {
        let (_dir, mut index) = encrypted_index();
        let document = index.insert_document(&new_document(1)).unwrap();

        let entry = index
            .add_review(document, ReviewReason::NoMatchingRule)
            .unwrap();

        assert_eq!(
            index.document(document).unwrap().unwrap().status,
            DocumentStatus::InReview
        );
        let open = index.open_reviews().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, entry);
        assert_eq!(open[0].document_id, document);
        assert_eq!(open[0].reason, ReviewReason::NoMatchingRule);
        assert_eq!(open[0].resolved_at, None);

        let err = index
            .add_review(document, ReviewReason::UnconfirmedRule)
            .unwrap_err();
        assert!(
            matches!(err, IndexError::ReviewAlreadyOpen(id) if id == entry),
            "{err}"
        );

        index.resolve_review(entry).unwrap();
        assert!(index.open_reviews().unwrap().is_empty());
        let resolved_twice = index.resolve_review(entry).unwrap_err();
        assert!(
            matches!(resolved_twice, IndexError::ReviewNotFound(id) if id == entry),
            "{resolved_twice}"
        );

        index
            .add_review(document, ReviewReason::UnconfirmedRule)
            .unwrap();
    }

    #[test]
    fn reasons_that_name_a_field_round_trip() {
        let (_dir, mut index) = encrypted_index();
        let first = index.insert_document(&new_document(1)).unwrap();
        let second = index.insert_document(&new_document(2)).unwrap();
        index
            .add_review(first, ReviewReason::MissingField(Field::Amount))
            .unwrap();
        index
            .add_review(second, ReviewReason::LowConfidence(Field::TaxContext))
            .unwrap();

        let reasons: Vec<_> = index
            .open_reviews()
            .unwrap()
            .into_iter()
            .map(|entry| entry.reason)
            .collect();

        assert_eq!(
            reasons,
            vec![
                ReviewReason::MissingField(Field::Amount),
                ReviewReason::LowConfidence(Field::TaxContext),
            ]
        );
    }

    #[test]
    fn review_for_an_unknown_document_is_refused() {
        let (_dir, mut index) = encrypted_index();

        let err = index
            .add_review(DocumentId::new(7), ReviewReason::NoMatchingRule)
            .unwrap_err();

        assert!(
            matches!(err, IndexError::DocumentNotFound(id) if id == DocumentId::new(7)),
            "{err}"
        );
    }

    #[test]
    fn resolving_an_unknown_entry_is_refused() {
        let (_dir, index) = encrypted_index();

        let err = index.resolve_review(ReviewEntryId::new(3)).unwrap_err();

        assert!(
            matches!(err, IndexError::ReviewNotFound(id) if id == ReviewEntryId::new(3)),
            "{err}"
        );
    }
}
