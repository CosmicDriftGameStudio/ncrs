use rusqlite::{params, Connection};

use crate::error::IndexError;
use crate::model::{Condition, ConditionKind};
use crate::regex_limits::compile_bounded_regex;

/// What a stored condition list belongs to. Rules and tax contexts keep their
/// conditions in tables of the same shape.
#[derive(Debug, Clone, Copy)]
pub enum ConditionOwner {
    Rule,
    TaxContext,
}

impl ConditionOwner {
    fn table(self) -> &'static str {
        match self {
            Self::Rule => "rule_condition",
            Self::TaxContext => "tax_context_condition",
        }
    }

    fn key_column(self) -> &'static str {
        match self {
            Self::Rule => "rule_id",
            Self::TaxContext => "tax_context_id",
        }
    }
}

pub fn require_compilable_regexes(list: &[Condition]) -> Result<(), IndexError> {
    for (position, condition) in list.iter().enumerate() {
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

pub fn replace_conditions(
    conn: &Connection,
    owner: ConditionOwner,
    id: i64,
    list: &[Condition],
) -> Result<(), IndexError> {
    let (table, key) = (owner.table(), owner.key_column());
    conn.execute(&format!("DELETE FROM {table} WHERE {key} = ?1"), [id])?;
    for (position, condition) in list.iter().enumerate() {
        conn.execute(
            &format!(
                "INSERT INTO {table} ({key}, position, kind, value, negated)
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![
                id,
                i64::try_from(position).unwrap_or(i64::MAX),
                condition.kind.as_str(),
                condition.value,
                condition.negated,
            ],
        )?;
    }
    Ok(())
}

pub fn read_conditions(
    conn: &Connection,
    owner: ConditionOwner,
    id: i64,
) -> Result<Vec<Condition>, IndexError> {
    let (table, key) = (owner.table(), owner.key_column());
    let mut statement = conn.prepare(&format!(
        "SELECT kind, value, negated FROM {table} WHERE {key} = ?1 ORDER BY position"
    ))?;
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
