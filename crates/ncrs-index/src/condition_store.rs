use rusqlite::{params, Connection};

use crate::error::IndexError;
use crate::model::{Condition, ConditionKind};
use crate::regex_limits::compile_bounded_regex;

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
    id: i64,
    list: &[Condition],
) -> Result<(), IndexError> {
    conn.execute("DELETE FROM rule_condition WHERE rule_id = ?1", [id])?;
    for (position, condition) in list.iter().enumerate() {
        conn.execute(
            "INSERT INTO rule_condition (rule_id, position, kind, value, negated)
             VALUES (?1, ?2, ?3, ?4, ?5)",
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

pub fn read_conditions(conn: &Connection, id: i64) -> Result<Vec<Condition>, IndexError> {
    let mut statement = conn.prepare(
        "SELECT kind, value, negated FROM rule_condition WHERE rule_id = ?1 ORDER BY position",
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
