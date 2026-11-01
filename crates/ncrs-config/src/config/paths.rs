use super::*;

/// Where the config file lives on this machine, or `None` when the variables
/// that say so are missing.
pub fn default_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_path");
    config_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

pub const PORTABLE_DIR: &str = "data";

/// The `data` folder next to the executable, when there is one: it makes
/// this copy of ncrs portable, and config and index live there instead of in
/// the user's profile. `None` means the normal layout. The executable path
/// should be canonical, so the result can serve as a stable key.
pub fn portable_data_dir_of(exe: &Path, is_dir: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let dir = exe.parent()?;
    if is_inside_app_bundle(dir) {
        return None;
    }
    let data = dir.join(PORTABLE_DIR);
    is_dir(&data).then_some(data)
}

/// `Foo.app/Contents/MacOS`: the bundle is replaced on update and may sit in
/// a read-only place, so a `data` folder there is not a portable marker.
pub fn is_inside_app_bundle(exe_dir: &Path) -> bool {
    let contents = exe_dir.parent();
    exe_dir.file_name() == Some("MacOS".as_ref())
        && contents.and_then(Path::file_name) == Some("Contents".as_ref())
        && contents
            .and_then(Path::parent)
            .and_then(Path::extension)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

/// The portable data directory of the running executable. A failing
/// `current_exe` or `canonicalize` means the normal layout.
pub fn portable_data_dir() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::portable_data_dir");
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()?;
    portable_data_dir_of(&exe, Path::is_dir)
}

/// Config and index both ask this, so they never end up in different modes.
pub fn config_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match portable {
        Some(dir) => Some(dir.join("config.toml")),
        None => path_from_env(var),
    }
}

pub fn index_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match portable {
        Some(dir) => Some(dir.join("index.db")),
        None => index_path_from_env(var),
    }
}

pub fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

/// Windows: `%APPDATA%\ncrs\config.toml`.
#[cfg(windows)]
pub fn path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("APPDATA")).map(|dir| dir.join("ncrs").join("config.toml"))
}

/// macOS: always `~/.config`. A Dock launch sees no shell exports, so honouring
/// `XDG_CONFIG_HOME` would make the file depend on how ncrs was started.
#[cfg(target_os = "macos")]
pub fn path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    home_config(var)
}

/// Other Unix: `$XDG_CONFIG_HOME` if set and absolute, else `~/.config`.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("XDG_CONFIG_HOME"))
        .map(|dir| dir.join("ncrs").join("config.toml"))
        .or_else(|| home_config(var))
}

/// No fallback to the working directory or `/` the way `fs::home_dir` has: a
/// config read from wherever ncrs started would be a surprise.
#[cfg(not(windows))]
pub fn home_config(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("HOME")).map(|home| home.join(".config").join("ncrs").join("config.toml"))
}

/// Where the index database lives on this machine, or `None` when the
/// variables that say so are missing.
pub fn default_index_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_index_path");
    index_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

/// Where the remembered session (panels, sort, window) lives, or `None` when
/// the variables that say so are missing. The only place that decides it, so a
/// sandboxed build has one path to change.
pub fn default_state_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_state_path");
    state_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

pub fn state_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    index_path_in(portable, var).map(|index| index.with_file_name("state.toml"))
}

/// Where the record of what ncrs moved to the trash lives (macOS can not list
/// its trash folder), or `None` when the variables that say so are missing.
pub fn default_journal_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_journal_path");
    journal_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

pub fn journal_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    index_path_in(portable, var).map(|index| index.with_file_name("journal.toml"))
}

/// Where the host keys of SFTP servers the user confirmed in ncrs are kept, or
/// `None` when the variables that say so are missing. In the data folder next
/// to the index, so the sandboxed build keeps it in its container.
pub fn default_known_hosts_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_known_hosts_path");
    known_hosts_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

pub fn known_hosts_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    index_path_in(portable, var).map(|index| index.with_file_name("known_hosts"))
}

/// The user's own OpenSSH `known_hosts`, which ncrs only reads. Inside the App
/// Sandbox it can be read once the home folder has been granted.
pub fn default_ssh_known_hosts_path() -> PathBuf {
    forbid_user_machine_in_tests("config::default_ssh_known_hosts_path");
    ncrs_fs::home_dir().join(".ssh").join("known_hosts")
}

/// Windows: `%APPDATA%\ncrs\index.db`.
#[cfg(windows)]
pub fn index_path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("APPDATA")).map(|dir| dir.join("ncrs").join("index.db"))
}

/// macOS: always `~/Library/Application Support`, for the same reason the
/// config ignores `XDG_CONFIG_HOME`.
#[cfg(target_os = "macos")]
pub fn index_path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("HOME")).map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("ncrs")
            .join("index.db")
    })
}

/// Other Unix: `$XDG_DATA_HOME` if set and absolute, else `~/.local/share`.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn index_path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("XDG_DATA_HOME"))
        .or_else(|| absolute(var("HOME")).map(|home| home.join(".local").join("share")))
        .map(|dir| dir.join("ncrs").join("index.db"))
}

/// Where F3 puts the copy of a file it unpacks from an archive, or `None`
/// when the variables that say so are missing. A cache folder: the copies
/// are temporary and nothing there is worth a backup.
pub fn default_view_copies_dir() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_view_copies_dir");
    view_copies_dir_in(portable_data_dir(), |name| std::env::var_os(name))
}

pub fn view_copies_dir_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match portable {
        Some(dir) => Some(dir.join("view")),
        None => view_copies_dir_from_env(var),
    }
}

/// Windows: `%LOCALAPPDATA%\ncrs\view`.
#[cfg(windows)]
pub fn view_copies_dir_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("LOCALAPPDATA")).map(|dir| dir.join("ncrs").join("view"))
}

/// macOS: `~/Library/Caches/ncrs/view`.
#[cfg(target_os = "macos")]
pub fn view_copies_dir_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("HOME")).map(|home| {
        home.join("Library")
            .join("Caches")
            .join("ncrs")
            .join("view")
    })
}

/// Other Unix: `$XDG_CACHE_HOME` if set and absolute, else `~/.cache`.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn view_copies_dir_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("XDG_CACHE_HOME"))
        .or_else(|| absolute(var("HOME")).map(|home| home.join(".cache")))
        .map(|dir| dir.join("ncrs").join("view"))
}

/// The home directory `~` stands for. Like the config path, only an absolute
/// one counts. In the App Sandbox it is the real home, not the container that
/// `$HOME` names: `~` in a config value means the user's folders.
pub fn home_from_env() -> Option<PathBuf> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    ncrs_fs::preferred_home(ncrs_fs::sandbox_home(), || {
        absolute(std::env::var_os(variable))
    })
}
