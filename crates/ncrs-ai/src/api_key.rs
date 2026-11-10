//! The API key of an own server, one keychain entry per server address. It is
//! only ever written to the keychain, never to the config or a log. The calls
//! block; run them on the blocking pool. The errors are text for a status line
//! and never contain the key.

use crate::http::{endpoint_key_name, Secret};

/// Like the SFTP passwords: the App Store build keeps its entries apart from
/// those of an installed ncrs.
const SERVICE: &str = "ncrs-ai";
const APP_STORE_SERVICE: &str = "ncrs-appstore-ai";

/// The keychain account of the key for `endpoint`: it names the host (and
/// port), so two servers never share a key.
pub fn api_key_account(endpoint: &str) -> Result<String, String> {
    endpoint_key_name(endpoint)
        .map(|name| format!("api-key:{name}"))
        .map_err(|problem| problem.to_string())
}

fn entry(endpoint: &str, sandboxed: bool) -> Result<keyring::Entry, String> {
    let service = if sandboxed {
        APP_STORE_SERVICE
    } else {
        SERVICE
    };
    keyring::Entry::new(service, &api_key_account(endpoint)?).map_err(|error| error.to_string())
}

/// `None` when no key is stored for the server.
pub fn load_api_key(endpoint: &str, sandboxed: bool) -> Result<Option<Secret>, String> {
    match entry(endpoint, sandboxed)?.get_secret() {
        Ok(stored) => String::from_utf8(stored)
            .map(|text| Some(Secret::new(text)))
            .map_err(|_not_text| "the entry is not text".to_owned()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Where a request finds its key when the config holds none: asked once for
/// every request, so a key saved after the provider was built is used. The
/// call may block; requests run on the blocking pool.
pub trait KeyLookup: Send + Sync + std::fmt::Debug {
    fn key(&self) -> Option<Secret>;
}

/// The keychain entry of one server.
#[derive(Debug, Clone)]
pub struct KeychainKey {
    pub endpoint: String,
    pub sandboxed: bool,
}

impl KeyLookup for KeychainKey {
    fn key(&self) -> Option<Secret> {
        load_api_key(&self.endpoint, self.sandboxed)
            .ok()
            .flatten()
            .filter(|key| !key.expose().is_empty())
    }
}

/// An empty key removes the entry: a server without a key needs none.
pub fn store_api_key(endpoint: &str, sandboxed: bool, key: &Secret) -> Result<(), String> {
    let kept = entry(endpoint, sandboxed)?;
    if key.expose().is_empty() {
        return match kept.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        };
    }
    kept.set_secret(key.expose().as_bytes())
        .map_err(|error| error.to_string())
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
    fn the_account_names_the_host_and_port_in_lower_case() {
        assert_eq!(
            api_key_account("https://OpenRouter.ai/api/v1").unwrap(),
            "api-key:openrouter.ai"
        );
        assert_eq!(
            api_key_account("http://localhost:11434/v1").unwrap(),
            "api-key:localhost:11434"
        );
        api_key_account("not an address").unwrap_err();
    }
}
