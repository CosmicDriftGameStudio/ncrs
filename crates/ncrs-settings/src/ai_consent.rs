//! What goes to a model on another computer, and the agreement to it, given
//! for each server and each kind of data.

use ncrs_ai::{ConsentPurpose, ConsentedOrigins};
use ncrs_config::config::AiProviderChoice;
use ncrs_i18n::Msg;

/// A place of the code that hands text to the AI model, the kinds of data it
/// carries, and the line of the question that tells the user about it. The
/// guard below finds every such place, so a new one cannot go to a server
/// unannounced.
#[derive(Debug, Clone, Copy)]
pub struct ModelCallSite {
    /// From the root of the repository.
    pub file: &'static str,
    /// The agreements the place needs; it sends nothing without all of them.
    pub needs: &'static [ConsentPurpose],
    /// `None` while the call goes to no server (the own-server model cannot
    /// choose folders yet), so the card must not announce it.
    pub announced_as: Option<Msg>,
}

pub const MODEL_CALL_SITES: &[ModelCallSite] = &[
    ModelCallSite {
        file: "crates/ncrs-pipeline/src/ai_step/mod.rs",
        needs: &[ConsentPurpose::Inbox],
        announced_as: Some(Msg::SettingsAiConsentItemDocuments),
    },
    ModelCallSite {
        file: "crates/ncrs-hermes-chat/src/understand.rs",
        needs: &[ConsentPurpose::Archive],
        announced_as: Some(Msg::SettingsAiConsentItemChat),
    },
    ModelCallSite {
        file: "crates/ncrs-hermes-chat/src/rule_flow/ask.rs",
        needs: &[ConsentPurpose::Archive, ConsentPurpose::Folders],
        announced_as: Some(Msg::SettingsAiConsentItemRules),
    },
    ModelCallSite {
        file: "crates/ncrs-filing/src/chat_work/trial.rs",
        needs: &[ConsentPurpose::Folders],
        announced_as: None,
    },
    ModelCallSite {
        file: "crates/ncrs-ops/src/batch_rename/flow.rs",
        needs: &[ConsentPurpose::Folders],
        announced_as: Some(Msg::SettingsAiConsentItemNames),
    },
];

/// The set-up server is off this computer and nothing is agreed to: nothing is
/// sent to it.
pub fn server_is_not_allowed(setting: &ncrs_config::config::AiSetting) -> bool {
    let ncrs_config::config::AiSetting::Http {
        endpoint,
        consented_origins,
        ..
    } = setting
    else {
        return false;
    };
    ConsentPurpose::ALL
        .into_iter()
        .all(|purpose| unagreed_origin(endpoint, consented_origins, purpose).is_some())
}

/// The origin of `endpoint` when it is a server off this computer that is not
/// among the ones agreed to for `purpose`.
fn unagreed_origin(
    endpoint: &str,
    consented: &ConsentedOrigins,
    purpose: ConsentPurpose,
) -> Option<String> {
    if ncrs_ai::endpoint_is_local(endpoint) != Ok(false) {
        return None;
    }
    let origin = ncrs_ai::endpoint_origin(endpoint).ok()?;
    let granted = ncrs_ai::ConsentGivenInSettings::new(consented);
    if ncrs_ai::ConsentLedger::granted(&granted, &origin, std::path::Path::new(""), purpose) {
        return None;
    }
    Some(origin)
}

/// What the AI page lists under `purpose`, in this order.
pub fn announced_items(purpose: ConsentPurpose) -> impl Iterator<Item = Msg> {
    MODEL_CALL_SITES
        .iter()
        .filter(move |site| site.needs.contains(&purpose))
        .filter_map(|site| site.announced_as)
}

/// The agreement a server still needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentAsk {
    pub choice: AiProviderChoice,
    pub purpose: ConsentPurpose,
    /// `scheme://host[:port]`, what the consent is kept for.
    pub origin: String,
}

/// Whether the user agreed to send `purpose` to the server at `endpoint`:
/// `None` for a server on this computer or an address that is no server,
/// which need no agreement.
pub fn server_agreed(
    endpoint: &str,
    consented_origins: &ConsentedOrigins,
    purpose: ConsentPurpose,
) -> Option<bool> {
    if ncrs_ai::endpoint_is_local(endpoint) != Ok(false) {
        return None;
    }
    Some(unagreed_origin(endpoint, consented_origins, purpose).is_none())
}

impl ConsentAsk {
    /// What `choice` still needs before it may send `purpose`: its server is
    /// not on this computer and not among the ones agreed to for it.
    pub fn needed_for(choice: &AiProviderChoice, purpose: ConsentPurpose) -> Option<Self> {
        let AiProviderChoice::Http {
            endpoint,
            consented_origins,
            ..
        } = choice
        else {
            return None;
        };
        let origin = unagreed_origin(endpoint, consented_origins, purpose)?;
        Some(Self {
            choice: choice.clone(),
            purpose,
            origin,
        })
    }

    /// The choice with the server added to the ones agreed to for the purpose.
    pub fn agreed(self) -> AiProviderChoice {
        match self.choice {
            AiProviderChoice::Http {
                endpoint,
                model,
                mut consented_origins,
            } => {
                consented_origins.of_mut(self.purpose).push(self.origin);
                AiProviderChoice::Http {
                    endpoint,
                    model,
                    consented_origins,
                }
            }
            local @ AiProviderChoice::Local { .. } => local,
        }
    }
}

/// The choice with the server taken out of the ones agreed to for `purpose`.
pub fn withdrawn(choice: AiProviderChoice, purpose: ConsentPurpose) -> AiProviderChoice {
    match choice {
        AiProviderChoice::Http {
            endpoint,
            model,
            mut consented_origins,
        } => {
            if let Ok(origin) = ncrs_ai::endpoint_origin(&endpoint) {
                consented_origins.withdraw(purpose, &origin);
            }
            AiProviderChoice::Http {
                endpoint,
                model,
                consented_origins,
            }
        }
        local @ AiProviderChoice::Local { .. } => local,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    /// What hands text to the model: the methods of `Provider` and
    /// `ChatModel`, and the helper that asks the chat model for names.
    const CALLS: &[&str] = &[
        "provider.extract(",
        ".understand(",
        ".generate_rule(",
        ".recognize_levels(",
        ".recognize_levels_again(",
        ".revise_levels(",
        ".choose_folders(",
        "propose_name_spec(",
    ];

    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn rust_files(folder: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if !["tests", "ui_tests", "e2e_tests", "target"].contains(&name.as_str()) {
                    rust_files(&path, found);
                }
            } else if name.ends_with(".rs")
                && !name.ends_with("tests.rs")
                && !name.ends_with("testing.rs")
            {
                found.push(path);
            }
        }
    }

    /// The code (before the test module, comments stripped) of every file
    /// outside `ncrs-ai`, which holds the models themselves, with its
    /// slash-separated path from the repository root, sorted.
    fn guarded_sources() -> Vec<(String, String)> {
        let root = repository();
        let mut files = Vec::new();
        rust_files(&root.join("src"), &mut files);
        for entry in fs::read_dir(root.join("crates")).unwrap() {
            let krate = entry.unwrap().path();
            if krate.file_name().unwrap() != "ncrs-ai" {
                rust_files(&krate.join("src"), &mut files);
            }
        }
        let mut sources: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                let path = file
                    .strip_prefix(&root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let source = fs::read_to_string(file).unwrap();
                let code = source
                    .split("#[cfg(test)]")
                    .next()
                    .unwrap_or_default()
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                (path, code)
            })
            .collect();
        sources.sort();
        sources
    }

    fn files_where(found: impl Fn(&str) -> bool) -> Vec<String> {
        guarded_sources()
            .into_iter()
            .filter(|(_, code)| found(code))
            .map(|(path, _)| path)
            .collect()
    }

    #[test]
    fn every_place_that_calls_the_model_is_announced_on_the_ai_page_and_only_those() {
        let mut announced: Vec<String> = MODEL_CALL_SITES
            .iter()
            .map(|site| site.file.to_owned())
            .collect();
        announced.sort();

        assert_eq!(
            files_where(|code| code
                .lines()
                .any(|line| CALLS.iter().any(|call| line.contains(call)))),
            announced,
            "a place that calls the model needs a line in the list on the AI page \
             (ai_consent.rs and strings.json); a place that no longer does must lose its line"
        );

        // The list the user agrees to is the one drawn from the call sites.
        let page = "crates/ncrs-settings/src/settings/view/ai_page.rs";
        assert!(
            guarded_sources()
                .iter()
                .any(|(file, code)| file == page && code.contains("announced_items(")),
            "the AI page must list every announced item"
        );
    }

    #[test]
    fn every_line_of_the_list_is_announced_once_for_each_agreement_it_needs() {
        let mut lines: Vec<(String, &str)> = ConsentPurpose::ALL
            .into_iter()
            .flat_map(|purpose| {
                announced_items(purpose).map(move |item| (format!("{purpose:?}"), item.key()))
            })
            .collect();
        let listed = lines.len();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), listed);
        assert_eq!(
            listed,
            MODEL_CALL_SITES
                .iter()
                .filter(|site| site.announced_as.is_some())
                .map(|site| site.needs.len())
                .sum::<usize>()
        );
        assert!(
            MODEL_CALL_SITES.iter().all(|site| !site.needs.is_empty()),
            "a place that calls the model needs at least one agreement"
        );
    }

    fn consents(origins: &[&str]) -> ConsentedOrigins {
        ConsentedOrigins::all(origins.iter().map(|origin| (*origin).to_owned()).collect())
    }

    fn http_with(endpoint: &str, consented: ConsentedOrigins) -> AiProviderChoice {
        AiProviderChoice::Http {
            endpoint: endpoint.to_owned(),
            model: "m".to_owned(),
            consented_origins: consented,
        }
    }

    fn http(endpoint: &str, consented: &[&str]) -> AiProviderChoice {
        http_with(endpoint, consents(consented))
    }

    const INBOX: ConsentPurpose = ConsentPurpose::Inbox;

    #[test]
    fn a_server_off_this_computer_needs_the_agreement_until_its_origin_is_agreed() {
        let far = http("https://ai.example.com/v1", &[]);
        let ask = ConsentAsk::needed_for(&far, INBOX).unwrap();
        assert_eq!(ask.origin, "https://ai.example.com");

        let AiProviderChoice::Http {
            consented_origins, ..
        } = ask.agreed()
        else {
            panic!("not an own server");
        };
        assert_eq!(consented_origins.inbox, ["https://ai.example.com"]);
        assert!(consented_origins.archive.is_empty());
        assert!(consented_origins.folders.is_empty());
        assert_eq!(
            ConsentAsk::needed_for(
                &http(
                    "https://ai.example.com/other/path",
                    &["https://ai.example.com"]
                ),
                INBOX
            ),
            None
        );
        assert!(ConsentAsk::needed_for(
            &http(
                "https://ai.example.com:8443/v1",
                &["https://ai.example.com"]
            ),
            INBOX
        )
        .is_some());
        assert!(ConsentAsk::needed_for(
            &http("https://other.example.org", &["https://ai.example.com"]),
            INBOX
        )
        .is_some());
    }

    #[test]
    fn each_kind_of_data_is_agreed_on_its_own() {
        let only_inbox = ConsentedOrigins {
            inbox: vec!["https://ai.example.com".to_owned()],
            ..ConsentedOrigins::default()
        };
        let endpoint = "https://ai.example.com/v1";
        assert_eq!(server_agreed(endpoint, &only_inbox, INBOX), Some(true));
        assert_eq!(
            server_agreed(endpoint, &only_inbox, ConsentPurpose::Archive),
            Some(false)
        );
        assert_eq!(
            server_agreed(endpoint, &only_inbox, ConsentPurpose::Folders),
            Some(false)
        );
        assert!(
            ConsentAsk::needed_for(&http_with(endpoint, only_inbox), ConsentPurpose::Folders)
                .is_some()
        );
    }

    #[test]
    fn the_agreement_is_asked_of_a_server_off_this_computer_only() {
        let given = consents(&["https://ai.example.com"]);
        let none = ConsentedOrigins::default();
        assert_eq!(
            server_agreed("https://ai.example.com/v1", &given, INBOX),
            Some(true)
        );
        assert_eq!(
            server_agreed("https://ai.example.com/v1", &none, INBOX),
            Some(false)
        );
        assert_eq!(server_agreed("http://localhost:11434", &none, INBOX), None);
        assert_eq!(server_agreed("not an address", &none, INBOX), None);
    }

    #[test]
    fn only_a_server_off_this_computer_without_any_consent_is_not_allowed() {
        use ncrs_config::config::AiSetting;
        let server = |endpoint: &str, consented: ConsentedOrigins| AiSetting::Http {
            endpoint: endpoint.to_owned(),
            model: "m".to_owned(),
            consented_origins: consented,
        };
        assert!(server_is_not_allowed(&server(
            "https://ai.example.com/v1",
            ConsentedOrigins::default()
        )));
        assert!(!server_is_not_allowed(&server(
            "https://ai.example.com/v1",
            consents(&["https://ai.example.com"])
        )));
        assert!(!server_is_not_allowed(&server(
            "https://ai.example.com/v1",
            ConsentedOrigins {
                folders: vec!["https://ai.example.com".to_owned()],
                ..ConsentedOrigins::default()
            }
        )));
        assert!(!server_is_not_allowed(&server(
            "http://localhost:11434",
            ConsentedOrigins::default()
        )));
        assert!(!server_is_not_allowed(&AiSetting::Off));
    }

    #[test]
    fn the_model_on_this_computer_never_needs_the_agreement() {
        assert_eq!(
            ConsentAsk::needed_for(&AiProviderChoice::Local { threads: None }, INBOX),
            None
        );
        assert_eq!(
            ConsentAsk::needed_for(&http("http://localhost:11434", &[]), INBOX),
            None
        );
        assert_eq!(
            ConsentAsk::needed_for(&http("http://127.0.0.1:8080/v1", &[]), INBOX),
            None
        );
    }
}
