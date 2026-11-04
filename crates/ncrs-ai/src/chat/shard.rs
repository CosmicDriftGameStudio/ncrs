//! Splits the chat evaluation over several runners: the sentences are slow on
//! a CPU, and the shards together are the full run.

use std::fmt;

pub const SHARD_VARIABLE: &str = "NCRS_EVAL_SHARD";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    pub index: usize,
    pub count: usize,
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.index, self.count)
    }
}

/// `<index>/<count>`, index counted from 0.
pub fn parse_shard(value: &str) -> Result<Shard, String> {
    let invalid = |reason: &str| {
        format!(
            "{SHARD_VARIABLE} must be <index>/<count> with index < count, got {value:?} ({reason})"
        )
    };
    let (index_text, count_text) = value.split_once('/').ok_or_else(|| invalid("no slash"))?;
    let index: usize = index_text
        .parse()
        .map_err(|error: std::num::ParseIntError| invalid(&error.to_string()))?;
    let count: usize = count_text
        .parse()
        .map_err(|error: std::num::ParseIntError| invalid(&error.to_string()))?;
    if count == 0 || index >= count {
        return Err(invalid("index out of range"));
    }
    Ok(Shard { index, count })
}

/// The rows at the positions that belong to the shard.
pub fn select<T>(rows: Vec<T>, shard: Shard) -> Vec<T> {
    rows.into_iter()
        .enumerate()
        .filter(|(position, _)| position % shard.count == shard.index)
        .map(|(_, row)| row)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index_and_count() {
        assert_eq!(parse_shard("2/4"), Ok(Shard { index: 2, count: 4 }));
        assert_eq!(parse_shard("0/1"), Ok(Shard { index: 0, count: 1 }));
    }

    #[test]
    fn rejects_invalid_values() {
        for value in [
            "", "3", "4/4", "5/4", "0/0", "-1/4", "a/4", "1/b", "1/2/3", " 1/4",
        ] {
            let message = parse_shard(value).err().unwrap_or_default();
            assert!(message.contains(SHARD_VARIABLE), "{value}: {message}");
        }
    }

    #[test]
    fn shards_together_are_all_rows_and_disjoint() {
        let rows: Vec<usize> = (0..174).collect();
        for count in [1, 3, 4, 7] {
            let mut seen = Vec::new();
            for index in 0..count {
                seen.extend(select(rows.clone(), Shard { index, count }));
            }
            seen.sort_unstable();
            assert_eq!(seen, rows, "count {count}");
        }
    }

    #[test]
    fn a_shard_takes_every_countth_row() {
        let rows: Vec<usize> = (0..10).collect();
        assert_eq!(select(rows, Shard { index: 1, count: 4 }), vec![1, 5, 9]);
    }
}
