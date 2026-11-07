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

impl Channel {
    pub fn detect(development: bool, exists: impl Fn(&Path) -> bool) -> Self {
        if development {
            return Self::Development;
        }
        MARKERS
            .iter()
            .find(|(path, _)| exists(Path::new(path)))
            .map_or(Self::Direct, |(_, manager)| Self::PackageManager(*manager))
    }

    pub fn detect_here(development: bool) -> Self {
        Self::detect(development, Path::exists)
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
        let channel = Channel::detect(true, |_| true);
        assert_eq!(channel, Channel::Development);
    }

    #[test]
    fn each_marker_names_its_package_manager() {
        for (path, manager) in MARKERS.iter() {
            assert_eq!(
                Channel::detect(false, only(path)),
                Channel::PackageManager(*manager),
                "{path}"
            );
        }
    }

    #[test]
    fn without_a_marker_it_is_a_direct_download() {
        assert_eq!(Channel::detect(false, |_| false), Channel::Direct);
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
