//! Hits to build the filters and the search view from, for the tests here and
//! the snapshot tests of the app.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use chrono::NaiveDate;
use ncrs_index::{Amount, Currency, DocumentFields, QueryHit};

pub fn found_hit(
    name: &str,
    date: Option<(i32, u32, u32)>,
    sender: Option<&str>,
    kind: Option<&str>,
    tags: &[&str],
) -> QueryHit {
    QueryHit {
        path: PathBuf::from(format!("archive/{name}")),
        fields: DocumentFields {
            sender: sender.map(str::to_owned),
            kind: kind.map(str::to_owned),
            date: date.and_then(|(year, month, day)| NaiveDate::from_ymd_opt(year, month, day)),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            amount: Some(Amount {
                minor_units: 12_345,
                currency: Currency::new("EUR").unwrap(),
            }),
            ..DocumentFields::default()
        },
        missing: false,
        stamp: None,
    }
}

/// Six documents: years 2023 to 2025, two senders, two kinds, some tags.
pub fn six_hits() -> Vec<QueryHit> {
    vec![
        found_hit(
            "a.pdf",
            Some((2025, 3, 1)),
            Some("Firma A"),
            Some("invoice"),
            &["open"],
        ),
        found_hit(
            "b.pdf",
            Some((2025, 6, 1)),
            Some("firma a"),
            Some("Invoice"),
            &["paid", "office"],
        ),
        found_hit(
            "c.pdf",
            Some((2024, 5, 1)),
            Some("Firma A"),
            Some("contract"),
            &["office"],
        ),
        found_hit(
            "d.pdf",
            Some((2024, 8, 1)),
            Some("Agency B"),
            Some("invoice"),
            &["paid"],
        ),
        found_hit(
            "e.pdf",
            Some((2023, 1, 1)),
            Some("Agency B"),
            Some("letter"),
            &[],
        ),
        found_hit("f.pdf", None, None, None, &[]),
    ]
}
