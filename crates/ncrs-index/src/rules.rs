use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    Combinator, Condition, ConditionKind, Rule, RuleConditions, RuleId, RuleSpec, TaxContext,
};
use crate::names::{name_id, NameTable};
use crate::regex_limits::compile_bounded_regex;

const RULE_COLUMNS: &str = "SELECT r.id, r.combinator,
        r.extract_number, r.extract_date, r.extract_amount, k.name, r.tax_context,
        r.filing_pattern, r.confirmed
    FROM rule r
    JOIN document_kind k ON k.id = r.kind_id";

struct RuleRow {
    id: i64,
    combinator: String,
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
            combinator: row.get(1)?,
            extract_number: row.get(2)?,
            extract_date: row.get(3)?,
            extract_amount: row.get(4)?,
            kind: row.get(5)?,
            tax_context: row.get(6)?,
            filing_pattern: row.get(7)?,
            confirmed: row.get(8)?,
        })
    }

    fn into_rule(self, list: Vec<Condition>, tags: BTreeSet<String>) -> Result<Rule, IndexError> {
        Ok(Rule {
            id: RuleId::new(self.id),
            spec: RuleSpec {
                conditions: RuleConditions {
                    combinator: Combinator::parse(&self.combinator)?,
                    list,
                },
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
    if spec.conditions.list.is_empty() {
        return Err(IndexError::RuleWithoutCondition);
    }
    for (position, condition) in spec.conditions.list.iter().enumerate() {
        if condition.kind != ConditionKind::Regex {
            continue;
        }
        if let Err(err) = compile_bounded_regex(&condition.value) {
            return Err(IndexError::InvalidRegex {
                position,
                reason: err.to_string(),
            });
        }
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

fn replace_rule_conditions(
    conn: &Connection,
    id: RuleId,
    list: &[Condition],
) -> Result<(), IndexError> {
    conn.execute("DELETE FROM rule_condition WHERE rule_id = ?1", [id.get()])?;
    for (position, condition) in list.iter().enumerate() {
        conn.execute(
            "INSERT INTO rule_condition (rule_id, position, kind, value, negated)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.get(),
                i64::try_from(position).unwrap_or(i64::MAX),
                condition.kind.as_str(),
                condition.value,
                condition.negated,
            ],
        )?;
    }
    Ok(())
}

impl Index {
    pub fn create_rule(&mut self, spec: &RuleSpec) -> Result<RuleId, IndexError> {
        require_condition(spec)?;
        let tx = self.conn_mut().transaction()?;
        let kind_id = name_id(&tx, NameTable::Kind, &spec.kind)?;
        tx.execute(
            "INSERT INTO rule (combinator, extract_number, extract_date, extract_amount,
                 kind_id, tax_context, filing_pattern, confirmed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                spec.conditions.combinator.as_str(),
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
        replace_rule_conditions(&tx, id, &spec.conditions.list)?;
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
        let list = self.rule_conditions(row.id)?;
        let tags = self.rule_tags(row.id)?;
        row.into_rule(list, tags).map(Some)
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
                let list = self.rule_conditions(row.id)?;
                let tags = self.rule_tags(row.id)?;
                row.into_rule(list, tags)
            })
            .collect()
    }

    pub fn update_rule(&mut self, id: RuleId, spec: &RuleSpec) -> Result<(), IndexError> {
        require_condition(spec)?;
        let tx = self.conn_mut().transaction()?;
        let kind_id = name_id(&tx, NameTable::Kind, &spec.kind)?;
        let changed = tx.execute(
            "UPDATE rule SET combinator = ?2, extract_number = ?3, extract_date = ?4,
                 extract_amount = ?5, kind_id = ?6, tax_context = ?7, filing_pattern = ?8,
                 confirmed = ?9
             WHERE id = ?1",
            params![
                id.get(),
                spec.conditions.combinator.as_str(),
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
        replace_rule_conditions(&tx, id, &spec.conditions.list)?;
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

    fn rule_conditions(&self, id: i64) -> Result<Vec<Condition>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT kind, value, negated FROM rule_condition
             WHERE rule_id = ?1 ORDER BY position",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(kind, value, negated)| {
                Ok(Condition {
                    kind: ConditionKind::parse(&kind)?,
                    value,
                    negated,
                })
            })
            .collect()
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
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Regex,
                "Agency B.*",
            )]),
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
            conditions: RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Regex, "Invoice \\d+"),
                Condition::new(ConditionKind::Sender, "Agency B"),
            ]),
            tax_context: TaxContext::Private,
            tags: BTreeSet::from(["other".to_owned()]),
            confirmed: true,
            ..rule_spec()
        };

        index.update_rule(id, &changed).unwrap();

        assert_eq!(index.rule(id).unwrap().unwrap().spec, changed);
    }

    #[test]
    fn any_of_and_negated_conditions_round_trip_in_order() {
        let (_dir, mut index) = encrypted_index();
        let spec = RuleSpec {
            conditions: RuleConditions::any_of(vec![
                Condition::new(ConditionKind::Substring, "Invoice"),
                Condition::not(ConditionKind::Sender, "Agency B"),
                Condition::new(ConditionKind::Regex, "No\\. \\d+"),
            ]),
            ..rule_spec()
        };

        let id = index.create_rule(&spec).unwrap();

        assert_eq!(index.rule(id).unwrap().unwrap().spec, spec);
        let shorter = RuleSpec {
            conditions: RuleConditions::all_of(vec![Condition::not(
                ConditionKind::Substring,
                "Reminder",
            )]),
            ..spec
        };
        index.update_rule(id, &shorter).unwrap();
        assert_eq!(index.rule(id).unwrap().unwrap().spec, shorter);
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
            conditions: RuleConditions::default(),
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

    fn regex_rule(source: &str) -> RuleSpec {
        RuleSpec {
            conditions: RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Word, "invoice"),
                Condition::new(ConditionKind::Regex, source),
            ]),
            ..rule_spec()
        }
    }

    fn rule_count(index: &Index) -> i64 {
        index
            .connection()
            .query_row("SELECT count(*) FROM rule", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_regex_that_does_not_compile_is_refused_and_nothing_is_written() {
        let (_dir, mut index) = encrypted_index();
        let id = index.create_rule(&rule_spec()).unwrap();
        let stored = index.rule(id).unwrap();

        for source in ["(unclosed", "(a{1000}){1000}{1000}"] {
            let created = index.create_rule(&regex_rule(source)).unwrap_err();
            let updated = index.update_rule(id, &regex_rule(source)).unwrap_err();

            for err in [created, updated] {
                assert!(
                    matches!(&err, IndexError::InvalidRegex { position: 1, reason } if !reason.is_empty()),
                    "{source}: {err}"
                );
            }
        }

        assert_eq!(rule_count(&index), 1);
        assert_eq!(index.rule(id).unwrap(), stored);
    }

    #[test]
    fn a_valid_regex_is_stored() {
        let (_dir, mut index) = encrypted_index();
        let spec = regex_rule(r"Invoice\s+\d+");

        let id = index.create_rule(&spec).unwrap();

        assert_eq!(index.rule(id).unwrap().unwrap().spec, spec);
        index.update_rule(id, &regex_rule(r"No\. \d+")).unwrap();
    }
}
