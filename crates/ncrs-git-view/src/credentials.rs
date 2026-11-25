//! The login questions of git: which tab a question belongs to, and the
//! dialog's draft. The answer never leaves the app as anything but a
//! [`Secret`], which prints as `***`.

use std::sync::atomic::{AtomicU64, Ordering};

use ncrs_git::askpass::{AskRequest, PromptKind, Secret};

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// The number a tab's interactive jobs carry; the question of a job comes
/// back with it so the app can find the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobToken(pub u64);

impl Default for JobToken {
    fn default() -> Self {
        Self(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed))
    }
}

/// A question git asked and the answer typed so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialAsk {
    pub id: u64,
    pub prompt: String,
    pub kind: PromptKind,
    pub typed: Secret,
}

impl CredentialAsk {
    pub fn of(request: &AskRequest) -> Self {
        Self {
            id: request.id,
            prompt: request.prompt.clone(),
            kind: request.kind,
            typed: Secret::new(""),
        }
    }

    /// What goes back to git for the "yes" or Enter of the dialog.
    pub fn answer(&self) -> Secret {
        match self.kind {
            PromptKind::Confirm => Secret::new("yes"),
            PromptKind::Text | PromptKind::Secret => self.typed.clone(),
        }
    }
}

/// Hands the question to the broker as refused.
pub fn refuse(id: u64) {
    ncrs_git::askpass::answer(id, None);
}

/// A tab that is closed takes its open questions with it.
pub fn cancel_job(token: JobToken) {
    ncrs_git::askpass::cancel_job(token.0);
}
