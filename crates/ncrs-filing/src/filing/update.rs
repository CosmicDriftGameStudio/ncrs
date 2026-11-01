//! How the module moves between phases.

use super::*;

impl FilingModule {
    pub fn update(&mut self, message: FilingMessage) -> Option<Work> {
        match message {
            FilingMessage::ChooseEncrypted(choice) => {
                if let Phase::AskEncryption {
                    encrypted,
                    busy: false,
                    ..
                } = &mut self.phase
                {
                    *encrypted = choice;
                }
                None
            }
            FilingMessage::CreateIndex => self.create_index(),
            FilingMessage::CopyRecoveryKey => None,
            FilingMessage::GroupInput(text) => {
                if let Phase::ShowRecoveryKey {
                    typed, mismatch, ..
                } = &mut self.phase
                {
                    *typed = text;
                    *mismatch = false;
                }
                None
            }
            FilingMessage::GroupSubmit => {
                self.submit_group();
                None
            }
            FilingMessage::KeyInput(text) => {
                if let Phase::AskRecoveryKey { typed, error, .. } = &mut self.phase {
                    *typed = text;
                    *error = None;
                }
                None
            }
            FilingMessage::ConfirmReplace(confirmed) => {
                if let Phase::AskRecoveryKey {
                    confirm_replace,
                    error,
                    ..
                } = &mut self.phase
                {
                    *confirm_replace = confirmed;
                    *error = None;
                }
                None
            }
            FilingMessage::KeySubmit => self.submit_key(),
            FilingMessage::ConfirmPlainFile => {
                if matches!(self.phase, Phase::PlainFileWarning) {
                    self.phase = Phase::Ready;
                }
                None
            }
            FilingMessage::Dismiss => {
                if self.may_dismiss() {
                    if let Phase::AskRecoveryKey { reason, .. } = &self.phase {
                        self.dismissed_key_problem = Some(*reason);
                    }
                    self.generation = next_generation();
                    self.phase = Phase::Dismissed;
                }
                None
            }
            FilingMessage::AskKeyAgain => {
                if let (Phase::Dismissed, Some(reason)) = (&self.phase, self.dismissed_key_problem)
                {
                    self.phase = ask_recovery_key(reason);
                }
                None
            }
            FilingMessage::WorkDone {
                generation,
                outcome,
            } => {
                if generation == self.generation {
                    self.finish(outcome);
                }
                None
            }
        }
    }

    fn create_index(&mut self) -> Option<Work> {
        let Phase::AskEncryption {
            encrypted: chosen,
            busy,
            error,
        } = &mut self.phase
        else {
            return None;
        };
        if *busy {
            return None;
        }
        *busy = true;
        *error = None;
        let encrypted = *chosen;
        Some(self.new_work(Job::Create { encrypted }))
    }

    fn submit_group(&mut self) {
        let Phase::ShowRecoveryKey {
            key,
            group,
            typed,
            mismatch,
        } = &mut self.phase
        else {
            return;
        };
        let shown = key.display_text();
        let matches = shown
            .split('-')
            .nth(*group)
            .is_some_and(|expected| typed.as_str().trim().eq_ignore_ascii_case(expected));
        if matches {
            self.phase = Phase::Ready;
        } else {
            *mismatch = true;
        }
    }

    fn submit_key(&mut self) -> Option<Work> {
        let job = {
            let Phase::AskRecoveryKey {
                reason,
                typed,
                confirm_replace,
                busy,
                error,
            } = &mut self.phase
            else {
                return None;
            };
            if *busy {
                return None;
            }
            let Ok(key) = RecoveryKey::from_str(typed.as_str()) else {
                *error = Some(KeyError::InvalidFormat);
                return None;
            };
            let job = match reason {
                KeyProblem::Missing => Job::Restore { key },
                KeyProblem::Mismatch if *confirm_replace => Job::Replace { key },
                KeyProblem::Mismatch => {
                    *error = Some(KeyError::ConfirmFirst);
                    return None;
                }
            };
            *busy = true;
            *error = None;
            job
        };
        Some(self.new_work(job))
    }

    fn finish(&mut self, outcome: WorkOutcome) {
        match outcome {
            WorkOutcome::Started { locations, opened } => {
                if matches!(self.phase, Phase::Starting) {
                    self.locations = locations;
                    self.enter(opened);
                }
            }
            WorkOutcome::Created(result) => self.finish_create(result),
            WorkOutcome::KeyApplied(result) => self.finish_key(result),
            WorkOutcome::Encrypted(report) => self.finish_encrypt(report),
            WorkOutcome::StartedOver(report) => self.finish_start_over(report),
            WorkOutcome::Crashed(reason) => self.finish_crashed(reason),
        }
        // Safe on the UI thread: no pipeline holds the index before `Ready`,
        // and no work is running once its result is here.
        if self.index_encrypted.is_none() {
            self.index_encrypted = lock(&self.index).as_ref().map(Index::is_encrypted);
        }
        self.give_kinds_to_index();
    }

    fn enter(&mut self, opened: OpenOutcome) {
        self.phase = match opened {
            OpenOutcome::NoIndexYet => Phase::AskEncryption {
                encrypted: true,
                busy: false,
                error: None,
            },
            OpenOutcome::Opened => Phase::Ready,
            OpenOutcome::OpenedWithoutKeyStore => {
                self.key_store_reachable = false;
                Phase::Ready
            }
            OpenOutcome::PlainFileWithKeyEntry => Phase::PlainFileWarning,
            OpenOutcome::KeyMissing => ask_recovery_key(KeyProblem::Missing),
            OpenOutcome::KeyMismatch => ask_recovery_key(KeyProblem::Mismatch),
            OpenOutcome::Failed(reason) => Phase::Failed(reason),
        };
    }

    fn finish_create(&mut self, result: Result<Option<ShownRecoveryKey>, String>) {
        if !matches!(self.phase, Phase::AskEncryption { busy: true, .. }) {
            return;
        }
        match result {
            Ok(Some(key)) => {
                self.phase = Phase::ShowRecoveryKey {
                    key,
                    group: (self.pick_group)(GROUP_COUNT) % GROUP_COUNT,
                    typed: SecretText::default(),
                    mismatch: false,
                };
            }
            Ok(None) => self.phase = Phase::Ready,
            Err(reason) => self.stop_waiting(reason),
        }
    }

    fn finish_key(&mut self, result: Result<OpenOutcome, KeyError>) {
        if !matches!(self.phase, Phase::AskRecoveryKey { busy: true, .. }) {
            return;
        }
        match result {
            Ok(opened) => self.enter(opened),
            Err(failure) => {
                if let Phase::AskRecoveryKey { busy, error, .. } = &mut self.phase {
                    *busy = false;
                    *error = Some(failure);
                }
            }
        }
    }

    fn finish_encrypt(&mut self, report: EncryptReport) {
        if !matches!(self.phase, Phase::Encrypting) {
            return;
        }
        match report.result {
            Ok(key) => {
                self.phase = Phase::ShowRecoveryKey {
                    key,
                    group: (self.pick_group)(GROUP_COUNT) % GROUP_COUNT,
                    typed: SecretText::default(),
                    mismatch: false,
                };
            }
            Err(reason) => {
                self.protection_note = Some(ProtectionNote::EncryptFailed(reason));
                self.enter(report.opened);
            }
        }
    }

    fn finish_start_over(&mut self, report: StartOverReport) {
        if !matches!(self.phase, Phase::StartingOver) {
            return;
        }
        match report.result {
            Ok(()) => {
                self.protection_note = report
                    .key_entry_warning
                    .map(ProtectionNote::KeyEntryNotRemoved);
            }
            Err(reason) => {
                self.protection_note = Some(ProtectionNote::StartOverFailed(reason));
            }
        }
        self.enter(report.opened);
    }

    fn finish_crashed(&mut self, reason: String) {
        if matches!(
            self.phase,
            Phase::Starting | Phase::Encrypting | Phase::StartingOver
        ) {
            self.phase = Phase::Failed(reason);
        } else {
            self.stop_waiting(reason);
        }
    }

    /// Back from a busy card, with the reason on it.
    fn stop_waiting(&mut self, reason: String) {
        match &mut self.phase {
            Phase::AskEncryption { busy, error, .. } => {
                *busy = false;
                *error = Some(reason);
            }
            Phase::AskRecoveryKey { busy, error, .. } => {
                *busy = false;
                *error = Some(KeyError::Other(reason));
            }
            Phase::Starting
            | Phase::Encrypting
            | Phase::StartingOver
            | Phase::ShowRecoveryKey { .. }
            | Phase::PlainFileWarning
            | Phase::Ready
            | Phase::Failed(_)
            | Phase::Dismissed => {}
        }
    }
}

pub(in crate::filing) fn ask_recovery_key(reason: KeyProblem) -> Phase {
    Phase::AskRecoveryKey {
        reason,
        typed: SecretText::default(),
        confirm_replace: false,
        busy: false,
        error: None,
    }
}

/// Unpredictable per start, without a random-number dependency: the standard
/// library seeds every `RandomState` from the system.
pub(in crate::filing) fn random_group(count: usize) -> usize {
    let hash = RandomState::new().build_hasher().finish();
    let groups = u64::try_from(count.max(1)).unwrap_or(1);
    usize::try_from(hash % groups).unwrap_or(0)
}
