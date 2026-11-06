//! The folders a target without an order of its own gets.

use std::path::PathBuf;

use chrono::Datelike as _;
use ncrs_index::{DocumentFields, Kinds};

use super::change::folder_value;

/// One folder level of the order a new target gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatternLevel {
    Year,
    Month,
    Sender,
    Kind,
}

fn folder_of(level: PatternLevel, fields: &DocumentFields, kinds: &Kinds) -> Option<String> {
    match level {
        PatternLevel::Year => folder_value("year", fields, kinds),
        PatternLevel::Month => fields.date.map(|date| format!("{:02}", date.month())),
        PatternLevel::Sender => folder_value("sender", fields, kinds),
        PatternLevel::Kind => folder_value("kind", fields, kinds),
    }
}

/// Where a file goes in a target that has no order of its own: below the
/// folders of `levels`, in none for an empty list. `None` when the file lacks
/// what a level needs.
pub(in crate::chat_work) fn layout_path(
    levels: &[PatternLevel],
    fields: &DocumentFields,
    kinds: &Kinds,
    file_name: &str,
) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for level in levels {
        path.push(folder_of(*level, fields, kinds)?);
    }
    Some(path.join(file_name))
}
