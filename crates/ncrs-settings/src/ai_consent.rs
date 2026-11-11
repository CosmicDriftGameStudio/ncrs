//! The one question before a model on another computer gets anything: what
//! goes to it, asked once for each server.

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
/// to it. The same check as the question before a switch.
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

/// What the question lists, in this order.
pub fn announced_items() -> impl Iterator<Item = Msg> {
    MODEL_CALL_SITES.iter().map(|site| site.announced_as)
}

/// The switch that waits for the user's yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentAsk {
    pub choice: AiProviderChoice,
    /// `scheme://host[:port]`, what the consent is kept for.
    pub origin: String,
    /// The computer the documents would go to.
    pub host: String,
}

impl ConsentAsk {
    /// The question `choice` needs before it is written: its server is not on
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
        let host = ncrs_ai::endpoint_host(endpoint).ok()?;
        let granted = ncrs_ai::ConsentGivenInSettings::new(consented_origins);
        if ncrs_ai::ConsentLedger::granted(&granted, &origin, std::path::Path::new("")) {
            return None;
        }
        Some(Self {
            choice: choice.clone(),
            origin,
            host,
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

    /// The files outside `ncrs-ai` (which holds the models themselves) whose
    /// code, before its test module, calls the model.
    fn files_calling_the_model() -> Vec<String> {
        let root = repository();
        let mut files = Vec::new();
        rust_files(&root.join("src"), &mut files);
        for entry in fs::read_dir(root.join("crates")).unwrap() {
            let krate = entry.unwrap().path();
            if krate.file_name().unwrap() != "ncrs-ai" {
                rust_files(&krate.join("src"), &mut files);
            }
        }
        let mut calling: Vec<String> = files
            .iter()
            .filter(|file| {
                let source = fs::read_to_string(file).unwrap();
                let code = source.split("#[cfg(test)]").next().unwrap_or_default();
                code.lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .any(|line| CALLS.iter().any(|call| line.contains(call)))
            })
            .map(|file| {
                file.strip_prefix(&root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        calling.sort();
        calling
    }

    #[test]
    fn every_place_that_calls_the_model_is_announced_in_the_question_and_only_those() {
        let mut announced: Vec<String> = MODEL_CALL_SITES
            .iter()
            .map(|site| site.file.to_owned())
            .collect();
        announced.sort();

        assert_eq!(
            files_calling_the_model(),
            announced,
            "a place that calls the model needs a line in the question (ai_consent.rs \
             and strings.json); a place that no longer does must lose its line"
        );

        let source_of = |file: &str| fs::read_to_string(repository().join(file)).unwrap();
        let code_of = |file: &str| {
            let source = source_of(file);
            source
                .split("#[cfg(test)]")
                .next()
                .unwrap_or_default()
                .to_owned()
        };
        // Every model is built from the resolved setting and nowhere else.
        let mut building: Vec<String> = Vec::new();
        let root = repository();
        let mut files = Vec::new();
        rust_files(&root.join("src"), &mut files);
        for entry in fs::read_dir(root.join("crates")).unwrap() {
            let krate = entry.unwrap().path();
            if krate.file_name().unwrap() != "ncrs-ai" {
                rust_files(&krate.join("src"), &mut files);
            }
        }
        for file in &files {
            let source = fs::read_to_string(file).unwrap();
            let code = source.split("#[cfg(test)]").next().unwrap_or_default();
            let calls = code
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"));
            if calls.clone().any(|line| {
                BUILDERS
                    .iter()
                    .any(|b| line.contains(b) && !line.contains("fn "))
            }) {
                let relative = file.strip_prefix(&root).unwrap();
                building.push(relative.to_string_lossy().into_owned());
            }
        }
        building.sort();
        assert_eq!(
            building,
            [
                "crates/ncrs-filing/src/filing/ai.rs",
                "src/app/hermes_chat.rs"
            ],
            "a new place that builds a model must take the resolved AiSetting"
        );
        let wiring = code_of("crates/ncrs-filing/src/filing/ai.rs");
        assert!(wiring.contains("AiSetting::Off => AiWiring::Off"));
        let chat = code_of("src/app/hermes_chat.rs");
        assert!(chat.contains("if !self.chat_ai_on() {\n            self.chat_model = None;"));
        // What reaches the chat, batch rename (local and SFTP) and the rules
        // flow is that model, handed over only while Hermes is on.
        for file in ["src/app/hermes_chat.rs", "src/app/ops.rs"] {
            for line in code_of(file)
                .lines()
                .filter(|line| line.contains(".chat_model.clone()"))
            {
                assert!(line.contains("chat_ai_on()"), "{file}: {line}");
            }
        }
        assert!(source_of("src/app/ops.rs").contains("model: self.chat_ai_on()"));
    }

    #[test]
    fn every_line_of_the_question_is_announced_once() {
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
    fn a_server_off_this_computer_needs_the_question_until_its_origin_is_agreed() {
        let far = http("https://ai.example.com/v1", &[]);
        let ask = ConsentAsk::needed_for(&far).unwrap();
        assert_eq!(ask.origin, "https://ai.example.com");
        assert_eq!(ask.host, "ai.example.com");

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
    fn the_model_on_this_computer_never_needs_the_question() {
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
