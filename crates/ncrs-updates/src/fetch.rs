//! The one request of the check. Not compiled into the App Store build.

use crate::release::LatestRelease;

/// The look-up as a plain function, so `update` is tested without a network.
/// The argument is the running app's version, sent in the User-Agent.
pub type FetchFn = fn(&str) -> Result<LatestRelease, String>;

#[cfg(not(feature = "sandbox"))]
fn user_agent(app_version: &str) -> String {
    format!("ncrs/{app_version}")
}

#[cfg(not(feature = "sandbox"))]
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/CosmicDriftGameStudio/ncrs/releases/latest";
#[cfg(not(feature = "sandbox"))]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(not(feature = "sandbox"))]
const MAX_ANSWER_BYTES: u64 = 256 * 1024;

#[cfg(not(feature = "sandbox"))]
pub fn latest_release(app_version: &str) -> Result<LatestRelease, String> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(TIMEOUT))
        .user_agent(user_agent(app_version))
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
pub fn latest_release(_app_version: &str) -> Result<LatestRelease, String> {
    Err("the update check is not part of the sandboxed build".to_owned())
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "sandbox"))]
    use super::user_agent;

    #[cfg(not(feature = "sandbox"))]
    #[test]
    fn user_agent_names_the_given_app_version() {
        assert_eq!(user_agent("1.4.0"), "ncrs/1.4.0");
    }
}
