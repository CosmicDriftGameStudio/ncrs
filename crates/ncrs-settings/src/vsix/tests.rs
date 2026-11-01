use super::fixtures::{manifest, package, THEME_JSON};
use super::*;

fn one_theme_package() -> Vec<u8> {
    package(&[
        (
            "package.json",
            manifest(&[("Example Dark", "vs-dark", "./themes/dark.json")]).as_bytes(),
        ),
        ("themes/dark.json", THEME_JSON.as_bytes()),
    ])
}

fn theme_at(path: &str) -> ContributedTheme {
    ContributedTheme {
        label: "Example".to_owned(),
        brightness: None,
        path: path.to_owned(),
    }
}

#[test]
fn paths_are_normalized_and_anything_that_could_leave_the_folder_is_refused() {
    assert_eq!(
        normalize_path("./themes/dark.json").as_deref(),
        Some("themes/dark.json")
    );
    assert_eq!(
        normalize_path("themes\\sub//dark.json").as_deref(),
        Some("themes/sub/dark.json")
    );
    assert_eq!(normalize_path("a/./b.json").as_deref(), Some("a/b.json"));
    for raw in [
        "",
        ".",
        "./",
        "../x.json",
        "a/../x.json",
        "a\\..\\x.json",
        "/abs.json",
        "\\abs.json",
        "C:x.json",
        "C:\\x.json",
        "a/b:c.json",
        "a/b\0.json",
    ] {
        assert_eq!(normalize_path(raw), None, "{raw:?}");
    }
}

#[test]
fn an_include_is_resolved_next_to_its_theme_and_never_above_it() {
    assert_eq!(
        resolve_include("themes/dark.json", "./base.json").unwrap(),
        "themes/base.json"
    );
    assert_eq!(
        resolve_include("dark.json", "base/x.json").unwrap(),
        "base/x.json"
    );
    assert_eq!(
        resolve_include("themes/dark.json", "../x.json"),
        Err(FetchError::UnsafePath("../x.json".to_owned()))
    );
}

#[test]
fn a_single_theme_is_listed_with_its_brightness() {
    let themes = list_themes(&one_theme_package()).unwrap();
    assert_eq!(
        themes,
        [ContributedTheme {
            label: "Example Dark".to_owned(),
            brightness: Some(Brightness::Dark),
            path: "themes/dark.json".to_owned(),
        }]
    );
}

#[test]
fn a_label_with_a_newline_or_bidi_control_is_cleaned() {
    let manifest = r#"{"contributes": {"themes": [{"label": "Ex\n\u202Eample\u2067 Dark", "uiTheme": "vs-dark", "path": "./a.json"}]}}"#;
    let vsix = package(&[("package.json", manifest.as_bytes())]);
    let themes = list_themes(&vsix).unwrap();
    assert_eq!(themes[0].label, "Example Dark");
}

#[test]
fn several_themes_are_listed_in_order_with_dark_and_light() {
    let vsix = package(&[(
        "package.json",
        manifest(&[
            ("Example Dark", "vs-dark", "./a.json"),
            ("Example Light", "vs", "./b.json"),
            ("Example Contrast", "hc-black", "./c.json"),
            ("Example Contrast Light", "hc-light", "./d.json"),
            ("Example Unknown", "other", "./e.json"),
        ])
        .as_bytes(),
    )]);
    let listed: Vec<(String, Option<Brightness>)> = list_themes(&vsix)
        .unwrap()
        .into_iter()
        .map(|theme| (theme.label, theme.brightness))
        .collect();
    assert_eq!(
        listed,
        [
            ("Example Dark".to_owned(), Some(Brightness::Dark)),
            ("Example Light".to_owned(), Some(Brightness::Light)),
            ("Example Contrast".to_owned(), Some(Brightness::Dark)),
            ("Example Contrast Light".to_owned(), Some(Brightness::Light)),
            ("Example Unknown".to_owned(), None),
        ]
    );
}

#[test]
fn a_package_without_themes_says_so() {
    for manifest in [
        r#"{"contributes": {"themes": []}}"#,
        r#"{"contributes": {}}"#,
        r#"{"name": "example"}"#,
    ] {
        let vsix = package(&[("package.json", manifest.as_bytes())]);
        assert_eq!(list_themes(&vsix), Err(FetchError::NoThemes), "{manifest}");
    }
}

#[test]
fn a_package_without_a_manifest_or_that_is_no_archive_is_refused() {
    let vsix = package(&[("readme.md", b"hello")]);
    assert_eq!(
        list_themes(&vsix),
        Err(FetchError::MissingFile("package.json".to_owned()))
    );
    assert!(matches!(
        list_themes(b"this is not a zip archive"),
        Err(FetchError::BadPackage(_))
    ));
    let broken = package(&[("package.json", b"{not json")]);
    assert!(matches!(
        list_themes(&broken),
        Err(FetchError::BadJson { .. })
    ));
}

#[test]
fn a_theme_path_that_leaves_the_package_fails_the_listing() {
    for path in ["../x.json", "/abs.json", "C:x.json", "a\\..\\x.json"] {
        let vsix = package(&[(
            "package.json",
            manifest(&[("Example", "vs-dark", path)]).as_bytes(),
        )]);
        assert_eq!(
            list_themes(&vsix),
            Err(FetchError::UnsafePath(path.to_owned())),
            "{path}"
        );
    }
}

#[test]
fn a_theme_and_its_license_are_read() {
    let listed = manifest(&[("Example", "vs-dark", "./dark.json")]);
    let vsix = package(&[
        ("package.json", listed.as_bytes()),
        ("dark.json", THEME_JSON.as_bytes()),
        ("LICENSE.md", b"MIT text"),
        ("license", b"lower"),
    ]);
    let files = read_theme_files(&vsix, &theme_at("dark.json")).unwrap();
    assert_eq!(files.theme.path, "dark.json");
    assert_eq!(files.theme.bytes, THEME_JSON.as_bytes());
    assert_eq!(files.include, None);
    let license = files.license.unwrap();
    assert_eq!(
        license.path, "LICENSE.md",
        "the first name in the list that exists"
    );
    assert_eq!(license.bytes, b"MIT text");
}

#[test]
fn a_missing_license_is_fine() {
    let files = read_theme_files(&one_theme_package(), &theme_at("themes/dark.json")).unwrap();
    assert_eq!(files.license, None);
}

#[test]
fn the_included_file_is_read_relative_to_the_theme() {
    let vsix = package(&[
        (
            "themes/dark.json",
            br#"{"include": "./base/common.json", "colors": {}}"#,
        ),
        ("themes/base/common.json", THEME_JSON.as_bytes()),
    ]);
    let files = read_theme_files(&vsix, &theme_at("themes/dark.json")).unwrap();
    let include = files.include.unwrap();
    assert_eq!(include.path, "themes/base/common.json");
    assert_eq!(include.bytes, THEME_JSON.as_bytes());
}

#[test]
fn an_include_that_climbs_out_is_refused() {
    for include in ["../x.json", "/x.json", "C:x.json", "a\\..\\x.json"] {
        let theme = format!(r#"{{"include": {include:?}, "colors": {{}}}}"#);
        let vsix = package(&[
            ("themes/dark.json", theme.as_bytes()),
            ("x.json", THEME_JSON.as_bytes()),
        ]);
        assert_eq!(
            read_theme_files(&vsix, &theme_at("themes/dark.json")),
            Err(FetchError::UnsafePath(include.to_owned())),
            "{include}"
        );
    }
}

#[test]
fn a_missing_theme_or_include_names_the_file() {
    let vsix = package(&[("readme.md", b"x")]);
    assert_eq!(
        read_theme_files(&vsix, &theme_at("themes/dark.json")),
        Err(FetchError::MissingFile("themes/dark.json".to_owned()))
    );
    let with_include = package(&[("dark.json", br#"{"include": "base.json"}"#)]);
    assert_eq!(
        read_theme_files(&with_include, &theme_at("dark.json")),
        Err(FetchError::MissingFile("base.json".to_owned()))
    );
}

#[test]
fn an_unsafe_theme_path_is_refused_when_reading_too() {
    let vsix = package(&[("package.json", b"{}")]);
    assert_eq!(
        read_theme_files(&vsix, &theme_at("../x.json")),
        Err(FetchError::UnsafePath("../x.json".to_owned()))
    );
}

#[test]
fn a_theme_that_is_not_json_is_refused() {
    let vsix = package(&[("dark.json", b"colors: nope")]);
    assert!(matches!(
        read_theme_files(&vsix, &theme_at("dark.json")),
        Err(FetchError::BadJson { .. })
    ));
}

#[test]
fn an_entry_is_cut_off_at_the_limit_whatever_the_archive_says() {
    let vsix = package(&[("big.json", &[b' '; 100])]);
    let mut archive = open(&vsix).unwrap();
    assert_eq!(
        read_entry(&mut archive, "extension/big.json", 100)
            .unwrap()
            .map(|bytes| bytes.len()),
        Some(100)
    );
    assert_eq!(
        read_entry(&mut archive, "extension/big.json", 99),
        Err(FetchError::FileTooLarge("extension/big.json".to_owned()))
    );
    assert_eq!(
        read_entry(&mut archive, "extension/none.json", 99),
        Ok(None)
    );
}

#[test]
fn a_theme_of_more_than_a_mebibyte_is_refused() {
    let limit = usize::try_from(SIZE_LIMIT).unwrap();
    let padding = " ".repeat(limit);
    let too_big = format!(r#"{{"colors": {{}}}}{padding}"#);
    let vsix = package(&[("dark.json", too_big.as_bytes())]);
    assert_eq!(
        read_theme_files(&vsix, &theme_at("dark.json")),
        Err(FetchError::FileTooLarge("extension/dark.json".to_owned()))
    );

    let exactly = format!(
        r#"{{"colors": {{}}}}{}"#,
        " ".repeat(limit - r#"{"colors": {}}"#.len())
    );
    assert_eq!(exactly.len(), limit);
    let at_the_limit = package(&[("dark.json", exactly.as_bytes())]);
    read_theme_files(&at_the_limit, &theme_at("dark.json")).unwrap();
}

#[test]
fn a_license_of_more_than_256_kib_is_refused() {
    let vsix = package(&[
        ("dark.json", THEME_JSON.as_bytes()),
        ("LICENSE", &vec![b'x'; 256 * 1024 + 1]),
    ]);
    assert_eq!(
        read_theme_files(&vsix, &theme_at("dark.json")),
        Err(FetchError::FileTooLarge("extension/LICENSE".to_owned()))
    );
}

#[test]
fn a_package_over_the_size_limit_is_refused_without_reading_it() {
    let size = usize::try_from(MAX_VSIX_BYTES).unwrap() + 1;
    assert_eq!(list_themes(&vec![0; size]), Err(FetchError::TooLarge));
}

fn example_id() -> ExtensionId {
    ExtensionId::parse("example.dark-themes").unwrap()
}

#[test]
fn an_installed_theme_lands_below_the_config_and_is_switched_on() {
    let base = tempfile::tempdir().unwrap();
    let config_path = base.path().join("config.toml");
    std::fs::write(&config_path, "# mine\nlang = \"en\"\n").unwrap();
    let vsix = package(&[
        (
            "themes/dark.json",
            br#"{"include": "./base/common.json", "colors": {}}"#,
        ),
        ("themes/base/common.json", THEME_JSON.as_bytes()),
        ("LICENSE", b"Example license"),
    ]);

    let config = install_theme(
        &vsix,
        &theme_at("themes/dark.json"),
        &example_id(),
        &config_path,
    )
    .unwrap();

    let folder = base.path().join("themes/example.dark-themes");
    assert_eq!(
        std::fs::read(folder.join("themes/base/common.json")).unwrap(),
        THEME_JSON.as_bytes()
    );
    assert!(folder.join("themes/dark.json").is_file());
    assert_eq!(
        std::fs::read(folder.join("LICENSE")).unwrap(),
        b"Example license"
    );
    let text = std::fs::read_to_string(&config_path).unwrap();
    assert!(text.starts_with("# mine\nlang = \"en\"\n"), "{text}");
    assert!(
        text.contains("vscode = \"themes/example.dark-themes/themes/dark.json\""),
        "{text}"
    );
    assert_eq!(config.vscode_warning, None);
    assert_eq!(
        config.palette.background,
        iced::Color::from_rgb8(0x10, 0x20, 0x30)
    );
}

#[test]
fn installing_again_replaces_the_files_and_the_setting() {
    let base = tempfile::tempdir().unwrap();
    let config_path = base.path().join("config.toml");
    let first = package(&[("themes/dark.json", THEME_JSON.as_bytes())]);
    install_theme(
        &first,
        &theme_at("themes/dark.json"),
        &example_id(),
        &config_path,
    )
    .unwrap();

    let second = package(&[(
        "themes/light.json",
        br##"{"colors": {"editor.background": "#f0f0f0"}}"##,
    )]);
    let config = install_theme(
        &second,
        &theme_at("themes/light.json"),
        &example_id(),
        &config_path,
    )
    .unwrap();

    let text = std::fs::read_to_string(&config_path).unwrap();
    assert_eq!(text.matches("vscode").count(), 1, "{text}");
    assert_eq!(
        config.palette.background,
        iced::Color::from_rgb8(0xf0, 0xf0, 0xf0)
    );
}

#[test]
fn a_package_that_fails_to_read_writes_nothing() {
    let base = tempfile::tempdir().unwrap();
    let config_path = base.path().join("config.toml");
    let vsix = package(&[("readme.md", b"x")]);

    let result = install_theme(
        &vsix,
        &theme_at("themes/dark.json"),
        &example_id(),
        &config_path,
    );

    assert_eq!(
        result,
        Err(FetchError::MissingFile("themes/dark.json".to_owned()))
    );
    assert!(!base.path().join("themes").exists());
    assert!(!config_path.exists());
}

#[test]
fn a_folder_that_cannot_be_made_is_a_write_error() {
    let base = tempfile::tempdir().unwrap();
    let config_path = base.path().join("config.toml");
    std::fs::write(base.path().join("themes"), "in the way").unwrap();
    let vsix = package(&[("themes/dark.json", THEME_JSON.as_bytes())]);

    let result = install_theme(
        &vsix,
        &theme_at("themes/dark.json"),
        &example_id(),
        &config_path,
    );

    assert!(
        matches!(result, Err(FetchError::WriteFailed { .. })),
        "{result:?}"
    );
    assert!(!config_path.exists());
}

const WEB_THEME: &str = r##"{"colors": {"editor.background": "#102030"}}"##;

#[test]
fn a_theme_file_from_a_link_is_installed_below_themes_web() {
    let base = tempfile::tempdir().unwrap();
    let config_path = base.path().join("config.toml");

    let (config, label) =
        install_theme_file(WEB_THEME.as_bytes(), "example.com-dark.json", &config_path).unwrap();

    assert_eq!(label, "example.com-dark");
    assert!(base
        .path()
        .join("themes/web/example.com-dark.json")
        .is_file());
    assert!(!base
        .path()
        .join("themes/web/example.com-dark.json.part")
        .exists());
    let text = std::fs::read_to_string(&config_path).unwrap();
    assert!(text.contains("themes/web/example.com-dark.json"), "{text}");
    drop(config);
}

#[test]
fn a_link_that_is_no_theme_leaves_the_old_theme_and_the_config_alone() {
    let base = tempfile::tempdir().unwrap();
    let config_path = base.path().join("config.toml");
    install_theme_file(WEB_THEME.as_bytes(), "example.com-dark.json", &config_path).unwrap();
    let before = std::fs::read_to_string(&config_path).unwrap();

    for bad in [
        &b"<html>not json</html>"[..],
        b"{}",
        b"{\"colors\": {}}",
        b"[1]",
    ] {
        let result = install_theme_file(bad, "example.com-dark.json", &config_path);
        assert!(
            matches!(result, Err(FetchError::BadJson { .. })),
            "{result:?}"
        );
    }
    let too_big = vec![b' '; usize::try_from(SIZE_LIMIT).unwrap() + 1];
    assert!(matches!(
        install_theme_file(&too_big, "example.com-dark.json", &config_path),
        Err(FetchError::FileTooLarge(_))
    ));

    assert_eq!(
        std::fs::read_to_string(base.path().join("themes/web/example.com-dark.json")).unwrap(),
        WEB_THEME
    );
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), before);
    assert!(!base
        .path()
        .join("themes/web/example.com-dark.json.part")
        .exists());
}
