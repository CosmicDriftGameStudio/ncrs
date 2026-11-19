//! PRIVACY.md names the host of the update check.

#![cfg(not(feature = "sandbox"))]
#![allow(clippy::expect_used, clippy::tests_outside_test_module)]

use ncrs_updates::fetch::LATEST_RELEASE_URL;

const PRIVACY: &str = include_str!("../../../PRIVACY.md");

#[test]
fn the_host_of_the_update_check_is_in_privacy() {
    let host = LATEST_RELEASE_URL
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .expect("the release URL is https");
    assert!(PRIVACY.contains(host), "PRIVACY.md does not name {host}");
}
