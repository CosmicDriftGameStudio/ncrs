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
            .map_or(AiWiring::Off, |path| wire_ai(&filing.ai, path));
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
            dismissed_key_problem: None,
            kinds: Arc::default(),
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
