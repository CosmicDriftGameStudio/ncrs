//! Text that must not reach a log: the typed key and the key being shown.

use super::*;

/// Text a user typed that may be, or may contain, key material.
#[derive(Clone, PartialEq, Default)]
pub struct SecretText(String);

impl SecretText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SecretText {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl From<String> for SecretText {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

/// The new recovery key while its card is up. Cloned only as a handle, so the
/// key text exists nowhere but in the card.
#[derive(Clone)]
pub struct ShownRecoveryKey(Arc<RecoveryKey>);

impl ShownRecoveryKey {
    pub fn new(key: RecoveryKey) -> Self {
        Self(Arc::new(key))
    }

    /// Called by the card view and the copy button, nowhere else.
    pub fn display_text(&self) -> String {
        self.0.to_string()
    }
}

impl PartialEq for ShownRecoveryKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ShownRecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShownRecoveryKey(<redacted>)")
    }
}
