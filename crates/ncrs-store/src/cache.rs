//! The last good store answer, so a Mac that is offline keeps its state.

use std::fs;
use std::path::Path;

use crate::status::StoreStatus;

const CACHE_FILE: &str = "store-status.json";

pub fn load_cached(folder: &Path) -> Option<StoreStatus> {
    let text = fs::read_to_string(folder.join(CACHE_FILE)).ok()?;
    StoreStatus::from_json(&text).ok()
}

/// A failed write only costs the offline fallback, so it is reported, not fatal.
pub fn save_cached(folder: &Path, status: &StoreStatus) -> std::io::Result<()> {
    fs::create_dir_all(folder)?;
    let target = folder.join(CACHE_FILE);
    let temporary = folder.join(format!("{CACHE_FILE}.tmp"));
    fs::write(&temporary, status.to_json())?;
    fs::rename(&temporary, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Environment;

    #[test]
    fn saves_and_loads_in_a_new_folder() {
        let Ok(root) = tempfile::tempdir() else {
            return;
        };
        let folder = root.path().join("nested");
        let status = StoreStatus {
            environment: Some(Environment::Production),
            original_app_version: Some("1.6.0".to_owned()),
            trial_started_at: Some(42),
            ..StoreStatus::default()
        };
        assert_eq!(save_cached(&folder, &status).ok(), Some(()));
        assert_eq!(load_cached(&folder), Some(status));
    }

    #[test]
    fn missing_or_broken_cache_is_none() {
        let Ok(root) = tempfile::tempdir() else {
            return;
        };
        assert_eq!(load_cached(root.path()), None);
        assert_eq!(
            fs::write(root.path().join(CACHE_FILE), "nope").ok(),
            Some(())
        );
        assert_eq!(load_cached(root.path()), None);
    }
}
