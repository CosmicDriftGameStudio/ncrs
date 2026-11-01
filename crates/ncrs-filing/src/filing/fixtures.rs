//! A module standing in a given phase, for the tests of the app.

use super::*;

#[allow(clippy::expect_used)]
impl FilingModule {
    /// A module standing in `phase`, with nothing behind it: no work, an
    /// in-memory store, a path that is never written.
    pub fn in_phase(phase: Phase) -> Self {
        Self {
            db_path: PathBuf::from("index.db"),
            store: Arc::new(ncrs_index::InMemoryKeyStore::empty()),
            index: Arc::default(),
            locations: Vec::new(),
            phase,
            load: Load::default(),
            generation: 0,
            pick_group: random_group,
            has_configured_locations: false,
            index_encrypted: None,
            key_store_reachable: true,
            protection_note: None,
            dismissed_key_problem: None,
            kinds: Arc::default(),
            ai: AiWiring::Off,
            own: OwnDetails::default(),
        }
    }

    /// Like `in_phase`, with `index` open behind it.
    pub fn in_phase_over_index(phase: Phase, index: Index) -> Self {
        Self {
            index: Arc::new(Mutex::new(Some(index))),
            ..Self::in_phase(phase)
        }
    }

    /// A ready module over `index` whose locations are `configured`, looked up
    /// on disk like the start work does.
    pub fn ready_over_index_with_locations(index: Index, configured: &[FilingLocation]) -> Self {
        Self {
            locations: resolve_locations(configured),
            ..Self::in_phase_over_index(Phase::Ready, index)
        }
    }

    /// The card for `reason` was put off by the user.
    pub fn dismissed_over(mut self, reason: KeyProblem) -> Self {
        self.phase = Phase::Dismissed;
        self.dismissed_key_problem = Some(reason);
        self
    }

    /// Like `in_phase`, as if the open index had the given encryption.
    pub fn in_phase_with_index(phase: Phase, encrypted: bool) -> Self {
        Self {
            index_encrypted: Some(encrypted),
            ..Self::in_phase(phase)
        }
    }

    /// Like `in_phase_with_locations`, with the index at `db_path`: work the
    /// test starts really creates it.
    pub fn in_phase_with_locations_at(phase: Phase, db_path: PathBuf) -> Self {
        Self {
            db_path,
            ..Self::in_phase_with_locations(phase)
        }
    }

    /// Like `in_phase`, wired for the AI of `setting`: the local model is
    /// not downloaded, so it reports that.
    pub fn in_phase_with_ai(phase: Phase, setting: &AiSetting) -> Self {
        Self {
            ai: wire_ai(setting, Path::new("index.db")),
            ..Self::in_phase(phase)
        }
    }

    /// Like `in_phase`, started from a config that listed locations.
    pub fn in_phase_with_locations(phase: Phase) -> Self {
        Self {
            has_configured_locations: true,
            ..Self::in_phase(phase)
        }
    }

    /// The all-zero key: valid to show, opens nothing.
    pub fn fixed_recovery_key() -> ShownRecoveryKey {
        let key = "A".repeat(52).parse::<RecoveryKey>();
        ShownRecoveryKey::new(key.expect("52 times A is a canonical recovery key"))
    }

    /// A made-up key whose 13 blocks all differ: ABCD-EFGH-...-TUVA. Opens nothing.
    pub fn varied_recovery_key() -> ShownRecoveryKey {
        let key = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567BCDEFGHJKLMNPQRSTUVA".parse::<RecoveryKey>();
        ShownRecoveryKey::new(key.expect("a canonical recovery key"))
    }
}
