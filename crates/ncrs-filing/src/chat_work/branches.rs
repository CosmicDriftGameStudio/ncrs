//! Helpers for reading the folders of a location: path segments and the
//! documents of a branch.

use std::path::{Component, Path};

use ncrs_intake::{Area, Branch, Level, PlanFile};

use super::FolderReading;

/// The normal components of a path as names, without root or prefix.
pub fn segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
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
