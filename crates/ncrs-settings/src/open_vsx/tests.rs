use super::*;

fn id(text: &str) -> ExtensionId {
    ExtensionId::parse(text).unwrap()
}

fn pair(text: &str) -> (String, String) {
    let parsed = id(text);
    (parsed.publisher, parsed.name)
}

fn expected(publisher: &str, name: &str) -> (String, String) {
    (publisher.to_owned(), name.to_owned())
}

#[test]
fn a_plain_id_is_split_at_the_first_dot() {
    assert_eq!(
        pair("example.dark-themes"),
        expected("example", "dark-themes")
    );
    assert_eq!(
        pair("  example.dark-themes\n"),
        expected("example", "dark-themes")
    );
    assert_eq!(id("example.dark-themes").to_string(), "example.dark-themes");
}

#[test]
fn a_marketplace_link_names_the_id_in_its_query() {
    for link in [
        "https://marketplace.visualstudio.com/items?itemName=example.dark-themes",
        "https://marketplace.visualstudio.com/items?ref=a&itemName=example.dark-themes&b=1",
        " https://marketplace.visualstudio.com/items?itemName=example.dark-themes ",
    ] {
        assert_eq!(pair(link), expected("example", "dark-themes"), "{link}");
    }
}

#[test]
fn an_open_vsx_link_names_namespace_and_name_and_ignores_the_rest() {
    for link in [
        "https://open-vsx.org/extension/example/dark-themes",
        "https://open-vsx.org/extension/example/dark-themes/",
        "https://open-vsx.org/extension/example/dark-themes/1.2.3",
        "https://open-vsx.org/extension/example/dark-themes/1.2.3/",
    ] {
        assert_eq!(pair(link), expected("example", "dark-themes"), "{link}");
    }
}

#[test]
fn anything_else_is_not_an_extension_id() {
    for text in [
        "",
        "   ",
        "example",
        "..",
        "a/b",
        "a.",
        ".b",
        "a.b?x",
        "a.b#x",
        "a.%2e",
        "%2e.b",
        "a b.c",
        "a.b c",
        "a..b",
        "a.b.c",
        "-a.b",
        "a._b",
        "https://example.com/items?itemName=a.b",
        "http://marketplace.visualstudio.com/items?itemName=a.b",
        "https://marketplace.visualstudio.com/items?itemName=a/b.c",
        "https://marketplace.visualstudio.com/items?itemName=..",
        "https://marketplace.visualstudio.com/items?other=a.b",
        "https://open-vsx.org/extension/example",
        "https://open-vsx.org/extension/../dark-themes",
        "https://open-vsx.org/extension/example/dark%2Dthemes",
        "https://open-vsx.org.evil.example/extension/a/b",
    ] {
        assert_eq!(ExtensionId::parse(text), None, "{text:?}");
    }
    let too_long = format!("{}.b", "a".repeat(129));
    assert_eq!(ExtensionId::parse(&too_long), None);
    let longest = format!("{}.b", "a".repeat(128));
    assert!(ExtensionId::parse(&longest).is_some());
}

#[test]
fn a_link_to_a_theme_file_is_told_from_an_extension_id() {
    let link = "https://raw.githubusercontent.com/user/repo/main/themes/dark.json?raw=1";
    let Some(ThemeSource::File(file)) = ThemeSource::parse(link) else {
        panic!("a theme file link");
    };
    assert_eq!(file.file_name(), "raw.githubusercontent.com-dark.json");
    assert!(matches!(
        ThemeSource::parse("example.dark-themes"),
        Some(ThemeSource::Extension(_))
    ));
    assert!(matches!(
        ThemeSource::parse("https://open-vsx.org/extension/example/dark-themes"),
        Some(ThemeSource::Extension(_))
    ));
}

#[test]
fn only_https_links_to_json_files_without_user_info_are_theme_files() {
    for text in [
        "http://example.com/theme.json",
        "ftp://example.com/theme.json",
        "https://example.com/theme.txt",
        "https://example.com/",
        "https://example.com/dir/",
        "https://user@example.com/theme.json",
        "https://example.com:8443/theme.json",
        "https://example.com/theme.json and more",
        "https://exa mple.com/theme.json",
        "https:///theme.json",
        "https://example.com/.json",
        "example.com/theme.json",
    ] {
        assert_eq!(ThemeSource::parse(text), None, "{text:?}");
    }
    assert!(ThemeFileUrl::parse("https://example.com:443/a/Theme.JSONC").is_some());
}

#[test]
fn the_file_name_on_disk_holds_no_separator() {
    let link = ThemeFileUrl::parse("https://example.com/a/b%2F..%5Cc.json").unwrap();
    assert_eq!(link.file_name(), "example.com-b_2F.._5Cc.json");
}

#[test]
fn a_theme_file_bigger_than_the_limit_is_refused() {
    assert_eq!(check_declared_size(Some(SIZE_LIMIT), SIZE_LIMIT), Ok(()));
    assert_eq!(
        check_declared_size(Some(SIZE_LIMIT + 1), SIZE_LIMIT),
        Err(FetchError::TooLarge)
    );
    assert_eq!(check_declared_size(None, SIZE_LIMIT), Ok(()));
}

#[test]
fn a_redirect_of_a_theme_file_stays_on_https() {
    assert_eq!(check_https_target("https://cdn.example.com/a.json"), Ok(()));
    for url in [
        "http://cdn.example.com/a.json",
        "https://user@cdn.example.com/a.json",
        "https:///a.json",
        "",
    ] {
        assert!(
            matches!(
                check_https_target(url),
                Err(FetchError::DownloadRefused { .. })
            ),
            "{url:?}"
        );
    }
}

#[test]
fn an_entry_with_a_leading_dot_lists_the_hosts_below_it_only() {
    let hosts = ["huggingface.co", ".hf.co"];
    for url in [
        "https://huggingface.co/a",
        "https://us.aws.cdn.hf.co/a",
        "https://CDN.HF.CO/a",
    ] {
        assert_eq!(check_host_in(url, &hosts), Ok(()), "{url}");
    }
    for url in [
        "https://hf.co/a",
        "https://evilhf.co/a",
        "https://hf.co.evil.example/a",
        "https://x.hf.co.evil.example/a",
        "http://x.hf.co/a",
        "https://user@x.hf.co/a",
        "https://x.hf.co:8443/a",
    ] {
        assert!(
            matches!(
                check_host_in(url, &hosts),
                Err(FetchError::DownloadRefused { .. })
            ),
            "{url}"
        );
    }
}

#[test]
fn the_hosts_of_open_vsx_pass_the_download_check() {
    for url in [
        "https://open-vsx.org/api/a/b/1/file/a.b-1.vsix",
        "https://openvsx.eclipsecontent.org/a/b.vsix",
        "https://OPEN-VSX.org/x",
        "https://open-vsx.org:443/x",
        "https://open-vsx.org",
    ] {
        assert_eq!(check_download_host(url), Ok(()), "{url}");
    }
}

#[test]
fn other_hosts_schemes_ports_and_user_info_are_refused() {
    for url in [
        "http://open-vsx.org/x",
        "ftp://open-vsx.org/x",
        "https://example.com/x",
        "https://open-vsx.org.evil.example/x",
        "https://evil.example/open-vsx.org/",
        "https://user@open-vsx.org/x",
        "https://open-vsx.org@evil.example/x",
        "https://open-vsx.org:8443/x",
        "https://open-vsx.org:@evil.example/x",
        "https://open-vsx.org\\@evil.example/x",
        "//open-vsx.org/x",
        "/x",
        "",
    ] {
        assert!(
            matches!(
                check_download_host(url),
                Err(FetchError::DownloadRefused { .. })
            ),
            "{url}"
        );
    }
    assert_eq!(
        check_download_host("https://example.com/x"),
        Err(FetchError::DownloadRefused {
            host: "example.com".to_owned()
        })
    );
}

#[test]
fn the_first_download_link_must_be_on_open_vsx_itself() {
    assert_eq!(
        check_download_url("https://open-vsx.org/api/a/b/1/file/a.vsix"),
        Ok(())
    );
    for url in [
        "https://openvsx.eclipsecontent.org/a.vsix",
        "https://example.com/a.vsix",
        "https://open-vsx.org.evil.example/a.vsix",
        "http://open-vsx.org/a.vsix",
    ] {
        assert!(check_download_url(url).is_err(), "{url}");
    }
}

#[test]
fn the_metadata_is_read_from_the_answer() {
    let body = r#"{
        // comments and trailing commas do not matter
        "namespace": "example",
        "name": "dark-themes",
        "version": "2.0.1",
        "displayName": "Example Dark",
        "license": "MIT",
        "files": { "download": "https://open-vsx.org/api/example/dark-themes/2.0.1/file/x.vsix", },
    }"#;
    assert_eq!(
        parse_metadata(body, &id("example.dark-themes")),
        Ok(Metadata {
            display_name: "Example Dark".to_owned(),
            version: "2.0.1".to_owned(),
            license: Some("MIT".to_owned()),
            download_url: "https://open-vsx.org/api/example/dark-themes/2.0.1/file/x.vsix"
                .to_owned(),
        })
    );
}

#[test]
fn a_missing_display_name_falls_back_to_the_name_and_a_missing_license_stays_empty() {
    let body = r#"{"version":"1","files":{"download":"https://open-vsx.org/x.vsix"}}"#;
    let metadata = parse_metadata(body, &id("example.dark-themes")).unwrap();
    assert_eq!(metadata.display_name, "dark-themes");
    assert_eq!(metadata.license, None);
}

#[test]
fn a_long_license_is_cut_with_an_ellipsis() {
    let body = format!(
        r#"{{"version":"1","license":"{}","files":{{"download":"https://open-vsx.org/x.vsix"}}}}"#,
        "L".repeat(300)
    );
    let metadata = parse_metadata(&body, &id("example.dark-themes")).unwrap();
    let license = metadata.license.unwrap();
    assert_eq!(license.chars().count(), MAX_SHOWN_CHARS);
    assert!(license.ends_with('…'));
}

#[test]
fn control_and_bidi_characters_are_dropped_from_shown_text() {
    assert_eq!(
        shown_text("  A\nB\u{202E}C\u{2067}D\u{200F}E\t ", 80),
        "ABCDE"
    );
    assert_eq!(shown_text("\u{202E}\n", 80), "");
}

#[test]
fn an_answer_without_a_download_link_is_an_error() {
    let extension = id("example.dark-themes");
    for body in [
        r#"{"version":"1"}"#,
        r#"{"files":{}}"#,
        r#"{"files":{"download":3}}"#,
    ] {
        assert_eq!(
            parse_metadata(body, &extension),
            Err(FetchError::NoDownloadLink)
        );
    }
    assert!(matches!(
        parse_metadata("not json", &extension),
        Err(FetchError::BadAnswer(_))
    ));
    assert!(matches!(
        parse_metadata("[1]", &extension),
        Err(FetchError::BadAnswer(_))
    ));
}

#[test]
fn a_package_declared_bigger_than_the_limit_is_refused_before_reading() {
    assert_eq!(check_declared_size(None, MAX_VSIX_BYTES), Ok(()));
    assert_eq!(
        check_declared_size(Some(MAX_VSIX_BYTES), MAX_VSIX_BYTES),
        Ok(())
    );
    assert_eq!(
        check_declared_size(Some(MAX_VSIX_BYTES + 1), MAX_VSIX_BYTES),
        Err(FetchError::TooLarge)
    );
}

#[test]
fn network_failures_are_told_apart() {
    assert_eq!(
        network_error(ureq::Error::Timeout(ureq::Timeout::Global)),
        FetchError::Timeout
    );
    assert!(matches!(
        network_error(ureq::Error::HostNotFound),
        FetchError::NoConnection(_)
    ));
    assert!(matches!(
        network_error(ureq::Error::ConnectionFailed),
        FetchError::NoConnection(_)
    ));
    assert!(matches!(
        network_error(ureq::Error::Tls("bad certificate")),
        FetchError::Tls(_)
    ));
    assert_eq!(
        network_error(ureq::Error::BodyExceedsLimit(1)),
        FetchError::TooLarge
    );
    assert!(matches!(
        network_error(ureq::Error::TooManyRedirects),
        FetchError::Network(_)
    ));
}
