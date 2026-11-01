use super::*;

#[test]
fn the_setup_writes_the_locations_and_they_load_back() {
    let folders = setup_folders();
    let path = folders.base.path().join("config.toml");
    std::fs::write(&path, "lang = \"de\"\n").unwrap();
    let chosen = one_of_each(&folders);

    let written = write_filing_locations_with_home(&path, &chosen, None).unwrap();

    assert!(written.filing.enabled);
    assert_eq!(written.filing.locations, chosen);
    assert_eq!(written.lang, Some(Language::German));
    let reloaded = load_with_home(&path, None).unwrap();
    assert_eq!(reloaded, written);
}

#[test]
fn comments_and_other_sections_survive_the_write() {
    let folders = setup_folders();
    let path = folders.base.path().join("config.toml");
    let old_location = folders.base.path().join("old");
    let original = format!(
        "# my settings\nlang = \"en\" # English for now\n\n\
         [open]\n# the viewer I like\nview = [\"viewer\"]\n\n\
         [modules.filing]\n# off until the drive is set up\nenabled = false # later\n\
         load = \"gentle\"\n\n\
         # where documents live\n[[modules.filing.locations]]\npath = {}\nmode = \"archive\"\ntarget = true\n\n\
         [theme]\n# the older look\npreset = \"classic\"\n",
        quoted(&old_location)
    );
    std::fs::write(&path, &original).unwrap();

    let written = write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    for kept in [
        "# my settings\n",
        "lang = \"en\" # English for now\n",
        "[open]\n# the viewer I like\nview = [\"viewer\"]\n",
        "# off until the drive is set up\nenabled = true # later\n",
        "load = \"gentle\"\n",
        "# where documents live\n[[modules.filing.locations]]\n",
        "[theme]\n# the older look\npreset = \"classic\"\n",
    ] {
        assert!(text.contains(kept), "{kept:?} is gone from:\n{text}");
    }
    assert!(
        !text.contains(&old_location.display().to_string()),
        "{text}"
    );
    assert_eq!(written.filing.load, Load::Gentle);
    assert_eq!(written.filing.locations, one_of_each(&folders));
    assert_eq!(written.open.view.unwrap().program(), "viewer");
}

#[test]
fn a_filing_section_written_inline_becomes_a_table() {
    let folders = setup_folders();
    let path = folders.base.path().join("config.toml");
    std::fs::write(&path, "modules = { filing = { enabled = false } }\n").unwrap();

    let written = write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

    assert!(written.filing.enabled);
    assert_eq!(written.filing.locations, one_of_each(&folders));
}

#[test]
fn an_invalid_choice_is_rejected_before_writing() {
    let folders = setup_folders();
    let path = folders.base.path().join("config.toml");
    let original = "# untouched\nlang = \"en\"\n";
    std::fs::write(&path, original).unwrap();
    let nested = folders.target.join("inside");
    std::fs::create_dir(&nested).unwrap();
    let cases = [
        (Vec::new(), FilingRuleViolation::NoTarget),
        (
            vec![
                chosen(&folders.inbox, LocationMode::Inbox, false, false),
                chosen(&folders.archive, LocationMode::Archive, false, false),
            ],
            FilingRuleViolation::NoTarget,
        ),
        (
            vec![
                chosen(&folders.archive, LocationMode::Archive, false, true),
                chosen(&folders.target, LocationMode::Archive, false, true),
            ],
            FilingRuleViolation::SecondTarget { index: 1 },
        ),
        (
            vec![chosen(&folders.inbox, LocationMode::Inbox, false, true)],
            FilingRuleViolation::InboxAsTarget { index: 0 },
        ),
        (
            vec![
                chosen(&folders.target, LocationMode::Archive, false, true),
                chosen(&nested, LocationMode::Inbox, false, false),
            ],
            FilingRuleViolation::Overlap {
                earlier: 0,
                later: 1,
            },
        ),
    ];
    for (locations, expected) in cases {
        let result = write_filing_locations_with_home(&path, &locations, None);
        assert_eq!(result, Err(FilingWriteError::Rule(expected)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}

#[test]
fn a_config_that_does_not_load_is_not_overwritten() {
    let folders = setup_folders();
    let path = folders.base.path().join("config.toml");
    for broken in ["[open\nview = [\"viewer\"]\n", "lang = 5\n", "colour = 1\n"] {
        std::fs::write(&path, broken).unwrap();
        let result = write_filing_locations_with_home(&path, &one_of_each(&folders), None);
        assert!(
            matches!(
                result,
                Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
            ),
            "{broken:?}: {result:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }
}

#[test]
fn a_missing_config_is_created_with_its_folder() {
    let folders = setup_folders();
    let path = folders.base.path().join("ncrs").join("config.toml");

    let written = write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

    assert_eq!(written.filing.locations, one_of_each(&folders));
    assert_eq!(load_with_home(&path, None).unwrap(), written);
}

#[cfg(unix)]
#[test]
fn a_symlinked_config_stays_a_symlink_and_its_target_is_written() {
    let folders = setup_folders();
    let real = folders.base.path().join("dotfiles").join("config.toml");
    std::fs::create_dir(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "lang = \"en\"\n").unwrap();
    let link = folders.base.path().join("config.toml");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    write_filing_locations_with_home(&link, &one_of_each(&folders), None).unwrap();

    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    let reloaded = load_with_home(&real, None).unwrap();
    assert_eq!(reloaded.filing.locations, one_of_each(&folders));
}

#[cfg(unix)]
#[test]
fn the_permissions_of_the_old_file_are_kept() {
    use std::os::unix::fs::PermissionsExt as _;
    let folders = setup_folders();
    let path = folders.base.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

pub fn theme_file(base: &tempfile::TempDir, relative: &str, background: &str) {
    let file = base.path().join(relative);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        file,
        format!(r#"{{"colors": {{"editor.background": "{background}"}}}}"#),
    )
    .unwrap();
}

#[test]
fn the_theme_is_written_and_its_palette_loads() {
    let base = tempfile::tempdir().unwrap();
    theme_file(&base, "themes/example.dark/dark.json", "#112233");
    let path = base.path().join("config.toml");
    std::fs::write(&path, "lang = \"de\"\n").unwrap();

    let written =
        write_vscode_theme_with_home(&path, "themes/example.dark/dark.json", None).unwrap();

    assert_eq!(written.vscode_warning, None);
    assert_eq!(
        written.palette.background,
        iced::Color::from_rgb8(0x11, 0x22, 0x33)
    );
    assert_eq!(written.lang, Some(Language::German));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("[theme]\nvscode = \"themes/example.dark/dark.json\"\n"),
        "{text}"
    );
    assert_eq!(load_with_home(&path, None).unwrap(), written);
}

#[test]
fn an_existing_vscode_setting_is_replaced_and_the_rest_stays() {
    let base = tempfile::tempdir().unwrap();
    theme_file(&base, "themes/example.dark/dark.json", "#112233");
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "# my settings\nlang = \"en\"\n\n[theme]\n# the look\npreset = \"classic\"\n\
         vscode = \"old/theme.json\" # before\n\n[open]\nview = [\"viewer\"]\n",
    )
    .unwrap();

    let written =
        write_vscode_theme_with_home(&path, "themes/example.dark/dark.json", None).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    for kept in [
        "# my settings\nlang = \"en\"\n",
        "[theme]\n# the look\npreset = \"classic\"\n",
        "vscode = \"themes/example.dark/dark.json\" # before\n",
        "[open]\nview = [\"viewer\"]\n",
    ] {
        assert!(text.contains(kept), "{kept:?} is gone from:\n{text}");
    }
    assert!(!text.contains("old/theme.json"), "{text}");
    assert_eq!(written.open.view.unwrap().program(), "viewer");
}

#[test]
fn a_theme_written_inline_becomes_a_table() {
    let base = tempfile::tempdir().unwrap();
    theme_file(&base, "t.json", "#112233");
    let path = base.path().join("config.toml");
    std::fs::write(&path, "theme = { preset = \"classic\" }\n").unwrap();

    let written = write_vscode_theme_with_home(&path, "t.json", None).unwrap();

    assert_eq!(written.vscode_warning, None);
    assert_eq!(
        written.palette.background,
        iced::Color::from_rgb8(0x11, 0x22, 0x33)
    );
}

#[test]
fn a_missing_config_is_created_for_the_theme() {
    let base = tempfile::tempdir().unwrap();
    theme_file(&base, "ncrs/themes/t.json", "#112233");
    let path = base.path().join("ncrs").join("config.toml");

    let written = write_vscode_theme_with_home(&path, "themes/t.json", None).unwrap();

    assert_eq!(load_with_home(&path, None).unwrap(), written);
    assert_eq!(written.vscode_warning, None);
}

#[test]
fn a_config_that_does_not_load_is_not_overwritten_by_the_theme() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    let broken = "[open\nview = [\"viewer\"]\n";
    std::fs::write(&path, broken).unwrap();

    let result = write_vscode_theme_with_home(&path, "t.json", None);

    assert!(
        matches!(
            result,
            Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
        ),
        "{result:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
}

#[test]
fn a_theme_file_that_is_missing_is_a_warning_in_the_written_config() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");

    let written = write_vscode_theme_with_home(&path, "themes/gone.json", None).unwrap();

    assert!(written.vscode_warning.is_some());
}
pub fn lines_of(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn the_preset_is_written_in_place_and_the_comments_stay() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "# mine\nlang = \"en\"\n\n[theme]\n# the look\npreset = \"classic\" # old\nbackground = \"#112233\"\n",
    )
    .unwrap();

    let written = write_setting(&path, theme_preset_setting(ThemePreset::Dracula), None).unwrap();

    let text = lines_of(&path);
    for kept in [
        "# mine\nlang = \"en\"\n",
        "# the look\npreset = \"dracula\" # old\n",
        "background = \"#112233\"\n",
    ] {
        assert!(text.contains(kept), "{kept:?} is gone from:\n{text}");
    }
    assert_eq!(written.theme_preset(), ThemePreset::Dracula);
    assert_eq!(load_with_home(&path, None).unwrap(), written);
}

#[test]
fn a_missing_theme_table_is_created_for_the_preset() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "lang = \"de\"\n").unwrap();

    let written = write_setting(&path, theme_preset_setting(ThemePreset::Teal), None).unwrap();

    assert!(lines_of(&path).contains("[theme]\npreset = \"default\"\n"));
    assert_eq!(written.theme_preset(), ThemePreset::Teal);
    assert_eq!(written.lang, Some(Language::German));
}

#[test]
fn the_language_is_written_at_the_top_and_round_trips() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "# first\n[theme]\npreset = \"classic\"\n").unwrap();

    let written = write_setting(&path, language_setting(Language::German), None).unwrap();

    let text = lines_of(&path);
    assert!(text.starts_with("lang = \"de\"\n"), "{text}");
    assert!(
        text.contains("# first\n[theme]\npreset = \"classic\"\n"),
        "{text}"
    );
    assert_eq!(written.lang, Some(Language::German));
    let again = write_setting(&path, language_setting(Language::English), None).unwrap();
    assert_eq!(again.lang, Some(Language::English));
    assert_eq!(lines_of(&path).matches("lang").count(), 1);
}

#[test]
fn filing_enabled_creates_its_table_and_keeps_the_rest() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "[open]\nview = [\"viewer\"] # mine\n").unwrap();

    let written = write_setting(&path, filing_enabled_setting(true), None).unwrap();

    let text = lines_of(&path);
    assert!(text.contains("view = [\"viewer\"] # mine\n"), "{text}");
    assert!(
        text.contains("[modules.filing]\nenabled = true\n"),
        "{text}"
    );
    assert!(written.filing.enabled);
    let off = write_setting(&path, filing_enabled_setting(false), None).unwrap();
    assert!(!off.filing.enabled);
}

#[test]
fn the_load_is_written_as_a_name_or_a_number_and_reads_back() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "[modules.filing]\nenabled = false\nload = \"gentle\" # easy\n",
    )
    .unwrap();
    let workers = Load::Workers(NonZeroUsize::new(3).unwrap());

    for load in [Load::Full, Load::Balanced, workers, Load::Gentle] {
        let written = write_setting(&path, filing_load_setting(load), None).unwrap();
        assert_eq!(written.filing.load, load);
    }
    let text = lines_of(&path);
    assert!(text.contains("load = \"gentle\" # easy\n"), "{text}");
    assert!(text.contains("enabled = false\n"), "{text}");
}

#[test]
fn ai_enabled_creates_the_nested_table_and_keeps_the_filing_keys() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "[modules.filing]\nenabled = true\nload = \"full\"\n").unwrap();

    let written = write_setting(&path, ai_enabled_setting(true), None).unwrap();

    let text = lines_of(&path);
    assert!(text.contains("load = \"full\"\n"), "{text}");
    assert!(
        text.contains("[modules.filing.ai]\nenabled = true\n"),
        "{text}"
    );
    assert!(matches!(written.filing.ai, AiSetting::Local { .. }));
    let off = write_setting(&path, ai_enabled_setting(false), None).unwrap();
    assert_eq!(off.filing.ai, AiSetting::Off);
}

#[test]
fn a_program_line_reads_back_as_it_is_shown() {
    let line = ProgramLine::from_text("  code --wait  \"{path}:{line}\" ").unwrap();
    assert_eq!(line, ProgramLine::new("code", &["--wait", "{path}:{line}"]));
    let spaced_program = if cfg!(windows) {
        r"C:\Program Files\Sublime Text\subl.exe"
    } else {
        "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl"
    };
    let spaced = ProgramLine::new(spaced_program, &["", "it's"]);
    assert_eq!(ProgramLine::from_text(&spaced.to_text()), Ok(spaced));
    let quoted_program = if cfg!(windows) {
        r"C:\my editor\run.exe"
    } else {
        "/opt/my editor/run"
    };
    assert_eq!(
        ProgramLine::from_text(&format!("'{quoted_program}' -n")),
        Ok(ProgramLine::new(quoted_program, &["-n"]))
    );
}

#[test]
fn a_word_with_both_quotes_and_a_space_reads_back_the_same() {
    let mixed = ProgramLine::new(
        if cfg!(windows) {
            r"C:\Tools\printf.exe"
        } else {
            "/usr/bin/printf"
        },
        &["say \"it's\" now", "\"'", "'\"", "a\"\"b'c d", "\"\"", "''"],
    );
    let shown = mixed.to_text();
    assert_eq!(ProgramLine::from_text(&shown), Ok(mixed), "{shown}");
}

#[test]
fn a_program_line_the_loader_would_refuse_is_refused_as_text() {
    assert_eq!(
        ProgramLine::from_text("   "),
        Err(ProgramLineProblem::NoProgram)
    );
    assert_eq!(
        ProgramLine::from_text("\"\" -x"),
        Err(ProgramLineProblem::EmptyProgram)
    );
    assert_eq!(
        ProgramLine::from_text("bin/edit"),
        Err(ProgramLineProblem::RelativePath)
    );
    assert_eq!(
        ProgramLine::from_text("vim \"x"),
        Err(ProgramLineProblem::UnclosedQuote)
    );
}

#[test]
fn a_program_is_written_to_open_and_dropped_again() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "lang = \"en\"\n").unwrap();
    let line = ProgramLine::opening_app("/Applications/TextEdit.app");

    let written = write_setting(&path, program_setting(OpenKind::Edit, Some(&line)), None).unwrap();

    assert_eq!(written.open.edit, Some(line));
    let text = lines_of(&path);
    assert!(
        text.contains("[open]\nedit = [\"open\", \"-a\", \"/Applications/TextEdit.app\"]\n"),
        "{text}"
    );
    let dropped = write_setting(&path, program_setting(OpenKind::Edit, None), None).unwrap();
    assert_eq!(dropped.open, OpenPrograms::default());
    assert!(lines_of(&path).starts_with("lang = \"en\"\n"));
}

#[test]
fn a_dropped_value_creates_no_table_and_leaves_a_file_without_it_alone() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    let before = "# mine\nlang = \"de\"\n";
    std::fs::write(&path, before).unwrap();

    write_setting(&path, color_setting(ColorRole::Accent, None), None).unwrap();
    write_setting(&path, vscode_theme_setting(None), None).unwrap();

    assert_eq!(lines_of(&path), before);
}

#[test]
fn a_color_is_set_and_dropped_and_a_bad_one_never_reaches_the_file() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "[theme]\npreset = \"classic\" # base\n").unwrap();

    let written = write_setting(
        &path,
        color_setting(ColorRole::Accent, Some("#ff0000")),
        None,
    )
    .unwrap();
    assert_eq!(
        written.palette.color(ColorRole::Accent),
        iced::Color::from_rgb8(0xFF, 0, 0)
    );
    let before = lines_of(&path);
    let refused = write_setting(&path, color_setting(ColorRole::Accent, Some("red")), None);
    assert!(matches!(refused, Err(FilingWriteError::Config(_))));
    assert_eq!(lines_of(&path), before);

    let dropped = write_setting(&path, color_setting(ColorRole::Accent, None), None).unwrap();
    assert_eq!(dropped.palette, ThemePreset::Classic.palette());
    assert_eq!(lines_of(&path), "[theme]\npreset = \"classic\" # base\n");
}

#[test]
fn the_vscode_theme_is_dropped_from_its_section() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "[theme]\nvscode = \"gone.json\"\naccent = \"#123\"\n",
    )
    .unwrap();

    let written = write_setting(&path, vscode_theme_setting(None), None).unwrap();

    assert_eq!(written.vscode_theme_path(), None);
    assert_eq!(lines_of(&path), "[theme]\naccent = \"#123\"\n");
}

#[test]
fn switching_the_ai_provider_takes_the_keys_of_the_other_one_away() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "[modules.filing.ai]\nenabled = true\nprovider = \"local\"\nthreads = 2\n",
    )
    .unwrap();
    let http = AiProviderChoice::Http {
        endpoint: "http://127.0.0.1:11434".to_owned(),
        model: "llama".to_owned(),
    };

    let written = write_settings(&path, &ai_provider_settings(&http), None).unwrap();

    assert_eq!(
        written.filing.ai,
        AiSetting::Http {
            endpoint: "http://127.0.0.1:11434".to_owned(),
            model: "llama".to_owned(),
        }
    );
    assert!(!lines_of(&path).contains("threads"));
    let local = AiProviderChoice::Local {
        threads: NonZeroU32::new(3),
    };
    let back = write_settings(&path, &ai_provider_settings(&local), None).unwrap();
    assert_eq!(
        back.filing.ai,
        AiSetting::Local {
            threads: NonZeroU32::new(3)
        }
    );
    let text = lines_of(&path);
    assert!(
        !text.contains("endpoint") && !text.contains("model"),
        "{text}"
    );
}

#[test]
fn an_inline_ai_table_loses_a_dropped_key_too() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "[modules.filing]\nai = { provider = \"http\", endpoint = \"http://localhost:1\", model = \"m\" }\n",
    )
    .unwrap();
    let local = AiProviderChoice::Local { threads: None };

    let written = write_settings(&path, &ai_provider_settings(&local), None).unwrap();

    assert_eq!(written.filing.ai, AiSetting::Off);
    let text = lines_of(&path);
    assert!(
        !text.contains("endpoint") && !text.contains("model"),
        "{text}"
    );
}

#[test]
fn an_endpoint_off_this_machine_is_refused_before_the_write() {
    assert_eq!(check_ai_endpoint("http://localhost:11434"), Ok(()));
    assert_eq!(
        check_ai_endpoint("https://example.com:11434"),
        Err(AiEndpointProblem::NotLocal)
    );
    assert!(matches!(
        check_ai_endpoint("ftp://localhost"),
        Err(AiEndpointProblem::Unusable(_))
    ));
}

pub fn key_change(action: KeyAction, chords: Option<&[&str]>) -> KeyChange {
    KeyChange {
        action,
        chords: chords.map(|list| list.iter().map(|chord| (*chord).to_owned()).collect()),
    }
}

#[test]
fn written_keys_replace_only_their_lines_and_keep_the_comments() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(
        &path,
        "lang = \"de\"\n\n# my keys\n[keys]\nquit = \"f10\" # old habit\nview = [\"f3\"]\n",
    )
    .unwrap();

    let written = write_keys_with_home(
        &path,
        &[
            key_change(KeyAction::Quit, Some(&["primary+q", "f10"])),
            key_change(KeyAction::View, None),
            key_change(KeyAction::Edit, Some(&["f4", "shift+f4"])),
        ],
        None,
    )
    .unwrap();

    let text = lines_of(&path);
    assert_eq!(
        text,
        "lang = \"de\"\n\n# my keys\n[keys]\nquit = [\"primary+q\", \"f10\"] # old habit\nedit = [\"f4\", \"shift+f4\"]\n"
    );
    let names: Vec<&str> = written
        .keys
        .entries
        .iter()
        .map(|entry| entry.action.get_ref().as_str())
        .collect();
    assert_eq!(names, ["quit", "edit"]);
}

#[test]
fn dropping_keys_from_a_file_without_them_adds_no_table() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "lang = \"en\"\n").unwrap();
    write_keys_with_home(&path, &[key_change(KeyAction::Quit, None)], None).unwrap();
    assert_eq!(lines_of(&path), "lang = \"en\"\n");
}

#[test]
fn keys_that_clash_are_not_written() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    let text = "[keys]\nquit = \"f10\"\n";
    std::fs::write(&path, text).unwrap();
    let result = write_keys_with_home(&path, &[key_change(KeyAction::View, Some(&["f10"]))], None);
    assert!(
        matches!(
            result,
            Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
        ),
        "{result:?}"
    );
    assert_eq!(lines_of(&path), text);
}

#[test]
fn a_setting_is_created_with_its_config_and_a_broken_config_is_left_alone() {
    let base = tempfile::tempdir().unwrap();
    let fresh = base.path().join("ncrs").join("config.toml");
    let written = write_setting(&fresh, language_setting(Language::English), None).unwrap();
    assert_eq!(written.lang, Some(Language::English));

    let broken = base.path().join("broken.toml");
    let text = "[open\nview = [\"viewer\"]\n";
    std::fs::write(&broken, text).unwrap();
    let result = write_setting(&broken, filing_enabled_setting(true), None);
    assert!(
        matches!(
            result,
            Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
        ),
        "{result:?}"
    );
    assert_eq!(lines_of(&broken), text);
}

#[test]
fn a_table_that_is_not_one_is_reported_not_replaced() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "theme = \"classic\"\n").unwrap();

    let result = write_setting(&path, theme_preset_setting(ThemePreset::Teal), None);

    assert!(
        matches!(result, Err(FilingWriteError::Config(_))),
        "{result:?}"
    );
    assert_eq!(lines_of(&path), "theme = \"classic\"\n");
}
