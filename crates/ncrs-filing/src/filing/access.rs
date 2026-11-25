//! The handles the pages get on the index, and the locations.

use super::*;

impl FilingModule {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn load(&self) -> Load {
        self.load
    }

    /// Remembers the choice for the page and hands the local model its new
    /// threads and priority; the running pipeline is told separately.
    pub fn set_load(&mut self, load: Load) {
        self.load = load;
        #[cfg(feature = "local-ai")]
        if let AiWiring::Local(model) = &self.ai {
            model.set_extraction(ai::extraction_threads(load, None), load.thread_priority());
        }
    }

    /// The module is busy with its start (opening, questions, conversion),
    /// not failed or dismissed.
    pub fn is_starting(&self) -> bool {
        !matches!(
            self.phase,
            Phase::Ready | Phase::FromNewerVersion | Phase::Failed(_) | Phase::Dismissed
        )
    }

    /// The handle of the Hermes chat: it reads folders and files into any
    /// folder, so the module being `Ready` is enough.
    pub fn chat_access(&self, trash: Arc<dyn ncrs_fs::Trash>) -> Option<ChatAccess> {
        if !matches!(self.phase, Phase::Ready) {
            return None;
        }
        let chain = chat_text_chain(&self.chat_text_chain, &self.db_path);
        Some(
            ChatAccess::new(Arc::clone(&self.index), Arc::clone(chain), trash)
                .with_own(self.own.clone())
                .with_load(self.load)
                .with_snapshots(self.snapshots.clone())
                .with_index_work(self.index_work.clone()),
        )
    }

    /// The handle of sorting, with the sources and targets as saved last; one
    /// copy per job.
    pub fn sorting_access(&self, trash: Arc<dyn ncrs_fs::Trash>) -> Option<SortingAccess> {
        if !matches!(self.phase, Phase::Ready) {
            return None;
        }
        Some(SortingAccess {
            slot: Arc::clone(&self.index),
            sources: self.sources.clone(),
            targets: self.targets.clone(),
            text_chain: Arc::clone(chat_text_chain(&self.chat_text_chain, &self.db_path)),
            own: self.own.clone(),
            load: self.load,
            trash,
            snapshots: self.snapshots.clone(),
            index_work: self.index_work.clone(),
        })
    }

    /// Takes the document kinds of a (re)loaded config; an open index uses
    /// them from now on, a pipeline already running keeps the ones it started with.
    pub fn set_kinds(&mut self, kinds: Arc<KindCatalog>) {
        self.kinds = kinds;
        self.snapshots.forget();
        self.give_kinds_to_index();
    }

    /// Skipped while a pipeline or a run holds the index; the next call
    /// hands them over.
    pub(in crate::filing) fn give_kinds_to_index(&self) {
        let Some(mut guard) = try_lock(&self.index) else {
            return;
        };
        if let Some(index) = guard.as_mut() {
            if !Arc::ptr_eq(index.kinds(), &self.kinds) {
                index.set_kinds(Arc::clone(&self.kinds));
            }
        }
    }

    /// The language `{kind}` folders are worded in. The running pipeline
    /// keeps the one it started with.
    pub fn set_kind_language(&mut self, kind_language: KindLanguage) {
        self.own.kind_language = kind_language;
        self.snapshots.forget();
    }

    /// Excluding and including paths only changes the index, so the module
    /// being `Ready` is enough.
    pub fn path_access(&self) -> Option<PathAccess> {
        matches!(self.phase, Phase::Ready).then(|| PathAccess::new(Arc::clone(&self.index)))
    }

    /// The handle of the saved queries: they only read the index, so the
    /// module being `Ready` is enough.
    pub fn query_access(&self) -> Option<QueryAccess> {
        matches!(self.phase, Phase::Ready).then(|| QueryAccess::new(Arc::clone(&self.index)))
    }

    /// The tags page reads and changes the index only, so the module being
    /// `Ready` is enough.
    pub fn tags_access(&self) -> Option<TagsAccess> {
        matches!(self.phase, Phase::Ready).then(|| TagsAccess::new(Arc::clone(&self.index)))
    }

    /// The rules page reads and changes the index only, so the module being
    /// `Ready` is enough.
    pub fn rules_access(&self) -> Option<RulesAccess> {
        matches!(self.phase, Phase::Ready).then(|| {
            RulesAccess::new(Arc::clone(&self.index)).with_snapshots(self.snapshots.clone())
        })
    }

    /// Takes the saved sources and targets, and copy or move for the locations
    /// this module already has, so the next job works as saved. A saved
    /// location the module does not have is left to the next start.
    pub fn adopt_sorting(&mut self, saved: &Filing) {
        self.sources.clone_from(&saved.sources);
        self.targets.clone_from(&saved.targets);
        for entry in &mut self.locations {
            if let Some(chosen) = saved
                .locations
                .iter()
                .find(|candidate| candidate.path == entry.configured.path)
            {
                entry.configured.transfer = chosen.transfer;
            }
        }
    }

    /// What `resolve_locations` is to look up for the saved locations the
    /// module did not start with: the ones it has plus the new ones, so the
    /// overlap check sees all. `None` when nothing is new or the module is
    /// not ready.
    pub fn configured_with_new(&self, saved: &[FilingLocation]) -> Option<Vec<FilingLocation>> {
        if !self.chat_ready() {
            return None;
        }
        let mut all: Vec<FilingLocation> = self
            .locations
            .iter()
            .map(|entry| entry.configured.clone())
            .collect();
        let known = all.len();
        for entry in saved {
            if !all.iter().any(|have| have.path == entry.path) {
                all.push(entry.clone());
            }
        }
        (all.len() > known).then_some(all)
    }

    /// Takes the entries of `resolve_locations` for locations the module does
    /// not have; the others keep their state. Returns the ones the pipeline
    /// can watch.
    pub fn take_resolved_locations(&mut self, resolved: Vec<ResolvedLocation>) -> Vec<Location> {
        let mut usable = Vec::new();
        for entry in resolved {
            if self
                .locations
                .iter()
                .any(|have| have.configured.path == entry.configured.path)
            {
                continue;
            }
            if let LocationStatus::Ready(location) = &entry.status {
                usable.push(location.clone());
            }
            self.locations.push(entry);
        }
        usable
    }

    pub fn locations(&self) -> &[ResolvedLocation] {
        &self.locations
    }

    /// The folders the intake may read.
    pub fn reachable_locations(&self) -> impl Iterator<Item = &Location> {
        self.locations
            .iter()
            .filter_map(|entry| match &entry.status {
                LocationStatus::Ready(location) => Some(location),
                LocationStatus::NotReachable { .. } | LocationStatus::Overlaps { .. } => None,
            })
    }
}

/// The chain the chat reads files with. Loading the PDF library is slow the
/// first time, so a started module builds it on a thread of its own.
fn chat_text_chain<'a>(cell: &'a OnceLock<Arc<TextChain>>, db_path: &Path) -> &'a Arc<TextChain> {
    cell.get_or_init(|| {
        let (chain, _library_missing) = shipped_text_chain(
            ocr_models_dir(db_path).unwrap_or_else(|| PathBuf::from(OCR_MODELS_FOLDER)),
        );
        Arc::new(chain)
    })
}

impl FilingModule {
    /// The chat can get its handle: cheap, builds nothing.
    pub fn chat_ready(&self) -> bool {
        matches!(self.phase, Phase::Ready)
    }

    pub(in crate::filing) fn warm_chat_text_chain(&self) {
        let cell = Arc::clone(&self.chat_text_chain);
        let db_path = self.db_path.clone();
        std::thread::spawn(move || {
            chat_text_chain(&cell, &db_path);
        });
    }
}
