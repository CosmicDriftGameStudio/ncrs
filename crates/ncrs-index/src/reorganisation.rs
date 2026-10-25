use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    hash_from_column, timestamp_from_column, Finding, PlanId, PlanItem, PlanOutcome,
    ReorganisationPlan,
};

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

impl Index {
    /// Stores a reorganisation plan for the location. The plan of that
    /// location that was not executed yet is replaced; plans of other
    /// locations and executed ones stay. Items are stored sorted by source.
    pub fn replace_reorganisation_plan(
        &mut self,
        location: &Path,
        items: &[PlanItem],
    ) -> Result<PlanId, IndexError> {
        let location_text = utf8(location)?;
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "DELETE FROM reorganisation_plan WHERE location = ?1 AND executed_at IS NULL",
            [location_text],
        )?;
        tx.execute(
            "INSERT INTO reorganisation_plan (location, created_at) VALUES (?1, ?2)",
            params![location_text, Utc::now().timestamp()],
        )?;
        let plan_id = tx.last_insert_rowid();
        for item in items {
            let source = utf8(&item.source)?;
            let (outcome, target, detail) = item.outcome.to_columns()?;
            tx.execute(
                "INSERT INTO reorganisation_item (plan_id, source, hash, outcome, target, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    plan_id,
                    source,
                    item.hash.0.as_slice(),
                    outcome,
                    target,
                    detail
                ],
            )?;
            for finding in &item.findings {
                tx.execute(
                    "INSERT INTO reorganisation_finding (plan_id, source, kind, detail)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        plan_id,
                        source,
                        finding.kind().as_str(),
                        finding.detail_column()?
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(PlanId::new(plan_id))
    }

    /// The plan of the location that was not executed yet; `None` when there
    /// is none.
    pub fn reorganisation_plan(
        &self,
        location: &Path,
    ) -> Result<Option<ReorganisationPlan>, IndexError> {
        let header: Option<(i64, i64)> = self
            .connection()
            .query_row(
                "SELECT id, created_at FROM reorganisation_plan
                 WHERE location = ?1 AND executed_at IS NULL ORDER BY id DESC LIMIT 1",
                [utf8(location)?],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((id, created_at)) = header else {
            return Ok(None);
        };
        let mut items = self.plan_items(PlanId::new(id))?;
        items.sort_by(|left, right| left.source.cmp(&right.source));
        Ok(Some(ReorganisationPlan {
            id: PlanId::new(id),
            location: location.to_path_buf(),
            created_at: timestamp_from_column("reorganisation_plan.created_at", created_at)?,
            items,
        }))
    }

    /// The plan has been carried out; `reorganisation_plan` no longer returns
    /// it, and a new plan of the location does not replace it.
    pub fn mark_executed(&self, plan: PlanId) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE reorganisation_plan SET executed_at = ?2 WHERE id = ?1",
            params![plan.get(), Utc::now().timestamp()],
        )?;
        Ok(())
    }

    fn plan_items(&self, id: PlanId) -> Result<Vec<PlanItem>, IndexError> {
        let plan_id = id.get();
        let mut statement = self.connection().prepare(
            "SELECT source, hash, outcome, target, detail FROM reorganisation_item
             WHERE plan_id = ?1",
        )?;
        let rows = statement
            .query_map([plan_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(source, hash, outcome, target, detail)| {
                let mut findings = self.plan_findings(plan_id, &source)?;
                findings.sort_by_key(Finding::kind);
                Ok(PlanItem {
                    source: PathBuf::from(source),
                    hash: hash_from_column(&hash)?,
                    findings,
                    outcome: PlanOutcome::from_columns(&outcome, target, detail.as_deref())?,
                })
            })
            .collect()
    }

    fn plan_findings(&self, plan_id: i64, source: &str) -> Result<Vec<Finding>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT kind, detail FROM reorganisation_finding
             WHERE plan_id = ?1 AND source = ?2",
        )?;
        let rows = statement
            .query_map(params![plan_id, source], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(kind, detail)| Finding::from_columns(&kind, &detail))
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::model::{ConflictReason, ContentHash, NoActionReason, RuleId};
    use crate::test_support::encrypted_index;

    fn item(source: &str, seed: u8, findings: Vec<Finding>, outcome: PlanOutcome) -> PlanItem {
        PlanItem {
            source: PathBuf::from(source),
            hash: ContentHash([seed; 32]),
            findings,
            outcome,
        }
    }

    fn every_kind() -> Vec<PlanItem> {
        vec![
            item(
                "b/Company A.pdf",
                2,
                vec![
                    Finding::Duplicate {
                        keeper: PathBuf::from("a/Company A.pdf"),
                    },
                    Finding::Misplaced {
                        rule: RuleId::new(7),
                    },
                    Finding::YearMismatch {
                        folder_year: 2023,
                        date_year: 2024,
                    },
                    Finding::NameOutlier {
                        pattern: "{date}_{name}".to_owned(),
                    },
                ],
                PlanOutcome::Move {
                    target: PathBuf::from("duplicates/b/Company A.pdf"),
                },
            ),
            item(
                "a/x.pdf",
                1,
                vec![Finding::NameOutlier {
                    pattern: "{date}_{name}".to_owned(),
                }],
                PlanOutcome::Rename {
                    target: PathBuf::from("a/2024-01-01_x.pdf"),
                },
            ),
            item(
                "c/y.pdf",
                3,
                vec![Finding::Misplaced {
                    rule: RuleId::new(1),
                }],
                PlanOutcome::Conflict {
                    target: Some(PathBuf::from("d/y.pdf")),
                    reason: ConflictReason::TargetExists,
                },
            ),
            item(
                "c/z.pdf",
                4,
                vec![Finding::YearMismatch {
                    folder_year: 2020,
                    date_year: 2021,
                }],
                PlanOutcome::Conflict {
                    target: None,
                    reason: ConflictReason::TargetLeavesLocation,
                },
            ),
            item(
                "c/w.pdf",
                5,
                vec![Finding::NameOutlier {
                    pattern: "{name}".to_owned(),
                }],
                PlanOutcome::NoAction(NoActionReason::MissingFields),
            ),
        ]
    }

    #[test]
    fn a_stored_plan_is_read_back_sorted_with_hash_source_and_reasons() {
        let (_dir, mut index) = encrypted_index();
        let location = Path::new("/archive");
        let stored = every_kind();

        let id = index
            .replace_reorganisation_plan(location, &stored)
            .unwrap();

        let plan = index.reorganisation_plan(location).unwrap().unwrap();
        assert_eq!(plan.id, id);
        assert_eq!(plan.location, location);
        let mut expected = stored;
        expected.sort_by(|left, right| left.source.cmp(&right.source));
        assert_eq!(plan.items, expected);
    }

    #[test]
    fn a_new_plan_replaces_the_unexecuted_plan_of_that_location_only() {
        let (_dir, mut index) = encrypted_index();
        let first = Path::new("/archive");
        let other = Path::new("/other");
        index
            .replace_reorganisation_plan(first, &every_kind())
            .unwrap();
        index
            .replace_reorganisation_plan(other, &every_kind())
            .unwrap();

        let replacement = vec![item(
            "n.pdf",
            9,
            Vec::new(),
            PlanOutcome::NoAction(NoActionReason::MissingFields),
        )];
        index
            .replace_reorganisation_plan(first, &replacement)
            .unwrap();

        assert_eq!(
            index.reorganisation_plan(first).unwrap().unwrap().items,
            replacement
        );
        assert_eq!(
            index
                .reorganisation_plan(other)
                .unwrap()
                .unwrap()
                .items
                .len(),
            5
        );
        let rows: i64 = index
            .connection()
            .query_row("SELECT count(*) FROM reorganisation_item", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 6);
    }

    #[test]
    fn an_executed_plan_is_not_returned_and_not_replaced() {
        let (_dir, mut index) = encrypted_index();
        let location = Path::new("/archive");
        let id = index
            .replace_reorganisation_plan(location, &every_kind())
            .unwrap();

        index.mark_executed(id).unwrap();

        assert!(index.reorganisation_plan(location).unwrap().is_none());
        index.replace_reorganisation_plan(location, &[]).unwrap();
        let rows: i64 = index
            .connection()
            .query_row("SELECT count(*) FROM reorganisation_plan", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[test]
    fn a_location_without_a_plan_has_none() {
        let (_dir, index) = encrypted_index();
        assert!(index
            .reorganisation_plan(Path::new("/archive"))
            .unwrap()
            .is_none());
    }
}
