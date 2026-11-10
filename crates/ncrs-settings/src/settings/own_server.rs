//! What the dialog of the own server remembers: the key typed in it, the answer
//! of the connection test and whether the user understood that documents go
//! to another computer.

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

    /// The host the documents would go to, when the typed address is a usable
    /// one that is not on this computer.
    pub fn host_documents_go_to(&self) -> Option<String> {
        let typed = self.typed_endpoint();
        match ncrs_ai::endpoint_is_local(&typed) {
            Ok(false) => ncrs_ai::endpoint_host(&typed).ok(),
            Ok(true) | Err(_) => None,
        }
    }

    /// "I understand" is ticked, or the address is the one already saved,
    /// which was understood when it was saved.
    pub fn understood_leaving(&self) -> bool {
        let typed = self.typed_endpoint();
        self.ai_understood
            || (!typed.is_empty() && typed == self.saved_text(SettingField::AiEndpoint).trim())
    }

    /// An address off this computer that was not understood yet: it is not
    /// written.
    pub fn save_is_blocked(&self) -> bool {
        self.host_documents_go_to().is_some() && !self.understood_leaving()
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
    /// Save in the dialog: address, model and key are written together. The
    /// dialog closes unless something is wrong, which then shows at its row.
    pub fn save_server(&mut self) -> Option<crate::message::SettingChange> {
        if self.provider() == ncrs_config::config::AiProviderName::Local {
            self.server_dialog = false;
            return None;
        }
        match self.http_change() {
            Ok(change) => {
                self.server_dialog = false;
                Some(change)
            }
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

    const SAVED: &str = "[modules.filing.ai]\nprovider = \"http\"\n\
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
    fn the_saved_address_starts_understood_and_a_new_one_off_this_computer_does_not() {
        let mut state = state_of(SAVED);
        assert!(!state.save_is_blocked());

        typed(
            &mut state,
            SettingField::AiEndpoint,
            "https://other.example.org",
        );
        assert!(state.save_is_blocked());
        assert_eq!(
            state.host_documents_go_to().as_deref(),
            Some("other.example.org")
        );

        state.ai_understood = true;
        assert!(!state.save_is_blocked());
    }

    #[test]
    fn an_address_on_this_computer_needs_no_tick() {
        let mut state = state_of(SAVED);
        typed(
            &mut state,
            SettingField::AiEndpoint,
            "http://localhost:11434",
        );
        assert_eq!(state.host_documents_go_to(), None);
        assert!(!state.save_is_blocked());
    }

    #[test]
    fn saving_an_address_that_was_not_understood_writes_nothing_and_says_why() {
        let mut state = state_of(SAVED);
        typed(
            &mut state,
            SettingField::AiEndpoint,
            "https://other.example.org",
        );
        assert_eq!(state.save_server(), None);
        assert_eq!(
            state.problems.get(&SettingField::AiEndpoint),
            Some(&crate::settings::FieldProblem::NotUnderstood)
        );

        state.ai_understood = true;
        state.server_dialog = true;
        assert_eq!(
            state.save_server(),
            Some(SettingChange::AiProvider(AiProviderChoice::Http {
                endpoint: "https://other.example.org".to_owned(),
                model: "gpt-5".to_owned(),
            }))
        );
        assert!(!state.server_dialog);
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
    fn leaving_the_dialog_for_the_local_model_writes_nothing() {
        let mut state = state_of(SAVED);
        state.server_dialog = true;
        assert!(state.choose_provider(AiProviderName::Local).is_some());
        state.server_dialog = true;
        assert_eq!(state.save_server(), None);
        assert!(!state.server_dialog);
    }
}
