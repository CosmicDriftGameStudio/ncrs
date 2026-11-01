//! Open VSX, the open registry for VS Code extensions: which extension the
//! user means, and the blocking download of its package. Nothing here runs on
//! the UI thread, and nothing is sent before the user asks for it.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

use ncrs_config::config::{ConfigError, FilingWriteError, SIZE_LIMIT};

const API_BASE: &str = "https://open-vsx.org/api/";
const DOWNLOAD_PREFIX: &str = "https://open-vsx.org/";
/// Open VSX forwards its downloads to the host of its file storage.
const DOWNLOAD_HOSTS: [&str; 2] = ["open-vsx.org", "openvsx.eclipsecontent.org"];
pub const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];
pub const MAX_REDIRECTS: usize = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_PART_LENGTH: usize = 128;
const MAX_THEME_FILE_NAME: usize = 120;

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

/// A direct link to a VS Code theme file: `https` only, no user info, no port
/// but 443, and a name that ends in `.json` or `.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFileUrl {
    url: String,
    file_name: String,
}

impl ThemeFileUrl {
    pub fn parse(input: &str) -> Option<Self> {
        let url = input.trim();
        if url.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
            return None;
        }
        let after_scheme = url.strip_prefix("https://")?;
        let authority = after_scheme.split(['/', '?', '#']).next()?;
        let rest = after_scheme.strip_prefix(authority).unwrap_or_default();
        let host = match authority.split_once(':') {
            Some((host, "443")) => host,
            Some(_) => return None,
            None => authority,
        };
        let host_is_plain = host
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
            && host
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-');
        if !host_is_plain {
            return None;
        }
        let path = rest.split(['?', '#']).next().unwrap_or_default();
        let last = path.rsplit('/').next().unwrap_or_default();
        let lower = last.to_ascii_lowercase();
        if !(lower.ends_with(".json") || lower.ends_with(".jsonc")) || last.starts_with('.') {
            return None;
        }
        let file_name: String = format!("{host}-{last}")
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .take(MAX_THEME_FILE_NAME)
            .collect();
        Some(Self {
            url: url.to_owned(),
            file_name,
        })
    }

    /// The name the file gets on disk: host and file name of the link, reduced
    /// to `[A-Za-z0-9._-]`.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }
}

/// What the user typed in the theme card: an extension on Open VSX, or a link
/// to one theme file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    Extension(ExtensionId),
    File(ThemeFileUrl),
}

impl ThemeSource {
    pub fn parse(input: &str) -> Option<Self> {
        ExtensionId::parse(input)
            .map(Self::Extension)
            .or_else(|| ThemeFileUrl::parse(input).map(Self::File))
    }
}

impl fmt::Display for ThemeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Extension(id) => id.fmt(f),
            Self::File(link) => f.write_str(&link.url),
        }
    }
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

fn check_declared_size(declared: Option<u64>, limit: u64) -> Result<(), FetchError> {
    match declared {
        Some(length) if length > limit => Err(FetchError::TooLarge),
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

/// A redirect of a theme file link may go anywhere, as long as it stays on
/// `https` and names no user.
fn check_https_target(url: &str) -> Result<(), FetchError> {
    let refused = || FetchError::DownloadRefused {
        host: shown_host(url),
    };
    let after_scheme = url.strip_prefix("https://").ok_or_else(refused)?;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(refused());
    }
    Ok(())
}

fn download(
    agent: &ureq::Agent,
    first_url: &str,
    limit: u64,
    check_target: fn(&str) -> Result<(), FetchError>,
) -> Result<Vec<u8>, FetchError> {
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
            check_target(&target)?;
            url = target;
            continue;
        }
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        check_declared_size(response.body().content_length(), limit)?;
        return response
            .body_mut()
            .with_config()
            .limit(limit)
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
    let vsix = download(
        &agent,
        &metadata.download_url,
        MAX_VSIX_BYTES,
        check_download_host,
    )?;
    Ok(FetchedExtension {
        id: id.clone(),
        display_name: metadata.display_name,
        version: metadata.version,
        license: metadata.license,
        vsix: vsix.into(),
    })
}

/// Downloads the theme file a link names, at most as big as a theme file
/// inside a package may be. Blocks: call it from a blocking thread only.
pub fn fetch_theme_file(link: &ThemeFileUrl) -> Result<Vec<u8>, FetchError> {
    download(&agent(), &link.url, SIZE_LIMIT, check_https_target)
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
mod tests;
