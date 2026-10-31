use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::{IndexError, TaxContextProblem};
use crate::index::Index;
use crate::model::{TaxContext, TaxContextEntry, TaxContextId, TaxContextUsage};

pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_SHORT_NAME_CHARS: usize = 40;

/// The row that stands for "no context"; it is never listed.
const NO_CONTEXT_ID: i64 = 0;

/// Case-insensitive on the short name. The sentinel's short name is not a
/// context a caller can name.
pub fn tax_context_id(conn: &Connection, context: &TaxContext) -> Result<i64, IndexError> {
    let TaxContext::Named(short_name) = context else {
        return Ok(NO_CONTEXT_ID);
    };
    conn.query_row(
        "SELECT id FROM tax_context WHERE short_name = ?1 AND id <> ?2",
        rusqlite::params![short_name, NO_CONTEXT_ID],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| IndexError::UnknownTaxContext(short_name.clone()))
}

pub fn optional_tax_context_id(
    conn: &Connection,
    context: Option<&TaxContext>,
) -> Result<Option<i64>, IndexError> {
    context.map(|named| tax_context_id(conn, named)).transpose()
}

fn refused(problem: TaxContextProblem) -> IndexError {
    IndexError::InvalidTaxContext(problem)
}

fn clean_name(raw: &str) -> Result<&str, IndexError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(refused(TaxContextProblem::NameEmpty));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(refused(TaxContextProblem::NameTooLong {
            max: MAX_NAME_CHARS,
        }));
    }
    if name.chars().any(char::is_control) {
        return Err(refused(TaxContextProblem::NameHasControlCharacter));
    }
    Ok(name)
}

fn is_short_name_character(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.' | ' ')
}

fn clean_short_name(raw: &str) -> Result<&str, IndexError> {
    let short_name = raw.trim();
    if short_name.is_empty() {
        return Err(refused(TaxContextProblem::ShortNameEmpty));
    }
    if short_name.chars().count() > MAX_SHORT_NAME_CHARS {
        return Err(refused(TaxContextProblem::ShortNameTooLong {
            max: MAX_SHORT_NAME_CHARS,
        }));
    }
    if let Some(forbidden) = short_name.chars().find(|ch| !is_short_name_character(*ch)) {
        return Err(refused(TaxContextProblem::ShortNameForbiddenCharacter(
            forbidden,
        )));
    }
    if short_name.starts_with('.') {
        return Err(refused(TaxContextProblem::ShortNameStartsWithDot));
    }
    if short_name.contains("..") {
        return Err(refused(TaxContextProblem::ShortNameHasDoubleDot));
    }
    if short_name.eq_ignore_ascii_case(TaxContext::NONE_SHORT_NAME) {
        return Err(refused(TaxContextProblem::ShortNameReserved));
    }
    Ok(short_name)
}

fn require_changeable(conn: &Connection, id: TaxContextId) -> Result<(), IndexError> {
    if id.get() == NO_CONTEXT_ID {
        return Err(refused(TaxContextProblem::NoContextIsFixed));
    }
    let exists = conn
        .query_row(
            "SELECT 1 FROM tax_context WHERE id = ?1",
            [id.get()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(IndexError::TaxContextNotFound(id))
    }
}

/// Compared in Rust because SQLite's NOCASE folds ASCII only.
fn require_free_names(
    conn: &Connection,
    name: &str,
    short_name: &str,
    own_id: Option<TaxContextId>,
) -> Result<(), IndexError> {
    let mut statement =
        conn.prepare("SELECT name, short_name FROM tax_context WHERE id <> ?1 AND id <> ?2")?;
    let others = statement
        .query_map(
            params![own_id.map_or(-1, TaxContextId::get), NO_CONTEXT_ID],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    if others.iter().any(|(other, _)| same(other, name)) {
        return Err(refused(TaxContextProblem::NameTaken));
    }
    if others.iter().any(|(_, other)| same(other, short_name)) {
        return Err(refused(TaxContextProblem::ShortNameTaken));
    }
    Ok(())
}

fn is_constraint_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn count(conn: &Connection, table: &str, id: TaxContextId) -> Result<usize, IndexError> {
    let found: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE tax_context_id = ?1"),
        [id.get()],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(found).unwrap_or(0))
}

impl Index {
    /// Both names are trimmed. The short name is what `{tax_context}` writes
    /// into filing paths, so it can never carry a path separator.
    pub fn create_tax_context(
        &mut self,
        raw_name: &str,
        raw_short_name: &str,
    ) -> Result<TaxContextEntry, IndexError> {
        let name = clean_name(raw_name)?;
        let short_name = clean_short_name(raw_short_name)?;
        let tx = self.conn_mut().transaction()?;
        require_free_names(&tx, name, short_name, None)?;
        tx.execute(
            "INSERT INTO tax_context (name, short_name) VALUES (?1, ?2)",
            params![name, short_name],
        )
        .map_err(|err| {
            if is_constraint_violation(&err) {
                refused(TaxContextProblem::ShortNameTaken)
            } else {
                err.into()
            }
        })?;
        let id = TaxContextId::new(tx.last_insert_rowid());
        tx.commit()?;
        Ok(TaxContextEntry {
            id,
            name: name.to_owned(),
            short_name: short_name.to_owned(),
        })
    }

    /// Documents, rules and saved queries follow by id, so paths planned
    /// afterwards use the new short name.
    pub fn rename_tax_context(
        &mut self,
        id: TaxContextId,
        raw_name: &str,
        raw_short_name: &str,
    ) -> Result<(), IndexError> {
        let name = clean_name(raw_name)?;
        let short_name = clean_short_name(raw_short_name)?;
        let tx = self.conn_mut().transaction()?;
        require_changeable(&tx, id)?;
        require_free_names(&tx, name, short_name, Some(id))?;
        tx.execute(
            "UPDATE tax_context SET name = ?2, short_name = ?3 WHERE id = ?1",
            params![id.get(), name, short_name],
        )
        .map_err(|err| {
            if is_constraint_violation(&err) {
                refused(TaxContextProblem::ShortNameTaken)
            } else {
                err.into()
            }
        })?;
        tx.commit()?;
        Ok(())
    }

    pub fn tax_context_usage(&self, id: TaxContextId) -> Result<TaxContextUsage, IndexError> {
        let conn = self.connection();
        require_changeable(conn, id)?;
        Ok(TaxContextUsage {
            documents: count(conn, "document", id)?,
            rules: count(conn, "rule", id)?,
            saved_queries: count(conn, "saved_query", id)?,
        })
    }

    /// Everything that points at the context moves to `move_to` in the same
    /// transaction that deletes it; saved queries keep filtering, now on
    /// `move_to`.
    pub fn delete_tax_context(
        &mut self,
        id: TaxContextId,
        move_to: TaxContext,
    ) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        require_changeable(&tx, id)?;
        let target = tax_context_id(&tx, &move_to)?;
        if target == id.get() {
            return Err(refused(TaxContextProblem::MoveToItself));
        }
        for table in ["document", "rule", "saved_query"] {
            tx.execute(
                &format!("UPDATE {table} SET tax_context_id = ?2 WHERE tax_context_id = ?1"),
                params![id.get(), target],
            )?;
        }
        tx.execute("DELETE FROM tax_context WHERE id = ?1", [id.get()])?;
        tx.commit()?;
        Ok(())
    }

    /// Ordered by name, without the "no context" row.
    pub fn tax_contexts(&self) -> Result<Vec<TaxContextEntry>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT id, name, short_name FROM tax_context WHERE id <> ?1
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let entries = statement
            .query_map([NO_CONTEXT_ID], |row| {
                Ok(TaxContextEntry {
                    id: TaxContextId::new(row.get(0)?),
                    name: row.get(1)?,
                    short_name: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::model::{DocumentFields, FieldOrigin, QuerySpec};
    use crate::test_support::{encrypted_index, new_document, rule_spec};

    fn problem_of(result: Result<impl std::fmt::Debug, IndexError>) -> TaxContextProblem {
        match result {
            Err(IndexError::InvalidTaxContext(problem)) => problem,
            other => panic!("expected a refused tax context, got {other:?}"),
        }
    }

    fn entry(index: &Index, short_name: &str) -> TaxContextEntry {
        index
            .tax_contexts()
            .unwrap()
            .into_iter()
            .find(|entry| entry.short_name == short_name)
            .unwrap()
    }

    fn file_with_context(index: &mut Index, seed: u8, context: &str) -> crate::DocumentId {
        let id = index.insert_document(&new_document(seed)).unwrap();
        index
            .set_document_fields(
                id,
                &DocumentFields {
                    tax_context: Some(TaxContext::named(context)),
                    tags: BTreeSet::new(),
                    ..DocumentFields::default()
                },
                |_| FieldOrigin::Unknown,
            )
            .unwrap();
        id
    }

    fn context_of(index: &Index, id: crate::DocumentId) -> Option<TaxContext> {
        index.document(id).unwrap().unwrap().fields.tax_context
    }

    #[test]
    fn a_created_context_is_listed_by_name_with_trimmed_names() {
        let (_dir, mut index) = encrypted_index();

        let created = index
            .create_tax_context("  Firma B ", " company-b ")
            .unwrap();

        assert_eq!(created.name, "Firma B");
        assert_eq!(created.short_name, "company-b");
        let names: Vec<_> = index
            .tax_contexts()
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["Firma", "Firma B", "Privat"]);
    }

    #[test]
    fn names_are_checked_before_anything_is_written() {
        let (_dir, mut index) = encrypted_index();
        let too_long_name = "n".repeat(MAX_NAME_CHARS + 1);
        let too_long_short = "s".repeat(MAX_SHORT_NAME_CHARS + 1);

        assert_eq!(
            problem_of(index.create_tax_context("  ", "x")),
            TaxContextProblem::NameEmpty
        );
        assert_eq!(
            problem_of(index.create_tax_context(&too_long_name, "x")),
            TaxContextProblem::NameTooLong {
                max: MAX_NAME_CHARS
            }
        );
        assert_eq!(
            problem_of(index.create_tax_context("A\nB", "x")),
            TaxContextProblem::NameHasControlCharacter
        );
        assert_eq!(
            problem_of(index.create_tax_context("privat", "x")),
            TaxContextProblem::NameTaken
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", " ")),
            TaxContextProblem::ShortNameEmpty
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", &too_long_short)),
            TaxContextProblem::ShortNameTooLong {
                max: MAX_SHORT_NAME_CHARS
            }
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", "a/b")),
            TaxContextProblem::ShortNameForbiddenCharacter('/')
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", "a\\b")),
            TaxContextProblem::ShortNameForbiddenCharacter('\\')
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", "a\tb")),
            TaxContextProblem::ShortNameForbiddenCharacter('\t')
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", ".hidden")),
            TaxContextProblem::ShortNameStartsWithDot
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", "a..b")),
            TaxContextProblem::ShortNameHasDoubleDot
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", "None")),
            TaxContextProblem::ShortNameReserved
        );
        assert_eq!(
            problem_of(index.create_tax_context("X", "COMPANY")),
            TaxContextProblem::ShortNameTaken
        );
        assert_eq!(index.tax_contexts().unwrap().len(), 2);
    }

    #[test]
    fn letters_digits_and_the_allowed_marks_are_accepted() {
        let (_dir, mut index) = encrypted_index();

        let created = index.create_tax_context("Büro", "Büro 2_a-b.c").unwrap();

        assert_eq!(created.short_name, "Büro 2_a-b.c");
    }

    #[test]
    fn a_context_can_keep_its_own_names_when_renamed() {
        let (_dir, mut index) = encrypted_index();
        let company = entry(&index, "company");

        index
            .rename_tax_context(company.id, "Firma", "company")
            .unwrap();
        assert_eq!(
            problem_of(index.rename_tax_context(company.id, "Privat", "company")),
            TaxContextProblem::NameTaken
        );
        assert_eq!(
            problem_of(index.rename_tax_context(company.id, "Firma", "Private")),
            TaxContextProblem::ShortNameTaken
        );
        index
            .rename_tax_context(company.id, "Firma A", "company-a")
            .unwrap();

        let renamed = entry(&index, "company-a");
        assert_eq!((renamed.id, renamed.name.as_str()), (company.id, "Firma A"));
    }

    #[test]
    fn the_no_context_row_and_unknown_ids_cannot_be_changed() {
        let (_dir, mut index) = encrypted_index();
        let sentinel = TaxContextId::new(NO_CONTEXT_ID);
        let unknown = TaxContextId::new(99);

        assert_eq!(
            problem_of(index.rename_tax_context(sentinel, "X", "x")),
            TaxContextProblem::NoContextIsFixed
        );
        assert_eq!(
            problem_of(index.delete_tax_context(sentinel, TaxContext::named("private"))),
            TaxContextProblem::NoContextIsFixed
        );
        assert!(matches!(
            index.rename_tax_context(unknown, "X", "x"),
            Err(IndexError::TaxContextNotFound(_))
        ));
        assert!(matches!(
            index.tax_context_usage(unknown),
            Err(IndexError::TaxContextNotFound(_))
        ));
    }

    #[test]
    fn usage_counts_documents_rules_and_saved_queries() {
        let (_dir, mut index) = encrypted_index();
        file_with_context(&mut index, 1, "company");
        file_with_context(&mut index, 2, "company");
        file_with_context(&mut index, 3, "private");
        index.create_rule(&rule_spec()).unwrap();
        index
            .save_query(
                "Company",
                &QuerySpec {
                    tax_context: Some(TaxContext::named("company")),
                    ..QuerySpec::default()
                },
            )
            .unwrap();

        let company = index
            .tax_context_usage(entry(&index, "company").id)
            .unwrap();
        let private = index
            .tax_context_usage(entry(&index, "private").id)
            .unwrap();

        assert_eq!(
            company,
            TaxContextUsage {
                documents: 2,
                rules: 1,
                saved_queries: 1
            }
        );
        assert!(!company.is_unused());
        assert_eq!(private.documents, 1);
        let unused = index.create_tax_context("Unused", "unused").unwrap();
        assert!(index.tax_context_usage(unused.id).unwrap().is_unused());
    }

    #[test]
    fn deleting_moves_documents_rules_and_queries_to_another_context() {
        let (_dir, mut index) = encrypted_index();
        let document = file_with_context(&mut index, 1, "company");
        let rule = index.create_rule(&rule_spec()).unwrap();
        let query_spec = QuerySpec {
            tax_context: Some(TaxContext::named("company")),
            ..QuerySpec::default()
        };
        index.save_query("Company", &query_spec).unwrap();
        let company = entry(&index, "company").id;

        index
            .delete_tax_context(company, TaxContext::named("private"))
            .unwrap();

        assert_eq!(
            context_of(&index, document),
            Some(TaxContext::named("private"))
        );
        let moved_rule = index
            .rules()
            .unwrap()
            .into_iter()
            .find(|r| r.id == rule)
            .unwrap();
        assert_eq!(moved_rule.spec.tax_context, TaxContext::named("private"));
        assert_eq!(
            index.saved_queries().unwrap()[0].spec.tax_context,
            Some(TaxContext::named("private"))
        );
        assert_eq!(
            index
                .run_query(&QuerySpec {
                    tax_context: Some(TaxContext::named("private")),
                    ..QuerySpec::default()
                })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(index.tax_contexts().unwrap().len(), 1);
        assert!(matches!(
            index.tax_context_usage(company),
            Err(IndexError::TaxContextNotFound(_))
        ));
    }

    #[test]
    fn deleting_can_move_everything_to_no_context() {
        let (_dir, mut index) = encrypted_index();
        let document = file_with_context(&mut index, 1, "company");
        index.create_rule(&rule_spec()).unwrap();
        index
            .save_query(
                "Company",
                &QuerySpec {
                    tax_context: Some(TaxContext::named("company")),
                    ..QuerySpec::default()
                },
            )
            .unwrap();
        let company = entry(&index, "company").id;

        index.delete_tax_context(company, TaxContext::None).unwrap();

        assert_eq!(context_of(&index, document), Some(TaxContext::None));
        assert_eq!(index.rules().unwrap()[0].spec.tax_context, TaxContext::None);
        assert_eq!(
            index.saved_queries().unwrap()[0].spec.tax_context,
            Some(TaxContext::None)
        );
        let dangling: i64 = index
            .connection()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM document WHERE tax_context_id = ?1)
                      + (SELECT COUNT(*) FROM rule WHERE tax_context_id = ?1)
                      + (SELECT COUNT(*) FROM saved_query WHERE tax_context_id = ?1)",
                [company.get()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0);
    }

    #[test]
    fn a_refused_delete_changes_nothing() {
        let (_dir, mut index) = encrypted_index();
        let document = file_with_context(&mut index, 1, "company");
        let company = entry(&index, "company").id;

        assert_eq!(
            problem_of(index.delete_tax_context(company, TaxContext::named("company"))),
            TaxContextProblem::MoveToItself
        );
        assert!(matches!(
            index.delete_tax_context(company, TaxContext::named("missing")),
            Err(IndexError::UnknownTaxContext(_))
        ));

        assert_eq!(index.tax_contexts().unwrap().len(), 2);
        assert_eq!(
            context_of(&index, document),
            Some(TaxContext::named("company"))
        );
    }

    #[test]
    fn a_saved_query_returns_the_same_documents_after_the_rename() {
        let (_dir, mut index) = encrypted_index();
        let document = file_with_context(&mut index, 1, "company");
        index
            .save_query(
                "Company",
                &QuerySpec {
                    tax_context: Some(TaxContext::named("company")),
                    ..QuerySpec::default()
                },
            )
            .unwrap();
        let company = entry(&index, "company").id;

        index
            .rename_tax_context(company, "Firma A", "company-a")
            .unwrap();

        let saved = index.saved_queries().unwrap().remove(0);
        assert_eq!(saved.spec.tax_context, Some(TaxContext::named("company-a")));
        let found: Vec<_> = index
            .run_query(&saved.spec)
            .unwrap()
            .into_iter()
            .map(|found| found.id)
            .collect();
        assert_eq!(found, [document]);
        assert_eq!(
            context_of(&index, document),
            Some(TaxContext::named("company-a"))
        );
    }
}
