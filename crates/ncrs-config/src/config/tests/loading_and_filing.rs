use super::*;

pub fn write(dir: &tempfile::TempDir, content: &[u8]) -> PathBuf {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, content).unwrap();
    path
}

pub fn target_of(filing: &Filing) -> Option<&FilingLocation> {
    filing.locations.iter().find(|location| location.is_target)
}

pub fn load_text(content: &str) -> Config {
    let dir = tempfile::tempdir().unwrap();
    load(&write(&dir, content.as_bytes())).unwrap()
}

/// The (line, reason) of the error the content produces.
pub fn error_line(content: &str) -> (Option<usize>, String) {
    let dir = tempfile::tempdir().unwrap();
    let err = load(&write(&dir, content.as_bytes())).unwrap_err();
    assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
    (err.line(), err.reason().to_owned())
}

#[test]
fn a_missing_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        load(&dir.path().join("nope.toml")).unwrap(),
        Config::default()
    );
}

#[test]
fn an_empty_file_gives_the_defaults() {
    assert_eq!(load_text(""), Config::default());
}

#[test]
fn a_full_file_is_read() {
    let config =
        load_text("version = 1\n\n[open]\nview = [\"viewer\", \"--x\"]\nedit = [\"editor\"]\n");
    let view = config.open.view.unwrap();
    assert_eq!(view.program(), "viewer");
    assert_eq!(view.args(), ["--x"]);
    let edit = config.open.edit.unwrap();
    assert_eq!(edit.program(), "editor");
    assert!(edit.args().is_empty());
}

#[test]
fn a_missing_version_means_version_1() {
    let config = load_text("[open]\nview = [\"viewer\"]\n");
    assert_eq!(config.version, Version(1));
    assert_eq!(load_text("version = 1\n").version, Version(1));
}

#[test]
fn version_0_does_not_exist() {
    let (line, reason) = error_line("version = 0\n");
    assert_eq!(line, Some(1));
    assert_eq!(reason, "version 0 does not exist, the first version is 1");
}

// Absolute on each platform, or a `~/` path below it is refused.
#[cfg(windows)]
pub const FIXTURE_HOME: &str = r"C:\Users\test";
#[cfg(not(windows))]
pub const FIXTURE_HOME: &str = "/home/test";

pub fn load_fixture(text: &str) -> Config {
    parse_config(
        text,
        Path::new("config.toml"),
        Some(Path::new(FIXTURE_HOME)),
    )
    .unwrap()
}

pub fn pressed(config: &Config, key: iced::keyboard::key::Named) -> Option<KeyAction> {
    config.keymap.action_for(
        &iced::keyboard::Key::Named(key),
        iced::keyboard::Modifiers::default(),
        crate::keymap::KeyScope::Panel,
    )
}

pub fn assert_the_0_3_0_settings(config: &Config) {
    use iced::keyboard::key::Named;
    assert_eq!(config.version, Version(1));
    assert_eq!(config.lang, Some(Language::German));
    let view = config.open.view.as_ref().unwrap();
    assert_eq!((view.program(), view.args()), ("open", &[] as &[String]));
    let edit = config.open.edit.as_ref().unwrap();
    assert_eq!(
        (edit.program(), edit.args()),
        ("open", &["-t".to_owned()][..])
    );
    assert_eq!(pressed(config, Named::F2), Some(KeyAction::Copy));
    assert_eq!(pressed(config, Named::F5), None);
    assert_eq!(pressed(config, Named::F3), Some(KeyAction::Edit));
    assert_eq!(pressed(config, Named::F4), Some(KeyAction::View));
    assert_eq!(config.keymap.bindings_of(KeyAction::Quit).count(), 2);
    assert_eq!(
        config
            .keymap
            .bindings_of(KeyAction::DeletePermanently)
            .count(),
        0
    );
    let classic = ThemePreset::Classic.palette();
    assert_eq!(config.palette.background, classic.background);
    assert_eq!(
        config.palette.directory,
        iced::Color::from_rgb8(0xff, 0xff, 0x55)
    );
    assert_eq!(
        config.palette.tagged,
        iced::Color::from_rgb8(0xaa, 0x55, 0x00)
    );
}

pub fn assert_the_filing_settings(config: &Config) {
    let home = Path::new(FIXTURE_HOME);
    let filing = &config.filing;
    assert!(filing.enabled);
    assert_eq!(filing.load, Load::Balanced);
    assert_eq!(
        filing.locations,
        [
            FilingLocation {
                path: home.join("Inbox"),
                mode: LocationMode::Inbox,
                images: false,
                is_target: false,
            },
            FilingLocation {
                path: home.join("Documents/Archive"),
                mode: LocationMode::Archive,
                images: true,
                is_target: true,
            },
        ]
    );
    assert_ne!(filing.ai, AiSetting::Off);
}

#[test]
fn the_config_of_0_3_0_still_loads() {
    let config = load_fixture(include_str!(
        "../../../../../tests/fixtures/config/v0.3.0.toml"
    ));
    assert_the_0_3_0_settings(&config);
    assert!(!config.filing.enabled);
}

#[test]
fn the_config_of_0_5_0_still_loads() {
    let config = load_fixture(include_str!(
        "../../../../../tests/fixtures/config/v0.5.0.toml"
    ));
    assert_the_0_3_0_settings(&config);
    assert_the_filing_settings(&config);
}

#[test]
fn the_config_of_0_7_0_still_loads() {
    use iced::keyboard::key::Named;
    let config = load_fixture(include_str!(
        "../../../../../tests/fixtures/config/v0.7.0.toml"
    ));
    assert_eq!(config.lang, Some(Language::German));
    assert_eq!(pressed(&config, Named::F12), Some(KeyAction::Copy));
    assert_eq!(pressed(&config, Named::F5), None);
    assert_eq!(pressed(&config, Named::F3), Some(KeyAction::Edit));
    assert_eq!(config.keymap.bindings_of(KeyAction::Quit).count(), 2);
    assert_eq!(
        config.palette.directory,
        iced::Color::from_rgb8(0xff, 0xff, 0x55)
    );
    assert_the_filing_settings(&config);
}

pub fn default_pattern_of(content: &str) -> Option<String> {
    load_filing(content, None).unwrap().filing.default_pattern
}

#[test]
fn a_valid_default_pattern_is_read_and_an_invalid_one_is_ignored() {
    assert_eq!(default_pattern_of("[modules.filing]\n"), None);
    assert_eq!(
        default_pattern_of("[modules.filing]\ndefault_pattern = \"{year}/{sender}/\"\n"),
        Some("{year}/{sender}/".to_owned())
    );
    for invalid in ["{nope}/x", "", "/abs/{year}", "{year"] {
        let text = format!("[modules.filing]\ndefault_pattern = '{invalid}'\n");
        assert_eq!(default_pattern_of(&text), None, "{invalid}");
    }
}

#[test]
fn a_default_pattern_of_an_older_version_is_upgraded_without_touching_the_file() {
    let text = "[modules.filing]\ndefault_pattern = \"{tax_context}/{year}/{sender}\"\n";
    assert_eq!(
        default_pattern_of(text),
        Some("{year}/{sender}/".to_owned())
    );
    assert_eq!(
        default_pattern_of("[modules.filing]\ndefault_pattern = \"{tax_context}\"\n"),
        Some(ncrs_intake::FALLBACK_PATTERN.to_owned())
    );
    assert_eq!(
        default_pattern_of("[modules.filing]\ndefault_pattern = \"{year}/{sender}\"\n"),
        Some("{year}/{sender}/".to_owned())
    );
}

#[test]
fn the_tags_of_the_file_list_default_to_kind_and_own_and_drop_what_is_unknown() {
    let default = from_text("version = 1\n").unwrap().file_list_tags;
    assert!(default.show);
    assert_eq!(default.groups, [TagGroup::Kind, TagGroup::Own]);

    let text = "[file_list_tags]\nshow = false\ngroups = [\"year\", \"nope\", \"sender\"]\nonly = [\" Firma A \", \"\", \"firma-a\"]\n";
    let read = from_text(text).unwrap().file_list_tags;
    assert!(!read.show);
    assert_eq!(read.groups, [TagGroup::Sender, TagGroup::Year]);
    assert!(from_text("[file_list_tags]\ngroups = []\n")
        .unwrap()
        .file_list_tags
        .groups
        .is_empty());
    from_text("[file_list_tags]\nmore = 1\n").unwrap_err();
}

#[test]
fn the_tags_of_the_file_list_are_written_keeping_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "version = 1\n# my note\nlang = \"en\"\n").unwrap();
    let setting = FileListTags {
        show: false,
        groups: vec![TagGroup::Recipient, TagGroup::Own],
    };

    let written = write_file_list_tags(&path, &setting).unwrap();

    assert_eq!(written.file_list_tags, setting);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my note"));
    assert!(text.contains("[file_list_tags]"));
    assert!(!text.contains("only"));
}

#[test]
fn an_old_file_with_the_only_key_loads_and_the_key_is_not_written_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n[file_list_tags]\nshow = false\nonly = [\"steuer\"]\n",
    )
    .unwrap();
    assert!(
        !from_text(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .file_list_tags
            .show
    );

    let written = write_file_list_tags(&path, &FileListTags::default()).unwrap();

    assert!(written.file_list_tags.show);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("only"), "{text}");
    assert!(!text.contains("steuer"), "{text}");
}

pub const VALID_IBAN: &str = "DE89 3704 0044 0532 0130 00";

#[test]
fn the_own_details_are_read_without_empty_repeated_or_invalid_entries() {
    let text = format!(
        "[modules.filing]\nown_names = [\" Max Muster \", \"\", \"Max Muster\", \"Muster GmbH\"]\nown_ibans = [\"{VALID_IBAN}\", \"DE00 1234\", \" \"]\nown_tax_ids = [\"98/765/43210\", \"\"]\n"
    );

    let filing = load_filing(&text, None).unwrap().filing;

    assert_eq!(filing.own_names, ["Max Muster", "Muster GmbH"]);
    assert_eq!(filing.own_ibans, [VALID_IBAN]);
    assert_eq!(filing.own_tax_ids, ["98/765/43210"]);
    assert!(load_filing("[modules.filing]\n", None)
        .unwrap()
        .filing
        .own_names
        .is_empty());
}

#[test]
fn the_own_details_are_written_and_dropped_keeping_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n# my note\nlang = \"en\"\n\n[modules.filing]\nenabled = true\n",
    )
    .unwrap();
    let names = vec!["Max Muster".to_owned(), "Muster GmbH".to_owned()];

    let written = write_own_details(&path, OwnDetail::Names, &names).unwrap();
    let with_iban = write_own_details(&path, OwnDetail::Ibans, &[VALID_IBAN.to_owned()]).unwrap();

    assert_eq!(written.filing.own_names, names);
    assert_eq!(with_iban.filing.own_names, names);
    assert_eq!(with_iban.filing.own_ibans, [VALID_IBAN]);
    let dropped = write_own_details(&path, OwnDetail::Names, &[]).unwrap();
    assert!(dropped.filing.own_names.is_empty());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my note"));
    assert!(text.contains("enabled = true"));
    assert!(!text.contains("own_names"));
}

#[test]
fn the_default_pattern_is_written_changed_and_dropped_keeping_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "version = 1\n# my note\nlang = \"en\"\n\n[modules.filing]\nenabled = true\n",
    )
    .unwrap();

    let written = write_default_pattern(&path, Some("{year}/{number}")).unwrap();
    assert_eq!(
        written.filing.default_pattern.as_deref(),
        Some("{year}/{number}")
    );
    let changed = write_default_pattern(&path, Some("{date}_{sender}")).unwrap();
    assert_eq!(
        changed.filing.default_pattern.as_deref(),
        Some("{date}_{sender}")
    );
    let dropped = write_default_pattern(&path, None).unwrap();
    assert_eq!(dropped.filing.default_pattern, None);

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my note"));
    assert!(text.contains("enabled = true"));
    assert!(!text.contains("default_pattern"));
}

#[test]
fn a_writer_refuses_a_file_of_a_newer_version_and_leaves_it_alone() {
    let dir = tempfile::tempdir().unwrap();
    for text in ["version = 2\nlang = \"en\"\n", "version = 0\n"] {
        let path = write(&dir, text.as_bytes());
        let err = write_language(&path, Language::German).unwrap_err();
        assert!(
            matches!(
                err,
                FilingWriteError::Config(ConfigError::Invalid { line: Some(1), .. })
            ),
            "{err:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn a_writer_does_not_touch_the_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, b"# mine\nversion = 1\n");
    write_language(&path, Language::German).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.starts_with("# mine\nversion = 1\n"), "{written}");
    let without_version = write(&dir, b"lang = \"en\"\n");
    write_language(&without_version, Language::German).unwrap();
    assert!(!std::fs::read_to_string(&without_version)
        .unwrap()
        .contains("version"));
}

#[test]
fn the_start_language_is_read_and_optional() {
    assert_eq!(load_text("lang = \"de\"\n").lang, Some(Language::German));
    assert_eq!(load_text("lang = \"en\"\n").lang, Some(Language::English));
    assert_eq!(load_text("").lang, None);
}

#[test]
fn the_futurama_variants_are_read() {
    assert_eq!(
        load_text("lang = \"en-futurama\"\n").lang,
        Some(Language::EnglishFuturama)
    );
    assert_eq!(
        load_text("lang = \"de-futurama\"\n").lang,
        Some(Language::GermanFuturama)
    );
    assert_eq!(load_text("lang = \"de\"\n").language_warning, None);
}

#[test]
fn a_version_one_file_of_an_earlier_release_keeps_its_language() {
    let config = load_text("version = 1\nlang = \"en\"\n");
    assert_eq!(config.lang, Some(Language::English));
    assert_eq!(config.language_warning, None);
}

#[test]
fn an_unknown_language_loads_with_english_and_a_warning_at_its_line() {
    let config = load_text("version = 1\nlang = \"fr\"\n");
    assert_eq!(config.lang, Some(Language::English));
    let warning = config.language_warning.unwrap();
    assert_eq!(warning.line, Some(2));
    assert_eq!(warning.code, "fr");
}

#[test]
fn kinds_without_an_id_and_a_word_of_two_kinds_are_skipped_with_a_warning() {
    let config = load_text(
        "version = 1\n\
         [[kinds]]\nid = \"  \"\nwords = [\"Notiz\"]\n\
         [[kinds]]\nid = \"payslip\"\nde = \"Lohnabrechnung\"\nwords = [\"Gehaltsabrechnung\"]\n\
         [[kinds]]\nid = \"wage\"\nwords = [\"Gehaltsabrechnung\", \"Lohnzettel\"]\n",
    );

    let warning = config.kind_warning.unwrap();
    assert_eq!(
        warning.skips,
        vec![
            KindSkip::EmptyId,
            KindSkip::Word {
                id: "wage".to_owned(),
                word: "Gehaltsabrechnung".to_owned(),
            },
        ]
    );
    assert_eq!(config.kinds.stored_kind("Gehaltsabrechnung"), "payslip");
    assert_eq!(config.kinds.stored_kind("Lohnabrechnung"), "payslip");
    assert_eq!(config.kinds.stored_kind("Lohnzettel"), "wage");
    assert_eq!(config.kinds.stored_kind("Notiz"), "Notiz");
    assert_eq!(config.kinds.stored_kind("Rechnung"), "invoice");
}

#[test]
fn valid_kinds_load_without_a_warning() {
    let config = load_text("version = 1\n[[kinds]]\nid = \"invoice\"\nwords = [\"Honorarnote\"]\n");
    assert_eq!(config.kind_warning, None);
    assert_eq!(config.kinds.stored_kind("honorarnote"), "invoice");
}

#[test]
fn the_futurama_language_is_written_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, b"# mine\nlang = \"en\"\n");
    write_language(&path, Language::GermanFuturama).unwrap();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("lang = \"de-futurama\""));
    assert_eq!(load(&path).unwrap().lang, Some(Language::GermanFuturama));
}

#[test]
fn a_program_name_and_an_absolute_path_are_valid() {
    #[cfg(windows)]
    let absolute = r#"["C:\\Tools\\edit.exe"]"#;
    #[cfg(not(windows))]
    let absolute = r#"["/usr/bin/vim", "-R"]"#;
    let text = format!("[open]\nview = [\"less\"]\nedit = {absolute}\n");
    let config = load_text(&text);
    assert_eq!(config.open.view.unwrap().program(), "less");
    assert!(config.open.edit.is_some());
}

#[test]
fn errors_name_the_exact_line() {
    let cases: &[(&str, &str, usize)] = &[
        ("unknown top-level key", "version = 1\n\nnope = 1\n", 3),
        (
            "unknown key in [open]",
            "version = 1\n[open]\ncolour = \"red\"\n",
            3,
        ),
        ("wrong type", "version = 1\n[open]\nview = \"open\"\n", 3),
        ("empty array", "version = 1\n[open]\nview = []\n", 3),
        ("empty program", "version = 1\n[open]\nview = [\"\"]\n", 3),
        (
            "relative path",
            "version = 1\n[open]\nedit = [\"bin/edit\"]\n",
            3,
        ),
        (
            "home-relative path",
            "version = 1\n[open]\nedit = [\"~/bin/edit\"]\n",
            3,
        ),
        ("NUL", "version = 1\n[open]\nedit = [\"a\\u0000b\"]\n", 3),
        #[cfg(windows)]
        (
            "drive-relative path",
            "version = 1\n[open]\nedit = [\"C:edit.exe\"]\n",
            3,
        ),
        ("version", "# comment\nversion = 2\n", 2),
        ("syntax", "version = 1\n\n[open\n", 3),
    ];
    for (name, text, line) in cases {
        let (found, reason) = error_line(text);
        assert_eq!(found, Some(*line), "{name}: {reason}");
    }
}

#[test]
fn the_reasons_say_what_is_wrong() {
    let cases = [
        ("version = 1\n[open]\nview = []\n", "at least the program"),
        ("version = 1\n[open]\nview = [\" \"]\n", "program is empty"),
        (
            "version = 1\n[open]\nview = [\"a/b\"]\n",
            "PATH or an absolute path",
        ),
        (
            "version = 2\n",
            "config.toml is version 2, written by a newer ncrs; this ncrs reads up to version 1",
        ),
    ];
    for (text, expected) in cases {
        let (_, found) = error_line(text);
        assert!(found.contains(expected), "{found}");
    }
}

#[test]
fn invalid_utf8_reports_its_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, b"version = 1\n# \xff\xfe\n");
    let err = load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
    assert_eq!(err.line(), Some(2));
}

#[test]
fn a_directory_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let err = load(dir.path()).unwrap_err();
    assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
    assert_eq!(err.line(), None);
}

#[cfg(unix)]
#[test]
fn a_fifo_is_refused_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("config.toml");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let err = load(&fifo).unwrap_err();
    assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
}

#[test]
fn a_file_over_one_mebibyte_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "#".repeat(1024 * 1024 + 1).as_bytes());
    let err = load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
}

#[test]
fn the_message_names_path_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, b"version = 1\n\nnope = 1\n");
    let text = load(&path).unwrap_err().to_string();
    assert!(text.contains(&path.display().to_string()), "{text}");
    assert!(text.contains("line 3"), "{text}");
}

pub fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(*value))
    }
}

pub fn dirs<'a>(existing: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
    move |path| existing.iter().any(|dir| Path::new(dir) == path)
}

#[test]
fn a_data_folder_next_to_the_exe_makes_it_portable() {
    let exe = Path::new("/home/user/ncrs/ncrs");
    assert_eq!(
        portable_data_dir_of(exe, dirs(&["/home/user/ncrs/data"])),
        Some(PathBuf::from("/home/user/ncrs/data"))
    );
    assert_eq!(portable_data_dir_of(exe, dirs(&[])), None);
}

#[test]
fn a_data_file_or_a_data_folder_elsewhere_is_not_a_marker() {
    let exe = Path::new("/home/user/ncrs/ncrs");
    // A file named `data` is not a directory, and is_dir says so.
    assert_eq!(portable_data_dir_of(exe, dirs(&[])), None);
    assert_eq!(
        portable_data_dir_of(exe, dirs(&["/home/user/data", "/home/user/ncrs/sub/data"])),
        None
    );
}

#[test]
fn a_real_file_named_data_does_not_count() {
    let dir = std::env::temp_dir().join(format!("ncrs-portable-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("data"), b"x").unwrap();
    let found = portable_data_dir_of(&dir.join("ncrs"), Path::is_dir);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(found, None);
}

#[test]
fn inside_a_macos_app_bundle_a_data_folder_is_ignored() {
    let exe = Path::new("/Applications/ncrs.app/Contents/MacOS/ncrs");
    let everything = |_: &Path| true;
    assert_eq!(portable_data_dir_of(exe, everything), None);
    let plain = Path::new("/home/user/MacOS/ncrs");
    assert!(portable_data_dir_of(plain, everything).is_some());
}

#[test]
fn portable_mode_wins_over_the_profile_directories() {
    let portable = Some(PathBuf::from("/home/user/ncrs/data"));
    let vars = [
        ("APPDATA", r"C:\Users\user\AppData\Roaming"),
        ("XDG_CONFIG_HOME", "/home/user/cfg"),
        ("XDG_DATA_HOME", "/home/user/share"),
        ("HOME", "/home/user"),
    ];
    assert_eq!(
        config_path_in(portable.clone(), env(&vars)),
        Some(PathBuf::from("/home/user/ncrs/data/config.toml"))
    );
    assert_eq!(
        index_path_in(portable, env(&vars)),
        Some(PathBuf::from("/home/user/ncrs/data/index.db"))
    );
}

#[test]
fn the_state_lives_next_to_the_index() {
    let vars = [
        ("HOME", "/home/user"),
        ("APPDATA", r"C:\Users\user\AppData"),
    ];
    let portable = Some(PathBuf::from("/home/user/ncrs/data"));
    assert_eq!(
        state_path_in(portable, env(&vars)),
        Some(PathBuf::from("/home/user/ncrs/data/state.toml"))
    );
    let index = index_path_in(None, env(&vars)).unwrap();
    let state = state_path_in(None, env(&vars)).unwrap();
    assert_eq!(state.parent(), index.parent());
    assert_eq!(state.file_name().unwrap(), "state.toml");
}

#[test]
fn the_trash_journal_lives_next_to_the_index() {
    let vars = [
        ("HOME", "/home/user"),
        ("APPDATA", r"C:\Users\user\AppData"),
    ];
    let index = index_path_in(None, env(&vars)).unwrap();
    let journal = journal_path_in(None, env(&vars)).unwrap();
    assert_eq!(journal.parent(), index.parent());
    assert_eq!(journal.file_name().unwrap(), "journal.toml");
}

#[test]
fn the_known_hosts_of_ncrs_live_next_to_the_index() {
    let vars = [
        ("HOME", "/home/user"),
        ("APPDATA", r"C:\Users\user\AppData"),
    ];
    let index = index_path_in(None, env(&vars)).unwrap();
    let hosts = known_hosts_path_in(None, env(&vars)).unwrap();
    assert_eq!(hosts.parent(), index.parent());
    assert_eq!(hosts.file_name().unwrap(), "known_hosts");
    let portable = Some(PathBuf::from("data"));
    assert_eq!(
        known_hosts_path_in(portable, env(&vars)),
        Some(PathBuf::from("data").join("known_hosts"))
    );
}

#[test]
fn config_and_index_share_the_mode() {
    let vars = [
        ("HOME", "/home/user"),
        ("APPDATA", r"C:\Users\user\AppData\Roaming"),
    ];
    let normal_config = config_path_in(None, env(&vars)).unwrap();
    let normal_index = index_path_in(None, env(&vars)).unwrap();
    assert!(!normal_config.starts_with("/home/user/ncrs/data"));
    assert!(!normal_index.starts_with("/home/user/ncrs/data"));

    let portable = Some(PathBuf::from("/home/user/ncrs/data"));
    let config = config_path_in(portable.clone(), env(&vars)).unwrap();
    let index = index_path_in(portable, env(&vars)).unwrap();
    assert_eq!(config.parent(), index.parent());
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_prefers_an_absolute_xdg_config_home() {
    let found = path_from_env(env(&[("XDG_CONFIG_HOME", "/x/cfg"), ("HOME", "/h")]));
    assert_eq!(found, Some(PathBuf::from("/x/cfg/ncrs/config.toml")));
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_falls_back_to_home_when_xdg_is_relative_or_empty() {
    for xdg in ["rel/cfg", ""] {
        let found = path_from_env(env(&[("XDG_CONFIG_HOME", xdg), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/h/.config/ncrs/config.toml")));
    }
}
