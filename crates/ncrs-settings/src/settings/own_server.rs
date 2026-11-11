//! What the own server on the AI page remembers: the key typed there and the
//! answer of the connection test.

use ncrs_ai::Secret;

use crate::ai_server::AiCheckResult;
use crate::message::SettingField;
use crate::settings::SettingsState;

/// Where "Test connection" stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AiCheck {
    #[default]
    Idle,
    Asking,
    Done(AiCheckResult),
}

impl SettingsState {
    /// The address as typed, else the saved one.
    pub fn typed_endpoint(&self) -> String {
        self.field_text(SettingField::AiEndpoint).trim().to_owned()
    }

    /// The key is typed again: what the test said about the old one is stale.
    pub fn type_key(&mut self, key: Secret) {
        self.ai_key = Some(key);
        self.ai_check = AiCheck::Idle;
    }

    /// The models the last test returned, if it did.
    pub fn listed_models(&self) -> Option<&[String]> {
        match &self.ai_check {
            AiCheck::Done(AiCheckResult::Listed(models)) => Some(models),
            AiCheck::Idle | AiCheck::Asking | AiCheck::Done(_) => None,
        }
    }

    /// An answer of the test: taken only while the address is still the one
    /// that was asked.
    pub fn checked(&mut self, endpoint: &str, result: AiCheckResult) {
        if self.typed_endpoint() == endpoint && self.ai_check == AiCheck::Asking {
            self.ai_check = AiCheck::Done(result);
        }
    }
}

impl SettingsState {
    /// Save on the AI page: address, model and key are written together. A
    /// wrong value shows at its row.
    pub fn save_server(&mut self) -> Option<crate::message::SettingChange> {
        if self.provider() == ncrs_config::config::AiProviderName::Local {
            return None;
        }
        match self.http_change() {
            Ok(change) => Some(change),
            Err((field, problem)) => {
                self.problems.insert(field, problem);
                None
            }
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
    use ncrs_config::config::{AiProviderChoice, AiProviderName};

    use super::*;
    use crate::message::SettingChange;

    const SAVED: &str = "[modules.filing.ai]\nenabled = true\nprovider = \"http\"\n\
                         endpoint = \"https://api.example.com/v1\"\nmodel = \"gpt-5\"\n";

    fn state_of(source: &str) -> SettingsState {
        SettingsState::new(
            false,
            Ok(Box::new(ncrs_config::config::from_text(source).unwrap())),
            false,
        )
    }

    fn typed(state: &mut SettingsState, field: SettingField, text: &str) {
        state.edit(field, text.to_owned());
    }

    #[test]
    fn saving_a_new_address_hands_the_change_on_with_the_servers_agreed_to_so_far() {
        let mut state = state_of(SAVED);
        typed(
            &mut state,
            SettingField::AiEndpoint,
            "https://other.example.org",
        );
        assert_eq!(
            state.save_server(),
            Some(SettingChange::AiProvider(AiProviderChoice::Http {
                endpoint: "https://other.example.org".to_owned(),
                model: "gpt-5".to_owned(),
                consented_origins: vec!["https://api.example.com".to_owned()],
            }))
        );
    }

    #[test]
    fn the_answer_for_an_address_that_was_changed_meanwhile_is_dropped() {
        let mut state = state_of(SAVED);
        state.ai_check = AiCheck::Asking;
        typed(
            &mut state,
            SettingField::AiEndpoint,
            "https://other.example.org",
        );
        state.checked(
            "https://api.example.com/v1",
            AiCheckResult::Listed(vec!["gpt-5".to_owned()]),
        );
        assert_eq!(state.listed_models(), None);
    }

    #[test]
    fn a_list_from_the_test_is_offered_and_typing_the_key_again_forgets_it() {
        let mut state = state_of(SAVED);
        state.ai_check = AiCheck::Asking;
        state.checked(
            "https://api.example.com/v1",
            AiCheckResult::Listed(vec!["a".to_owned(), "b".to_owned()]),
        );
        assert_eq!(state.listed_models().map(<[String]>::len), Some(2));

        state.type_key(Secret::new("sk-new".to_owned()));
        assert_eq!(state.listed_models(), None);
        assert_eq!(state.ai_check, AiCheck::Idle);
    }

    #[test]
    fn choosing_the_local_model_forgets_the_key_and_the_test_and_saving_writes_nothing() {
        let mut state = state_of(SAVED);
        state.type_key(Secret::new("sk-new".to_owned()));
        state.ai_check = AiCheck::Asking;
        assert!(state.choose_provider(AiProviderName::Local).is_some());
        assert!(state.ai_key.is_none() && state.ai_check == AiCheck::Idle);
        assert_eq!(state.save_server(), None);
    }
}
