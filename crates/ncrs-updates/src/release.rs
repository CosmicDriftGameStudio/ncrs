//! The newest release as GitHub reports it, and whether it is news.

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};
use semver::Version;

/// Where the notes of a release live. The link is opened in the browser, so
/// anything else in the answer of the server is refused.
pub const TRUSTED_NOTES_PREFIX: &str = "https://github.com/CosmicDriftGameStudio/ncrs/releases/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRelease {
    pub version: Version,
    pub notes_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    NotJson(String),
    Missing(&'static str),
    BadVersion(String),
    NotStable,
    UntrustedUrl,
}

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJson(why) => write!(f, "the answer is not JSON: {why}"),
            Self::Missing(field) => write!(f, "the answer has no `{field}`"),
            Self::BadVersion(tag) => write!(f, "the tag `{tag}` is not a version"),
            Self::NotStable => f.write_str("the release is a draft or a pre-release"),
            Self::UntrustedUrl => f.write_str("the release link is not on the releases page"),
        }
    }
}

pub fn parse_latest(body: &str) -> Result<LatestRelease, ReleaseError> {
    let parsed = parse_to_value(body, &ParseOptions::default())
        .map_err(|err| ReleaseError::NotJson(err.to_string()))?;
    let Some(JsonValue::Object(mut root)) = parsed else {
        return Err(ReleaseError::NotJson("not an object".to_owned()));
    };
    if root.take_boolean("draft").unwrap_or(false)
        || root.take_boolean("prerelease").unwrap_or(false)
    {
        return Err(ReleaseError::NotStable);
    }
    let tag = root
        .take_string("tag_name")
        .ok_or(ReleaseError::Missing("tag_name"))?;
    let notes_url = root
        .take_string("html_url")
        .ok_or(ReleaseError::Missing("html_url"))?;
    checked_release(&tag, &notes_url)
}

/// The one check every release passes, whether it comes from GitHub or from
/// the file of the last check: a stable version and a link on the releases
/// page.
pub fn checked_release(tag: &str, notes_url: &str) -> Result<LatestRelease, ReleaseError> {
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|err| ReleaseError::BadVersion(format!("{tag}: {err}")))?;
    if !version.pre.is_empty() {
        return Err(ReleaseError::NotStable);
    }
    if !notes_url.starts_with(TRUSTED_NOTES_PREFIX) {
        return Err(ReleaseError::UntrustedUrl);
    }
    Ok(LatestRelease {
        version,
        notes_url: notes_url.to_owned(),
    })
}

/// The version without the ` (hash)` tail of a development build.
pub fn base_version(display_version: &str) -> Option<Version> {
    let end = display_version
        .find([' ', '-', '+'])
        .unwrap_or(display_version.len());
    Version::parse(&display_version[..end]).ok()
}

/// News only when the release is newer than the running base version and not
/// the one the user chose to skip; a still newer release is news again.
pub fn is_news(release: &LatestRelease, running: &Version, skipped: Option<&str>) -> bool {
    let skipped_version = skipped.and_then(|text| Version::parse(text).ok());
    release.version.pre.is_empty()
        && release.version > *running
        && skipped_version.is_none_or(|skip| skip != release.version)
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{
        "tag_name": "v1.3.0",
        "html_url": "https://github.com/CosmicDriftGameStudio/ncrs/releases/tag/v1.3.0",
        "draft": false,
        "prerelease": false,
        "name": "ncrs 1.3.0"
    }"#;

    fn release(version: &str) -> LatestRelease {
        LatestRelease {
            version: Version::parse(version).unwrap(),
            notes_url: format!("{TRUSTED_NOTES_PREFIX}tag/v{version}"),
        }
    }

    #[test]
    fn the_answer_of_github_is_read() {
        let parsed = parse_latest(ANSWER).unwrap();
        assert_eq!(parsed.version, Version::new(1, 3, 0));
        assert!(parsed.notes_url.ends_with("/tag/v1.3.0"));
    }

    #[test]
    fn a_link_outside_the_releases_page_is_refused() {
        for url in [
            "https://evil.example/CosmicDriftGameStudio/ncrs/releases/tag/v1.3.0",
            "http://github.com/CosmicDriftGameStudio/ncrs/releases/tag/v1.3.0",
            "https://github.com/CosmicDriftGameStudio/ncrs-evil/releases/tag/v1.3.0",
            "https://github.com/CosmicDriftGameStudio/ncrs/releases.evil.example/",
        ] {
            let body = ANSWER.replace(
                "https://github.com/CosmicDriftGameStudio/ncrs/releases/tag/v1.3.0",
                url,
            );
            assert_eq!(
                parse_latest(&body),
                Err(ReleaseError::UntrustedUrl),
                "{url}"
            );
        }
    }

    #[test]
    fn drafts_and_pre_releases_are_refused() {
        for flag in ["draft", "prerelease"] {
            let body = ANSWER.replace(&format!("\"{flag}\": false"), &format!("\"{flag}\": true"));
            assert_eq!(parse_latest(&body), Err(ReleaseError::NotStable), "{flag}");
        }
        let body = ANSWER.replace("v1.3.0\",", "v1.3.0-rc.1\",");
        assert_eq!(parse_latest(&body), Err(ReleaseError::NotStable));
    }

    #[test]
    fn broken_answers_are_errors() {
        assert!(matches!(
            parse_latest("nope"),
            Err(ReleaseError::NotJson(_))
        ));
        assert!(matches!(parse_latest("[]"), Err(ReleaseError::NotJson(_))));
        assert_eq!(parse_latest("{}"), Err(ReleaseError::Missing("tag_name")));
        let body = ANSWER.replace("v1.3.0\",", "latest\",");
        assert!(matches!(
            parse_latest(&body),
            Err(ReleaseError::BadVersion(_))
        ));
    }

    #[test]
    fn the_base_version_drops_the_development_tail() {
        assert_eq!(base_version("1.2.0"), Some(Version::new(1, 2, 0)));
        assert_eq!(base_version("1.2.0 (568c38e)"), Some(Version::new(1, 2, 0)));
        assert_eq!(base_version("garbage"), None);
    }

    #[test]
    fn only_a_newer_release_is_news() {
        let running = Version::new(1, 2, 0);
        assert!(is_news(&release("1.2.1"), &running, None));
        assert!(!is_news(&release("1.2.0"), &running, None));
        assert!(!is_news(&release("1.1.9"), &running, None));
    }

    #[test]
    fn a_skipped_version_is_quiet_but_a_newer_one_is_not() {
        let running = Version::new(1, 2, 0);
        assert!(!is_news(&release("1.3.0"), &running, Some("1.3.0")));
        assert!(is_news(&release("1.3.1"), &running, Some("1.3.0")));
    }
}
