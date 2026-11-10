//! The two things the dialog of the own server asks of the outside: whether the
//! server answers (and what models it offers), and where the API key is kept.
//! Both block; the update runs them on the blocking pool.

use ncrs_ai::{AiError, ModelList, OpenAiCompatibleConfig, Secret};
use ncrs_fs::forbid_user_machine_in_tests;

/// What "Test connection" asks.
#[derive(Debug, Clone)]
pub struct AiCheckJob {
    pub endpoint: String,
    /// The key as typed in the dialog; without one, the key in the keychain is
    /// used.
    pub typed_key: Option<Secret>,
    pub sandboxed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiCheckResult {
    /// The server answers and offers these models, sorted.
    Listed(Vec<String>),
    /// The server answers, but has no list.
    NotListed,
    KeyRefused,
    /// The server answered with this error status.
    ServerError(u16),
    NoAnswer,
}

pub fn check_ai_server(job: &AiCheckJob) -> AiCheckResult {
    let api_key = match &job.typed_key {
        Some(typed) => Some(typed.clone()),
        None => {
            forbid_user_machine_in_tests("the system keychain for the AI key");
            ncrs_ai::load_api_key(&job.endpoint, job.sandboxed)
                .ok()
                .flatten()
        }
    }
    .filter(|key| !key.expose().is_empty());
    let config = OpenAiCompatibleConfig {
        endpoint: job.endpoint.clone(),
        model: String::new(),
        api_key,
        key_lookup: None,
        usage_log: None,
    };
    answer_of(ncrs_ai::list_models(&config))
}

fn answer_of(listed: Result<ModelList, AiError>) -> AiCheckResult {
    match listed {
        Ok(ModelList::Listed(models)) => AiCheckResult::Listed(models),
        Ok(ModelList::NotListed) => AiCheckResult::NotListed,
        Err(AiError::Status(401 | 403)) => AiCheckResult::KeyRefused,
        Err(AiError::Status(status)) => AiCheckResult::ServerError(status),
        Err(_other) => AiCheckResult::NoAnswer,
    }
}

/// Keeps `key` in the keychain under the host of `endpoint`; an empty key
/// removes the entry.
pub fn store_ai_key(endpoint: &str, sandboxed: bool, key: &Secret) -> Result<(), String> {
    forbid_user_machine_in_tests("the system keychain for the AI key");
    ncrs_ai::store_api_key(endpoint, sandboxed, key)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn the_answers_of_the_server_become_the_four_outcomes() {
        let listed = Ok(ModelList::Listed(vec!["a".to_owned()]));
        assert_eq!(
            answer_of(listed),
            AiCheckResult::Listed(vec!["a".to_owned()])
        );
        assert_eq!(
            answer_of(Ok(ModelList::NotListed)),
            AiCheckResult::NotListed
        );
        assert_eq!(
            answer_of(Err(AiError::Status(401))),
            AiCheckResult::KeyRefused
        );
        assert_eq!(
            answer_of(Err(AiError::Status(403))),
            AiCheckResult::KeyRefused
        );
        assert_eq!(
            answer_of(Err(AiError::Status(500))),
            AiCheckResult::ServerError(500)
        );
        assert_eq!(
            answer_of(Err(AiError::Transport("the request timed out"))),
            AiCheckResult::NoAnswer
        );
    }
}
