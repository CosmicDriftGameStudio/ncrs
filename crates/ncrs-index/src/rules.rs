use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{Rule, RuleId, RuleSpec, TaxContext, TextMatch};
use crate::names::{name_id, optional_name_id, NameTable};

const RULE_COLUMNS: &str = "SELECT r.id, r.match_text, r.match_text_is_regex, s.name,
        r.extract_number, r.extract_date, r.extract_amount, k.name, r.tax_context,
        r.filing_pattern, r.confirmed
    FROM rule r
    LEFT JOIN sender s ON s.id = r.match_sender_id
    JOIN document_kind k ON k.id = r.kind_id";

struct RuleRow {
    id: i64,
    match_text: Option<String>,
    match_text_is_regex: bool,
    match_sender: Option<String>,
    extract_number: Option<String>,
    extract_date: Option<String>,
    extract_amount: Option<String>,
    kind: String,
    tax_context: String,
    filing_pattern: String,
    confirmed: bool,
}

impl RuleRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            match_text: row.get(1)?,
            match_text_is_regex: row.get(2)?,
            match_sender: row.get(3)?,
            extract_number: row.get(4)?,
            extract_date: row.get(5)?,
            extract_amount: row.get(6)?,
            kind: row.get(7)?,
            tax_context: row.get(8)?,
            filing_pattern: row.get(9)?,
            confirmed: row.get(10)?,
        })
    }

    fn into_rule(self, tags: BTreeSet<String>) -> Result<Rule, IndexError> {
        let match_text = self.match_text.map(|text| {
            if self.match_text_is_regex {
                TextMatch::Regex(text)
            } else {
                TextMatch::Contains(text)
            }
        });
        Ok(Rule {
            id: RuleId::new(self.id),
            spec: RuleSpec {
                match_text,
                match_sender: self.match_sender,
                extract_number: self.extract_number,
                extract_date: self.extract_date,
                extract_amount: self.extract_amount,
                kind: self.kind,
                tax_context: TaxContext::parse("rule.tax_context", &self.tax_context)?,
                tags,
                filing_pattern: self.filing_pattern,
                confirmed: self.confirmed,
            },
        })
    }
}

fn require_condition(spec: &RuleSpec) -> Result<(), IndexError> {
    if spec.match_text.is_none() && spec.match_sender.is_none() {
        return Err(IndexError::RuleWithoutCondition);
    }
    Ok(())
}

fn replace_rule_tags(
    conn: &Connection,
    id: RuleId,
    tags: &BTreeSet<String>,
) -> Result<(), IndexError> {
    conn.execute("DELETE FROM rule_tag WHERE rule_id = ?1", [id.get()])?;
    for tag in tags {
        let tag_id = name_id(conn, NameTable::Tag, tag)?;
        conn.execute(
            "INSERT OR IGNORE INTO rule_tag (rule_id, tag_id) VALUES (?1, ?2)",
            [id.get(), tag_id],
        )?;
    }
    Ok(())
}

fn match_text_columns(spec: &RuleSpec) -> (Option<&str>, bool) {
    match &spec.match_text {
        Some(TextMatch::Contains(text)) => (Some(text), false),
        Some(TextMatch::Regex(text)) => (Some(text), true),
        None => (None, false),
    }
}

impl Index {
    pub fn create_rule(&mut self, spec: &RuleSpec) -> Result<RuleId, IndexError> {
        require_condition(spec)?;
        let tx = self.conn_mut().transaction()?;
        let sender_id = optional_name_id(&tx, NameTable::Sender, spec.match_sender.as_deref())?;
        let kind_id = name_id(&tx, NameTable::Kind, &spec.kind)?;
        let (match_text, is_regex) = match_text_columns(spec);
        tx.execute(
            "INSERT INTO rule (match_text, match_text_is_regex, match_sender_id,
                 extract_number, extract_date, extract_amount, kind_id, tax_context,
                 filing_pattern, confirmed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                match_text,
                is_regex,
                sender_id,
                spec.extract_number,
                spec.extract_date,
                spec.extract_amount,
                kind_id,
                spec.tax_context.as_str(),
                spec.filing_pattern,
                spec.confirmed,
            ],
        )?;
        let id = RuleId::new(tx.last_insert_rowid());
        replace_rule_tags(&tx, id, &spec.tags)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn rule(&self, id: RuleId) -> Result<Option<Rule>, IndexError> {
        let Some(row) = self
            .connection()
            .query_row(
                &format!("{RULE_COLUMNS} WHERE r.id = ?1"),
                [id.get()],
                RuleRow::read,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let tags = self.rule_tags(row.id)?;
        row.into_rule(tags).map(Some)
    }

    /// Ordered by id.
    pub fn rules(&self) -> Result<Vec<Rule>, IndexError> {
        let mut statement = self
            .connection()
            .prepare(&format!("{RULE_COLUMNS} ORDER BY r.id"))?;
        let rows = statement
            .query_map([], RuleRow::read)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| {
                let tags = self.rule_tags(row.id)?;
                row.into_rule(tags)
            })
            .collect()
    }

    pub fn update_rule(&mut self, id: RuleId, spec: &RuleSpec) -> Result<(), IndexError> {
        require_condition(spec)?;
        let tx = self.conn_mut().transaction()?;
        let sender_id = optional_name_id(&tx, NameTable::Sender, spec.match_sender.as_deref())?;
        let kind_id = name_id(&tx, NameTable::Kind, &spec.kind)?;
        let (match_text, is_regex) = match_text_columns(spec);
        let changed = tx.execute(
            "UPDATE rule SET match_text = ?2, match_text_is_regex = ?3, match_sender_id = ?4,
                 extract_number = ?5, extract_date = ?6, extract_amount = ?7, kind_id = ?8,
                 tax_context = ?9, filing_pattern = ?10, confirmed = ?11
             WHERE id = ?1",
            params![
                id.get(),
                match_text,
                is_regex,
                sender_id,
                spec.extract_number,
                spec.extract_date,
                spec.extract_amount,
                kind_id,
                spec.tax_context.as_str(),
                spec.filing_pattern,
                spec.confirmed,
            ],
        )?;
        if changed == 0 {
            return Err(IndexError::RuleNotFound(id));
        }
        replace_rule_tags(&tx, id, &spec.tags)?;
        tx.commit()?;
        Ok(())
    }

    /// Whether a rule was there to delete.
    pub fn delete_rule(&self, id: RuleId) -> Result<bool, IndexError> {
        let changed = self
            .connection()
            .execute("DELETE FROM rule WHERE id = ?1", [id.get()])?;
        Ok(changed > 0)
    }

    fn rule_tags(&self, id: i64) -> Result<BTreeSet<String>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT t.name FROM rule_tag rt JOIN tag t ON t.id = rt.tag_id
             WHERE rt.rule_id = ?1",
        )?;
        let tags = statement
            .query_map([id], |row| row.get(0))?
            .collect::<Result<BTreeSet<String>, _>>()?;
        Ok(tags)
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
    use crate::test_support::{encrypted_index, rule_spec};

    #[test]
    fn rule_round_trips_and_is_listed_by_id() {
        let (_dir, mut index) = encrypted_index();
        let first = rule_spec();
        let second = RuleSpec {
            match_text: Some(TextMatch::Regex("Agency B.*".to_owned())),
            match_sender: None,
            tags: BTreeSet::new(),
            confirmed: true,
            ..rule_spec()
        };

        let first_id = index.create_rule(&first).unwrap();
        let second_id = index.create_rule(&second).unwrap();

        assert_eq!(
            index.rule(first_id).unwrap(),
            Some(Rule {
                id: first_id,
                spec: first.clone()
            })
        );
        assert_eq!(
            index.rules().unwrap(),
            vec![
                Rule {
                    id: first_id,
                    spec: first
                },
                Rule {
                    id: second_id,
                    spec: second
                },
            ]
        );
    }

    #[test]
    fn update_replaces_the_spec_and_tags() {
        let (_dir, mut index) = encrypted_index();
        let id = index.create_rule(&rule_spec()).unwrap();
        let changed = RuleSpec {
            match_text: Some(TextMatch::Regex("Invoice \\d+".to_owned())),
            match_sender: Some("Agency B".to_owned()),
            tax_context: TaxContext::Private,
            tags: BTreeSet::from(["other".to_owned()]),
            confirmed: true,
            ..rule_spec()
        };

        index.update_rule(id, &changed).unwrap();

        assert_eq!(index.rule(id).unwrap().unwrap().spec, changed);
    }

    #[test]
    fn updating_an_unknown_rule_is_reported() {
        let (_dir, mut index) = encrypted_index();

        let err = index.update_rule(RuleId::new(5), &rule_spec()).unwrap_err();

        assert!(
            matches!(err, IndexError::RuleNotFound(id) if id == RuleId::new(5)),
            "{err}"
        );
    }

    #[test]
    fn deleting_a_rule_removes_its_tag_links() {
        let (_dir, mut index) = encrypted_index();
        let id = index.create_rule(&rule_spec()).unwrap();

        assert!(index.delete_rule(id).unwrap());

        assert_eq!(index.rule(id).unwrap(), None);
        let links: i64 = index
            .connection()
            .query_row("SELECT count(*) FROM rule_tag", [], |row| row.get(0))
            .unwrap();
        assert_eq!(links, 0);
        assert!(!index.delete_rule(id).unwrap());
    }

    #[test]
    fn a_rule_without_a_condition_is_refused() {
        let (_dir, mut index) = encrypted_index();
        let spec = RuleSpec {
            match_text: None,
            match_sender: None,
            ..rule_spec()
        };

        let created = index.create_rule(&spec).unwrap_err();
        let id = index.create_rule(&rule_spec()).unwrap();
        let updated = index.update_rule(id, &spec).unwrap_err();

        assert!(
            matches!(created, IndexError::RuleWithoutCondition),
            "{created}"
        );
        assert!(
            matches!(updated, IndexError::RuleWithoutCondition),
            "{updated}"
        );
    }
}
