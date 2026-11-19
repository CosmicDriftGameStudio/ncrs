//! Starting the filing module from the config.

use super::*;

impl FilingModule {
    /// `None` when the module is off: nothing is opened, created or resolved.
    pub fn start(
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: SharedStore,
    ) -> Option<(Self, Option<Work>)> {
        Self::start_picking(filing, db_path, store, random_group)
    }

    /// Whether the settling that follows the confirmation of a plain file is
    /// still running.
    pub fn is_settling(&self) -> bool {
        self.settling
    }

    /// Why the interrupted runs of the open index could not all be settled.
    pub fn settle_problem(&self) -> Option<String> {
        self.settle_problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(in crate::filing) fn start_picking(
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: SharedStore,
        pick_group: fn(usize) -> usize,
    ) -> Option<(Self, Option<Work>)> {
        if !filing.enabled {
            return None;
        }
        let ai = db_path
            .as_deref()
            .map_or(AiWiring::Off, |path| wire_ai(&filing.ai, filing.load, path));
        let mut module = Self {
            ai,
            own: own_details(filing),
            db_path: db_path.clone().unwrap_or_default(),
            store,
            index: Arc::default(),
            locations: Vec::new(),
            phase: Phase::Starting,
            load: filing.load,
            generation: 0,
            pick_group,
            has_configured_locations: !filing.locations.is_empty(),
            index_encrypted: None,
            key_store_reachable: true,
            protection_note: None,
            after_recovery_key: None,
            dismissed_key_problem: None,
            kinds: Arc::default(),
            chat_text_chain: OnceLock::new(),
            pipeline_text_chain: Mutex::new(None),
            settle_problem: SettleNote::default(),
            settling: false,
        };
        if db_path.is_none() {
            module.phase = Phase::Failed(NO_DATA_DIRECTORY.to_owned());
            return Some((module, None));
        }
        let work = module.new_work(Job::Start {
            locations: filing.locations.clone(),
        });
        Some((module, Some(work)))
    }
}
