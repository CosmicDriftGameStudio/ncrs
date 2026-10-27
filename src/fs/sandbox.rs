//! What differs when ncrs runs as the App Store build inside the App Sandbox:
//! the real home folder, the folder grants and the trash. Everything here is
//! switched on by [`sandbox_active`], which is true only for a build with the
//! `sandbox` feature that really runs in the sandbox. Any other build, and any
//! other system, behaves as if this file did not exist.

use std::path::{Path, PathBuf};

/// Whether the sandbox rules apply: built with the `sandbox` feature and
/// started inside the App Sandbox.
#[cfg(feature = "sandbox")]
pub fn sandbox_active() -> bool {
    super::open::runs_in_app_sandbox()
}

#[cfg(not(feature = "sandbox"))]
pub const fn sandbox_active() -> bool {
    false
}

/// The real home folder while the sandbox is active, `None` otherwise. Inside
/// the sandbox `$HOME` names the container, which holds the config, the state
/// and the index, and is not what the user means by home.
#[cfg(all(feature = "sandbox", target_os = "macos"))]
pub fn sandbox_home() -> Option<PathBuf> {
    sandbox_active().then(ncrs_sandbox::real_home).flatten()
}

#[cfg(not(all(feature = "sandbox", target_os = "macos")))]
pub const fn sandbox_home() -> Option<PathBuf> {
    None
}

/// The home folder to use: the real one from the sandbox if there is one,
/// otherwise what `from_env` says.
pub fn preferred_home(
    sandbox_home: Option<PathBuf>,
    from_env: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    sandbox_home.or_else(from_env)
}

/// The sandbox side of the app, as one value so tests can put in their own.
#[derive(Debug, Clone)]
pub struct SandboxAccess {
    pub active: bool,
    /// Where the grants are kept; `None` without a data folder.
    pub grants_dir: Option<PathBuf>,
    /// Whether the home folder can be listed, which it cannot before the user
    /// has granted it.
    pub home_readable: fn() -> bool,
    /// Saves a grant for a folder in a grants folder.
    pub grant: fn(&Path, &Path) -> Result<(), String>,
}

impl SandboxAccess {
    /// What this process is: active or not, and the real checks.
    pub fn detect(index_path: Option<&Path>) -> Self {
        Self {
            active: sandbox_active(),
            grants_dir: index_path.and_then(grants_dir),
            home_readable: home_is_readable,
            grant: grant_folder,
        }
    }

    /// Not sandboxed, with checks that touch nothing.
    #[cfg(test)]
    pub fn inactive() -> Self {
        Self {
            active: false,
            grants_dir: None,
            home_readable: || true,
            grant: |_, _| Ok(()),
        }
    }

    /// Whether to ask the user for the home folder: only in the sandbox, and
    /// only while listing it fails.
    pub fn needs_home_grant(&self) -> bool {
        self.active && !(self.home_readable)()
    }
}

#[cfg(all(feature = "sandbox", target_os = "macos"))]
fn grants_dir(index_path: &Path) -> Option<PathBuf> {
    super::grants::GrantStore::dir_beside(index_path)
}

#[cfg(not(all(feature = "sandbox", target_os = "macos")))]
fn grants_dir(_index_path: &Path) -> Option<PathBuf> {
    None
}

fn home_is_readable() -> bool {
    std::fs::read_dir(super::home_dir()).is_ok()
}

/// The open panel is where the user hands out access, so the dialog starts in
/// the real home.
pub fn grant_start_dir() -> PathBuf {
    super::home_dir()
}

#[cfg(all(feature = "sandbox", target_os = "macos"))]
fn grant_folder(grants_dir: &Path, folder: &Path) -> Result<(), String> {
    super::grants::GrantStore::new(grants_dir.to_path_buf())
        .add(&SystemBookmarks, folder)
        .map_err(|error| error.to_string())
}

#[cfg(not(all(feature = "sandbox", target_os = "macos")))]
fn grant_folder(_grants_dir: &Path, _folder: &Path) -> Result<(), String> {
    Err("folder grants exist only in the App Store build".to_owned())
}

/// Brings back the access to every folder the user granted in an earlier run.
/// Called once at the start, before the panels read anything. Nothing happens
/// outside the sandbox.
#[cfg(all(feature = "sandbox", target_os = "macos"))]
pub fn restore_grants(index_path: Option<&Path>) {
    if !sandbox_active() {
        return;
    }
    let Some(dir) = index_path.and_then(grants_dir) else {
        return;
    };
    let report = super::grants::GrantStore::new(dir).restore_all(&SystemBookmarks);
    eprintln!("ncrs: {report}");
}

#[cfg(not(all(feature = "sandbox", target_os = "macos")))]
pub const fn restore_grants(_index_path: Option<&Path>) {}

/// The bookmarks of the system, through `ncrs-sandbox`.
#[cfg(all(feature = "sandbox", target_os = "macos"))]
struct SystemBookmarks;

#[cfg(all(feature = "sandbox", target_os = "macos"))]
impl super::grants::Bookmarks for SystemBookmarks {
    fn create(&self, path: &Path) -> Result<Vec<u8>, super::grants::BookmarkFailure> {
        ncrs_sandbox::create_bookmark(path).map_err(failure)
    }

    fn resolve(
        &self,
        bytes: &[u8],
    ) -> Result<super::grants::Resolution, super::grants::BookmarkFailure> {
        ncrs_sandbox::resolve_bookmark(bytes)
            .map(|resolved| super::grants::Resolution {
                path: resolved.path,
                stale: resolved.stale,
                access_started: resolved.access_started,
            })
            .map_err(failure)
    }

    fn path_of(&self, bytes: &[u8]) -> Result<PathBuf, super::grants::BookmarkFailure> {
        ncrs_sandbox::bookmark_path(bytes).map_err(failure)
    }
}

#[cfg(all(feature = "sandbox", target_os = "macos"))]
fn failure(error: ncrs_sandbox::BookmarkError) -> super::grants::BookmarkFailure {
    match error {
        ncrs_sandbox::BookmarkError::Corrupt(reason) => {
            super::grants::BookmarkFailure::Corrupt(reason)
        }
        ncrs_sandbox::BookmarkError::Unavailable(reason) => {
            super::grants::BookmarkFailure::Unavailable(reason)
        }
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "sandbox"))]
    #[test]
    fn without_the_feature_the_sandbox_is_never_active() {
        assert!(!sandbox_active());
        assert_eq!(sandbox_home(), None);
    }

    /// A developer's shell is no sandbox, with the feature or without.
    #[test]
    fn outside_the_sandbox_nothing_is_active() {
        if std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_none() {
            assert!(!sandbox_active());
        }
    }

    #[test]
    fn the_real_home_wins_over_the_environment() {
        let real = Some(PathBuf::from("real"));
        let from_env = || Some(PathBuf::from("container"));
        assert_eq!(preferred_home(real, from_env), Some(PathBuf::from("real")));
    }

    #[test]
    fn the_environment_home_is_used_without_a_real_one() {
        let from_env = || Some(PathBuf::from("container"));
        assert_eq!(
            preferred_home(None, from_env),
            Some(PathBuf::from("container"))
        );
        assert_eq!(preferred_home(None, || None), None);
    }

    #[test]
    fn the_environment_is_not_even_asked_when_the_real_home_is_known() {
        let home = preferred_home(Some(PathBuf::from("real")), || {
            panic!("the environment was read")
        });
        assert_eq!(home, Some(PathBuf::from("real")));
    }

    fn access(active: bool, readable: bool) -> SandboxAccess {
        SandboxAccess {
            active,
            grants_dir: None,
            home_readable: if readable { || true } else { || false },
            grant: |_, _| Ok(()),
        }
    }

    #[test]
    fn the_home_grant_is_asked_for_only_in_the_sandbox_and_only_while_home_is_closed() {
        assert!(access(true, false).needs_home_grant());
        assert!(!access(true, true).needs_home_grant());
        assert!(!access(false, false).needs_home_grant());
    }
}
