//! Open VSX, the open registry for VS Code extensions: which extension the
//! user means, and the blocking download of its package. Nothing here runs on
//! the UI thread, and nothing is sent before the user asks for it.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

use crate::config::{ConfigError, FilingWriteError};

const API_BASE: &str = "https://open-vsx.org/api/";
const DOWNLOAD_PREFIX: &str = "https://open-vsx.org/";
/// Open VSX forwards its downloads to the host of its file storage.
const DOWNLOAD_HOSTS: [&str; 2] = ["open-vsx.org", "openvsx.eclipsecontent.org"];
pub const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];
pub const MAX_REDIRECTS: usize = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_PART_LENGTH: usize = 128;

/// The largest package taken; ureq's own default would be 10 MiB.
pub const MAX_VSIX_BYTES: u64 = 20 * 1024 * 1024;

/// What can go wrong between typing an ID and having the theme installed. The
/// card words each case in the active language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    InvalidId,
    NotFound(String),
    Status(u16),
    NoConnection(String),
    Timeout,
    Tls(String),
    Network(String),
    BadAnswer(String),
    NoDownloadLink,
    DownloadRefused { host: String },
    TooManyRedirects,
    TooLarge,
    BadPackage(String),
    NoThemes,
    MissingFile(String),
    UnsafePath(String),
    FileTooLarge(String),
    BadJson { file: String, reason: String },
    NoConfigPath,
    Config(ConfigError),
    WriteFailed { path: String, reason: String },
}

/// `publisher.name`, each part checked: it ends up in a URL and in a folder
/// name, so nothing outside `[A-Za-z0-9_-]` gets through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionId {
    publisher: String,
    name: String,
}

impl ExtensionId {
    /// What the user typed: an ID, a Marketplace link or an Open VSX link.
    /// `None` for anything else.
    pub fn parse(input: &str) -> Option<Self> {
        let text = input.trim();
        if let Some(query) = text.strip_prefix("https://marketplace.visualstudio.com/items?") {
            let item = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("itemName="))?;
            return Self::from_dotted(item);
        }
        if let Some(path) = text.strip_prefix("https://open-vsx.org/extension/") {
            let mut parts = path.split('/');
            return Self::from_parts(parts.next()?, parts.next()?);
        }
        Self::from_dotted(text)
    }

    fn from_dotted(text: &str) -> Option<Self> {
        let (publisher, name) = text.split_once('.')?;
        Self::from_parts(publisher, name)
    }

    fn from_parts(publisher: &str, name: &str) -> Option<Self> {
        (is_valid_part(publisher) && is_valid_part(name)).then(|| Self {
            publisher: publisher.to_owned(),
            name: name.to_owned(),
        })
    }
}

impl fmt::Display for ExtensionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.publisher, self.name)
    }
}

fn is_valid_part(part: &str) -> bool {
    let mut characters = part.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && part.len() <= MAX_PART_LENGTH
        && characters.all(|next| next.is_ascii_alphanumeric() || next == '_' || next == '-')
}

/// What Open VSX knows about an extension, and its package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedExtension {
    pub id: ExtensionId,
    pub display_name: String,
    pub version: String,
    pub license: Option<String>,
    pub vsix: Arc<[u8]>,
}

/// The answer of `/api/<namespace>/<name>`, as far as the download needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Metadata {
    display_name: String,
    version: String,
    license: Option<String>,
    download_url: String,
}

fn parse_metadata(body: &str, id: &ExtensionId) -> Result<Metadata, FetchError> {
    let parsed = parse_to_value(body, &ParseOptions::default())
        .map_err(|err| FetchError::BadAnswer(err.to_string()))?;
    let Some(JsonValue::Object(mut root)) = parsed else {
        return Err(FetchError::BadAnswer("not a JSON object".to_owned()));
    };
    let download_url = root
        .take_object("files")
        .and_then(|mut files| files.take_string("download"))
        .map(|url| url.into_owned())
        .ok_or(FetchError::NoDownloadLink)?;
    let text_of = |value: Option<std::borrow::Cow<'_, str>>| {
        value
            .map(|text| shown_text(&text, MAX_SHOWN_CHARS))
            .filter(|text| !text.is_empty())
    };
    Ok(Metadata {
        display_name: text_of(root.take_string("displayName")).unwrap_or_else(|| id.name.clone()),
        version: text_of(root.take_string("version")).unwrap_or_default(),
        license: text_of(root.take_string("license")),
        download_url,
    })
}

pub const MAX_SHOWN_CHARS: usize = 80;

/// Text from the server or a package, made safe to show: control and bidi
/// control characters dropped, trimmed, cut to `max_chars` with an ellipsis.
pub fn shown_text(raw: &str, max_chars: usize) -> String {
    const BIDI_CONTROLS: [char; 4] = ['\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}'];
    let kept: String = raw
        .chars()
        .filter(|ch| {
            !ch.is_control()
                && !BIDI_CONTROLS.contains(ch)
                && !('\u{202C}'..='\u{202E}').contains(ch)
                && !('\u{2066}'..='\u{2069}').contains(ch)
        })
        .collect();
    let trimmed = kept.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let cut: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// The part between `https://` and the first `/`, `?` or `#`, for a message.
fn shown_host(url: &str) -> String {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme)
        .chars()
        .take(80)
        .collect()
}

/// `url` must be `https://`, name one of the Open VSX hosts exactly, carry no
/// user info and use no port but 443.
fn check_download_host(url: &str) -> Result<(), FetchError> {
    check_host_in(url, &DOWNLOAD_HOSTS)
}

/// The same check for any other fixed list of hosts, so every download of the
/// app is held to one rule.
pub fn check_host_in(url: &str, hosts: &[&str]) -> Result<(), FetchError> {
    let refused = || FetchError::DownloadRefused {
        host: shown_host(url),
    };
    let after_scheme = url.strip_prefix("https://").ok_or_else(refused)?;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let allowed = !authority.contains('@')
        && port.is_none_or(|digits| digits == "443")
        && hosts.iter().any(|known| host_is_listed(host, known));
    if allowed {
        Ok(())
    } else {
        Err(refused())
    }
}

/// An entry that starts with a dot lists every host below that name (`.hf.co`
/// takes `us.cdn.hf.co`, not `hf.co` and not `evilhf.co`). Hugging Face
/// redirects a download to a regional CDN host under hf.co, whose name is not
/// known in advance.
fn host_is_listed(host: &str, known: &str) -> bool {
    if known.starts_with('.') {
        host.len() > known.len() && host.to_ascii_lowercase().ends_with(known)
    } else {
        host.eq_ignore_ascii_case(known)
    }
}

/// The metadata names where the package is. Only Open VSX itself may be named.
fn check_download_url(url: &str) -> Result<(), FetchError> {
    if !url.starts_with(DOWNLOAD_PREFIX) {
        return Err(FetchError::DownloadRefused {
            host: shown_host(url),
        });
    }
    check_download_host(url)
}

fn check_declared_size(declared: Option<u64>) -> Result<(), FetchError> {
    match declared {
        Some(length) if length > MAX_VSIX_BYTES => Err(FetchError::TooLarge),
        _ => Ok(()),
    }
}

pub fn network_error(error: ureq::Error) -> FetchError {
    let detail = error.to_string();
    match error {
        ureq::Error::Timeout(_) => FetchError::Timeout,
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            FetchError::NoConnection(detail)
        }
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => FetchError::Tls(detail),
        ureq::Error::BodyExceedsLimit(_) => FetchError::TooLarge,
        _ => FetchError::Network(detail),
    }
}

pub fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("ncrs/", env!("CARGO_PKG_VERSION")))
        // Redirects are followed by hand: every hop is checked.
        .max_redirects(0)
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(config)
}

fn fetch_metadata(agent: &ureq::Agent, id: &ExtensionId) -> Result<Metadata, FetchError> {
    let url = format!("{API_BASE}{}/{}", id.publisher, id.name);
    let mut response = agent.get(&url).call().map_err(network_error)?;
    match response.status().as_u16() {
        200 => {}
        404 => return Err(FetchError::NotFound(id.to_string())),
        other => return Err(FetchError::Status(other)),
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_METADATA_BYTES)
        .read_to_vec()
        .map_err(network_error)?;
    let text = String::from_utf8(body)
        .map_err(|_not_utf8| FetchError::BadAnswer("not valid UTF-8".to_owned()))?;
    let metadata = parse_metadata(&text, id)?;
    check_download_url(&metadata.download_url)?;
    Ok(metadata)
}

fn download(agent: &ureq::Agent, first_url: &str) -> Result<Vec<u8>, FetchError> {
    let mut url = first_url.to_owned();
    for hop in 0..=MAX_REDIRECTS {
        let mut response = agent.get(&url).call().map_err(network_error)?;
        let status = response.status().as_u16();
        if REDIRECT_STATUSES.contains(&status) {
            if hop == MAX_REDIRECTS {
                return Err(FetchError::TooManyRedirects);
            }
            let target = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            check_download_host(&target)?;
            url = target;
            continue;
        }
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        check_declared_size(response.body().content_length())?;
        return response
            .body_mut()
            .with_config()
            .limit(MAX_VSIX_BYTES)
            .read_to_vec()
            .map_err(network_error);
    }
    Err(FetchError::TooManyRedirects)
}

/// Asks Open VSX about `id` and downloads the package. Blocks: call it from a
/// blocking thread only.
pub fn fetch_extension(id: &ExtensionId) -> Result<FetchedExtension, FetchError> {
    let agent = agent();
    let metadata = fetch_metadata(&agent, id)?;
    let vsix = download(&agent, &metadata.download_url)?;
    Ok(FetchedExtension {
        id: id.clone(),
        display_name: metadata.display_name,
        version: metadata.version,
        license: metadata.license,
        vsix: vsix.into(),
    })
}

impl From<FilingWriteError> for FetchError {
    fn from(error: FilingWriteError) -> Self {
        match error {
            FilingWriteError::Config(config) => Self::Config(config),
            FilingWriteError::Io { path, reason } => Self::WriteFailed {
                path: path.display().to_string(),
                reason,
            },
            FilingWriteError::Rule(_) | FilingWriteError::PathNotText(_) => Self::WriteFailed {
                path: String::new(),
                reason: "not a theme setting".to_owned(),
            },
        }
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
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
        assert_eq!(check_declared_size(None), Ok(()));
        assert_eq!(check_declared_size(Some(MAX_VSIX_BYTES)), Ok(()));
        assert_eq!(
            check_declared_size(Some(MAX_VSIX_BYTES + 1)),
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
}
