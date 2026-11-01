use crate::app::*;

/// What the snapshot of the search view shows: a plain file and a source file
/// with two hits.
#[cfg(test)]
pub(in crate::app::tests::support) fn fixed_search_results(
    root: &Path,
) -> Vec<(fs::FileEntry, Vec<crate::fs::search::LineMatch>)> {
    use crate::fs::search::LineMatch;
    let hit = |number: u64, text: &str, from: usize, to: usize| LineMatch {
        number,
        text: text.to_string(),
        ranges: std::iter::once(from..to).collect(),
    };
    vec![
        (
            fixed_result_entry(root.join("notes.txt"), 4_300, (2026, 9, 12, 16, 20)),
            vec![hit(3, "say hello to the world", 4, 9)],
        ),
        (
            fixed_result_entry(root.join("src/main.rs"), 18_432, (2026, 10, 2, 11, 5)),
            vec![
                hit(1, "// hello world example", 3, 8),
                hit(12, "println!(\"hello\");", 10, 15),
            ],
        ),
    ]
}

#[cfg(test)]
pub(in crate::app::tests::support) fn fixed_hit_scope() -> PathBuf {
    PathBuf::from("/Users/alex/Documents")
}

/// Paths below the scope, with fixed sizes and local-time dates, so the rows
/// print the same text in every time zone.
#[cfg(test)]
pub(in crate::app::tests::support) fn fixed_hit_entries() -> Vec<fs::FileEntry> {
    let scope = fixed_hit_scope();
    let hit = |relative: &str, size: u64, date: (i32, u32, u32, u32, u32)| {
        fixed_result_entry(scope.join(relative), size, date)
    };
    vec![
        hit("Telekom/2024/invoice.pdf", 184_320, (2024, 11, 4, 9, 12)),
        hit("Telekom/2025/invoice.pdf", 201_728, (2025, 11, 3, 8, 47)),
        hit("Telekom/contract-dsl.pdf", 1_153_433, (2023, 2, 17, 14, 5)),
        hit("Telekom-mobile-notes.txt", 2_048, (2026, 5, 6, 18, 44)),
    ]
}

#[cfg(test)]
pub(in crate::app::tests::support) fn fixed_hit_search_results(
) -> Vec<(fs::FileEntry, Vec<crate::fs::search::LineMatch>)> {
    use crate::fs::search::LineMatch;
    fixed_hit_entries()
        .into_iter()
        .map(|entry| {
            let line = LineMatch {
                number: 1,
                text: "Customer account with Telekom".to_owned(),
                ranges: std::iter::once(22..29).collect(),
            };
            (entry, vec![line])
        })
        .collect()
}

/// A search result with a fixed size and a local-time date, so the rows print
/// the same text in every time zone.
#[cfg(test)]
pub fn fixed_result_entry(
    path: PathBuf,
    size: u64,
    (year, month, day, hour, minute): (i32, u32, u32, u32, u32),
) -> fs::FileEntry {
    use chrono::{Local, TimeZone as _};
    fs::FileEntry {
        name: path.file_name().map(OsString::from).unwrap_or_default(),
        path,
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size,
        modified: Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .map(std::time::SystemTime::from),
        folder_size: None,
    }
}
