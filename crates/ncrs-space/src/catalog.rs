use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ncrs_config::space::SpaceSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// A known kind of regenerable folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogId {
    CargoTarget,
    Sccache,
    CargoRegistryCache,
    Yarn,
    Npm,
    Bun,
    Pnpm,
    Pip,
    Homebrew,
    DerivedData,
    UnavailableSimulators,
    Gradle,
    /// A known tool's folder under `~/Library/Caches`, `~/.cache` or `%LOCALAPPDATA%`, by folder name.
    AppCache(&'static str),
    WinTempOld,
    /// A folder of the user's own that a tool regenerates, named in the settings.
    OwnRegenerable,
}

impl CatalogId {
    /// The stable name used in the config file.
    pub fn name(self) -> &'static str {
        match self {
            Self::CargoTarget => "cargo-target",
            Self::Sccache => "sccache",
            Self::CargoRegistryCache => "cargo-registry-cache",
            Self::Yarn => "yarn",
            Self::Npm => "npm",
            Self::Bun => "bun",
            Self::Pnpm => "pnpm",
            Self::Pip => "pip",
            Self::Homebrew => "homebrew",
            Self::DerivedData => "derived-data",
            Self::UnavailableSimulators => "unavailable-simulators",
            Self::Gradle => "gradle",
            Self::AppCache(name) => name,
            Self::WinTempOld => "temp-old",
            Self::OwnRegenerable => "own-regenerable",
        }
    }

    /// Process names (without `.exe`, compared ignoring case) of the tools that write the folder.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Self::CargoTarget | Self::CargoRegistryCache => &["cargo", "rustc"],
            Self::Sccache => &["sccache"],
            Self::Yarn => &["yarn", "node"],
            Self::Npm => &["npm", "npx", "node"],
            Self::Bun => &["bun"],
            Self::Pnpm => &["pnpm", "node"],
            Self::Pip => &["pip", "pip3", "uv"],
            Self::Homebrew => &["brew"],
            Self::DerivedData => &["xcodebuild", "Xcode"],
            Self::UnavailableSimulators => &["simctl", "xcodebuild", "Xcode"],
            Self::Gradle => &["gradle", "java"],
            Self::AppCache(name) => match name {
                "ms-playwright" => &["playwright"],
                "go-build" => &["go"],
                "uv" => &["uv"],
                "pre-commit" => &["pre-commit"],
                _ => &[],
            },
            Self::WinTempOld | Self::OwnRegenerable => &[],
        }
    }
}

/// One catalog folder that exists on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub id: CatalogId,
    pub path: PathBuf,
    /// Process names that write the folder; one of them running skips it.
    pub tools: &'static [&'static str],
}

/// Folder names of tools whose cache is regenerated on the next run, under the OS cache folder.
const KNOWN_APP_CACHES: &[&str] = &[
    "ms-playwright",
    "go-build",
    "typescript",
    "node-gyp",
    "pypoetry",
    "CocoaPods",
    "org.swift.swiftpm",
    "Cypress",
    "puppeteer",
    "pre-commit",
    "uv",
    "mypy",
    "JetBrains",
];

const TEMP_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Exact paths only, and only those that exist. A path under `never_touch` is dropped.
pub fn catalog(
    os: Os,
    home: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    repos: &[PathBuf],
    settings: &SpaceSettings,
) -> Vec<Place> {
    let env_path = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let mut places: Vec<Place> = Vec::new();
    let mut add = |id: CatalogId, path: PathBuf, files_too: bool| {
        if exists_exactly(&path, files_too) && !places.iter().any(|known| known.path == path) {
            places.push(Place {
                id,
                tools: id.tools(),
                path,
            });
        }
    };

    let local_app_data = env_path("LOCALAPPDATA");
    let cache_root = match os {
        Os::MacOs => Some(home.join("Library/Caches")),
        Os::Linux => Some(env_path("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"))),
        Os::Windows => None,
    };
    let in_app_data = |tool: &str, below: &str| {
        local_app_data
            .as_ref()
            .map(|root| root.join(tool).join(below))
    };

    let cargo_home = env_path("CARGO_HOME").unwrap_or_else(|| home.join(".cargo"));
    add(
        CatalogId::CargoRegistryCache,
        cargo_home.join("registry/cache"),
        false,
    );

    let sccache = env_path("SCCACHE_DIR").or_else(|| match os {
        Os::MacOs => cache_root.as_ref().map(|root| root.join("Mozilla.sccache")),
        Os::Linux => cache_root.as_ref().map(|root| root.join("sccache")),
        Os::Windows => in_app_data("Mozilla", "sccache/cache"),
    });
    if let Some(path) = sccache {
        add(CatalogId::Sccache, path, false);
    }

    let yarn = match os {
        Os::MacOs => cache_root.as_ref().map(|root| root.join("Yarn")),
        Os::Linux => cache_root.as_ref().map(|root| root.join("yarn")),
        Os::Windows => in_app_data("Yarn", "Cache"),
    };
    let pip = match os {
        Os::MacOs | Os::Linux => cache_root.as_ref().map(|root| root.join("pip")),
        Os::Windows => in_app_data("pip", "Cache"),
    };
    let pnpm = match os {
        Os::MacOs => Some(home.join("Library/pnpm/store")),
        Os::Linux => Some(home.join(".local/share/pnpm/store")),
        Os::Windows => in_app_data("pnpm", "store"),
    };
    let npm = match os {
        Os::Windows => in_app_data("npm-cache", "_cacache"),
        Os::MacOs | Os::Linux => Some(home.join(".npm/_cacache")),
    };
    for (id, path) in [
        (CatalogId::Yarn, yarn),
        (CatalogId::Pip, pip),
        (CatalogId::Pnpm, pnpm),
        (CatalogId::Npm, npm),
        (CatalogId::Bun, Some(home.join(".bun/install/cache"))),
        (CatalogId::Gradle, Some(home.join(".gradle/caches"))),
    ] {
        if let Some(path) = path {
            add(id, path, false);
        }
    }

    if os == Os::MacOs {
        add(
            CatalogId::Homebrew,
            home.join("Library/Caches/Homebrew"),
            false,
        );
        add(
            CatalogId::DerivedData,
            home.join("Library/Developer/Xcode/DerivedData"),
            false,
        );
        if cfg!(all(target_os = "macos", not(feature = "sandbox"))) {
            add(
                CatalogId::UnavailableSimulators,
                home.join("Library/Developer/CoreSimulator/Devices"),
                false,
            );
        }
    }

    for &name in KNOWN_APP_CACHES {
        let path = match os {
            Os::Windows => in_app_data(name, "Cache"),
            Os::MacOs | Os::Linux => cache_root.as_ref().map(|root| root.join(name)),
        };
        if let Some(path) = path {
            add(CatalogId::AppCache(name), path, false);
        }
    }

    for project in project_folders(repos) {
        if project.join("Cargo.toml").is_file() {
            add(CatalogId::CargoTarget, project.join("target"), false);
        }
    }
    for shared in [
        env_path("CARGO_TARGET_DIR"),
        Some(home.join(".cargo-target")),
    ]
    .into_iter()
    .flatten()
    {
        add(CatalogId::CargoTarget, shared, false);
    }

    if os == Os::Windows {
        if let Some(temp) = env_path("TEMP") {
            for old in old_children(&temp) {
                add(CatalogId::WinTempOld, old, true);
            }
        }
    }

    for own in &settings.own_regenerable {
        add(CatalogId::OwnRegenerable, own.clone(), false);
    }

    places.retain(|place| {
        !settings
            .catalog_off
            .iter()
            .any(|off| off == place.id.name())
    });
    places.retain(|place| !is_under_any(&place.path, &settings.never_touch));
    places
}

/// Whether `path` is `root` or lies below it, compared by path components.
pub(crate) fn is_under_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

/// A real folder (a symlink does not count), or with `files_too` a real file.
fn exists_exactly(path: &Path, files_too: bool) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|found| found.file_type().is_dir() || (files_too && found.file_type().is_file()))
}

/// The repositories and their linked worktrees, each once.
fn project_folders(repos: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for repo in repos {
        let admin = repo.join(".git/worktrees");
        let linked = std::fs::read_dir(admin)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| std::fs::read_to_string(entry.path().join("gitdir")).ok())
            .filter_map(|line| PathBuf::from(line.trim()).parent().map(Path::to_path_buf));
        for folder in std::iter::once(repo.clone()).chain(linked) {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    }
    folders
}

/// Direct children of `folder` not touched for a week.
fn old_children(folder: &Path) -> Vec<PathBuf> {
    let now = SystemTime::now();
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|found| found.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|age| age > TEMP_MAX_AGE)
                })
        })
        .map(|entry| entry.path())
        .collect()
}
