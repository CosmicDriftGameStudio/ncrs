//! The handles the pages get on the index, and the locations.

use super::*;

impl FilingModule {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn load(&self) -> Load {
        self.load
    }

    /// Only remembers the choice for the page; the running pipeline is told
    /// separately.
    pub fn set_load(&mut self, load: Load) {
        self.load = load;
    }

    /// The handle of the rule suggestions view: only when the module is
    /// `Ready`, and over the archive locations that can be reached.
    pub fn suggestion_access(&self) -> Option<SuggestionAccess> {
        self.archive_handle(SuggestionAccess::new)
    }

    /// The handle of the reorganisation plan view, on the same terms.
    pub fn plan_access(&self) -> Option<PlanAccess> {
        self.archive_handle(PlanAccess::new)
            .map(|access| access.with_kind_language(self.own.kind_language))
    }

    /// Takes the document kinds of a (re)loaded config; an open index uses
    /// them from now on, a pipeline already running keeps the ones it started with.
    pub fn set_kinds(&mut self, kinds: Arc<KindCatalog>) {
        self.kinds = kinds;
        self.give_kinds_to_index();
    }

    pub(in crate::filing) fn give_kinds_to_index(&self) {
        if let Some(index) = lock(&self.index).as_mut() {
            if !Arc::ptr_eq(index.kinds(), &self.kinds) {
                index.set_kinds(Arc::clone(&self.kinds));
            }
        }
    }

    /// The language `{kind}` folders are worded in. The running pipeline
    /// keeps the one it started with.
    pub fn set_kind_language(&mut self, kind_language: KindLanguage) {
        self.own.kind_language = kind_language;
    }

    /// The handle of the filing job: only when the module is `Ready` and the
    /// archive marked as the target can be reached.
    pub fn filing_access(&self) -> Option<FilingAccess> {
        let root = self.target_root()?;
        Some(
            FilingAccess::new(Arc::clone(&self.index), root)
                .with_kind_language(self.own.kind_language),
        )
    }

    /// The handle of the review list, on the same terms as the filing job: it
    /// files into the same target.
    pub fn review_access(&self) -> Option<ReviewAccess> {
        let root = self.target_root()?;
        Some(
            ReviewAccess::new(Arc::clone(&self.index), root)
                .with_kind_language(self.own.kind_language),
        )
    }

    /// The handle of re-planning and refiling: the inbox locations that can be
    /// reached bound what it may touch, the archive is the filing target.
    pub fn replan_access(&self) -> Option<ReplanAccess> {
        let root = self.target_root()?;
        let inbox_roots = self
            .reachable_locations()
            .filter(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.clone())
            .collect();
        Some(
            ReplanAccess::new(Arc::clone(&self.index), inbox_roots, root)
                .with_kind_language(self.own.kind_language),
        )
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
    /// `Ready` is enough; the try reads the documents of the locations that
    /// can be reached.
    pub fn rules_access(&self) -> Option<RulesAccess> {
        matches!(self.phase, Phase::Ready).then(|| {
            RulesAccess::new(
                Arc::clone(&self.index),
                self.reachable_locations().cloned().collect(),
            )
            .with_kind_language(self.own.kind_language)
        })
    }

    fn target_root(&self) -> Option<PathBuf> {
        if !matches!(self.phase, Phase::Ready) {
            return None;
        }
        self.locations
            .iter()
            .filter(|entry| entry.configured.is_target)
            .find_map(|entry| match &entry.status {
                LocationStatus::Ready(location) => Some(location.path.clone()),
                LocationStatus::NotReachable { .. } | LocationStatus::Overlaps { .. } => None,
            })
    }

    fn archive_handle<T>(&self, make: fn(IndexSlot, Vec<PathBuf>) -> T) -> Option<T> {
        match self.phase {
            Phase::Ready => Some(make(
                Arc::clone(&self.index),
                self.reachable_locations()
                    .filter(|location| location.mode == LocationMode::Archive)
                    .map(|location| location.path.clone())
                    .collect(),
            )),
            _ => None,
        }
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
