//! Tax contexts as an index lists them, for tests: the entries cannot be
//! built without one.
#![allow(clippy::panic)]

use ncrs_index::{Index, TaxContextEntry};

const SEEDED: [&str; 2] = ["company", "private"];

/// The seeded `company` and `private`, then the given (name, short name)
/// contexts, read back from a real index.
pub fn stored_contexts(extra: &[(&str, &str)]) -> Vec<TaxContextEntry> {
    let dir = tempfile::TempDir::new().unwrap_or_else(|err| panic!("{err}"));
    let index =
        Index::create_plain(&dir.path().join("index.db")).unwrap_or_else(|err| panic!("{err}"));
    for (name, short_name) in extra {
        index
            .connection()
            .execute(
                "INSERT INTO tax_context (name, short_name) VALUES (?1, ?2)",
                [name, short_name],
            )
            .unwrap_or_else(|err| panic!("{err}"));
    }
    index.tax_contexts().unwrap_or_else(|err| panic!("{err}"))
}

/// Only the given contexts, without the seeded ones (a seeded one that is
/// given is kept).
pub fn only_contexts(short_names: &[&str]) -> Vec<TaxContextEntry> {
    let all = stored_contexts(
        &short_names
            .iter()
            .filter(|short_name| !SEEDED.contains(short_name))
            .map(|short_name| (*short_name, *short_name))
            .collect::<Vec<_>>(),
    );
    all.into_iter()
        .filter(|entry| short_names.contains(&entry.short_name.as_str()))
        .collect()
}
