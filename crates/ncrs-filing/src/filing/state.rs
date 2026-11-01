//! The phases of the filing module and the reasons it holds the index back.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProblem {
    /// The credential store has no key for the encrypted index.
    Missing,
    /// The stored key does not open the index.
    Mismatch,
}

/// Why the recovery key was not applied. Worded in the view.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyError {
    InvalidFormat,
    DoesNotOpen,
    ConfirmFirst,
    /// The `Display` of an `IndexError`, which carries no key material.
    Other(String),
}

#[derive(Debug, PartialEq)]
pub enum Phase {
    Starting,
    AskEncryption {
        encrypted: bool,
        busy: bool,
        error: Option<String>,
    },
    ShowRecoveryKey {
        key: ShownRecoveryKey,
        /// 0-based group the user has to type back.
        group: usize,
        typed: SecretText,
        mismatch: bool,
    },
    AskRecoveryKey {
        reason: KeyProblem,
        typed: SecretText,
        confirm_replace: bool,
        busy: bool,
        error: Option<KeyError>,
    },
    PlainFileWarning,
    Ready,
    /// The plain index is being turned into an encrypted one.
    Encrypting,
    /// The index files and the key are being removed.
    StartingOver,
    Failed(String),
    Dismissed,
}

impl Phase {
    pub(in crate::filing) fn name(&self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::AskEncryption { .. } => "AskEncryption",
            Self::ShowRecoveryKey { .. } => "ShowRecoveryKey",
            Self::AskRecoveryKey { .. } => "AskRecoveryKey",
            Self::PlainFileWarning => "PlainFileWarning",
            Self::Ready => "Ready",
            Self::Encrypting => "Encrypting",
            Self::StartingOver => "StartingOver",
            Self::Failed(_) => "Failed",
            Self::Dismissed => "Dismissed",
        }
    }

    pub(in crate::filing) fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::AskEncryption { busy: true, .. }
                | Self::AskRecoveryKey { busy: true, .. }
                | Self::Encrypting
                | Self::StartingOver
        )
    }
}

/// Why the last attempt to change the protection of the index did not go
/// through, worded in the view. Never carries key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionNote {
    EncryptFailed(String),
    StartOverFailed(String),
    /// The index is gone; its entry in the credential store could not be removed.
    KeyEntryNotRemoved(String),
}

/// Why the intake does not get the index right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldReason {
    Starting,
    WaitingForUser,
    PlainFileUnconfirmed,
    Dismissed,
    Failed,
}

/// The name of the person using the app, as the system has it; `None` where
/// the system has none to give, as nothing else would be more than a guess.
#[cfg(target_os = "macos")]
pub(in crate::filing) fn own_full_name() -> Option<String> {
    let full_name = objc2_foundation::NSFullUserName().to_string();
    let trimmed = full_name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(not(target_os = "macos"))]
pub(in crate::filing) fn own_full_name() -> Option<String> {
    None
}

/// What is the user's own: the system's full name and what the config lists.
pub(in crate::filing) fn own_details(filing: &Filing) -> OwnDetails {
    OwnDetails {
        names: own_full_name()
            .into_iter()
            .chain(filing.own_names.iter().cloned())
            .collect(),
        ibans: filing.own_ibans.clone(),
        tax_ids: filing.own_tax_ids.clone(),
        kind_language: KindLanguage::default(),
    }
}

pub(in crate::filing) fn next_generation() -> u64 {
    static LAST_GENERATION: AtomicU64 = AtomicU64::new(0);
    LAST_GENERATION
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
}
