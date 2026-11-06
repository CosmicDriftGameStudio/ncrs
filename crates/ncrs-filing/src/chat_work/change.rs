//! Helpers for reading the folders of a location: path segments, the folder
//! name a placeholder writes, and the documents of a branch.

use std::path::{Component, Path};

use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::{render_pattern_in, Area, Branch, Level, PlanFile};

use super::FolderReading;

pub(in crate::chat_work) fn segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// The one folder name a document gets for `placeholder`, written the way a
/// filing pattern writes it.
pub(in crate::chat_work) fn folder_value(
    placeholder: &str,
    fields: &DocumentFields,
    kinds: &Kinds,
) -> Option<String> {
    let path = render_pattern_in(&format!("{{{placeholder}}}"), fields, kinds)?;
    let mut names = segments(&path);
    (names.len() == 1).then(|| names.remove(0))
}

fn matches_branch(parts: &[String], area: &Area, branch: &Branch) -> bool {
    parts.len() == branch.levels.len() + 2
        && parts.first() == Some(&area.folder)
        && branch
            .levels
            .iter()
            .zip(parts.iter().skip(1))
            .all(|(level, part)| match level {
                Level::Literal(name) => name == part,
                Level::Choice(names) => names.contains(part),
                Level::Placeholder(_) => true,
            })
}

/// A real document of the branch to show its pattern with: the newest dated
/// one, else the first by path.
pub fn branch_example<'a>(
    reading: &'a FolderReading,
    area: &Area,
    branch: &Branch,
) -> Option<&'a PlanFile> {
    reading
        .files
        .iter()
        .filter(|file| matches_branch(&segments(&file.relative_path), area, branch))
        .max_by(|left, right| {
            left.fields
                .date
                .cmp(&right.fields.date)
                .then_with(|| right.relative_path.cmp(&left.relative_path))
        })
}
