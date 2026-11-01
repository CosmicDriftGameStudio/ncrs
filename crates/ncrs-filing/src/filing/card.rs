//! What the first-run and the protection cards ask of the module.

use super::*;

impl FilingModule {
    /// The config this module started from listed at least one location.
    pub fn has_configured_locations(&self) -> bool {
        self.has_configured_locations
    }

    /// Only once the open index is known to be unencrypted.
    pub fn index_is_plain(&self) -> bool {
        self.index_encrypted == Some(false)
    }

    /// `None` until an index is open.
    pub fn index_encryption(&self) -> Option<bool> {
        self.index_encrypted
    }

    /// Whether a new module may take this one's place: no create, restore or
    /// replace in flight, and no recovery key on screen that would be lost.
    /// A module dismissed while starting may still run its start job; that job
    /// only opens an existing index, and its result is ignored by generation.
    pub fn can_be_replaced(&self) -> bool {
        !matches!(self.phase, Phase::Starting | Phase::ShowRecoveryKey { .. })
            && !self.phase.is_busy()
    }

    pub fn has_card(&self) -> bool {
        matches!(
            self.phase,
            Phase::ShowRecoveryKey { .. }
                | Phase::AskRecoveryKey { .. }
                | Phase::PlainFileWarning
                | Phase::Failed(_)
        )
    }

    /// Whether "Not now", the cross and Esc may leave the card. The recovery
    /// key is shown once, so leaving its card would lose it; a card that works
    /// cannot be left either.
    pub fn card_can_be_dismissed(&self) -> bool {
        self.has_card() && self.may_dismiss()
    }

    /// The index does not exist yet and the module waits for the user to have
    /// it created.
    pub fn asks_encryption(&self) -> bool {
        matches!(self.phase, Phase::AskEncryption { .. })
    }

    /// The index is being created.
    pub fn is_creating_index(&self) -> bool {
        matches!(self.phase, Phase::AskEncryption { busy: true, .. })
    }

    /// Why the last attempt to create the index failed.
    pub fn index_creation_error(&self) -> Option<&str> {
        match &self.phase {
            Phase::AskEncryption { error, .. } => error.as_deref(),
            _ => None,
        }
    }

    /// Chooses the encryption and starts creating the index. Nothing happens
    /// unless the module asks for it and is not busy.
    pub fn create_index_with(&mut self, encrypted: bool) -> Option<Work> {
        self.update(FilingMessage::ChooseEncrypted(encrypted));
        self.update(FilingMessage::CreateIndex)
    }

    /// No module that already works on configured locations: a new one may
    /// take this one's place with what the first-time setup wrote.
    pub fn can_take_first_setup(&self) -> bool {
        let works_on_locations =
            matches!(self.phase, Phase::Ready) && self.has_configured_locations;
        self.can_be_replaced() && !works_on_locations
    }

    /// The index is open, unencrypted, and the system store can keep a key.
    pub fn can_encrypt(&self) -> bool {
        matches!(self.phase, Phase::Ready)
            && self.index_encrypted == Some(false)
            && self.key_store_reachable
    }

    /// Whether the index files may be removed to begin again: the module is
    /// at rest and has a place for them.
    pub fn can_start_over(&self) -> bool {
        let at_rest = match &self.phase {
            Phase::Ready | Phase::PlainFileWarning | Phase::Failed(_) => true,
            Phase::AskRecoveryKey { busy, .. } => !busy,
            Phase::Dismissed => self.dismissed_key_problem.is_some(),
            _ => false,
        };
        at_rest && self.db_path.parent().is_some() && self.db_path.file_name().is_some()
    }

    /// An encryption or a fresh start is under way.
    pub fn is_changing_protection(&self) -> bool {
        matches!(self.phase, Phase::Encrypting | Phase::StartingOver)
    }

    pub fn protection_note(&self) -> Option<&ProtectionNote> {
        self.protection_note.as_ref()
    }

    /// The key problem the user has to solve: on the card, or left behind
    /// when the card was dismissed.
    pub fn key_problem(&self) -> Option<KeyProblem> {
        match &self.phase {
            Phase::AskRecoveryKey { reason, .. } => Some(*reason),
            Phase::Dismissed => self.dismissed_key_problem,
            _ => None,
        }
    }

    /// The work closes the index itself; the caller stops the pipeline first.
    pub fn begin_encrypting(&mut self) -> Option<Work> {
        if !self.can_encrypt() {
            return None;
        }
        self.protection_note = None;
        self.index_encrypted = None;
        self.phase = Phase::Encrypting;
        Some(self.new_work(Job::Encrypt))
    }

    /// Removes the index and its key; the caller stops the pipeline first.
    pub fn begin_starting_over(&mut self) -> Option<Work> {
        if !self.can_start_over() {
            return None;
        }
        let data_dir = self.db_path.parent()?.to_path_buf();
        self.protection_note = None;
        self.dismissed_key_problem = None;
        self.index_encrypted = None;
        self.phase = Phase::StartingOver;
        Some(self.new_work(Job::StartOver { data_dir }))
    }

    pub(in crate::filing) fn may_dismiss(&self) -> bool {
        let stays = matches!(
            self.phase,
            Phase::Ready | Phase::Dismissed | Phase::ShowRecoveryKey { .. }
        );
        !stays && !self.phase.is_busy()
    }

    /// For the clipboard only.
    pub fn recovery_key_text(&self) -> Option<String> {
        match &self.phase {
            Phase::ShowRecoveryKey { key, .. } => Some(key.display_text()),
            _ => None,
        }
    }
}
