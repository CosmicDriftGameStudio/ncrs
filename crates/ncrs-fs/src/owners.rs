//! Owner and group names of a listing. The core entry only has room for the
//! result; looking ids up in the user database is this layer's job.

use ncrs_core::FileEntry;

#[cfg(unix)]
pub fn fill_owners(entries: &mut [FileEntry]) {
    use std::collections::HashMap;
    use std::os::unix::fs::MetadataExt as _;

    use nix::unistd::{Gid, Group, Uid, User};

    // One lookup per id and listing: a folder of thousands of files has a
    // handful of owners.
    let mut users: HashMap<u32, String> = HashMap::new();
    let mut groups: HashMap<u32, String> = HashMap::new();
    for entry in entries.iter_mut().filter(|entry| !entry.is_parent) {
        let Ok(metadata) =
            std::fs::metadata(&entry.path).or_else(|_| entry.path.symlink_metadata())
        else {
            continue;
        };
        let (uid, gid) = (metadata.uid(), metadata.gid());
        let owner = users.entry(uid).or_insert_with(|| {
            User::from_uid(Uid::from_raw(uid))
                .ok()
                .flatten()
                .map_or_else(|| uid.to_string(), |user| user.name)
        });
        entry.details.owner = Some(owner.clone());
        let group = groups.entry(gid).or_insert_with(|| {
            Group::from_gid(Gid::from_raw(gid))
                .ok()
                .flatten()
                .map_or_else(|| gid.to_string(), |group| group.name)
        });
        entry.details.group = Some(group.clone());
    }
}

#[cfg(not(unix))]
pub(crate) fn fill_owners(_entries: &mut [FileEntry]) {}
