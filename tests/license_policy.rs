//! deny.toml and about.toml must never let a GPL-family license in.
//!
//! cargo-deny cannot express the ban itself: its license check has only an
//! allow list. Adding "LGPL-2.1-or-later" to `allow`, or as a per-crate
//! exception, would make `cargo deny check licenses` pass again, so this test
//! is what keeps the ban from being undone by a one-line edit.

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    const GPL_FAMILY_PREFIXES: [&str; 3] = ["GPL-", "LGPL-", "AGPL-"];

    #[derive(Deserialize)]
    struct DenyConfig {
        licenses: DenyLicenses,
    }

    #[derive(Deserialize)]
    struct DenyLicenses {
        allow: Vec<String>,
        #[serde(default)]
        exceptions: Vec<DenyException>,
    }

    #[derive(Deserialize)]
    struct DenyException {
        allow: Vec<String>,
        #[serde(rename = "crate")]
        crate_name: String,
    }

    #[derive(Deserialize)]
    struct AboutConfig {
        accepted: Vec<String>,
    }

    /// True for any SPDX expression that names a GPL-family license, also
    /// inside `WITH` or `OR`: an exception does not change the license it
    /// amends.
    fn names_gpl_family(expression: &str) -> bool {
        expression
            .split(|separator: char| {
                separator.is_whitespace() || separator == '(' || separator == ')'
            })
            .any(|identifier| {
                GPL_FAMILY_PREFIXES
                    .iter()
                    .any(|prefix| identifier.starts_with(prefix))
            })
    }

    /// Every way deny.toml lets a GPL-family license through, as readable
    /// lines. An empty allow list is reported too, so a moved table cannot
    /// turn this into a check of nothing.
    fn deny_toml_violations(source: &str) -> Result<Vec<String>, String> {
        let config: DenyConfig = toml_edit::de::from_str(source).map_err(|err| err.to_string())?;
        let mut violations = Vec::new();
        if config.licenses.allow.is_empty() {
            violations.push("deny.toml has no [licenses] allow list".to_owned());
        }
        for license in &config.licenses.allow {
            if names_gpl_family(license) {
                violations.push(format!("deny.toml allows {license}"));
            }
        }
        for exception in &config.licenses.exceptions {
            for license in &exception.allow {
                if names_gpl_family(license) {
                    violations.push(format!(
                        "deny.toml makes an exception for {license} in {}",
                        exception.crate_name
                    ));
                }
            }
        }
        Ok(violations)
    }

    fn about_toml_violations(source: &str) -> Result<Vec<String>, String> {
        let config: AboutConfig = toml_edit::de::from_str(source).map_err(|err| err.to_string())?;
        let mut violations = Vec::new();
        if config.accepted.is_empty() {
            violations.push("about.toml has no accepted list".to_owned());
        }
        for license in &config.accepted {
            if names_gpl_family(license) {
                violations.push(format!("about.toml accepts {license}"));
            }
        }
        Ok(violations)
    }

    #[test]
    fn recognises_gpl_family_identifiers() {
        for gpl in [
            "GPL-2.0-only",
            "GPL-3.0-or-later",
            "LGPL-2.1-or-later",
            "AGPL-3.0-only",
            "GPL-2.0-only WITH Classpath-exception-2.0",
            "(MIT OR LGPL-2.1-or-later)",
        ] {
            assert!(names_gpl_family(gpl), "{gpl} not recognised");
        }
        for permissive in [
            "MIT",
            "Apache-2.0 WITH LLVM-exception",
            "BSD-3-Clause",
            "MPL-2.0",
            "BSL-1.0",
            "CDLA-Permissive-2.0",
        ] {
            assert!(!names_gpl_family(permissive), "{permissive} taken for GPL");
        }
    }

    #[test]
    fn a_gpl_family_entry_in_deny_toml_is_reported() {
        let source = r#"
            [licenses]
            allow = ["MIT", "LGPL-2.1-or-later"]
            exceptions = [{ allow = ["GPL-2.0-only"], crate = "some-crate" }]
        "#;
        assert_eq!(
            deny_toml_violations(source),
            Ok(vec![
                "deny.toml allows LGPL-2.1-or-later".to_owned(),
                "deny.toml makes an exception for GPL-2.0-only in some-crate".to_owned(),
            ])
        );
    }

    #[test]
    fn deny_toml_allows_no_gpl_family_license() {
        assert_eq!(
            deny_toml_violations(include_str!("../deny.toml")),
            Ok(Vec::new())
        );
    }

    #[test]
    fn about_toml_accepts_no_gpl_family_license() {
        assert_eq!(
            about_toml_violations(include_str!("../about.toml")),
            Ok(Vec::new())
        );
    }
}
