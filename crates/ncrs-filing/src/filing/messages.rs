//! What the filing module is sent and what its work reports back.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum FilingMessage {
    ChooseEncrypted(bool),
    CreateIndex,
    CopyRecoveryKey,
    GroupInput(SecretText),
    GroupSubmit,
    KeyInput(SecretText),
    ConfirmReplace(bool),
    KeySubmit,
    ConfirmPlainFile,
    Dismiss,
    /// Brings back the key card that "Not now" left.
    AskKeyAgain,
    WorkDone {
        generation: u64,
        outcome: WorkOutcome,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkOutcome {
    Started {
        locations: Vec<ResolvedLocation>,
        opened: OpenOutcome,
        /// What the check of the opened file found wrong with it.
        damage: Option<String>,
    },
    /// `Some` holds the recovery key of a new encrypted index.
    Created(Result<Option<ShownRecoveryKey>, String>),
    KeyApplied(Result<OpenOutcome, KeyError>),
    Encrypted(EncryptReport),
    StartedOver(StartOverReport),
    /// The interrupted runs of a plain index were settled after the user accepted it.
    Settled,
    /// The blocking thread did not finish (it panicked).
    Crashed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenOutcome {
    NoIndexYet,
    Opened,
    /// An unencrypted index opened, but the credential store cannot be reached.
    OpenedWithoutKeyStore,
    PlainFileWithKeyEntry,
    KeyMissing,
    KeyMismatch,
    /// The file was written by a newer ncrs; this one leaves it alone.
    FromNewerVersion,
    Failed(String),
}
