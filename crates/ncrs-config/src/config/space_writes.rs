use super::setting_writes::{write_settings, Setting};
use super::*;
use crate::space::{Depth, SpaceSettings};

pub const SPACE_TABLE: &str = "space";

/// The whole `[space]` section, every line, like `repos_settings`.
pub fn space_settings(settings: &SpaceSettings) -> Vec<Setting> {
    let strings = |entries: Vec<String>| {
        toml_edit::Value::Array(entries.iter().map(String::as_str).collect())
    };
    let folders = |folders: &[PathBuf]| {
        strings(
            folders
                .iter()
                .map(|folder| folder.to_string_lossy().into_owned())
                .collect(),
        )
    };
    let setting = |key: &'static str, value: toml_edit::Value| Setting {
        tables: &[SPACE_TABLE],
        key,
        value: Some(value),
    };
    let depth: toml_edit::Value = match settings.depth {
        Depth::Auto => "auto".into(),
        Depth::Levels(levels) => i64::from(levels).into(),
    };
    vec![
        setting(
            "warn_from_percent",
            i64::from(settings.warn_from_percent).into(),
        ),
        setting("catalog_off", strings(settings.catalog_off.clone())),
        setting("own_regenerable", folders(&settings.own_regenerable)),
        setting("never_touch", folders(&settings.never_touch)),
        setting("show_worktrees", settings.show_worktrees.into()),
        setting("depth", depth),
        setting("other_drives", settings.other_drives.into()),
    ]
}

/// Writes the whole `[space]` section; same guarantees as [`write_theme_preset`].
pub fn write_space(path: &Path, settings: &SpaceSettings) -> Result<Config, FilingWriteError> {
    write_settings(path, &space_settings(settings), home_from_env().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_is_written_and_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let settings = SpaceSettings {
            warn_from_percent: 80,
            catalog_off: vec!["Yarn".to_owned()],
            own_regenerable: vec![PathBuf::from("/work/build-cache")],
            never_touch: vec![PathBuf::from("/work/keep")],
            show_worktrees: false,
            depth: Depth::Levels(3),
            other_drives: true,
        };
        let read_back = write_space(&path, &settings).unwrap().space;
        assert_eq!(read_back, settings);
    }

    #[test]
    fn auto_depth_is_written_as_a_word_and_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let read_back = write_space(&path, &SpaceSettings::default()).unwrap().space;
        assert_eq!(read_back.depth, Depth::Auto);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("depth = \"auto\""), "{text}");
    }

    #[test]
    fn writing_keeps_the_comments_of_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(
            &path,
            "# my notes\n[space]\n# how full is full\nwarn_from_percent = 70\n",
        )
        .unwrap();
        let settings = SpaceSettings {
            warn_from_percent: 85,
            ..SpaceSettings::default()
        };
        write_space(&path, &settings).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my notes"), "{text}");
        assert!(text.contains("# how full is full"), "{text}");
        assert!(text.contains("warn_from_percent = 85"), "{text}");
    }

    #[test]
    fn an_unknown_key_in_the_section_is_refused() {
        from_text("[space]\nspeed = 3\n").unwrap_err();
    }

    #[test]
    fn the_keys_of_the_platz_tab_are_read() {
        let config = from_text("[keys.space]\nrescan = [\"ctrl+r\"]\n").unwrap();
        assert_eq!(config.space_keys.entries.len(), 1);
    }
}
