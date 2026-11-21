//! A file beside the index that says the recovery key was shown but not yet
//! confirmed, so a quit or a crash before the confirmation shows it again. It
//! holds nothing: the key is read from the credential store when it is needed.

use std::io::Write as _;

use super::index_ops::remove_if_present;
use super::*;

const MARKER_SUFFIX: &str = ".recovery-key-unconfirmed";

fn marker_path(db_path: &Path) -> Option<PathBuf> {
    let mut name = db_path.file_name()?.to_owned();
    name.push(MARKER_SUFFIX);
    Some(db_path.with_file_name(name))
}

/// Durable before it returns: the key must not be shown while the marker could
/// still be lost with a crash.
pub(in crate::filing) fn mark_unconfirmed(db_path: &Path) -> std::io::Result<()> {
    let Some(marker) = marker_path(db_path) else {
        return Ok(());
    };
    let mut pending = marker.clone().into_os_string();
    pending.push(".tmp");
    let pending = PathBuf::from(pending);
    let mut file = std::fs::File::create(&pending)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&pending, &marker)?;
    #[cfg(unix)]
    if let Some(parent) = marker.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub(in crate::filing) fn is_unconfirmed(db_path: &Path) -> bool {
    marker_path(db_path).is_some_and(|marker| marker.exists())
}

pub(in crate::filing) fn clear_unconfirmed(db_path: &Path) -> std::io::Result<()> {
    marker_path(db_path).map_or(Ok(()), |marker| remove_if_present(&marker))
}
