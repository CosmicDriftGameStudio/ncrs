//! The work behind the saved queries: running one against the index, and
//! exporting what it found. It blocks, so `App` runs it on a blocking thread,
//! never in `update`.
//!
//! An export copies and never moves, overwrites or deletes: a name that is
//! taken in the target is skipped and counted. The values in the CSV come from
//! document text, so they are data, and a spreadsheet must not read them as a
//! formula.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::NaiveDateTime;
use ncrs_index::{Index, IndexError, QuerySpec, SavedQuery, SavedQueryId};

pub use ncrs_index::QueryHit;

use crate::fs::flat_names::target_name;
use crate::fs::transfer::{transfer_to, Copy, Counting, OnConflict};
use crate::review_work::amount_digits;
use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// What the export did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    pub copied: usize,
    pub skipped_existing: usize,
    pub skipped_missing: usize,
    pub csv_path: PathBuf,
}

/// What the saved queries may do with the index.
#[derive(Clone)]
pub struct QueryAccess {
    slot: IndexSlot,
}

impl QueryAccess {
    pub fn new(slot: IndexSlot) -> Self {
        Self { slot }
    }

    pub fn load(&self) -> Result<Vec<SavedQuery>, String> {
        self.with_index(|index| index.saved_queries())
    }

    /// A new query when `id` is `None`, else a change of that one. A name that
    /// another query has is an error.
    pub fn save(
        &self,
        id: Option<SavedQueryId>,
        name: &str,
        spec: &QuerySpec,
    ) -> Result<SavedQueryId, String> {
        self.with_index(|index| match id {
            Some(existing) => index
                .update_saved_query(existing, name, spec)
                .map(|()| existing),
            None => index.save_query(name, spec),
        })
    }

    pub fn delete(&self, id: SavedQueryId) -> Result<bool, String> {
        self.with_index(|index| index.delete_saved_query(id))
    }

    pub fn run(&self, spec: &QuerySpec) -> Result<Vec<QueryHit>, String> {
        self.run_inside(spec, None)
    }

    fn run_inside(&self, spec: &QuerySpec, under: Option<&Path>) -> Result<Vec<QueryHit>, String> {
        self.with_index(|index| {
            Ok(index
                .run_query_under(spec, under)?
                .into_iter()
                .map(|document| QueryHit {
                    missing: document.missing_since.is_some(),
                    stamp: document.stamp,
                    path: document.path,
                    fields: document.fields,
                })
                .collect())
        })
    }

    /// How many documents each of the queries finds.
    pub fn counts(&self, queries: &[SavedQuery]) -> Result<Vec<(SavedQueryId, usize)>, String> {
        self.with_index(|index| {
            queries
                .iter()
                .map(|query| Ok((query.id, index.run_query_under(&query.spec, None)?.len())))
                .collect()
        })
    }

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index).map_err(|err| err.to_string())
    }
}

impl ncrs_search::SearchIndex for QueryAccess {
    /// Like [`QueryAccess::run`], but only the documents at or below `under`.
    fn run_under(&self, spec: &QuerySpec, under: &Path) -> Result<Vec<QueryHit>, String> {
        self.run_inside(spec, Some(under))
    }

    /// The pieces of text around `words` of the documents at `paths`, marked
    /// as [`Index::text_snippets`] does. A path the index does not know or the
    /// words do not match has none.
    fn snippets(&self, paths: &[PathBuf], words: &str) -> Result<Vec<(PathBuf, String)>, String> {
        self.with_index(|index| {
            let mut ids = Vec::new();
            let mut paths_of = Vec::new();
            for path in paths {
                if let Some(document) = index.document_by_path(path)? {
                    ids.push(document.id);
                    paths_of.push((document.id, path.clone()));
                }
            }
            Ok(index
                .text_snippets(&ids, words)?
                .into_iter()
                .filter_map(|(id, snippet)| {
                    let (_, path) = paths_of.iter().find(|(known, _)| *known == id)?;
                    Some((path.clone(), snippet))
                })
                .collect())
        })
    }

    /// Overwrites the criteria of the query `id` and keeps its name.
    fn replace_spec(&self, id: SavedQueryId, spec: &QuerySpec) -> Result<(), String> {
        self.with_index(|index| {
            let saved = index
                .saved_queries()?
                .into_iter()
                .find(|query| query.id == id)
                .ok_or(IndexError::SavedQueryNotFound(id))?;
            index.update_saved_query(id, &saved.name, spec)
        })
    }
}

/// How many of the hits an export would copy: the ones that are not missing.
pub fn copies_of(hits: &[QueryHit]) -> usize {
    hits.iter().filter(|hit| !hit.missing).count()
}

pub fn csv_file_name(now: NaiveDateTime) -> String {
    format!("ncrs-export-{}.csv", now.format("%Y%m%d-%H%M%S"))
}

const EXPORTED: &str = "yes";
const SKIPPED_EXISTS: &str = "skipped-exists";
const SKIPPED_MISSING: &str = "skipped-missing";
const FAILED: &str = "failed";

const CSV_HEADER: [&str; 11] = [
    "file",
    "exported_as",
    "original_path",
    "sender",
    "kind",
    "date",
    "number",
    "amount",
    "currency",
    "tags",
    "exported",
];

/// Copies the files of `hits` into `target_dir` and writes the CSV beside
/// them. A file keeps its name; a later hit with a name an earlier hit of this
/// run took becomes `stem (2).ext`, `stem (3).ext`, and so on. Runs without the
/// index: the hits carry everything. Nothing in the target is overwritten, the
/// CSV included. The first copy that fails hard ends the run; the CSV then
/// lists the hits up to and including that one.
pub fn export(
    hits: &[QueryHit],
    target_dir: &Path,
    now: NaiveDateTime,
) -> Result<ExportSummary, String> {
    std::fs::create_dir_all(target_dir)
        .map_err(|err| format!("{}: {err}", target_dir.display()))?;
    let csv_path = target_dir.join(csv_file_name(now));
    if csv_path.exists() {
        return Err(format!("{} already exists", csv_path.display()));
    }
    let mut summary = ExportSummary {
        copied: 0,
        skipped_existing: 0,
        skipped_missing: 0,
        csv_path,
    };
    let mut rows = Vec::with_capacity(hits.len());
    let mut names_taken: HashSet<String> = HashSet::new();
    for hit in hits {
        let proposed_name = (!hit.missing)
            .then(|| target_name(&hit.path, &mut names_taken))
            .flatten();
        let outcome = match (&proposed_name, hit.missing) {
            (_, true) => Ok(Copied::Missing),
            (Some(name), false) => copy_hit(hit, &target_dir.join(name)),
            (None, false) => Err(format!("{} has no file name", hit.path.display())),
        };
        let exported_as = proposed_name.unwrap_or_default();
        match outcome {
            Ok(done) => {
                match done {
                    Copied::Yes => summary.copied += 1,
                    Copied::Exists => summary.skipped_existing += 1,
                    Copied::Missing => summary.skipped_missing += 1,
                    Copied::Failed => {}
                }
                rows.push(csv_row(hit, done, &exported_as));
            }
            Err(reason) => {
                rows.push(csv_row(hit, Copied::Failed, &exported_as));
                return Err(match write_csv(&summary.csv_path, &rows) {
                    Ok(()) => format!(
                        "{reason}; the CSV with the rows up to here is {}",
                        summary.csv_path.display()
                    ),
                    Err(csv_err) => format!("{reason}; the CSV could not be written: {csv_err}"),
                });
            }
        }
    }
    write_csv(&summary.csv_path, &rows)?;
    Ok(summary)
}

#[derive(Clone, Copy)]
enum Copied {
    Yes,
    Exists,
    Missing,
    Failed,
}

fn copy_hit(hit: &QueryHit, target: &Path) -> Result<Copied, String> {
    match transfer_to(&hit.path, target, &Copy, &mut Counting, OnConflict::Fail) {
        Ok(_) => Ok(Copied::Yes),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(Copied::Exists),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Copied::Missing),
        Err(err) => Err(format!("{}: {err}", hit.path.display())),
    }
}

fn csv_row(hit: &QueryHit, outcome: Copied, exported_as: &OsStr) -> Vec<String> {
    let fields = &hit.fields;
    let text = |value: &Option<String>| guarded(value.as_deref().unwrap_or_default());
    let tags = fields
        .tags
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(";");
    vec![
        guarded(
            &hit.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        match outcome {
            Copied::Missing => String::new(),
            _ => guarded(&exported_as.to_string_lossy()),
        },
        guarded(&hit.path.to_string_lossy()),
        text(&fields.sender),
        text(&fields.kind),
        fields.date.map(|date| date.to_string()).unwrap_or_default(),
        text(&fields.number),
        fields
            .amount
            .as_ref()
            .map(amount_digits)
            .unwrap_or_default(),
        fields
            .amount
            .as_ref()
            .map(|amount| amount.currency.as_str().to_owned())
            .unwrap_or_default(),
        guarded(&tags),
        match outcome {
            Copied::Yes => EXPORTED,
            Copied::Exists => SKIPPED_EXISTS,
            Copied::Missing => SKIPPED_MISSING,
            Copied::Failed => FAILED,
        }
        .to_owned(),
    ]
}

/// A cell that starts like a formula gets a quote in front, so a spreadsheet
/// shows it as text. The values here come from documents.
fn guarded(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}

/// RFC 4180: a cell with a comma, a quote or a line break is quoted, and a
/// quote inside is doubled. Lines end in CRLF.
fn csv_line<S: AsRef<str>>(cells: &[S]) -> String {
    let quoted = cells.iter().map(|raw_cell| {
        let cell = raw_cell.as_ref();
        if cell.contains([',', '"', '\r', '\n']) {
            format!("\"{}\"", cell.replace('"', "\"\""))
        } else {
            cell.to_owned()
        }
    });
    let mut line = quoted.collect::<Vec<_>>().join(",");
    line.push_str("\r\n");
    line
}

fn write_csv(path: &Path, rows: &[Vec<String>]) -> Result<(), String> {
    let mut text = csv_line(&CSV_HEADER);
    for row in rows {
        text.push_str(&csv_line(row));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|err| format!("{}: {err}", path.display()))
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::{NaiveDate, NaiveDateTime};
    use ncrs_index::{Amount, Currency, DocumentFields};
    use tempfile::TempDir;

    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2025, 1, 31)
            .unwrap()
            .and_hms_opt(8, 5, 9)
            .unwrap()
    }

    fn hit_in(dir: &Path, name: &str, content: &str) -> QueryHit {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        QueryHit {
            path,
            fields: DocumentFields {
                sender: Some("Company A".to_owned()),
                kind: Some("invoice".to_owned()),
                date: NaiveDate::from_ymd_opt(2024, 3, 9),
                recipient: None,
                number: Some("R-1".to_owned()),
                amount: Some(Amount {
                    minor_units: 1234,
                    currency: Currency::new("EUR").unwrap(),
                }),
                tags: BTreeSet::from(["office".to_owned(), "tax".to_owned()]),
            },
            missing: false,
            stamp: None,
        }
    }

    fn csv_of(summary: &ExportSummary) -> String {
        std::fs::read_to_string(&summary.csv_path).unwrap()
    }

    #[test]
    fn the_export_copies_the_files_and_writes_one_csv_line_per_document() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let hits = [
            hit_in(source.path(), "a/one.pdf", "one"),
            hit_in(source.path(), "b/two.pdf", "two"),
        ];

        let summary = export(&hits, target.path(), now()).unwrap();

        assert_eq!(
            (
                summary.copied,
                summary.skipped_existing,
                summary.skipped_missing
            ),
            (2, 0, 0)
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join("one.pdf")).unwrap(),
            "one"
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join("two.pdf")).unwrap(),
            "two"
        );
        assert!(hits[0].path.exists(), "the source stays");
        assert_eq!(
            summary.csv_path,
            target.path().join("ncrs-export-20250131-080509.csv")
        );
        let csv = csv_of(&summary);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "file,exported_as,original_path,sender,kind,date,number,amount,currency,tags,exported"
        );
        assert_eq!(lines.len(), 4, "header, two documents, the empty rest");
        assert!(lines[1].starts_with("one.pdf,one.pdf,"));
        assert!(lines[1].ends_with(",Company A,invoice,2024-03-09,R-1,12.34,EUR,office;tax,yes"));
    }

    #[test]
    fn a_name_taken_in_the_target_is_skipped_and_never_overwritten() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        std::fs::write(target.path().join("one.pdf"), "already here").unwrap();
        let hits = [
            hit_in(source.path(), "a/one.pdf", "first"),
            hit_in(source.path(), "b/two.pdf", "second"),
            hit_in(source.path(), "c/two.pdf", "same name as the one before"),
        ];

        let summary = export(&hits, target.path(), now()).unwrap();

        assert_eq!((summary.copied, summary.skipped_existing), (2, 1));
        assert_eq!(
            std::fs::read_to_string(target.path().join("one.pdf")).unwrap(),
            "already here"
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join("two.pdf")).unwrap(),
            "second"
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join("two (2).pdf")).unwrap(),
            "same name as the one before"
        );
        let csv = csv_of(&summary);
        assert_eq!(csv.matches(",skipped-exists\r\n").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_copy_error_stops_the_run_and_still_writes_the_csv() {
        use std::os::unix::fs::PermissionsExt as _;

        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let hits = [
            hit_in(source.path(), "a/one.pdf", "one"),
            hit_in(source.path(), "b/two.pdf", "two"),
            hit_in(source.path(), "c/three.pdf", "three"),
        ];
        let locked = hits[1].path.clone();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&locked).is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let result = export(&hits, target.path(), now());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

        let message = result.unwrap_err();
        let csv_path = target.path().join(csv_file_name(now()));
        assert!(message.contains("two.pdf"), "{message}");
        assert!(
            message.contains(&csv_path.display().to_string()),
            "{message}"
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join("one.pdf")).unwrap(),
            "one"
        );
        assert!(!target.path().join("three.pdf").exists());
        let csv = std::fs::read_to_string(&csv_path).unwrap();
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines.len(), 4, "header, two rows, the empty rest");
        assert!(lines[1].ends_with(",yes"));
        assert!(lines[2].ends_with(",failed"));
    }

    #[test]
    fn hits_with_one_file_name_get_numbered_names_within_a_run() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let hits = [
            hit_in(source.path(), "a/invoice.pdf", "first"),
            hit_in(source.path(), "b/invoice.pdf", "second"),
            hit_in(source.path(), "c/invoice.pdf", "third"),
            hit_in(source.path(), "a/notes", "n1"),
            hit_in(source.path(), "b/notes", "n2"),
            hit_in(source.path(), "d/Report.pdf", "upper"),
            hit_in(source.path(), "e/report.pdf", "lower"),
            hit_in(source.path(), "f/invoice (2).pdf", "already numbered"),
        ];

        let summary = export(&hits, target.path(), now()).unwrap();

        assert_eq!((summary.copied, summary.skipped_existing), (8, 0));
        for (name, content) in [
            ("invoice.pdf", "first"),
            ("invoice (2).pdf", "second"),
            ("invoice (3).pdf", "third"),
            ("notes", "n1"),
            ("notes (2)", "n2"),
            ("Report.pdf", "upper"),
            ("report (2).pdf", "lower"),
            ("invoice (2) (2).pdf", "already numbered"),
        ] {
            assert_eq!(
                std::fs::read_to_string(target.path().join(name)).unwrap(),
                content,
                "{name}"
            );
        }
        let csv = csv_of(&summary);
        assert!(csv.contains("invoice.pdf,invoice (2).pdf,"));
        assert!(csv.contains("invoice.pdf,invoice (3).pdf,"));

        let later = now() + chrono::Duration::seconds(1);
        let again = export(&hits, target.path(), later).unwrap();
        assert_eq!((again.copied, again.skipped_existing), (0, 8));
    }

    #[test]
    fn a_missing_document_is_skipped_and_counted() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let mut known_gone = hit_in(source.path(), "gone.pdf", "x");
        known_gone.missing = true;
        let vanished = hit_in(source.path(), "vanished.pdf", "x");
        std::fs::remove_file(&vanished.path).unwrap();

        let summary = export(&[known_gone, vanished], target.path(), now()).unwrap();

        assert_eq!((summary.copied, summary.skipped_missing), (0, 2));
        assert!(!target.path().join("gone.pdf").exists());
        assert_eq!(csv_of(&summary).matches(",skipped-missing\r\n").count(), 2);
    }

    #[test]
    fn an_existing_csv_is_not_overwritten() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let csv = target.path().join(csv_file_name(now()));
        std::fs::write(&csv, "mine").unwrap();

        let result = export(&[hit_in(source.path(), "a.pdf", "a")], target.path(), now());

        result.unwrap_err();
        assert_eq!(std::fs::read_to_string(&csv).unwrap(), "mine");
    }

    #[test]
    fn cells_are_quoted_by_rfc_4180() {
        assert_eq!(
            csv_line(&["plain", "a,b", "say \"hi\"", "two\nlines", ""]),
            "plain,\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\r\n"
        );
    }

    #[test]
    fn a_cell_that_looks_like_a_formula_is_defused() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let mut hit = hit_in(source.path(), "a.pdf", "a");
        hit.fields.sender = Some("=HYPERLINK(\"http://example.invalid\")".to_owned());
        hit.fields.number = Some("+1".to_owned());
        hit.fields.kind = Some("@SUM(A1)".to_owned());
        hit.fields.tags = BTreeSet::from(["-x".to_owned()]);

        let summary = export(&[hit], target.path(), now()).unwrap();
        let csv = csv_of(&summary);

        assert!(csv.contains("\"'=HYPERLINK(\"\"http://example.invalid\"\")\""));
        assert!(csv.contains(",'+1,"));
        assert!(csv.contains(",'@SUM(A1),"));
        assert!(csv.contains(",'-x,"));
        assert_eq!(guarded("\tx"), "'\tx");
        assert_eq!(guarded("fine = ok"), "fine = ok");
    }

    #[test]
    fn only_the_files_that_exist_count_as_copies() {
        let source = TempDir::new().unwrap();
        let mut gone = hit_in(source.path(), "gone.pdf", "x");
        gone.missing = true;

        assert_eq!(copies_of(&[hit_in(source.path(), "a.pdf", "a"), gone]), 1);
    }
}
