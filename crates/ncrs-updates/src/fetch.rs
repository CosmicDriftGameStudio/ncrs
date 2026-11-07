//! The one request of the check. Not compiled into the App Store build.

use crate::release::LatestRelease;

/// The look-up as a plain function, so `update` is tested without a network.
pub type FetchFn = fn() -> Result<LatestRelease, String>;

#[cfg(not(feature = "sandbox"))]
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/CosmicDriftGameStudio/ncrs/releases/latest";
#[cfg(not(feature = "sandbox"))]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(not(feature = "sandbox"))]
const MAX_ANSWER_BYTES: u64 = 256 * 1024;

#[cfg(not(feature = "sandbox"))]
pub fn latest_release() -> Result<LatestRelease, String> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("ncrs/", env!("CARGO_PKG_VERSION")))
        .max_redirects(0)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|err| err.to_string())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_string()
        .map_err(|err| err.to_string())?;
    crate::release::parse_latest(&body).map_err(|err| err.to_string())
}

#[cfg(feature = "sandbox")]
pub fn latest_release() -> Result<LatestRelease, String> {
    Err("the update check is not part of the sandboxed build".to_owned())
}
