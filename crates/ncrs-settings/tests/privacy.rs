//! PRIVACY.md names every host the settings crate can reach, and the consent
//! card for an AI server says the same as the list in it.

#![allow(clippy::tests_outside_test_module)]
use ncrs_i18n::Language;
use ncrs_settings::ai_consent::MODEL_CALL_SITES;
use ncrs_settings::ai_model::AI_MODEL_HOSTS;

const PRIVACY: &str = include_str!("../../../PRIVACY.md");

fn assert_named(host: &str) {
    let name = host.trim_start_matches('.');
    assert!(PRIVACY.contains(name), "PRIVACY.md does not name {name}");
}

#[test]
fn the_theme_hosts_are_in_privacy() {
    assert_named("open-vsx.org");
    assert_named("openvsx.eclipsecontent.org");
}

#[test]
fn the_model_download_hosts_are_in_privacy() {
    for host in AI_MODEL_HOSTS {
        assert_named(host);
    }
    assert_named(ncrs_intake::OCR_MODELS_HOST);
}

#[test]
fn the_page_that_switches_the_update_check_is_in_privacy() {
    assert!(PRIVACY.contains("Updates page"));
}

#[test]
fn every_line_of_the_consent_card_is_in_privacy() {
    let privacy = PRIVACY.to_lowercase();
    for site in MODEL_CALL_SITES {
        let Some(item) = site.announced_as else {
            continue;
        };
        let text = Language::English.text(item);
        let line = text.trim_end_matches('.').to_lowercase();
        assert!(
            privacy.contains(&line),
            "PRIVACY.md lacks the consent line: {line}"
        );
    }
}
