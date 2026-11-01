//! The filters of the search view live in `ncrs-search`; this keeps the old path.

pub use ncrs_search::filters::*;

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
pub mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono::NaiveDate;
    use ncrs_index::DocumentFields;

    use super::*;

    pub use ncrs_search::test_support::found_hit;

    fn search_for(words: &str) -> SearchFilters {
        let mut search = SearchFilters::new(Arc::default());
        search.ask(words);
        search
    }

    /// Saving what the page shows as a report finds the same documents.
    #[test]
    fn a_saved_search_finds_the_documents_the_page_showed() {
        use std::sync::{Arc, Mutex};

        use chrono::{TimeZone as _, Utc};
        use ncrs_index::{ContentHash, DocumentSource, Index, NewDocument};

        use crate::query_work::QueryAccess;

        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        type Stock<'a> = (&'a str, i32, &'a str, &'a str, &'a [&'a str], &'a str);
        let stock: [Stock; 7] = [
            (
                "a",
                2025,
                "Firma A",
                "invoice",
                &["office", "paid"],
                "rent for the office",
            ),
            (
                "b",
                2025,
                "firma a ",
                "Invoice",
                &["Office", "paid"],
                "office rent march",
            ),
            (
                "c",
                2025,
                "Firma A",
                "invoice",
                &["office"],
                "rent without the other tag",
            ),
            (
                "d",
                2024,
                "Firma A",
                "invoice",
                &["office", "paid"],
                "rent of another year",
            ),
            (
                "e",
                2025,
                "Agency B",
                "invoice",
                &["office", "paid"],
                "rent of another sender",
            ),
            (
                "f",
                2025,
                "Firma A",
                "letter",
                &["office", "paid"],
                "rent of another kind",
            ),
            (
                "g",
                2025,
                "Firma A",
                "invoice",
                &["office", "paid"],
                "insurance, no such word",
            ),
        ];
        for (seed, (name, year, sender, kind, tags, text)) in (1..).zip(stock) {
            let id = index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: PathBuf::from(format!("archive/{name}.pdf")),
                    source: DocumentSource::Archive,
                    received_at: Utc.with_ymd_and_hms(2026, 3, 14, 9, 30, 0).unwrap(),
                    stamp: None,
                })
                .unwrap();
            index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        sender: Some(sender.to_owned()),
                        kind: Some(kind.to_owned()),
                        date: NaiveDate::from_ymd_opt(year, 5, 1),
                        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                        ..DocumentFields::default()
                    },
                    |_| ncrs_index::FieldOrigin::Unknown,
                )
                .unwrap();
            index.set_document_text(id, text).unwrap();
        }
        let access = QueryAccess::new(Arc::new(Mutex::new(Some(index))));

        // The last word is a prefix while it is typed.
        let mut search = search_for("office ren");
        let (_, asked) = search.ask("office ren");
        search.ran(access.run(&asked).unwrap());
        search.toggle(FacetValue::Year(2025));
        search.toggle(FacetValue::Sender("Firma A".to_owned()));
        search.toggle(FacetValue::Kind("invoice".to_owned()));
        search.toggle(FacetValue::Tag("office".to_owned()));
        search.toggle(FacetValue::Tag("paid".to_owned()));
        let shown: BTreeSet<PathBuf> = search.shown().map(|hit| hit.path.clone()).collect();
        assert_eq!(
            shown,
            BTreeSet::from([
                PathBuf::from("archive/a.pdf"),
                PathBuf::from("archive/b.pdf")
            ])
        );

        let id = access.save(None, "Office rent", &search.spec()).unwrap();
        let saved = access.load().unwrap();
        let query = saved.iter().find(|query| query.id == id).unwrap();
        let rerun: BTreeSet<PathBuf> = access
            .run(&query.spec)
            .unwrap()
            .into_iter()
            .map(|hit| hit.path)
            .collect();

        assert_eq!(rerun, shown);
    }
}
