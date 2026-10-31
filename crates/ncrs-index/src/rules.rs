use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension as _};

use crate::condition_store::{read_conditions, replace_conditions, require_compilable_regexes};
use crate::error::IndexError;
use crate::index::Index;
use crate::model::{Combinator, Condition, Rule, RuleConditions, RuleId, RuleOrigin, RuleSpec};
use crate::names::{name_id, optional_kind_id, NameTable};

const RULE_COLUMNS: &str = "SELECT r.id, r.combinator,
        r.extract_number, r.extract_date, r.extract_amount, k.name,
        r.filing_pattern, r.confirmed, r.enabled, r.position, r.origin, r.name
    FROM rule r
    LEFT JOIN document_kind k ON k.id = r.kind_id";

struct RuleRow {
    id: i64,
    combinator: String,
    extract_number: Option<String>,
    extract_date: Option<String>,
    extract_amount: Option<String>,
    kind: Option<String>,
    filing_pattern: String,
    confirmed: bool,
    enabled: bool,
    position: i64,
    origin: String,
    name: Option<String>,
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
            filing_pattern: row.get(6)?,
            confirmed: row.get(7)?,
            enabled: row.get(8)?,
            position: row.get(9)?,
            origin: row.get(10)?,
            name: row.get(11)?,
        })
    }

    fn into_rule(self, list: Vec<Condition>, tags: BTreeSet<String>) -> Result<Rule, IndexError> {
        Ok(Rule {
            id: RuleId::new(self.id),
            position: self.position,
            origin: RuleOrigin::parse(&self.origin)?,
            spec: RuleSpec {
                name: self.name,
                conditions: RuleConditions {
                    combinator: Combinator::parse(&self.combinator)?,
                    list,
                },
                extract_number: self.extract_number,
                extract_date: self.extract_date,
                extract_amount: self.extract_amount,
                kind: self.kind,
                tags,
                filing_pattern: self.filing_pattern,
                confirmed: self.confirmed,
                enabled: self.enabled,
            },
        })
    }
}

fn require_condition(spec: &RuleSpec) -> Result<(), IndexError> {
    if spec.conditions.list.is_empty() {
        return Err(IndexError::RuleWithoutCondition);
    }
    require_compilable_regexes(&spec.conditions.list)?;
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

impl Index {
    pub fn create_rule(&mut self, spec: &RuleSpec) -> Result<RuleId, IndexError> {
        self.create_rule_from(spec, RuleOrigin::Hand)
    }

    pub fn create_rule_from(
        &mut self,
        spec: &RuleSpec,
        origin: RuleOrigin,
    ) -> Result<RuleId, IndexError> {
        require_condition(spec)?;
        let kinds = Arc::clone(self.kinds());
        let tx = self.conn_mut().transaction()?;
        let kind_id = optional_kind_id(&tx, &kinds, spec.kind.as_deref())?;
        tx.execute(
            "INSERT INTO rule (combinator, extract_number, extract_date, extract_amount,
                 kind_id, filing_pattern, confirmed, enabled, origin, name, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 (SELECT COALESCE(MAX(position), 0) + 1 FROM rule))",
            params![
                spec.conditions.combinator.as_str(),
                spec.extract_number,
                spec.extract_date,
                spec.extract_amount,
                kind_id,
                spec.filing_pattern,
                spec.confirmed,
                spec.enabled,
                origin.as_str(),
                spec.name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty()),
            ],
        )?;
        let id = RuleId::new(tx.last_insert_rowid());
        replace_conditions(&tx, id.get(), &spec.conditions.list)?;
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

    /// In evaluation order: by position, then id.
    pub fn rules(&self) -> Result<Vec<Rule>, IndexError> {
        let mut statement = self
            .connection()
            .prepare(&format!("{RULE_COLUMNS} ORDER BY r.position, r.id"))?;
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
        let kinds = Arc::clone(self.kinds());
        let tx = self.conn_mut().transaction()?;
        let kind_id = optional_kind_id(&tx, &kinds, spec.kind.as_deref())?;
        let changed = tx.execute(
            "UPDATE rule SET combinator = ?2, extract_number = ?3, extract_date = ?4,
                 extract_amount = ?5, kind_id = ?6, filing_pattern = ?7,
                 confirmed = ?8, enabled = ?9, name = ?10
             WHERE id = ?1",
            params![
                id.get(),
                spec.conditions.combinator.as_str(),
                spec.extract_number,
                spec.extract_date,
                spec.extract_amount,
                kind_id,
                spec.filing_pattern,
                spec.confirmed,
                spec.enabled,
                spec.name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty()),
            ],
        )?;
        if changed == 0 {
            return Err(IndexError::RuleNotFound(id));
        }
        replace_conditions(&tx, id.get(), &spec.conditions.list)?;
        replace_rule_tags(&tx, id, &spec.tags)?;
        tx.commit()?;
        Ok(())
    }

    /// Moves a rule to `to_index` of the ordered list (clamped to the end) and
    /// renumbers all positions from 1.
    pub fn move_rule(&mut self, id: RuleId, to_index: usize) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        let mut ids = tx
            .prepare("SELECT id FROM rule ORDER BY position, id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let Some(from) = ids.iter().position(|&existing| existing == id.get()) else {
            return Err(IndexError::RuleNotFound(id));
        };
        ids.remove(from);
        ids.insert(to_index.min(ids.len()), id.get());
        for (index, rule_id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE rule SET position = ?2 WHERE id = ?1",
                params![rule_id, i64::try_from(index + 1).unwrap_or(i64::MAX)],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Whether a rule was there to switch.
    pub fn set_rule_enabled(&self, id: RuleId, enabled: bool) -> Result<bool, IndexError> {
        let changed = self.connection().execute(
            "UPDATE rule SET enabled = ?2 WHERE id = ?1",
            params![id.get(), enabled],
        )?;
        Ok(changed > 0)
    }

    /// Documents whose fields a rule has set, per rule; rules without hits are absent.
    pub fn rule_hits(&self) -> Result<HashMap<RuleId, u64>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT rule_id, COUNT(DISTINCT document_id) FROM document_field_origin
             WHERE origin = 'rule' AND rule_id IS NOT NULL GROUP BY rule_id",
        )?;
        let hits = statement
            .query_map([], |row| {
                Ok((
                    RuleId::new(row.get::<_, i64>(0)?),
                    u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
                ))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(hits)
    }

    /// Whether a rule was there to delete.
    pub fn delete_rule(&self, id: RuleId) -> Result<bool, IndexError> {
        let changed = self
            .connection()
            .execute("DELETE FROM rule WHERE id = ?1", [id.get()])?;
        Ok(changed > 0)
    }

    fn rule_conditions(&self, id: i64) -> Result<Vec<Condition>, IndexError> {
        read_conditions(self.connection(), id)
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
    use crate::model::ConditionKind;
    use crate::test_support::{encrypted_index, rule_spec};

    #[test]
    fn a_rule_may_have_a_name_a_tag_condition_and_no_kind() {
        let (_dir, mut index) = encrypted_index();
        let spec = RuleSpec {
            name: Some("  Bills of Firma A ".to_owned()),
            conditions: RuleConditions::all_of(vec![Condition::not(ConditionKind::Tag, "firma-a")]),
            kind: None,
            ..rule_spec()
        };

        let id = index.create_rule(&spec).unwrap();

        let stored = index.rule(id).unwrap().unwrap().spec;
        assert_eq!(stored.name.as_deref(), Some("Bills of Firma A"));
        assert_eq!(stored.kind, None);
        assert_eq!(stored.conditions, spec.conditions);
        index
            .update_rule(
                id,
                &RuleSpec {
                    name: Some(" ".to_owned()),
                    kind: Some("invoice".to_owned()),
                    ..spec
                },
            )
            .unwrap();
        let updated = index.rule(id).unwrap().unwrap().spec;
        assert_eq!(updated.name, None);
        assert_eq!(updated.kind.as_deref(), Some("invoice"));
    }

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
                position: 1,
                origin: RuleOrigin::Hand,
                spec: first.clone()
            })
        );
        assert_eq!(
            index.rules().unwrap(),
            vec![
                Rule {
                    id: first_id,
                    position: 1,
                    origin: RuleOrigin::Hand,
                    spec: first
                },
                Rule {
                    id: second_id,
                    position: 2,
                    origin: RuleOrigin::Hand,
                    spec: second
                },
            ]
        );
    }

    fn ids_in_order(index: &Index) -> Vec<RuleId> {
        index.rules().unwrap().iter().map(|rule| rule.id).collect()
    }

    fn three_rules(index: &mut Index) -> [RuleId; 3] {
        [(); 3].map(|()| index.create_rule(&rule_spec()).unwrap())
    }

    #[test]
    fn a_created_rule_is_appended_to_the_order() {
        let (_dir, mut index) = encrypted_index();
        let ids = three_rules(&mut index);

        let positions: Vec<_> = index.rules().unwrap().iter().map(|r| r.position).collect();

        assert_eq!(ids_in_order(&index), ids);
        assert_eq!(positions, [1, 2, 3]);
    }

    #[test]
    fn a_rule_moves_to_the_front_the_middle_and_the_end() {
        let (_dir, mut index) = encrypted_index();
        let [a, b, c] = three_rules(&mut index);

        index.move_rule(c, 0).unwrap();
        assert_eq!(ids_in_order(&index), [c, a, b]);
        index.move_rule(c, 1).unwrap();
        assert_eq!(ids_in_order(&index), [a, c, b]);
        index.move_rule(a, 99).unwrap();
        assert_eq!(ids_in_order(&index), [c, b, a]);
        let positions: Vec<_> = index.rules().unwrap().iter().map(|r| r.position).collect();
        assert_eq!(positions, [1, 2, 3]);
    }

    #[test]
    fn moving_an_unknown_rule_is_an_error() {
        let (_dir, mut index) = encrypted_index();
        three_rules(&mut index);

        let result = index.move_rule(RuleId::new(99), 0);

        assert!(matches!(result, Err(IndexError::RuleNotFound(_))));
    }

    #[test]
    fn the_enabled_flag_round_trips_and_an_update_keeps_the_position() {
        let (_dir, mut index) = encrypted_index();
        let [a, b, _c] = three_rules(&mut index);
        index.move_rule(b, 0).unwrap();

        index
            .update_rule(
                a,
                &RuleSpec {
                    enabled: false,
                    ..rule_spec()
                },
            )
            .unwrap();

        let rule = index.rule(a).unwrap().unwrap();
        assert!(!rule.spec.enabled);
        assert_eq!(rule.position, 2);
        assert!(index.rule(b).unwrap().unwrap().spec.enabled);
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

    #[test]
    fn each_way_to_create_a_rule_records_its_origin_and_an_update_keeps_it() {
        let (_dir, mut index) = encrypted_index();
        let by_hand = index.create_rule(&rule_spec()).unwrap();
        let from_filing = index
            .create_rule_from(&rule_spec(), RuleOrigin::Filing)
            .unwrap();
        let outcome = index
            .suggest_rule(
                &RuleSpec {
                    conditions: RuleConditions::all_of(vec![Condition::new(
                        ConditionKind::Word,
                        "rent",
                    )]),
                    ..rule_spec()
                },
                crate::model::RuleEvidence {
                    matching_in_folder: 3,
                    folder_documents: 3,
                    matching_elsewhere: 0,
                    subtree: None,
                },
            )
            .unwrap();
        let crate::model::RuleSuggestion::Created(suggested) = outcome else {
            panic!("not created");
        };

        index
            .update_rule(
                from_filing,
                &RuleSpec {
                    confirmed: true,
                    ..rule_spec()
                },
            )
            .unwrap();
        index
            .confirm_suggested_rule(
                suggested,
                &RuleSpec {
                    confirmed: true,
                    ..rule_spec()
                },
            )
            .unwrap();

        let origin_of = |id| index.rule(id).unwrap().unwrap().origin;
        assert_eq!(origin_of(by_hand), RuleOrigin::Hand);
        assert_eq!(origin_of(from_filing), RuleOrigin::Filing);
        assert_eq!(origin_of(suggested), RuleOrigin::Suggestion);
    }

    #[test]
    fn a_rule_is_switched_on_and_off_without_touching_anything_else() {
        let (_dir, mut index) = encrypted_index();
        let [a, b, _c] = three_rules(&mut index);

        assert!(index.set_rule_enabled(a, false).unwrap());
        assert!(!index.rule(a).unwrap().unwrap().spec.enabled);
        assert!(index.rule(b).unwrap().unwrap().spec.enabled);
        assert!(index.set_rule_enabled(a, true).unwrap());
        assert_eq!(index.rule(a).unwrap().unwrap().spec, rule_spec());
        assert!(!index.set_rule_enabled(RuleId::new(99), false).unwrap());
    }

    #[test]
    fn hits_count_the_documents_a_rule_set_a_field_on() {
        use crate::model::{DocumentFields, Field, FieldOrigin};
        use crate::test_support::new_document;

        let (_dir, mut index) = encrypted_index();
        let [a, b, _c] = three_rules(&mut index);
        let first = index.insert_document(&new_document(1)).unwrap();
        let second = index.insert_document(&new_document(2)).unwrap();
        let third = index.insert_document(&new_document(3)).unwrap();
        let fields = DocumentFields {
            sender: Some("Company A".to_owned()),
            kind: Some("invoice".to_owned()),
            ..DocumentFields::default()
        };
        for (document, origin) in [
            (first, FieldOrigin::Rule(Some(a))),
            (second, FieldOrigin::Rule(Some(a))),
            (third, FieldOrigin::Rule(Some(b))),
        ] {
            index
                .set_document_fields(document, &fields, |_: Field| origin.clone())
                .unwrap();
        }
        let manual = DocumentFields {
            sender: Some("Company B".to_owned()),
            ..DocumentFields::default()
        };
        let fourth = index.insert_document(&new_document(4)).unwrap();
        index
            .set_document_fields(fourth, &manual, |_| FieldOrigin::Manual)
            .unwrap();

        let hits = index.rule_hits().unwrap();

        assert_eq!(hits.get(&a), Some(&2));
        assert_eq!(hits.get(&b), Some(&1));
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn deleting_a_rule_removes_it_and_reports_an_unknown_one() {
        let (_dir, mut index) = encrypted_index();
        let [a, b, c] = three_rules(&mut index);

        assert!(index.delete_rule(b).unwrap());
        assert!(!index.delete_rule(b).unwrap());

        assert_eq!(ids_in_order(&index), [a, c]);
    }
}
