//! What the Hermes column of a file panel shows: the state of the files of one
//! folder, the counts of a folder and what the user excluded from filing.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR};

use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{tag_name, DocumentStatus, ReviewReason, StatusCounts};

/// The stored state of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathState {
    pub path: PathBuf,
    pub status: DocumentStatus,
    /// Why the file waits for a decision; set while its review is open.
    pub review: Option<ReviewReason>,
    pub tags: FileTags,
}

/// The tags of a file as the file list shows them, by where they come from.
/// Each is in the spelling of a tag; a field the document lacks is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTags {
    pub kind: Option<String>,
    pub recipient: Option<String>,
    pub sender: Option<String>,
    pub year: Option<String>,
    /// The tags the user set, sorted.
    pub own: Vec<String>,
}

/// The documents of a folder: those directly in it and those anywhere below.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderCounts {
    pub direct: StatusCounts,
    pub deep: StatusCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncludeOutcome {
    Included,
    NotExcluded,
    /// An enclosing folder is excluded, which this path cannot undo.
    ExcludedByAncestor(PathBuf),
}

/// The character after the separator, so that `[dir + SEP, dir + NEXT)` holds
/// exactly the paths below `dir`: `/Scan/x` is in, `/Scans/x` is not.
const fn after_separator(separator: char) -> char {
    match separator {
        '\\' => ']',
        _ => '0',
    }
}

struct SubtreeRange {
    from: String,
    until: String,
    /// Characters of the folder name with its separator: what the rest of a
    /// path starts after.
    prefix_chars: i64,
}

impl SubtreeRange {
    fn of(directory: &Path) -> Result<Self, IndexError> {
        let text = utf8(directory)?;
        let trimmed = text.trim_end_matches(MAIN_SEPARATOR);
        Ok(Self {
            from: format!("{trimmed}{MAIN_SEPARATOR}"),
            until: format!("{trimmed}{}", after_separator(MAIN_SEPARATOR)),
            prefix_chars: i64::try_from(trimmed.chars().count() + 1).unwrap_or(i64::MAX),
        })
    }
}

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

/// The stored spelling: components joined, so `a/b/` and `a/b` are one row.
fn stored(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

const COUNT_COLUMNS: &str = "COALESCE(SUM(status = 'new'), 0),
     COALESCE(SUM(status = 'planned'), 0),
     COALESCE(SUM(status = 'in_review'), 0),
     COALESCE(SUM(status = 'filed'), 0)";

const IS_DIRECT: &str = "instr(substr(path, ?3 + 1), char(?4)) = 0";

impl Index {
    pub fn exclude_path(&self, path: &Path) -> Result<(), IndexError> {
        let key = stored(path);
        self.connection().execute(
            "INSERT OR IGNORE INTO excluded_path (path) VALUES (?1)",
            [utf8(&key)?],
        )?;
        Ok(())
    }

    /// Removes the row of exactly this path. A path that is excluded only
    /// because a folder around it is stays excluded.
    pub fn include_path(&self, path: &Path) -> Result<IncludeOutcome, IndexError> {
        let key = stored(path);
        let removed = self
            .connection()
            .execute("DELETE FROM excluded_path WHERE path = ?1", [utf8(&key)?])?;
        if removed > 0 {
            return Ok(IncludeOutcome::Included);
        }
        Ok(match excluded_ancestor(self.connection(), &key)? {
            Some(ancestor) => IncludeOutcome::ExcludedByAncestor(ancestor),
            None => IncludeOutcome::NotExcluded,
        })
    }

    /// Whether the path or a folder around it is excluded.
    pub fn is_excluded(&self, path: &Path) -> Result<bool, IndexError> {
        let key = stored(path);
        let connection = self.connection();
        let exact = connection
            .query_row(
                "SELECT 1 FROM excluded_path WHERE path = ?1",
                [utf8(&key)?],
                |_| Ok(()),
            )
            .optional()?;
        if exact.is_some() {
            return Ok(true);
        }
        Ok(excluded_ancestor(connection, &key)?.is_some())
    }

    /// The excluded paths strictly below `directory`, sorted.
    pub fn excluded_paths_under(&self, directory: &Path) -> Result<Vec<PathBuf>, IndexError> {
        let range = SubtreeRange::of(directory)?;
        let mut statement = self.connection().prepare(
            "SELECT path FROM excluded_path WHERE path >= ?1 AND path < ?2 ORDER BY path",
        )?;
        let found = statement
            .query_map([&range.from, &range.until], |row| {
                row.get::<_, String>(0).map(PathBuf::from)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(found)
    }

    /// The documents, not missing, that lie directly in `directory`, with the
    /// reason of their open review and their tags. Sorted by path.
    pub fn document_states_in(&self, directory: &Path) -> Result<Vec<PathState>, IndexError> {
        let range = SubtreeRange::of(directory)?;
        let mut statement = self.connection().prepare(&format!(
            "SELECT d.path, d.status, r.reason, r.field,
                    k.name, d.recipient, s.name, substr(d.document_date, 1, 4)
             FROM document d
             LEFT JOIN sender s ON s.id = d.sender_id
             LEFT JOIN document_kind k ON k.id = d.kind_id
             LEFT JOIN review_entry r ON r.document_id = d.id AND r.resolved_at IS NULL
             WHERE d.missing_since IS NULL AND d.path >= ?1 AND d.path < ?2
               AND {}
             ORDER BY d.path",
            IS_DIRECT.replace("path", "d.path")
        ))?;
        let rows = statement
            .query_map(
                params![
                    range.from,
                    range.until,
                    range.prefix_chars,
                    u32::from(MAIN_SEPARATOR)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        [
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ],
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut own = self.own_tags_in(&range)?;
        rows.into_iter()
            .map(
                |(path, status, reason, field, [kind, recipient, sender, year])| {
                    let spelled = |name: Option<String>| {
                        name.map(|text| tag_name(&text))
                            .filter(|tag| !tag.is_empty())
                    };
                    Ok(PathState {
                        tags: FileTags {
                            kind: spelled(kind),
                            recipient: spelled(recipient),
                            sender: spelled(sender),
                            year: spelled(year),
                            own: own.remove(&path).unwrap_or_default(),
                        },
                        path: PathBuf::from(path),
                        status: DocumentStatus::parse(&status)?,
                        review: reason
                            .map(|named| ReviewReason::parse(&named, field.as_deref()))
                            .transpose()?,
                    })
                },
            )
            .collect()
    }

    /// The tags the user set, by path, for the documents directly in the
    /// folder: one query for all of them.
    fn own_tags_in(
        &self,
        range: &SubtreeRange,
    ) -> Result<HashMap<String, Vec<String>>, IndexError> {
        let mut statement = self.connection().prepare(&format!(
            "SELECT d.path, t.name
             FROM document d
             JOIN document_tag dt ON dt.document_id = d.id AND dt.automatic = 0
             JOIN tag t ON t.id = dt.tag_id
             WHERE d.missing_since IS NULL AND d.path >= ?1 AND d.path < ?2
               AND {}
             ORDER BY d.path, t.name",
            IS_DIRECT.replace("path", "d.path")
        ))?;
        let rows = statement
            .query_map(
                params![
                    range.from,
                    range.until,
                    range.prefix_chars,
                    u32::from(MAIN_SEPARATOR)
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut by_path: HashMap<String, Vec<String>> = HashMap::new();
        for (path, tag) in rows {
            by_path.entry(path).or_default().push(tag);
        }
        Ok(by_path)
    }

    /// Counts of the documents, not missing, for every folder, in the order
    /// given. Excluded documents are counted like the others.
    pub fn status_counts_under(
        &self,
        directories: &[PathBuf],
    ) -> Result<Vec<FolderCounts>, IndexError> {
        let direct = self.direct_status_counts_under(directories)?;
        let mut deep = self.connection().prepare(&format!(
            "SELECT {COUNT_COLUMNS} FROM document
             WHERE missing_since IS NULL AND path >= ?1 AND path < ?2"
        ))?;
        directories
            .iter()
            .zip(direct)
            .map(|(directory, direct_counts)| {
                let range = SubtreeRange::of(directory)?;
                Ok(FolderCounts {
                    direct: direct_counts,
                    deep: deep.query_row([&range.from, &range.until], read_counts)?,
                })
            })
            .collect()
    }

    /// The `direct` half of [`Self::status_counts_under`], which is the cheap
    /// one: only the files in the folder itself.
    pub fn direct_status_counts_under(
        &self,
        directories: &[PathBuf],
    ) -> Result<Vec<StatusCounts>, IndexError> {
        let mut direct = self.connection().prepare(&format!(
            "SELECT {COUNT_COLUMNS} FROM document
             WHERE missing_since IS NULL AND path >= ?1 AND path < ?2 AND {IS_DIRECT}"
        ))?;
        directories
            .iter()
            .map(|directory| {
                let range = SubtreeRange::of(directory)?;
                Ok(direct.query_row(
                    params![
                        range.from,
                        range.until,
                        range.prefix_chars,
                        u32::from(MAIN_SEPARATOR)
                    ],
                    read_counts,
                )?)
            })
            .collect()
    }
}

fn read_counts(row: &rusqlite::Row<'_>) -> rusqlite::Result<StatusCounts> {
    let column = |index| row.get::<_, i64>(index).map(i64::unsigned_abs);
    Ok(StatusCounts {
        new: column(0)?,
        planned: column(1)?,
        in_review: column(2)?,
        filed: column(3)?,
    })
}

/// The nearest excluded folder strictly around `path`.
fn excluded_ancestor(connection: &Connection, path: &Path) -> Result<Option<PathBuf>, IndexError> {
    let mut statement = connection.prepare("SELECT 1 FROM excluded_path WHERE path = ?1")?;
    for ancestor in path.ancestors().skip(1) {
        if ancestor.as_os_str().is_empty() {
            break;
        }
        if statement.exists([utf8(ancestor)?])? {
            return Ok(Some(ancestor.to_path_buf()));
        }
    }
    Ok(None)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    fn p(path: &str) -> PathBuf {
        PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::model::{DocumentId, Field, FileStamp};
    use crate::test_support::{new_document, plain_index};

    fn document_at(index: &Index, seed: u8, path: &str) -> DocumentId {
        let id = index.insert_document(&new_document(seed)).unwrap();
        index
            .set_document_path(
                id,
                &p(path),
                FileStamp {
                    size: 1,
                    mtime: UNIX_EPOCH,
                },
            )
            .unwrap();
        id
    }

    #[test]
    fn states_list_only_the_files_directly_in_the_folder_and_never_a_sibling_by_prefix() {
        let (_dir, index) = plain_index();
        document_at(&index, 1, "/d/Scan/a.pdf");
        document_at(&index, 2, "/d/Scans/b.pdf");
        document_at(&index, 3, "/d/Scan/deeper/c.pdf");
        let gone = document_at(&index, 4, "/d/Scan/gone.pdf");
        index.mark_document_missing(gone).unwrap();

        let states = index.document_states_in(&p("/d/Scan")).unwrap();

        assert_eq!(
            states.iter().map(|s| s.path.clone()).collect::<Vec<_>>(),
            vec![p("/d/Scan/a.pdf")]
        );
        assert_eq!(states[0].status, DocumentStatus::New);
        assert_eq!(states[0].review, None);
    }

    #[test]
    fn states_carry_the_tags_of_the_fields_and_the_own_ones() {
        let (_dir, mut index) = plain_index();
        let read = document_at(&index, 1, "/d/a.pdf");
        document_at(&index, 2, "/d/b.pdf");
        index
            .set_document_fields(
                read,
                &crate::model::DocumentFields {
                    sender: Some("Deutsche Telekom AG".to_owned()),
                    recipient: Some("Firma A GmbH".to_owned()),
                    kind: Some("Rechnung".to_owned()),
                    date: chrono::NaiveDate::from_ymd_opt(2026, 3, 9),
                    tags: ["Steuer".to_owned(), "büro".to_owned()].into(),
                    ..crate::model::DocumentFields::default()
                },
                |_| crate::model::FieldOrigin::Unknown,
            )
            .unwrap();

        let states = index.document_states_in(&p("/d")).unwrap();

        assert_eq!(
            states[0].tags,
            FileTags {
                kind: Some("invoice".to_owned()),
                recipient: Some("firma-a-gmbh".to_owned()),
                sender: Some("deutsche-telekom-ag".to_owned()),
                year: Some("2026".to_owned()),
                own: vec!["büro".to_owned(), "steuer".to_owned()],
            }
        );
        assert_eq!(states[1].tags, FileTags::default());
    }

    #[test]
    fn states_carry_the_open_review_reason_only() {
        let (_dir, mut index) = plain_index();
        let open = document_at(&index, 1, "/d/a.pdf");
        let closed = document_at(&index, 2, "/d/b.pdf");
        index
            .add_review(open, ReviewReason::MissingField(Field::Amount))
            .unwrap();
        let entry = index
            .add_review(closed, ReviewReason::TargetExists)
            .unwrap();
        index.resolve_review(entry).unwrap();

        let states = index.document_states_in(&p("/d/")).unwrap();

        assert_eq!(states[0].status, DocumentStatus::InReview);
        assert_eq!(
            states[0].review,
            Some(ReviewReason::MissingField(Field::Amount))
        );
        assert_eq!(states[1].review, None);
    }

    #[test]
    fn counts_separate_direct_from_deep_and_skip_siblings_and_missing_documents() {
        let (_dir, index) = plain_index();
        document_at(&index, 1, "/d/Scan/a.pdf");
        let filed = document_at(&index, 2, "/d/Scan/deeper/b.pdf");
        index
            .set_document_status(filed, DocumentStatus::Filed)
            .unwrap();
        document_at(&index, 3, "/d/Scans/c.pdf");
        let gone = document_at(&index, 4, "/d/Scan/gone.pdf");
        index.mark_document_missing(gone).unwrap();

        let counts = index
            .status_counts_under(&[p("/d/Scan"), p("/d/Scan/deeper"), p("/nowhere")])
            .unwrap();

        assert_eq!(counts[0].direct.new, 1);
        assert_eq!(counts[0].direct.filed, 0);
        assert_eq!(counts[0].deep.new, 1);
        assert_eq!(counts[0].deep.filed, 1);
        assert_eq!(counts[1].direct.filed, 1);
        assert_eq!(counts[1].deep.filed, 1);
        assert_eq!(counts[2], FolderCounts::default());
    }

    #[test]
    fn a_folder_with_a_trailing_separator_counts_the_same() {
        let (_dir, index) = plain_index();
        document_at(&index, 1, "/d/a.pdf");
        let counts = index.status_counts_under(&[p("/d/"), p("/")]).unwrap();
        assert_eq!(counts[0].direct.new, 1);
        assert_eq!(counts[1].deep.new, 1);
        assert_eq!(counts[1].direct.new, 0);
    }

    #[test]
    fn a_folder_is_excluded_with_everything_in_it_but_not_its_prefix_sibling() {
        let (_dir, index) = plain_index();
        index.exclude_path(&p("/d/Scan/")).unwrap();
        index.exclude_path(&p("/d/Scan")).unwrap();

        assert!(index.is_excluded(&p("/d/Scan")).unwrap());
        assert!(index.is_excluded(&p("/d/Scan/a/b.pdf")).unwrap());
        assert!(!index.is_excluded(&p("/d/Scans/b.pdf")).unwrap());
        assert!(!index.is_excluded(&p("/d")).unwrap());
    }

    #[test]
    fn including_removes_only_the_exact_row_and_reports_an_excluding_ancestor() {
        let (_dir, index) = plain_index();
        index.exclude_path(&p("/d/Scan")).unwrap();
        index.exclude_path(&p("/d/Scan/inner/x.pdf")).unwrap();

        assert_eq!(
            index.include_path(&p("/d/Scan/other")).unwrap(),
            IncludeOutcome::ExcludedByAncestor(p("/d/Scan"))
        );
        assert_eq!(
            index.include_path(&p("/elsewhere")).unwrap(),
            IncludeOutcome::NotExcluded
        );
        assert_eq!(
            index.include_path(&p("/d/Scan/inner/x.pdf")).unwrap(),
            IncludeOutcome::Included
        );
        assert!(index.is_excluded(&p("/d/Scan/inner/x.pdf")).unwrap());
        assert_eq!(
            index.include_path(&p("/d/Scan")).unwrap(),
            IncludeOutcome::Included
        );
        assert!(!index.is_excluded(&p("/d/Scan/inner/x.pdf")).unwrap());
    }

    #[test]
    fn excluded_paths_under_lists_the_ones_below_and_not_the_folder_itself() {
        let (_dir, index) = plain_index();
        for path in ["/d/Scan", "/d/Scan/a", "/d/Scan/b/c", "/d/Scans/x"] {
            index.exclude_path(Path::new(path)).unwrap();
        }

        let below = index.excluded_paths_under(&p("/d/Scan")).unwrap();

        assert_eq!(below, vec![p("/d/Scan/a"), p("/d/Scan/b/c")]);
    }
}
