//! Cloning a repository into a folder, without anyone to ask for a login.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{ActionError, Git};

/// A clone is the one unattended call that may take long: a big history over a slow line.
const CLONE_TIMEOUT: Duration = Duration::from_secs(1800);

/// The folder a clone of `url` gets: the last part of the address without a
/// trailing `/` or `.git`, `None` where nothing is left.
fn folder_name_of(url: &str) -> Option<String> {
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':', '\\']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_owned())
}

/// Clones `url` into a new folder inside `parent`, named after the address.
/// Never asks for a login (git fails with an authentication error instead)
/// and never runs the `ext::` transport. Returns the new folder.
pub fn clone(address: &str, parent: &Path) -> Result<PathBuf, ActionError> {
    let url = address.trim();
    let invalid = || ActionError::InvalidName {
        name: url.to_owned(),
    };
    if url.is_empty() || url.starts_with('-') {
        return Err(invalid());
    }
    let name = folder_name_of(url).ok_or_else(invalid)?;
    let git = Git::open_unattended(parent)?.with_timeout(CLONE_TIMEOUT);
    git.talk(
        &["-c", "protocol.ext.allow=never", "clone", "--", url, &name],
        url,
    )?;
    Ok(parent.join(name))
}

#[cfg(test)]
mod tests {
    use super::folder_name_of;

    #[test]
    fn the_folder_is_named_after_the_address() {
        for (url, name) in [
            ("https://github.com/a/b.git", "b"),
            ("https://github.com/a/b/", "b"),
            ("git@github.com:a/b.git", "b"),
            ("git@host:b", "b"),
            ("/srv/git/b", "b"),
        ] {
            assert_eq!(folder_name_of(url).as_deref(), Some(name), "{url}");
        }
        assert_eq!(folder_name_of("https://host/.git"), None);
    }
}
