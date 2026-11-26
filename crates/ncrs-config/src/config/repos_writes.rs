use super::setting_writes::{write_settings, Setting};
use super::*;
use crate::repos::ReposSettings;

pub const REPOS_TABLE: &str = "repos";

/// The whole `[repos]` section. Every line is written, so what the page shows
/// is what the file says even when a default changes in a later version.
pub fn repos_settings(settings: &ReposSettings) -> Vec<Setting> {
    let strings = |entries: Vec<String>| {
        toml_edit::Value::Array(entries.iter().map(String::as_str).collect())
    };
    let setting = |key: &'static str, value: toml_edit::Value| Setting {
        tables: &[REPOS_TABLE],
        key,
        value: Some(value),
    };
    let width = |key: &'static str, pixels: Option<u16>| Setting {
        tables: &[REPOS_TABLE],
        key,
        value: pixels.map(|width| i64::from(width).into()),
    };
    vec![
        setting(
            "sources",
            strings(
                settings
                    .sources
                    .iter()
                    .map(|folder| folder.to_string_lossy().into_owned())
                    .collect(),
            ),
        ),
        setting("scan_home", settings.scan_home.into()),
        setting("depth", i64::from(settings.depth).into()),
        setting("skip", strings(settings.skip.clone())),
        setting("skip_cloud", settings.skip_cloud.into()),
        setting("skip_network", settings.skip_network.into()),
        setting("learn_from_panel", settings.learn_from_panel.into()),
        setting("concurrency", i64::from(settings.concurrency).into()),
        Setting {
            tables: &[REPOS_TABLE],
            key: "show_worktrees",
            value: (!settings.show_worktrees).then(|| false.into()),
        },
        width("name_width", settings.columns.name),
        width("branch_width", settings.columns.branch),
        width("standing_width", settings.columns.standing),
        width("changes_width", settings.columns.changes),
        width("fetched_width", settings.columns.fetched),
    ]
}

/// Writes the whole `[repos]` section; same guarantees as
/// [`write_theme_preset`].
pub fn write_repos(path: &Path, settings: &ReposSettings) -> Result<Config, FilingWriteError> {
    write_settings(path, &repos_settings(settings), home_from_env().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_is_written_and_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let settings = ReposSettings {
            sources: vec![PathBuf::from("/work/code")],
            scan_home: true,
            depth: 4,
            skip: vec!["vendor".to_owned()],
            skip_cloud: false,
            skip_network: true,
            learn_from_panel: false,
            concurrency: 8,
            show_worktrees: false,
            columns: crate::repos::ReposColumnWidths {
                name: Some(300),
                standing: Some(180),
                ..Default::default()
            },
        };
        let read_back = write_repos(&path, &settings).unwrap().repos;
        assert_eq!(read_back, settings);
    }

    #[test]
    fn the_standard_switch_and_widths_write_no_line() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        write_repos(&path, &ReposSettings::default()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("show_worktrees"), "{text}");
        assert!(!text.contains("_width"), "{text}");
    }

    #[test]
    fn an_unknown_key_in_the_section_is_refused() {
        from_text("[repos]\nspeed = 3\n").unwrap_err();
    }

    #[test]
    fn the_keys_of_the_repos_tab_are_read() {
        let config = from_text("[keys.repos]\nfetch = [\"ctrl+shift+f\"]\n").unwrap();
        assert_eq!(config.repos_keys.entries.len(), 1);
    }
}
