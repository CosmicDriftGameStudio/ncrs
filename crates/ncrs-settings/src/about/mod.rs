//! The about card: version, licence and the file with the licences of the
//! libraries ncrs is built from. The state has no widgets; the view is
//! `view`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub mod view;

pub use view::{view, CLOSE_FOCUS, LICENSES_FOCUS, REPOSITORY_FOCUS};

pub const REPOSITORY_URL: &str = "https://github.com/CosmicDriftGameStudio/ncrs";

const LICENSES_FILE: &str = "THIRD_PARTY_LICENSES.html";

/// Where each packaging puts the licence file, relative to the executable's
/// folder: next to it (archives), in the app bundle's `Resources`, and under
/// `share/doc/ncrs` of a prefix (brew, and deb and rpm, whose real executable is
/// `/usr/lib/ncrs/ncrs` behind the `/usr/bin/ncrs` symlink).
const LICENSES_LOCATIONS: [&str; 4] = [
    LICENSES_FILE,
    "../Resources/THIRD_PARTY_LICENSES.html",
    "../share/doc/ncrs/THIRD_PARTY_LICENSES.html",
    "../../share/doc/ncrs/THIRD_PARTY_LICENSES.html",
];

pub fn third_party_licenses_candidates(exe: &Path) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    LICENSES_LOCATIONS
        .iter()
        .map(|location| dir.join(location))
        .collect()
}

pub fn third_party_licenses_path(exe: &Path, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    third_party_licenses_candidates(exe)
        .into_iter()
        .find(|candidate| exists(candidate))
}

/// The licence file of the running installation. A failing `current_exe` or
/// `canonicalize` means there is none to find.
pub fn installed_third_party_licenses() -> Option<PathBuf> {
    ncrs_fs::forbid_user_machine_in_tests("about::installed_third_party_licenses");
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()?;
    third_party_licenses_path(&exe, Path::is_file)
}

/// The release page copy of the licence file, for builds that do not carry it.
pub fn third_party_licenses_url(version: &str) -> String {
    format!("{REPOSITORY_URL}/releases/download/v{version}/{LICENSES_FILE}")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AboutCard {
    licenses_missing: bool,
}

impl AboutCard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn licenses_missing(&self) -> bool {
        self.licenses_missing
    }

    pub fn report_licenses_missing(&mut self) {
        self.licenses_missing = true;
    }
}

/// Everything the about card sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutMessage {
    Open,
    Close,
    OpenLicenses,
    OpenRepository,
}

/// What the card asks the app for: starting the system's opener, which can
/// fail, and a failed launch closes the card so the error in the status bar is
/// not hidden behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Launch { target: OsString, name: String },
}

/// Applies `message` to the card that is up, if any. `find_licenses` names the
/// licence file of the running installation.
pub fn update(
    card: &mut Option<AboutCard>,
    message: AboutMessage,
    find_licenses: fn() -> Option<PathBuf>,
) -> Option<Action> {
    match message {
        AboutMessage::Open => *card = Some(AboutCard::new()),
        AboutMessage::Close => *card = None,
        AboutMessage::OpenLicenses => match find_licenses() {
            Some(path) => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                return Some(Action::Launch {
                    target: path.into_os_string(),
                    name,
                });
            }
            None => {
                if let Some(open) = card.as_mut() {
                    open.report_licenses_missing();
                }
            }
        },
        AboutMessage::OpenRepository => {
            return Some(Action::Launch {
                target: REPOSITORY_URL.into(),
                name: REPOSITORY_URL.to_owned(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(present: &'static str) -> impl Fn(&Path) -> bool {
        move |path| path == Path::new(present)
    }

    #[test]
    fn a_file_next_to_the_executable_is_found() {
        let found = third_party_licenses_path(
            Path::new("/opt/ncrs/ncrs"),
            only("/opt/ncrs/THIRD_PARTY_LICENSES.html"),
        );
        assert_eq!(
            found,
            Some(PathBuf::from("/opt/ncrs/THIRD_PARTY_LICENSES.html"))
        );
    }

    #[test]
    fn a_file_in_the_app_bundle_resources_is_found() {
        let found = third_party_licenses_path(
            Path::new("/Applications/ncrs.app/Contents/MacOS/ncrs"),
            only("/Applications/ncrs.app/Contents/MacOS/../Resources/THIRD_PARTY_LICENSES.html"),
        );
        assert!(found.is_some());
    }

    #[test]
    fn a_file_under_share_doc_of_the_prefix_is_found() {
        let found = third_party_licenses_path(
            Path::new("/usr/bin/ncrs"),
            only("/usr/bin/../share/doc/ncrs/THIRD_PARTY_LICENSES.html"),
        );
        assert!(found.is_some());
    }

    #[test]
    fn a_file_under_share_doc_is_found_from_the_resolved_deb_executable() {
        let found = third_party_licenses_path(
            Path::new("/usr/lib/ncrs/ncrs"),
            only("/usr/lib/ncrs/../../share/doc/ncrs/THIRD_PARTY_LICENSES.html"),
        );
        assert!(found.is_some());
    }

    #[test]
    fn without_a_file_nothing_is_found() {
        assert_eq!(
            third_party_licenses_path(Path::new("/usr/bin/ncrs"), |_| false),
            None
        );
    }

    #[test]
    fn the_online_copy_is_the_release_asset_of_the_version() {
        assert_eq!(
            third_party_licenses_url("1.2.3"),
            "https://github.com/CosmicDriftGameStudio/ncrs/releases/download/v1.2.3/THIRD_PARTY_LICENSES.html"
        );
    }

    #[test]
    fn a_missing_licenses_file_is_reported_on_the_card_and_launches_nothing() {
        let mut card = Some(AboutCard::new());
        let action = update(&mut card, AboutMessage::OpenLicenses, || None);
        assert_eq!(action, None);
        assert!(card.is_some_and(|open| open.licenses_missing()));
    }

    #[test]
    fn the_repository_button_asks_for_a_launch_and_close_drops_the_card() {
        let mut card = None;
        update(&mut card, AboutMessage::Open, || None);
        assert!(card.is_some());
        let action = update(&mut card, AboutMessage::OpenRepository, || None);
        assert_eq!(
            action,
            Some(Action::Launch {
                target: REPOSITORY_URL.into(),
                name: REPOSITORY_URL.to_owned()
            })
        );
        update(&mut card, AboutMessage::Close, || None);
        assert!(card.is_none());
    }
}
