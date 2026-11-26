//! The list the tab shows: the repositories in their groups, each with the
//! worktrees below it. Pure: the same inputs give the same rows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::store::{Origin, Store};
use crate::summary::Reading;

/// A group of the list. The order of the variants is the order on screen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKey {
    Favourites,
    Open,
    Unreachable,
    /// Found by the scan that began in this folder.
    FoundIn(PathBuf),
    /// Learned from a panel or added by hand.
    Added,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Repo,
    /// A linked worktree below its repository: not markable, not a target.
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub kind: RowKind,
    /// The repository the row belongs to: itself for a repo.
    pub owner: PathBuf,
    /// The branch a worktree row shows.
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub key: GroupKey,
    /// Repositories only, worktrees not counted.
    pub count: usize,
    pub open: bool,
    /// The rows when the group is open, none when it is folded.
    pub rows: Vec<Row>,
}

/// What shapes the list besides the repositories themselves.
#[derive(Debug, Clone, Copy)]
pub struct Shape<'a> {
    pub readings: &'a BTreeMap<PathBuf, Reading>,
    pub open_roots: &'a [PathBuf],
    pub filter: &'a str,
    pub only_needing_action: bool,
    /// The worktrees of a repository stand under it.
    pub show_worktrees: bool,
    pub folded: &'a BTreeSet<GroupKey>,
}

/// The name of a repository: its folder.
pub fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

pub fn is_hidden_worktree(
    path: &Path,
    store: &Store,
    readings: &BTreeMap<PathBuf, Reading>,
) -> bool {
    readings
        .get(path)
        .and_then(Reading::summary)
        .and_then(|summary| summary.main_worktree.as_deref())
        .is_some_and(|main| main != path && store.contains(main))
}

/// The group a repository stands in. A favourite stands once, in the first
/// group of the list; one whose folder is gone is not reachable first.
pub fn group_of(path: &Path, store: &Store, shape: &Shape<'_>) -> GroupKey {
    if shape
        .readings
        .get(path)
        .is_some_and(Reading::is_unreachable)
    {
        return GroupKey::Unreachable;
    }
    if store.is_favourite(path) {
        return GroupKey::Favourites;
    }
    if shape.open_roots.iter().any(|root| root == path) {
        return GroupKey::Open;
    }
    match store.entry(path) {
        Some(entry) if entry.origin == Origin::Found => entry
            .source
            .clone()
            .map_or(GroupKey::Added, GroupKey::FoundIn),
        _ => GroupKey::Added,
    }
}

fn matches_filter(path: &Path, filter: &str) -> bool {
    let wanted = filter.trim().to_lowercase();
    wanted.is_empty()
        || name_of(path).to_lowercase().contains(&wanted)
        || path.to_string_lossy().to_lowercase().contains(&wanted)
}

/// Builds the groups, without the empty ones.
pub fn build(store: &Store, shape: &Shape<'_>) -> Vec<Group> {
    let mut by_group: BTreeMap<GroupKey, Vec<&Path>> = BTreeMap::new();
    for entry in &store.repos {
        let path = entry.path.as_path();
        if is_hidden_worktree(path, store, shape.readings) {
            continue;
        }
        if !matches_filter(path, shape.filter) {
            continue;
        }
        let reading = shape.readings.get(path);
        if shape.only_needing_action
            && !reading.is_some_and(|read| read.needs_action() || read.is_unreachable())
        {
            continue;
        }
        by_group
            .entry(group_of(path, store, shape))
            .or_default()
            .push(path);
    }
    by_group
        .into_iter()
        .map(|(key, mut paths)| {
            // The favourites keep the order of the file: the user put them there.
            if key != GroupKey::Favourites {
                paths.sort_by_cached_key(|path| (name_of(path).to_lowercase(), path.to_path_buf()));
            }
            let open = !shape.folded.contains(&key);
            let count = paths.len();
            let rows = if open {
                paths
                    .into_iter()
                    .flat_map(|path| {
                        repo_with_worktrees(path, shape.readings, shape.show_worktrees)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Group {
                key,
                count,
                open,
                rows,
            }
        })
        .collect()
}

fn repo_with_worktrees(
    path: &Path,
    readings: &BTreeMap<PathBuf, Reading>,
    show_worktrees: bool,
) -> Vec<Row> {
    let mut rows = vec![Row {
        path: path.to_path_buf(),
        kind: RowKind::Repo,
        owner: path.to_path_buf(),
        branch: None,
    }];
    let summary = readings.get(path).and_then(Reading::summary);
    if let Some(summary) = summary.filter(|_| show_worktrees) {
        rows.extend(
            summary
                .worktrees
                .iter()
                .filter(|worktree| worktree.path != path)
                .map(|worktree| Row {
                    path: worktree.path.clone(),
                    kind: RowKind::Worktree,
                    owner: path.to_path_buf(),
                    branch: worktree.branch.clone(),
                }),
        );
    }
    rows
}

/// Every row of every open group, top to bottom: what the cursor walks over.
pub fn visible_paths(groups: &[Group]) -> Vec<PathBuf> {
    groups
        .iter()
        .flat_map(|group| group.rows.iter().map(|row| row.path.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_git::{RepoSummary, WorktreeSummary};

    fn store_of(entries: &[(&str, Origin, Option<&str>, bool)]) -> Store {
        let mut store = Store::default();
        for (path, origin, source, favourite) in entries {
            store.add(
                PathBuf::from(path),
                origin.clone(),
                source.map(PathBuf::from),
            );
            if *favourite {
                store.toggle_favourite(Path::new(path));
            }
        }
        store
    }

    fn read(summary: RepoSummary) -> Reading {
        Reading::Read(Box::new(summary))
    }

    fn shape<'a>(
        readings: &'a BTreeMap<PathBuf, Reading>,
        open_roots: &'a [PathBuf],
        folded: &'a BTreeSet<GroupKey>,
    ) -> Shape<'a> {
        Shape {
            readings,
            open_roots,
            filter: "",
            only_needing_action: false,
            show_worktrees: true,
            folded,
        }
    }

    #[test]
    fn a_favourite_stands_once_and_the_groups_come_in_order() {
        let store = store_of(&[
            ("/code/zeta", Origin::Found, Some("/code"), false),
            ("/code/alpha", Origin::Found, Some("/code"), true),
            ("/usb/notes", Origin::Learned, None, false),
            ("/code/open", Origin::Found, Some("/code"), false),
            ("/gone", Origin::Manual, None, true),
        ]);
        let mut readings = BTreeMap::new();
        readings.insert(PathBuf::from("/gone"), Reading::Gone);
        let open = [PathBuf::from("/code/open")];
        let folded = BTreeSet::new();
        let groups = build(&store, &shape(&readings, &open, &folded));
        let keys: Vec<_> = groups.iter().map(|group| group.key.clone()).collect();
        assert_eq!(
            keys,
            [
                GroupKey::Favourites,
                GroupKey::Open,
                GroupKey::Unreachable,
                GroupKey::FoundIn(PathBuf::from("/code")),
                GroupKey::Added,
            ]
        );
        let all: Vec<_> = groups
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| row.path.clone()))
            .collect();
        assert_eq!(all.iter().filter(|path| path.ends_with("alpha")).count(), 1);
    }

    #[test]
    fn rows_are_sorted_by_name_without_regard_to_case() {
        let store = store_of(&[
            ("/c/Beta", Origin::Manual, None, false),
            ("/c/alpha", Origin::Manual, None, false),
            ("/c/Gamma", Origin::Manual, None, false),
        ]);
        let readings = BTreeMap::new();
        let folded = BTreeSet::new();
        let groups = build(&store, &shape(&readings, &[], &folded));
        let names: Vec<_> = groups
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| name_of(&row.path)))
            .collect();
        assert_eq!(names, ["alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn favourites_keep_the_order_of_the_file() {
        let store = store_of(&[
            ("/c/zulu", Origin::Manual, None, true),
            ("/c/alpha", Origin::Manual, None, true),
            ("/c/mike", Origin::Manual, None, true),
        ]);
        let readings = BTreeMap::new();
        let folded = BTreeSet::new();
        let groups = build(&store, &shape(&readings, &[], &folded));
        let names: Vec<_> = groups
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| name_of(&row.path)))
            .collect();
        assert_eq!(names, ["zulu", "alpha", "mike"]);
    }

    #[test]
    fn a_worktree_hangs_below_its_repo_and_is_not_a_row_of_its_own() {
        let store = store_of(&[
            ("/c/main", Origin::Manual, None, false),
            ("/c/main-feature", Origin::Manual, None, false),
        ]);
        let mut readings = BTreeMap::new();
        readings.insert(
            PathBuf::from("/c/main"),
            read(RepoSummary {
                worktrees: vec![WorktreeSummary {
                    path: PathBuf::from("/c/main-feature"),
                    branch: Some("feature".to_owned()),
                }],
                ..RepoSummary::default()
            }),
        );
        readings.insert(
            PathBuf::from("/c/main-feature"),
            read(RepoSummary {
                main_worktree: Some(PathBuf::from("/c/main")),
                ..RepoSummary::default()
            }),
        );
        let folded = BTreeSet::new();
        let groups = build(&store, &shape(&readings, &[], &folded));
        let rows = &groups.first().unwrap().rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.first().unwrap().kind, RowKind::Repo);
        assert_eq!(rows[1].kind, RowKind::Worktree);
        assert_eq!(rows[1].owner, PathBuf::from("/c/main"));
        assert_eq!(groups.first().unwrap().count, 1);
    }

    #[test]
    fn the_filter_matches_the_name_and_the_place() {
        let store = store_of(&[
            ("/code/ncrs", Origin::Manual, None, false),
            ("/work/api", Origin::Manual, None, false),
        ]);
        let readings = BTreeMap::new();
        let folded = BTreeSet::new();
        let mut asked = shape(&readings, &[], &folded);
        asked.filter = "NCR";
        assert_eq!(
            visible_paths(&build(&store, &asked)),
            [PathBuf::from("/code/ncrs")]
        );
        asked.filter = "work";
        assert_eq!(
            visible_paths(&build(&store, &asked)),
            [PathBuf::from("/work/api")]
        );
    }

    #[test]
    fn only_what_needs_action_stays_when_asked() {
        let store = store_of(&[
            ("/a/clean", Origin::Manual, None, false),
            ("/a/behind", Origin::Manual, None, false),
            ("/a/gone", Origin::Manual, None, false),
        ]);
        let mut readings = BTreeMap::new();
        readings.insert(PathBuf::from("/a/clean"), read(RepoSummary::default()));
        readings.insert(
            PathBuf::from("/a/behind"),
            read(RepoSummary {
                behind: 2,
                ..RepoSummary::default()
            }),
        );
        readings.insert(PathBuf::from("/a/gone"), Reading::Gone);
        let folded = BTreeSet::new();
        let mut asked = shape(&readings, &[], &folded);
        asked.only_needing_action = true;
        let paths = visible_paths(&build(&store, &asked));
        assert_eq!(
            paths,
            [PathBuf::from("/a/gone"), PathBuf::from("/a/behind")]
        );
    }

    #[test]
    fn a_folded_group_keeps_its_count_and_loses_its_rows() {
        let store = store_of(&[("/a/x", Origin::Manual, None, false)]);
        let readings = BTreeMap::new();
        let folded = BTreeSet::from([GroupKey::Added]);
        let groups = build(&store, &shape(&readings, &[], &folded));
        let group = groups.first().unwrap();
        assert_eq!((group.count, group.open, group.rows.len()), (1, false, 0));
    }
}
