use super::setting_writes::{write_settings, Setting};
use super::*;
use crate::git::GitSettings;

pub const GIT_TABLE: &str = "git";

/// The whole `[git]` section, every line, like `repos_settings`.
pub fn git_settings(settings: &GitSettings) -> Vec<Setting> {
    let setting = |key: &'static str, name: &str| Setting {
        tables: &[GIT_TABLE],
        key,
        value: Some(name.into()),
    };
    vec![
        setting("pull", settings.pull.name()),
        setting("fetch", settings.fetch.name()),
    ]
}

/// Writes the whole `[git]` section; same guarantees as [`write_theme_preset`].
pub fn write_git(path: &Path, settings: &GitSettings) -> Result<Config, FilingWriteError> {
    write_settings(path, &git_settings(settings), home_from_env().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{FetchInterval, PullDefault};

    #[test]
    fn the_section_is_written_and_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let settings = GitSettings {
            pull: PullDefault::Rebase,
            fetch: FetchInterval::Off,
        };
        let read_back = write_git(&path, &settings).unwrap().git;
        assert_eq!(read_back, settings);
    }

    #[test]
    fn an_unknown_key_in_the_section_is_refused() {
        assert!(from_text("[git]\nspeed = 3\n").is_err());
    }
}
