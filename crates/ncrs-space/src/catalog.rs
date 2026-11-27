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
    /// A build folder in the shared target folder (`~/.cargo-target/<repo>-<worktree>`) of a
    /// worktree that is merged or gone.
    CargoTargetHome,
    /// `node_modules` of a project that was not touched for a long time.
    NodeModulesStale,
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
            Self::CargoTargetHome => "cargo-target-home",
            Self::NodeModulesStale => NODE_MODULES_STALE,
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
            Self::CargoTarget | Self::CargoTargetHome | Self::CargoRegistryCache => {
                &["cargo", "rustc"]
            }
            Self::NodeModulesStale => &["yarn", "npm", "pnpm", "bun"],
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
                "Google" | "google-chrome" => &["Google Chrome", "Google Chrome Helper", "chrome"],
                _ => &[],
            },
            Self::WinTempOld | Self::OwnRegenerable => &[],
        }
    }
}

impl CatalogId {
    /// The command that makes the folder again, as shown after a delete ("Neu erzeugt durch ...").
    pub fn regenerated_by(self) -> &'static str {
        match self {
            Self::CargoTarget | Self::CargoTargetHome => "cargo build",
            Self::Sccache => "cargo build",
            Self::CargoRegistryCache => "cargo fetch",
            Self::Yarn | Self::NodeModulesStale => "yarn install",
            Self::Npm => "npm install",
            Self::Bun => "bun install",
            Self::Pnpm => "pnpm install",
            Self::Pip => "pip install",
            Self::Homebrew => "brew install",
            Self::DerivedData => "xcodebuild",
            Self::UnavailableSimulators => "xcrun simctl",
            Self::Gradle => "gradle build",
            Self::AppCache(_) | Self::WinTempOld | Self::OwnRegenerable => "",
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
    "Google",
    "google-chrome",
];

pub use ncrs_config::space::NODE_MODULES_STALE;

/// `node_modules` counts as stale when nothing of the project changed for this long.
const NODE_MODULES_STALE_AFTER: Duration = Duration::from_secs(90 * 24 * 60 * 60);
const LOCK_FILES: [&str; 5] = [
    "package.json",
    "yarn.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "bun.lock",
];

const TEMP_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Exact paths only, and only those that exist. A path under `never_touch` is dropped.
/// Without the merged worktrees, only removed worktrees count as old ones.
pub fn catalog(
    os: Os,
    home: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    repos: &[PathBuf],
    settings: &SpaceSettings,
) -> Vec<Place> {
    catalog_for(os, home, env, repos, settings, &[])
}

/// [`catalog`] that knows which worktrees are merged (`merged`), so their build folders in the
/// shared target folder are listed as [`CatalogId::CargoTargetHome`].
pub fn catalog_for(
    os: Os,
    home: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    repos: &[PathBuf],
    settings: &SpaceSettings,
    merged: &[PathBuf],
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
    let known_repos = known_repos(repos);
    for shared in [
        env_path("CARGO_TARGET_DIR"),
        Some(home.join(".cargo-target")),
    ]
    .into_iter()
    .flatten()
    {
        if is_one_target(&shared) {
            add(CatalogId::CargoTarget, shared, false);
            continue;
        }
        for child in subfolders(&shared) {
            let id = shared_target_id(&child, &known_repos, merged);
            add(id, child, false);
        }
    }

    if !settings
        .catalog_off
        .iter()
        .any(|off| off == NODE_MODULES_STALE)
    {
        for project in project_folders(repos) {
            if is_stale_node_modules(&project) {
                add(
                    CatalogId::NodeModulesStale,
                    project.join("node_modules"),
                    false,
                );
            }
        }
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

/// The project a Rust build folder belongs to: the folder holding `target`, or the folder
/// below the shared target folder. `None` for the shared folder itself.
pub fn cargo_project_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if name == "target" {
        return path
            .parent()?
            .file_name()
            .and_then(|parent| parent.to_str())
            .map(str::to_owned);
    }
    (name != ".cargo-target").then(|| name.to_owned())
}

/// A repository by its main folder, with the linked worktrees that exist.
struct KnownRepo {
    name: String,
    worktrees: Vec<PathBuf>,
}

fn known_repos(repos: &[PathBuf]) -> Vec<KnownRepo> {
    let mut known: Vec<KnownRepo> = Vec::new();
    for repo in repos {
        let admin = repo.join(".git/worktrees");
        let Some(name) = repo
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|_| admin.parent().is_some_and(Path::is_dir))
        else {
            continue;
        };
        let worktrees = std::fs::read_dir(admin)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| std::fs::read_to_string(entry.path().join("gitdir")).ok())
            .filter_map(|line| PathBuf::from(line.trim()).parent().map(Path::to_path_buf))
            .collect();
        if !known.iter().any(|other| other.name == name) {
            known.push(KnownRepo {
                name: name.to_owned(),
                worktrees,
            });
        }
    }
    known
}

/// Whether the folder is a target folder itself, not a folder of target folders.
fn is_one_target(folder: &Path) -> bool {
    folder.join("debug").is_dir()
        || folder.join("release").is_dir()
        || folder.join("CACHEDIR.TAG").is_file()
}

fn subfolders(folder: &Path) -> Vec<PathBuf> {
    let mut children: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    children.sort();
    children
}

/// `<repo>` is the current project's; `<repo>-<worktree>` is of an old worktree when that
/// worktree is merged or no longer exists.
fn shared_target_id(child: &Path, repos: &[KnownRepo], merged: &[PathBuf]) -> CatalogId {
    let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
        return CatalogId::CargoTarget;
    };
    if repos.iter().any(|repo| repo.name == name) {
        return CatalogId::CargoTarget;
    }
    let owner = repos
        .iter()
        .filter_map(|repo| {
            let worktree = name.strip_prefix(&repo.name)?.strip_prefix('-')?;
            Some((repo, worktree))
        })
        .max_by_key(|(repo, _)| repo.name.len());
    let Some((repo, worktree_name)) = owner else {
        return CatalogId::CargoTarget;
    };
    let existing = repo.worktrees.iter().find(|worktree| {
        worktree
            .file_name()
            .and_then(|folder| folder.to_str())
            .is_some_and(|folder| folder == worktree_name)
    });
    match existing {
        Some(worktree) if !merged.contains(worktree) => CatalogId::CargoTarget,
        Some(_) | None => CatalogId::CargoTargetHome,
    }
}

/// `node_modules` beside a `package.json`, with nothing of the project changed for 90 days.
fn is_stale_node_modules(project: &Path) -> bool {
    let modules = project.join("node_modules");
    if !project.join("package.json").is_file() || !exists_exactly(&modules, false) {
        return false;
    }
    let newest = std::iter::once(modules)
        .chain(LOCK_FILES.iter().map(|name| project.join(name)))
        .filter_map(|path| {
            std::fs::metadata(path)
                .and_then(|found| found.modified())
                .ok()
        })
        .max();
    newest.is_some_and(|newest| {
        SystemTime::now()
            .duration_since(newest)
            .is_ok_and(|age| age > NODE_MODULES_STALE_AFTER)
    })
}
