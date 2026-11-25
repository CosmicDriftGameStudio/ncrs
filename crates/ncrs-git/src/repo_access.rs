use std::path::Path;

use gix::ObjectId;

use crate::GitError;

const SHORT_HASH_LENGTH: usize = 7;

/// Opens the repository without the commands its own config names: a
/// repository from an unzipped download carries its own `.git/config`, and
/// showing a folder must never execute what that file names. Drivers from the
/// user's and the system's config stay, so LFS and similar filters still apply.
pub fn open(root: &Path) -> Result<gix::Repository, GitError> {
    let mut repository = gix::open(root).map_err(GitError::open)?;
    drop_repository_command_drivers(&mut repository);
    Ok(repository)
}

fn is_repository_driver_section(section: &gix::config::file::SectionRef<'_>, kind: &[u8]) -> bool {
    let header = section.header();
    section.meta().source.kind() == gix::config::source::Kind::Repository
        && header.subsection_name().is_some()
        && header.name().eq_ignore_ascii_case(kind)
}

fn drop_repository_command_drivers(repository: &mut gix::Repository) {
    let mut snapshot = repository.config_snapshot_mut();
    let drivers: Vec<_> = snapshot
        .sections_and_ids()
        .filter(|(section, _)| {
            is_repository_driver_section(section, b"filter")
                || is_repository_driver_section(section, b"diff")
        })
        .map(|(_, id)| id)
        .collect();
    for id in drivers {
        snapshot.remove_section_by_id(id);
    }
}

/// The names of the filter drivers the repository's own config defines: the
/// ones a `git` call for reading has to switch off.
pub fn repository_filter_names(root: &Path) -> Vec<String> {
    let Ok(mut repository) = gix::open(root) else {
        return Vec::new();
    };
    let snapshot = repository.config_snapshot_mut();
    let mut names: Vec<String> = snapshot
        .sections_and_ids()
        .filter(|(section, _)| is_repository_driver_section(section, b"filter"))
        .filter_map(|(section, _)| {
            section
                .header()
                .subsection_name()
                .map(|name| name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn short_hash(id: &ObjectId) -> String {
    id.to_string().chars().take(SHORT_HASH_LENGTH).collect()
}

pub fn parse_id(sha: &str) -> Result<ObjectId, GitError> {
    ObjectId::from_hex(sha.as_bytes()).map_err(GitError::read)
}

/// The branch the others are measured against, and where its tips are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultBranch {
    pub name: String,
    /// The local branch; `None` when it was never checked out here.
    pub local_tip: Option<ObjectId>,
    /// The same branch on `origin`, as last fetched.
    pub remote_tip: Option<ObjectId>,
}

impl DefaultBranch {
    /// The local tip, else the remote one.
    pub fn tip(&self) -> Option<ObjectId> {
        self.local_tip.or(self.remote_tip)
    }
}

fn tip_of(repository: &gix::Repository, full_name: &str) -> Option<ObjectId> {
    let reference = repository.find_reference(full_name).ok()?;
    Some(reference.into_fully_peeled_id().ok()?.detach())
}

/// The target of `refs/remotes/origin/HEAD`, else `main`, else `master`.
pub fn default_branch(repository: &gix::Repository) -> Option<DefaultBranch> {
    const ORIGIN_PREFIX: &str = "refs/remotes/origin/";
    let named_by_origin = repository
        .find_reference("refs/remotes/origin/HEAD")
        .ok()
        .and_then(|head| match head.target() {
            gix::refs::TargetRef::Symbolic(name) => name
                .as_bstr()
                .to_string()
                .strip_prefix(ORIGIN_PREFIX)
                .map(str::to_string),
            gix::refs::TargetRef::Object(_) => None,
        });
    let describe = |name: &str| {
        let local_tip = tip_of(repository, &format!("refs/heads/{name}"));
        let remote_tip = tip_of(repository, &format!("{ORIGIN_PREFIX}{name}"));
        (local_tip.is_some() || remote_tip.is_some()).then(|| DefaultBranch {
            name: name.to_string(),
            local_tip,
            remote_tip,
        })
    };
    named_by_origin
        .as_deref()
        .and_then(describe)
        .or_else(|| ["main", "master"].into_iter().find_map(describe))
}

/// `path` as `tree` spells it. Where the disk ignores case, a file panel or an
/// older commit may have another spelling than the tree.
pub fn path_as_in_tree(repository: &gix::Repository, tree: &gix::Tree<'_>, path: &str) -> String {
    let ignores_case = repository
        .config_snapshot()
        .boolean("core.ignorecase")
        .unwrap_or(false);
    let is_exact = matches!(tree.lookup_entry_by_path(path), Ok(Some(_)));
    if !ignores_case || is_exact {
        return path.to_string();
    }
    respelled(tree.clone(), path).unwrap_or_else(|| path.to_string())
}

fn respelled(mut tree: gix::Tree<'_>, path: &str) -> Option<String> {
    let mut spelled: Vec<String> = Vec::new();
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        let wanted = part.to_lowercase();
        let (name, below) = {
            let found = tree
                .iter()
                .filter_map(Result::ok)
                .find(|entry| entry.filename().to_string().to_lowercase() == wanted)?;
            let name = found.filename().to_string();
            let below = match parts.peek() {
                Some(_) => Some(found.object().ok()?.try_into_tree().ok()?),
                None => None,
            };
            (name, below)
        };
        spelled.push(name);
        if let Some(folder) = below {
            tree = folder;
        }
    }
    Some(spelled.join("/"))
}
