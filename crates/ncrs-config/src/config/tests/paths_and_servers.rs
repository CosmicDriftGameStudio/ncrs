use super::*;

#[cfg(not(windows))]
#[test]
fn no_usable_home_means_no_path() {
    assert_eq!(path_from_env(env(&[])), None);
    assert_eq!(path_from_env(env(&[("HOME", "")])), None);
    assert_eq!(path_from_env(env(&[("HOME", "rel")])), None);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_ignores_xdg_config_home() {
    let found = path_from_env(env(&[("XDG_CONFIG_HOME", "/x/cfg"), ("HOME", "/h")]));
    assert_eq!(found, Some(PathBuf::from("/h/.config/ncrs/config.toml")));
}

#[cfg(windows)]
#[test]
fn windows_uses_appdata() {
    let found = path_from_env(env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]));
    assert_eq!(
        found,
        Some(PathBuf::from(
            r"C:\Users\u\AppData\Roaming\ncrs\config.toml"
        ))
    );
    assert_eq!(path_from_env(env(&[("APPDATA", "rel")])), None);
    assert_eq!(path_from_env(env(&[])), None);
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_index_prefers_an_absolute_xdg_data_home() {
    let found = index_path_from_env(env(&[("XDG_DATA_HOME", "/x/data"), ("HOME", "/h")]));
    assert_eq!(found, Some(PathBuf::from("/x/data/ncrs/index.db")));
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_index_falls_back_to_home_when_xdg_is_relative_or_empty() {
    for xdg in ["rel/data", ""] {
        let found = index_path_from_env(env(&[("XDG_DATA_HOME", xdg), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/h/.local/share/ncrs/index.db")));
    }
}

#[cfg(not(windows))]
#[test]
fn no_usable_home_means_no_index_path() {
    assert_eq!(index_path_from_env(env(&[])), None);
    assert_eq!(index_path_from_env(env(&[("HOME", "")])), None);
    assert_eq!(index_path_from_env(env(&[("HOME", "rel")])), None);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_index_lives_in_application_support_and_ignores_xdg() {
    let found = index_path_from_env(env(&[("XDG_DATA_HOME", "/x/data"), ("HOME", "/h")]));
    assert_eq!(
        found,
        Some(PathBuf::from(
            "/h/Library/Application Support/ncrs/index.db"
        ))
    );
}

#[cfg(windows)]
#[test]
fn windows_index_uses_appdata() {
    let found = index_path_from_env(env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]));
    assert_eq!(
        found,
        Some(PathBuf::from(r"C:\Users\u\AppData\Roaming\ncrs\index.db"))
    );
    assert_eq!(index_path_from_env(env(&[("APPDATA", "rel")])), None);
    assert_eq!(index_path_from_env(env(&[])), None);
}

#[test]
fn portable_mode_keeps_the_view_copies_in_the_data_folder() {
    let portable = std::env::temp_dir().join("ncrs").join("data");
    let vars = [("HOME", "/home/user"), ("LOCALAPPDATA", "/local")];
    assert_eq!(
        view_copies_dir_in(Some(portable.clone()), env(&vars)),
        Some(portable.join("view"))
    );
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_view_copies_prefer_an_absolute_xdg_cache_home() {
    let found = view_copies_dir_from_env(env(&[("XDG_CACHE_HOME", "/x/cache"), ("HOME", "/h")]));
    assert_eq!(found, Some(PathBuf::from("/x/cache/ncrs/view")));
    for xdg in ["rel/cache", ""] {
        let fallback = view_copies_dir_from_env(env(&[("XDG_CACHE_HOME", xdg), ("HOME", "/h")]));
        assert_eq!(fallback, Some(PathBuf::from("/h/.cache/ncrs/view")));
    }
}

#[cfg(not(windows))]
#[test]
fn no_usable_home_means_no_view_copies_dir() {
    assert_eq!(view_copies_dir_from_env(env(&[])), None);
    assert_eq!(view_copies_dir_from_env(env(&[("HOME", "")])), None);
    assert_eq!(view_copies_dir_from_env(env(&[("HOME", "rel")])), None);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_view_copies_live_in_library_caches_and_ignore_xdg() {
    let found = view_copies_dir_from_env(env(&[("XDG_CACHE_HOME", "/x/cache"), ("HOME", "/h")]));
    assert_eq!(found, Some(PathBuf::from("/h/Library/Caches/ncrs/view")));
}

#[cfg(windows)]
#[test]
fn windows_view_copies_use_local_appdata() {
    let found = view_copies_dir_from_env(env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")]));
    assert_eq!(
        found,
        Some(PathBuf::from(r"C:\Users\u\AppData\Local\ncrs\view"))
    );
    assert_eq!(
        view_copies_dir_from_env(env(&[("LOCALAPPDATA", "rel")])),
        None
    );
    assert_eq!(view_copies_dir_from_env(env(&[])), None);
}

pub fn load_filing(content: &str, home: Option<&Path>) -> Result<Config, ConfigError> {
    let dir = tempfile::tempdir().map_err(|err| ConfigError::Unreadable {
        path: PathBuf::new(),
        reason: err.to_string(),
    })?;
    load_with_home(&write(&dir, content.as_bytes()), home)
}

#[test]
fn a_vscode_theme_is_found_under_the_home_folder_by_tilde() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("themes")).unwrap();
    std::fs::write(
        home.path().join("themes/t.json"),
        r##"{"colors": {"editor.background": "#112233"}}"##,
    )
    .unwrap();
    let config = load_filing(
        "[open]\n\n[theme]\nvscode = \"~/themes/t.json\"\n",
        Some(home.path()),
    )
    .unwrap();
    assert_eq!(config.vscode_warning, None);
    assert_eq!(
        config.palette.background,
        iced::Color::from_rgb8(0x11, 0x22, 0x33)
    );
}

#[test]
fn a_tilde_without_a_home_folder_is_a_warning_and_not_an_error() {
    let config = load_filing("[open]\n\n[theme]\nvscode = \"~/t.json\"\n", None).unwrap();
    assert!(config.vscode_warning.is_some());
    assert_eq!(
        config.palette.background,
        crate::palette::ThemePreset::DEFAULT.palette().background
    );
}

/// A TOML literal string, so Windows backslashes stay as they are.
pub fn quoted(path: &Path) -> String {
    format!("'{}'", path.display())
}

pub fn location(path: &str, rest: &str) -> String {
    format!("[[modules.filing.locations]]\npath = {path}\n{rest}\n")
}

pub fn filing_text(enabled: bool, locations: &[String]) -> String {
    let mut text = format!("[modules.filing]\nenabled = {enabled}\n");
    text.extend(locations.iter().map(String::as_str));
    text
}

/// 1-based number of the last line that contains `needle`.
pub fn last_line_with(text: &str, needle: &str) -> usize {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .last()
        .unwrap()
        .0
        + 1
}

pub fn filing_error(content: &str, home: Option<&Path>) -> (Option<usize>, String) {
    let err = load_filing(content, home).unwrap_err();
    assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
    (err.line(), err.reason().to_owned())
}

#[test]
fn a_full_filing_section_is_read() {
    let base = tempfile::tempdir().unwrap();
    let (inbox, archive_a, archive_b) = (
        base.path().join("inbox"),
        base.path().join("archive-a"),
        base.path().join("archive-b"),
    );
    let text = filing_text(
        true,
        &[
            location(&quoted(&inbox), "mode = \"inbox\""),
            location(
                &quoted(&archive_a),
                "mode = \"archive\"\nimages = true\ntarget = true",
            ),
            location(&quoted(&archive_b), "mode = \"archive\"\ntarget = false"),
        ],
    );
    let filing = load_filing(&text, None).unwrap().filing;
    assert!(filing.enabled);
    let found: Vec<_> = filing
        .locations
        .iter()
        .map(|l| (l.path.clone(), l.mode, l.images, l.is_target))
        .collect();
    assert_eq!(
        found,
        [
            (inbox, LocationMode::Inbox, false, false),
            (archive_a.clone(), LocationMode::Archive, true, true),
            (archive_b, LocationMode::Archive, false, false),
        ]
    );
    assert_eq!(target_of(&filing).unwrap().path, archive_a);
}

#[test]
fn folders_that_do_not_exist_load_without_error() {
    let base = tempfile::tempdir().unwrap();
    let gone = base.path().join("unplugged").join("archive-a");
    for enabled in [true, false] {
        let text = filing_text(
            enabled,
            &[location(
                &quoted(&gone),
                "mode = \"archive\"\ntarget = true",
            )],
        );
        let filing = load_filing(&text, None).unwrap().filing;
        assert_eq!(filing.enabled, enabled);
        assert_eq!(target_of(&filing).unwrap().path, gone);
    }
}

#[test]
fn a_missing_section_means_disabled_without_locations() {
    let filing = load_text("version = 1\n").filing;
    assert!(!filing.enabled);
    assert!(filing.locations.is_empty());
    assert!(target_of(&filing).is_none());
}

#[test]
fn enabled_without_locations_loads() {
    let filing = load_filing("[modules.filing]\nenabled = true\n", None)
        .unwrap()
        .filing;
    assert!(filing.enabled);
    assert!(filing.locations.is_empty());
}

#[test]
fn a_leading_tilde_is_the_home_directory() {
    let home = tempfile::tempdir().unwrap();
    let archive = "mode = \"archive\"\ntarget = true";
    let in_home = filing_text(true, &[location("'~/archive-a'", archive)]);
    let filing = load_filing(&in_home, Some(home.path())).unwrap().filing;
    assert_eq!(filing.locations[0].path, home.path().join("archive-a"));

    let home_itself = filing_text(true, &[location("'~'", archive)]);
    let bare = load_filing(&home_itself, Some(home.path())).unwrap().filing;
    assert_eq!(bare.locations[0].path, home.path());

    let climbing = filing_text(true, &[location("'~/../x'", archive)]);
    let (line, _) = filing_error(&climbing, Some(home.path()));
    assert_eq!(line, Some(4));
}

#[test]
fn a_tilde_is_the_real_home_when_the_sandbox_knows_it() {
    let real = tempfile::tempdir().unwrap();
    let container = tempfile::tempdir().unwrap();
    let home = ncrs_fs::preferred_home(Some(real.path().to_path_buf()), || {
        Some(container.path().to_path_buf())
    });
    let archive = "mode = \"archive\"\ntarget = true";
    let text = filing_text(true, &[location("'~/archive-a'", archive)]);

    let filing = load_filing(&text, home.as_deref()).unwrap().filing;

    assert_eq!(filing.locations[0].path, real.path().join("archive-a"));
}

pub fn entry(name: Option<&str>, host: &str, port: u16, path: Option<&str>) -> ServerEntry {
    ServerEntry {
        name: name.map(str::to_owned),
        host: host.to_owned(),
        port,
        user: "marc".to_owned(),
        path: path.map(str::to_owned),
    }
}

#[test]
fn servers_section_round_trips_and_keeps_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        &dir,
        b"# my config\nlang = \"en\"\n\n# the servers\n[[servers]]\nname = \"Work\"\nhost = \"files.example.org\"\nuser = \"marc\"\n\n[theme]\npreset = \"classic\"\n",
    );
    let loaded = load_with_home(&path, None).unwrap();
    assert_eq!(
        loaded.servers,
        [entry(Some("Work"), "files.example.org", 22, None)]
    );

    let wanted = vec![
        entry(Some("Work"), "files.example.org", 22, None),
        entry(None, "backup.example.org", 2222, Some("/srv/backup")),
    ];
    let written = write_servers_with_home(&path, &wanted, None).unwrap();

    assert_eq!(written.servers, wanted);
    let text = std::fs::read_to_string(&path).unwrap();
    for kept in [
        "# my config",
        "# the servers",
        "preset = \"classic\"",
        "lang = \"en\"",
    ] {
        assert!(text.contains(kept), "{kept} is gone from\n{text}");
    }
    assert!(text.contains("port = 2222") && !text.contains("port = 22\n"));
    assert_eq!(load_with_home(&path, None).unwrap().servers, wanted);

    let emptied = write_servers_with_home(&path, &[], None).unwrap();
    assert!(emptied.servers.is_empty());
    let left = std::fs::read_to_string(&path).unwrap();
    assert!(
        !left.contains("servers") && left.contains("# my config"),
        "{left}"
    );
}

#[test]
fn writing_servers_to_a_missing_file_creates_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let wanted = vec![entry(None, "h.example.org", 22, None)];

    let written = write_servers_with_home(&path, &wanted, None).unwrap();

    assert_eq!(written.servers, wanted);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("[[servers]]"));
}

#[test]
fn server_with_bad_port_or_relative_path_is_a_config_error() {
    let with = |host: &str, user: &str, extra: &str| {
        format!("[[servers]]\nhost = \"{host}\"\nuser = \"{user}\"\n{extra}\n")
    };
    let port_text = "port must be a number from 1 to 65535";
    let bad = [
        (with("h", "u", "port = 0"), port_text),
        (with("h", "u", "port = 70000"), port_text),
        (with("h", "u", "port = -1"), port_text),
        (
            with("h", "u", "path = \"srv/files\""),
            "path must be absolute",
        ),
        (with("a b", "u", ""), "host must not be empty"),
        (with("", "u", ""), "host must not be empty"),
        (with("a/b", "u", ""), "host must not be empty"),
        (with("h", "a@b", ""), "user must not be empty"),
        (with("h", "a:secret", ""), "user must not be empty"),
    ];
    for (text, reason) in bad {
        let (line, found) = filing_error(&text, None);
        assert!(found.contains(reason), "{text}: {found}");
        assert!(line.is_some(), "{text}");
    }
}

#[test]
fn a_server_without_host_or_user_is_a_config_error() {
    for text in ["[[servers]]\nuser = \"u\"\n", "[[servers]]\nhost = \"h\"\n"] {
        let (line, reason) = filing_error(text, None);
        assert!(reason.contains("missing field"), "{reason}");
        assert!(line.is_some());
    }
}

#[test]
fn a_password_has_no_place_in_the_config() {
    let text = "[[servers]]\nhost = \"h\"\nuser = \"u\"\npassword = \"secret\"\n";
    let (_, reason) = filing_error(text, None);
    assert!(reason.contains("unknown field"), "{reason}");
    assert!(!reason.contains("secret"));
}

#[test]
fn a_saved_server_opens_at_its_start_folder_or_the_root() {
    assert_eq!(
        entry(None, "H.example.org", 22, Some("/srv")).url(),
        PathBuf::from("sftp://marc@h.example.org/srv")
    );
    assert_eq!(
        entry(None, "h.example.org", 2222, None).url(),
        PathBuf::from("sftp://marc@h.example.org:2222/")
    );
    assert_eq!(entry(Some("Work"), "h", 22, None).label(), "Work");
    assert_eq!(entry(None, "h", 22, None).label(), "marc@h");
}

pub fn load_of(setting: &str) -> Load {
    let text = format!("[modules.filing]\nenabled = true\n{setting}");
    load_filing(&text, None).unwrap().filing.load
}

#[test]
fn the_load_is_gentle_unless_set_and_reads_names_and_numbers() {
    let workers = |count| Load::Workers(NonZeroUsize::new(count).unwrap());
    assert_eq!(load_of(""), Load::Gentle);
    assert_eq!(load_of("load = \"gentle\"\n"), Load::Gentle);
    assert_eq!(load_of("load = \"balanced\"\n"), Load::Balanced);
    assert_eq!(load_of("load = \"full\"\n"), Load::Full);
    assert_eq!(load_of("load = 3\n"), workers(3));
    assert_eq!(load_of("load = 500\n"), workers(500));
}

#[test]
fn a_load_that_is_no_name_and_no_positive_number_names_its_line() {
    for setting in ["load = \"fast\"", "load = 0", "load = -2", "load = true"] {
        let text = format!("[modules.filing]\nenabled = true\n{setting}\n");
        let (line, reason) = filing_error(&text, None);
        assert_eq!(line, Some(3), "{setting}");
        assert!(reason.contains("gentle"), "{setting}: {reason}");
    }
}

pub fn ai_setting(table: &str) -> AiSetting {
    load_filing(&format!("[modules.filing.ai]\n{table}"), None)
        .unwrap()
        .filing
        .ai
}

#[test]
fn without_an_ai_table_or_when_not_enabled_the_ai_is_off() {
    assert_eq!(
        load_filing("[modules.filing]\nenabled = true\n", None)
            .unwrap()
            .filing
            .ai,
        AiSetting::Off
    );
    assert_eq!(ai_setting(""), AiSetting::Off);
    assert_eq!(ai_setting("provider = \"local\"\n"), AiSetting::Off);
    assert_eq!(ai_setting("enabled = false\nthreads = 2\n"), AiSetting::Off);
}

#[test]
fn the_local_provider_is_the_default_and_threads_are_optional() {
    assert_eq!(
        ai_setting("enabled = true\n"),
        AiSetting::Local { threads: None }
    );
    assert_eq!(
        ai_setting("enabled = true\nprovider = \"local\"\nthreads = 3\n"),
        AiSetting::Local {
            threads: NonZeroU32::new(3)
        }
    );
}

#[test]
fn the_http_provider_needs_an_endpoint_and_a_model() {
    assert_eq!(
        ai_setting(
            "enabled = true\nprovider = \"http\"\nendpoint = \"http://localhost:8080/v1\"\nmodel = \"qwen\"\n"
        ),
        AiSetting::Http {
            endpoint: "http://localhost:8080/v1".to_owned(),
            model: "qwen".to_owned()
        }
    );
}

#[test]
fn the_http_provider_accepts_every_way_to_say_this_machine() {
    for endpoint in [
        "http://localhost:8080",
        "http://127.0.0.1:8080/v1",
        "http://[::1]:8080",
        "https://localhost:8443",
    ] {
        let text = format!(
            "enabled = true\nprovider = \"http\"\nmodel = \"q\"\nendpoint = \"{endpoint}\"\n"
        );
        assert!(
            matches!(ai_setting(&text), AiSetting::Http { .. }),
            "{endpoint}"
        );
    }
}

#[test]
fn ai_errors_name_the_exact_line() {
    let cases: [(&str, &str, usize, &str); 9] = [
        (
            "http without an endpoint",
            "enabled = true\nprovider = \"http\"\nmodel = \"qwen\"\n",
            1,
            "needs endpoint",
        ),
        (
            "http without a model",
            "enabled = true\nprovider = \"http\"\nendpoint = \"http://localhost:8080\"\n",
            1,
            "needs model",
        ),
        (
            "an endpoint that is not accepted",
            "enabled = true\nprovider = \"http\"\nmodel = \"q\"\nendpoint = \"http://example.com/v1\"\n",
            5,
            "needs https",
        ),
        (
            "an endpoint on another machine",
            "enabled = true\nprovider = \"http\"\nmodel = \"q\"\nendpoint = \"https://api.example.com/v1\"\n",
            5,
            "must be on this machine",
        ),
        (
            "threads of zero",
            "enabled = true\nthreads = 0\n",
            3,
            "from 1 up",
        ),
        (
            "threads for http",
            "provider = \"http\"\nthreads = 2\n",
            3,
            "only for provider = \"local\"",
        ),
        (
            "an endpoint for the local provider",
            "enabled = true\nendpoint = \"http://localhost:8080\"\n",
            3,
            "only for provider = \"http\"",
        ),
        (
            "a model for the local provider",
            "enabled = true\nmodel = \"qwen\"\n",
            3,
            "only for provider = \"http\"",
        ),
        (
            "an unknown provider",
            "enabled = true\nprovider = \"cloud\"\n",
            3,
            "cloud",
        ),
    ];
    for (name, table, expected_line, expected_reason) in cases {
        let (line, reason) = filing_error(&format!("[modules.filing.ai]\n{table}"), None);
        assert_eq!(line, Some(expected_line), "{name}");
        assert!(reason.contains(expected_reason), "{name}: {reason}");
    }
}

#[test]
fn filing_errors_name_the_exact_line() {
    let base = tempfile::tempdir().unwrap();
    let at = |name: &str| quoted(&base.path().join(name));
    let archive = |name: &str| location(&at(name), "mode = \"archive\"\ntarget = true");
    let plain_archive = |name: &str| location(&at(name), "mode = \"archive\"");
    let with_slash = quoted(&base.path().join("archive-a").join(""));
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "relative path",
            filing_text(true, &[location("'archive-a'", "mode = \"archive\"")]),
            "archive-a",
        ),
        (
            "tilde without home",
            filing_text(true, &[location("'~/archive-a'", "mode = \"archive\"")]),
            "~/archive-a",
        ),
        (
            "parent component",
            filing_text(
                true,
                &[location(
                    &quoted(&base.path().join("..").join("x")),
                    "mode = \"archive\"",
                )],
            ),
            "..",
        ),
        (
            "unknown mode",
            filing_text(true, &[location(&at("a"), "mode = \"outbox\"")]),
            "outbox",
        ),
        (
            "unknown field in a location",
            filing_text(
                true,
                &[location(&at("a"), "mode = \"archive\"\ncolour = 1")],
            ),
            "colour",
        ),
        (
            "unknown field in the section",
            "[modules.filing]\nenabled = true\nnope = 1\n".to_owned(),
            "nope",
        ),
        (
            "target on an inbox",
            filing_text(
                true,
                &[
                    archive("archive-a"),
                    location(&at("inbox"), "mode = \"inbox\"\ntarget = true"),
                ],
            ),
            "target = true",
        ),
        (
            "two targets",
            filing_text(true, &[archive("archive-a"), archive("archive-b")]),
            "target = true",
        ),
        (
            "no target",
            filing_text(false, &[plain_archive("archive-a")]),
            "[[modules.filing.locations]]",
        ),
        (
            "nested locations",
            filing_text(
                true,
                &[archive("archive-a"), plain_archive("archive-a/inside")],
            ),
            "inside",
        ),
        (
            "outer after inner",
            filing_text(
                true,
                &[archive("archive-a/inside"), plain_archive("archive-a")],
            ),
            "archive-a'",
        ),
        (
            "the same folder twice",
            filing_text(true, &[archive("archive-a"), plain_archive("archive-a")]),
            "archive-a'",
        ),
        (
            "the same folder twice with a trailing slash",
            filing_text(
                true,
                &[
                    archive("archive-a"),
                    location(&with_slash, "mode = \"archive\""),
                ],
            ),
            "archive-a",
        ),
    ];
    for (name, text, marker) in &cases {
        let (found, reason) = filing_error(text, None);
        let expected = match *name {
            "no target" => 3,
            _ => last_line_with(text, marker),
        };
        assert_eq!(found, Some(expected), "{name}: {reason}\n{text}");
    }
}

#[test]
fn filing_reasons_say_what_is_wrong() {
    let base = tempfile::tempdir().unwrap();
    let path = quoted(&base.path().join("a"));
    let cases = [
        (
            filing_text(true, &[location("'rel'", "mode = \"archive\"")]),
            "must be absolute",
        ),
        (
            filing_text(true, &[location("'~/a'", "mode = \"archive\"")]),
            "no home directory to expand `~`",
        ),
        (
            filing_text(true, &[location(&path, "mode = \"outbox\"")]),
            "inbox",
        ),
        (
            filing_text(true, &[location(&path, "mode = \"outbox\"")]),
            "archive",
        ),
        (
            filing_text(true, &[location(&path, "mode = \"inbox\"\ntarget = true")]),
            "inbox cannot be the target",
        ),
        (
            filing_text(true, &[location(&path, "mode = \"archive\"")]),
            "no location has `target = true`",
        ),
    ];
    for (text, expected) in cases {
        let (_, found) = filing_error(&text, None);
        assert!(found.contains(expected), "{found}");
    }
}

/// Three existing folders below a temporary directory, canonical the way
/// the setup hands them over.
pub fn setup_folders() -> SetupFolders {
    let base = tempfile::tempdir().unwrap();
    let canonical = |name: &str| {
        let folder = base.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        std::fs::canonicalize(folder).unwrap()
    };
    let (inbox, archive, target) = (
        canonical("inbox"),
        canonical("archive"),
        canonical("target"),
    );
    SetupFolders {
        base,
        inbox,
        archive,
        target,
    }
}

pub fn chosen(path: &Path, mode: LocationMode, images: bool, is_target: bool) -> FilingLocation {
    FilingLocation {
        path: path.to_path_buf(),
        mode,
        images,
        is_target,
    }
}

pub fn one_of_each(folders: &SetupFolders) -> Vec<FilingLocation> {
    vec![
        chosen(&folders.inbox, LocationMode::Inbox, true, false),
        chosen(&folders.archive, LocationMode::Archive, false, false),
        chosen(&folders.target, LocationMode::Archive, true, true),
    ]
}
