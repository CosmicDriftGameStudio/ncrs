//! What goes to a model on another computer, and the agreement to it, given
//! once for each server.

use ncrs_config::config::AiProviderChoice;
use ncrs_i18n::Msg;

/// A place of the code that hands text to the AI model, and the line of the
/// question that tells the user about it. The guard below finds every such
/// place, so a new one cannot go to a server unannounced.
#[derive(Debug, Clone, Copy)]
pub struct ModelCallSite {
    /// From the root of the repository.
    pub file: &'static str,
    pub announced_as: Msg,
}

pub const MODEL_CALL_SITES: &[ModelCallSite] = &[
    ModelCallSite {
        file: "crates/ncrs-pipeline/src/ai_step/mod.rs",
        announced_as: Msg::SettingsAiConsentItemDocuments,
    },
    ModelCallSite {
        file: "crates/ncrs-hermes-chat/src/understand.rs",
        announced_as: Msg::SettingsAiConsentItemChat,
    },
    ModelCallSite {
        file: "crates/ncrs-hermes-chat/src/rule_flow/ask.rs",
        announced_as: Msg::SettingsAiConsentItemRules,
    },
    ModelCallSite {
        file: "crates/ncrs-ops/src/batch_rename/flow.rs",
        announced_as: Msg::SettingsAiConsentItemNames,
    },
];

/// The set-up server is off this computer and not agreed to: nothing is sent
/// to it.
pub fn server_is_not_allowed(setting: &ncrs_config::config::AiSetting) -> bool {
    let ncrs_config::config::AiSetting::Http {
        endpoint,
        model,
        consented_origins,
    } = setting
    else {
        return false;
    };
    ConsentAsk::needed_for(&AiProviderChoice::Http {
        endpoint: endpoint.clone(),
        model: model.clone(),
        consented_origins: consented_origins.clone(),
    })
    .is_some()
}

/// What the AI page lists under the server, in this order.
pub fn announced_items() -> impl Iterator<Item = Msg> {
    MODEL_CALL_SITES.iter().map(|site| site.announced_as)
}

/// The agreement a server still needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentAsk {
    pub choice: AiProviderChoice,
    /// `scheme://host[:port]`, what the consent is kept for.
    pub origin: String,
}

/// Whether the user agreed to send to the server at `endpoint`: `None` for a
/// server on this computer or an address that is no server, which need no
/// agreement.
pub fn server_agreed(endpoint: &str, consented_origins: &[String]) -> Option<bool> {
    if ncrs_ai::endpoint_is_local(endpoint) != Ok(false) {
        return None;
    }
    let asked = ConsentAsk::needed_for(&AiProviderChoice::Http {
        endpoint: endpoint.to_owned(),
        model: String::new(),
        consented_origins: consented_origins.to_vec(),
    });
    Some(asked.is_none())
}

impl ConsentAsk {
    /// What `choice` still needs before it may send: its server is not on
    /// this computer and not among the ones agreed to.
    pub fn needed_for(choice: &AiProviderChoice) -> Option<Self> {
        let AiProviderChoice::Http {
            endpoint,
            consented_origins,
            ..
        } = choice
        else {
            return None;
        };
        if ncrs_ai::endpoint_is_local(endpoint) != Ok(false) {
            return None;
        }
        let origin = ncrs_ai::endpoint_origin(endpoint).ok()?;
        let granted = ncrs_ai::ConsentGivenInSettings::new(consented_origins);
        if ncrs_ai::ConsentLedger::granted(&granted, &origin, std::path::Path::new("")) {
            return None;
        }
        Some(Self {
            choice: choice.clone(),
            origin,
        })
    }

    /// The choice with the server added to the ones agreed to.
    pub fn agreed(self) -> AiProviderChoice {
        match self.choice {
            AiProviderChoice::Http {
                endpoint,
                model,
                mut consented_origins,
            } => {
                consented_origins.push(self.origin);
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
        "propose_name_spec(",
    ];

    /// What builds a model: only the module's wiring and the chat's model
    /// may, both from the resolved `AiSetting`, which is `Off` when `[hermes]`
    /// is switched off.
    const BUILDERS: &[&str] = &["http_ai_provider(", "shared_local_model("];

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

        // Every model is built from the resolved setting and nowhere else.
        assert_eq!(
            files_where(|code| code.lines().any(|line| BUILDERS
                .iter()
                .any(|builder| line.contains(builder) && !line.contains("fn ")))),
            [
                "crates/ncrs-filing/src/filing/ai.rs",
                "src/app/hermes_chat.rs"
            ],
            "a new place that builds a model must take the resolved AiSetting"
        );
        // The list the user agrees to is the one drawn from the call sites.
        let page = "crates/ncrs-settings/src/settings/view/ai_page.rs";
        assert!(
            guarded_sources()
                .iter()
                .any(|(file, code)| file == page && code.contains("announced_items()")),
            "the AI page must list every announced item"
        );
        // What reaches the chat, batch rename (local and SFTP) and the rules
        // flow is that model, handed over only while Hermes is on.
        for (file, code) in guarded_sources() {
            if file == "src/app/hermes_chat.rs" || file == "src/app/ops.rs" {
                for line in code
                    .lines()
                    .filter(|line| line.contains(".chat_model.clone()"))
                {
                    assert!(line.contains("chat_ai_on()"), "{file}: {line}");
                }
            }
        }
    }

    #[test]
    fn every_line_of_the_list_is_announced_once() {
        let mut lines: Vec<&str> = announced_items().map(|item| item.key()).collect();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), MODEL_CALL_SITES.len());
    }

    fn http(endpoint: &str, consented: &[&str]) -> AiProviderChoice {
        AiProviderChoice::Http {
            endpoint: endpoint.to_owned(),
            model: "m".to_owned(),
            consented_origins: consented
                .iter()
                .map(|origin| (*origin).to_owned())
                .collect(),
        }
    }

    #[test]
    fn a_server_off_this_computer_needs_the_agreement_until_its_origin_is_agreed() {
        let far = http("https://ai.example.com/v1", &[]);
        let ask = ConsentAsk::needed_for(&far).unwrap();
        assert_eq!(ask.origin, "https://ai.example.com");

        let AiProviderChoice::Http {
            consented_origins, ..
        } = ask.agreed()
        else {
            panic!("not an own server");
        };
        assert_eq!(consented_origins, ["https://ai.example.com"]);
        assert_eq!(
            ConsentAsk::needed_for(&http(
                "https://ai.example.com/other/path",
                &["https://ai.example.com"]
            )),
            None
        );
        assert!(ConsentAsk::needed_for(&http(
            "https://ai.example.com:8443/v1",
            &["https://ai.example.com"]
        ))
        .is_some());
        assert!(ConsentAsk::needed_for(&http(
            "https://other.example.org",
            &["https://ai.example.com"]
        ))
        .is_some());
    }

    #[test]
    fn the_agreement_is_asked_of_a_server_off_this_computer_only() {
        let given = vec!["https://ai.example.com".to_owned()];
        assert_eq!(
            server_agreed("https://ai.example.com/v1", &given),
            Some(true)
        );
        assert_eq!(server_agreed("https://ai.example.com/v1", &[]), Some(false));
        assert_eq!(server_agreed("http://localhost:11434", &[]), None);
        assert_eq!(server_agreed("not an address", &[]), None);
    }

    #[test]
    fn only_a_server_off_this_computer_without_consent_is_not_allowed() {
        use ncrs_config::config::AiSetting;
        let server = |endpoint: &str, consented: &[&str]| AiSetting::Http {
            endpoint: endpoint.to_owned(),
            model: "m".to_owned(),
            consented_origins: consented
                .iter()
                .map(|origin| (*origin).to_owned())
                .collect(),
        };
        assert!(server_is_not_allowed(&server(
            "https://ai.example.com/v1",
            &[]
        )));
        assert!(!server_is_not_allowed(&server(
            "https://ai.example.com/v1",
            &["https://ai.example.com"]
        )));
        assert!(!server_is_not_allowed(&server(
            "http://localhost:11434",
            &[]
        )));
        assert!(!server_is_not_allowed(&AiSetting::Off));
    }

    #[test]
    fn the_model_on_this_computer_never_needs_the_agreement() {
        assert_eq!(
            ConsentAsk::needed_for(&AiProviderChoice::Local { threads: None }),
            None
        );
        assert_eq!(
            ConsentAsk::needed_for(&http("http://localhost:11434", &[])),
            None
        );
        assert_eq!(
            ConsentAsk::needed_for(&http("http://127.0.0.1:8080/v1", &[])),
            None
        );
    }
}
