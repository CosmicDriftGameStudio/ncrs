//! Writes `index-v0.6.0.db`, an encrypted index as release v0.6.0 makes it.
//!
//! Not compiled by cargo in the main repository. Run in an extract of the tag:
//!
//! ```text
//! git archive "v0.6.0^{commit}" | tar -x -C <dir>
//! cp make_index_v0_6_0.rs <dir>/crates/ncrs-index/examples/
//! cd <dir> && cargo run --locked -p ncrs-index --example make_index_v0_6_0 -- <out>.db
//! ```
//!
//! The key is the fixed test key bytes 0x00..0x1f, which the tests put in an
//! `InMemoryKeyStore`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, TimeZone as _, Utc};
use ncrs_index::{
    Amount, ContentHash, Currency, DocumentFields, DocumentSource, DocumentStatus, InMemoryKeyStore,
    Index, NewDocument, QuerySpec, ReviewReason, RuleSpec, SubtreePattern, TaxContext, TextMatch,
};

fn tags(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn document(seed: u8, path: &str, source: DocumentSource, day: u32) -> NewDocument {
    NewDocument {
        hash: ContentHash([seed; 32]),
        path: PathBuf::from(path),
        source,
        received_at: Utc.with_ymd_and_hms(2026, 1, day, 9, 30, 0).unwrap(),
        stamp: None,
    }
}

fn main() {
    let out = std::env::args().nth(1).expect("output path");
    let out = Path::new(&out);
    let key: Vec<u8> = (0u8..32).collect();
    let store = InMemoryKeyStore::with_entry(key);
    let (mut index, _recovery_key) = Index::create_encrypted(out, &store).unwrap();

    let invoice = index
        .insert_document(&document(
            1,
            "archive/2026/Company A/invoice-4711.pdf",
            DocumentSource::Archive,
            15,
        ))
        .unwrap();
    index
        .set_document_text(invoice, "Invoice 4711 from Company A for hosting services")
        .unwrap();
    index
        .set_document_fields(
            invoice,
            &DocumentFields {
                sender: Some("Company A".to_owned()),
                kind: Some("Invoice".to_owned()),
                tax_context: Some(TaxContext::Company),
                date: NaiveDate::from_ymd_opt(2026, 1, 15),
                number: Some("4711".to_owned()),
                amount: Some(Amount {
                    minor_units: 11900,
                    currency: Currency::new("EUR").unwrap(),
                }),
                tags: tags(&["services", "2026"]),
            },
        )
        .unwrap();
    index
        .set_document_status(invoice, DocumentStatus::Filed)
        .unwrap();

    let receipt = index
        .insert_document(&document(2, "inbox/scan-2.pdf", DocumentSource::Inbox, 16))
        .unwrap();
    index
        .set_document_text(receipt, "Receipt for an office chair, delivered to Berlin")
        .unwrap();
    index
        .set_document_status(receipt, DocumentStatus::Planned)
        .unwrap();

    let notice = index
        .insert_document(&document(3, "inbox/scan-3.pdf", DocumentSource::Inbox, 17))
        .unwrap();
    index
        .set_document_text(notice, "Tax notice for the year 2025 from the Finanzamt")
        .unwrap();
    index
        .set_document_status(notice, DocumentStatus::InReview)
        .unwrap();
    index
        .add_review(notice, ReviewReason::NoMatchingRule)
        .unwrap();

    index
        .create_rule(&RuleSpec {
            match_text: Some(TextMatch::Contains("Invoice".to_owned())),
            match_sender: Some("Company A".to_owned()),
            extract_number: Some(r"No\. (\d+)".to_owned()),
            extract_date: None,
            extract_amount: None,
            kind: "Invoice".to_owned(),
            tax_context: TaxContext::Company,
            tags: tags(&["services", "2026"]),
            filing_pattern: "{year}/{sender}".to_owned(),
            confirmed: true,
        })
        .unwrap();

    index
        .replace_location_profile(
            Path::new("archive"),
            &[SubtreePattern {
                subtree: "2026".to_owned(),
                pattern: Some("{year}/{sender}".to_owned()),
                documents: 1,
                total: 1,
            }],
        )
        .unwrap();

    index
        .save_query(
            "Invoices 2026",
            &QuerySpec {
                year: Some(2026),
                tax_context: Some(TaxContext::Company),
                kind: Some("Invoice".to_owned()),
                tags: tags(&["services"]),
                full_text: Some("hosting".to_owned()),
            },
        )
        .unwrap();
}
