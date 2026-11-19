//! How this copy of ncrs got onto the machine, decided at run time: the .deb
//! and .rpm files are the same for a direct download and for the package repo.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Zypper,
    Homebrew,
}

impl PackageManager {
    /// What the user runs to update; shown, never run by ncrs.
    pub const fn update_command(self) -> &'static str {
        match self {
            Self::Apt => "sudo apt update && sudo apt install --only-upgrade ncrs",
            Self::Dnf => "sudo dnf upgrade ncrs",
            Self::Zypper => "sudo zypper update ncrs",
            Self::Homebrew => "brew upgrade --cask ncrs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Direct,
    PackageManager(PackageManager),
    Development,
}

const MARKERS: [(&str, PackageManager); 5] = [
    ("/etc/apt/sources.list.d/ncrs.list", PackageManager::Apt),
    ("/etc/yum.repos.d/ncrs.repo", PackageManager::Dnf),
    ("/etc/zypp/repos.d/ncrs.repo", PackageManager::Zypper),
    ("/opt/homebrew/Caskroom/ncrs", PackageManager::Homebrew),
    ("/usr/local/Caskroom/ncrs", PackageManager::Homebrew),
];

/// Whether a program at `exe` is where `manager` installs it. Not knowing the
/// location counts as yes.
fn installed_by(manager: PackageManager, exe: Option<&Path>) -> bool {
    let Some(exe) = exe else {
        return true;
    };
    match manager {
        PackageManager::Apt | PackageManager::Dnf | PackageManager::Zypper => {
            exe.starts_with("/usr")
        }
        PackageManager::Homebrew => exe.starts_with("/Applications"),
    }
}

impl Channel {
    /// `exe` is where the running program lives: a package marker only counts
    /// when this copy sits where that package manager puts it, so a repository
    /// set up next to a downloaded copy does not turn it into a package install.
    pub fn detect(development: bool, exists: impl Fn(&Path) -> bool, exe: Option<&Path>) -> Self {
        if development {
            return Self::Development;
        }
        MARKERS
            .iter()
            .find(|(path, manager)| exists(Path::new(path)) && installed_by(*manager, exe))
            .map_or(Self::Direct, |(_, manager)| Self::PackageManager(*manager))
    }

    pub fn detect_here(development: bool) -> Self {
        Self::detect(
            development,
            Path::exists,
            std::env::current_exe().ok().as_deref(),
        )
    }

    /// The check is on by default only for a direct download: the package
    /// manager and the developer have their own way to learn about a release.
    pub const fn checks_by_default(self) -> bool {
        matches!(self, Self::Direct)
    }

    pub const fn update_command(self) -> Option<&'static str> {
        match self {
            Self::PackageManager(manager) => Some(manager.update_command()),
            Self::Direct | Self::Development => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(present: &'static str) -> impl Fn(&Path) -> bool {
        move |path| path == Path::new(present)
    }

    #[test]
    fn a_dev_build_is_development_even_when_a_marker_exists() {
        let channel = Channel::detect(true, |_| true, None);
        assert_eq!(channel, Channel::Development);
    }

    #[test]
    fn each_marker_names_its_package_manager() {
        for (path, manager) in MARKERS.iter() {
            assert_eq!(
                Channel::detect(false, only(path), None),
                Channel::PackageManager(*manager),
                "{path}"
            );
        }
    }

    #[test]
    fn a_marker_does_not_count_for_a_copy_outside_the_package_location() {
        let apt = only("/etc/apt/sources.list.d/ncrs.list");
        assert_eq!(
            Channel::detect(false, &apt, Some(Path::new("/home/me/Downloads/ncrs"))),
            Channel::Direct
        );
        assert_eq!(
            Channel::detect(false, &apt, Some(Path::new("/usr/lib/ncrs/ncrs"))),
            Channel::PackageManager(PackageManager::Apt)
        );
        let cask = only("/opt/homebrew/Caskroom/ncrs");
        assert_eq!(
            Channel::detect(
                false,
                &cask,
                Some(Path::new(
                    "/Users/me/Downloads/ncrs.app/Contents/MacOS/ncrs"
                ))
            ),
            Channel::Direct
        );
    }

    #[test]
    fn without_a_marker_it_is_a_direct_download() {
        assert_eq!(Channel::detect(false, |_| false, None), Channel::Direct);
    }

    #[test]
    fn only_a_direct_download_checks_by_default() {
        assert!(Channel::Direct.checks_by_default());
        assert!(!Channel::Development.checks_by_default());
        assert!(!Channel::PackageManager(PackageManager::Apt).checks_by_default());
    }

    #[test]
    fn the_update_commands_are_the_documented_ones() {
        assert_eq!(
            PackageManager::Apt.update_command(),
            "sudo apt update && sudo apt install --only-upgrade ncrs"
        );
        assert_eq!(
            PackageManager::Dnf.update_command(),
            "sudo dnf upgrade ncrs"
        );
        assert_eq!(
            PackageManager::Zypper.update_command(),
            "sudo zypper update ncrs"
        );
        assert_eq!(
            PackageManager::Homebrew.update_command(),
            "brew upgrade --cask ncrs"
        );
        assert_eq!(Channel::Direct.update_command(), None);
    }
}
