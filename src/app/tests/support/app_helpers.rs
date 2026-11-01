use super::fixed::*;
use crate::app::*;

impl App {
    /// Fixed panels with the connect dialog up.
    pub fn with_connect_dialog(dialog: ncrs_remote::dialog::ConnectDialog) -> Self {
        let mut app = Self::with_fixed_panels();
        app.remote_ui.dialog = Some(dialog);
        app
    }

    /// Fixed panels with the settings tab open on `feature`, with `filing` as
    /// the config says.
    pub fn with_settings_open(feature: crate::messages::SettingsFeature, filing: Filing) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.filing_config = filing;
        let mut state = settings_ui::SettingsState::new(false, Ok(Box::default()), false);
        state.feature = feature;
        app.settings.page = Some(state);
        app
    }

    /// Fixed panels with the settings tab open on `feature`, showing `file`
    /// as the config read when it opened. A config that loads is in effect.
    pub fn with_settings_file(
        feature: crate::messages::SettingsFeature,
        file: Result<Config, ConfigError>,
    ) -> Self {
        let filing = file
            .as_ref()
            .map(|config| config.filing.clone())
            .unwrap_or_default();
        let mut app = Self::with_settings_open(feature, filing);
        if let Ok(config) = &file {
            app.open_programs = config.open.clone();
            app.saved_servers = config.servers.clone();
            app.apply_look(config.clone());
        }
        if let Some(state) = app.settings.page.as_mut() {
            state.file = file.map(Box::new);
        }
        app
    }

    /// The status page open, with the AI line in the state of `ai`.
    pub fn with_filing_status_ai(ai: crate::filing::AiStatus) -> Self {
        let mut app = Self::with_filing_status_page(
            ncrs_pipeline::Snapshot::off(),
            std::time::SystemTime::UNIX_EPOCH,
        );
        app.hermes.filing_status.set_ai(ai);
        app
    }

    /// Fixed panels with the theme download card up, in the state of `card`.
    pub fn with_theme_fetch_card(card: ThemeFetch) -> Self {
        let mut app = Self::with_fixed_panels();
        app.settings.theme_fetch = Some(card);
        app
    }

    pub fn rules_for_test(&self) -> Option<&RulesPage> {
        self.hermes.rules.as_ref()
    }

    /// Fixed panels with the rules page up, in the state of `view`.
    pub fn with_rules(view: RulesPage) -> Self {
        let mut app = Self::with_fixed_panels();
        app.set_idle_hermes_status();
        app.hermes.rules = Some(view);
        app
    }

    pub fn rule_suggestions_for_test(&self) -> Option<&RuleSuggestions> {
        self.hermes.rule_suggestions.as_ref()
    }

    /// Fixed panels with the rule suggestions view up, in the state of `view`.
    pub fn with_rule_suggestions(view: RuleSuggestions) -> Self {
        let mut app = Self::with_fixed_panels();
        app.set_idle_hermes_status();
        app.hermes.rule_suggestions = Some(view);
        app
    }

    pub fn review_list_for_test(&self) -> Option<&ReviewList> {
        self.hermes.review_list.as_ref()
    }

    pub fn review_list_generation_for_test(&self) -> u64 {
        self.hermes.review_list_generation
    }

    /// Fixed panels with the review list up, in the state of `view`.
    pub fn with_review_list(view: ReviewList) -> Self {
        let mut app = Self::with_fixed_panels();
        app.set_idle_hermes_status();
        app.hermes.review_list = Some(view);
        app
    }

    /// A ready, idle Hermes module, so the status line of the filing pages
    /// shows its sentence.
    fn set_idle_hermes_status(&mut self) {
        let mut idle = ncrs_pipeline::Snapshot::off();
        idle.state = ncrs_pipeline::RunnerState::Idle;
        self.hermes
            .filing_status
            .set_snapshot(idle, self.wall_clock.now());
    }

    /// The counts of Hermes as the index last gave them.
    pub fn with_hermes_counts(mut self, counts: ncrs_index::HermesCounts) -> Self {
        self.hermes.filing_status.counts_arrived(Ok(counts));
        self
    }

    /// The PDF library could not be loaded; `searched` is where it was looked for.
    pub fn with_pdf_library_missing(mut self, searched: PathBuf) -> Self {
        self.hermes.pdf_library_missing = Some(searched);
        self
    }

    /// Puts the arrow keys of the Hermes card on its sidebar or its page.
    pub fn with_hermes_focus(mut self, focus: HermesFocus) -> Self {
        self.hermes_focus = focus;
        self
    }

    pub fn saved_queries_generation_for_test(&self) -> u64 {
        self.hermes.saved_queries_generation
    }

    /// The search view up over `filters`, which `hits_below` builds from the
    /// folder of the search, as if the filing had an index to ask.
    pub fn with_search_filters(
        query: &str,
        hits_below: impl FnOnce(&Path) -> SearchFilters,
    ) -> Self {
        let mut app = Self::with_fixed_panels();
        // The home the README screenshots use, so the search shows the same paths.
        app.panels.left.path = PathBuf::from("/Users/alex/Documents");
        drop(app.update(Message::OpenSearch));
        let root = app.panel(PanelSide::Left).path.clone();
        app.refresh_search_rows();
        let rows = app.search.rows;
        if let Some(view) = app.search.view.as_mut() {
            view.query = query.to_owned();
            view.filters = Some(hits_below(&root));
            view.mirror_index(rows);
        }
        app
    }

    pub fn search_rows_for_test(&self) -> usize {
        self.search.rows
    }

    pub fn search_for_test_mut(&mut self) -> Option<&mut SearchView> {
        self.search.view.as_mut()
    }

    pub fn tags_for_test(&self) -> Option<&TagsPage> {
        self.hermes.tags.as_ref()
    }

    pub fn tags_generation_for_test(&self) -> u64 {
        self.hermes.tags_generation
    }

    pub fn tag_exceptions_for_test(&self) -> &TagExceptions {
        &self.tag_exceptions
    }

    /// Fixed panels with the tags page up, in the state of `page`.
    pub fn with_tags(page: TagsPage) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.tags = Some(page);
        app
    }

    pub fn saved_queries_for_test(&self) -> Option<&SavedQueries> {
        self.hermes.saved_queries.as_ref()
    }

    /// Fixed panels with the saved queries page up, in the state of `view`.
    pub fn with_saved_queries(view: SavedQueries) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.saved_queries = Some(view);
        app
    }

    pub fn reorganisation_for_test(&self) -> Option<&ReorganisationView> {
        self.hermes.reorganisation.as_ref()
    }

    /// Fixed panels with the reorganisation plan view up, in the state of `view`.
    pub fn with_reorganisation_plan(view: ReorganisationView) -> Self {
        let mut app = Self::with_fixed_panels();
        app.set_idle_hermes_status();
        app.hermes.reorganisation = Some(view);
        app
    }

    /// A Hermes module that is switched on but has no index yet, so its pages
    /// open and complain instead of reading anything.
    pub fn with_starting_hermes_for_test(mut self) -> Self {
        self.hermes.filing = Some(FilingModule::in_phase(Phase::Starting));
        self
    }

    /// A ready Hermes module without an index, so Quick Open asks for documents.
    pub fn with_ready_hermes_for_test(mut self) -> Self {
        self.hermes.filing = Some(FilingModule::in_phase_with_index(Phase::Ready, false));
        self
    }

    /// Opens Quick Open, ranks `files` for `query` and types the text, as the
    /// field would. Hands the answer to the full-text search back to `update`.
    pub fn type_into_quick_open_for_test(&mut self, query: &str) {
        drop(
            self.update(Message::QuickOpen(QuickOpenMessage::QueryChanged(
                query.to_owned(),
            ))),
        );
        self.rank_quick_open_for_test();
    }

    /// Finishes the name ranking the way its background task would.
    fn rank_quick_open_for_test(&mut self) {
        if let Some(quick_open) = self.quick_open.as_mut() {
            quick_open.rank_now();
        }
    }

    pub fn deliver_documents_for_test(
        &mut self,
        result: Result<Vec<crate::filing::FoundDocument>, String>,
    ) {
        let Some(quick_open) = self.quick_open.as_ref() else {
            return;
        };
        let message = QuickOpenMessage::DocumentsFound {
            walk_generation: quick_open.walk_generation(),
            generation: quick_open.documents_generation(),
            result,
        };
        drop(self.update(Message::QuickOpen(message)));
    }

    pub fn quick_open_for_test(&self) -> Option<&QuickOpen> {
        self.quick_open.as_ref()
    }

    /// Opens Quick Open as Cmd/Ctrl+P does and fills it as a finished walk
    /// over `files` (relative to the active panel's directory) would, ranked
    /// for `query` at once.
    pub fn open_quick_open_for_test(&mut self, files: &[&str], query: &str) {
        use crate::quick_open::WalkEvent;
        drop(self.update(Message::QuickOpen(QuickOpenMessage::Open)));
        let Some(quick_open) = self.quick_open.as_mut() else {
            return;
        };
        let mut events: Vec<WalkEvent> = files
            .iter()
            .map(|file| WalkEvent::File {
                path: quick_open.root().join(file),
            })
            .collect();
        events.push(WalkEvent::Finished { truncated: false });
        quick_open.apply_events(events);
        if let Some(ranking) = quick_open.set_query(query.to_string()) {
            if let QuickOpenMessage::Ranked {
                walk_generation,
                generation,
                hits,
                chunks_scored,
            } = ranking.run()
            {
                quick_open.apply_ranked(walk_generation, generation, hits, chunks_scored);
            }
        }
    }

    pub fn panel_mut_for_test(&mut self, side: PanelSide) -> &mut PanelState {
        self.panel_mut(side)
    }

    pub fn set_visible_rows_for_test(&mut self, rows: usize) {
        self.visible_rows = rows;
    }

    /// Whether the job queue is empty.
    ///
    /// Read by the end-to-end tests, which cannot reach the private field.
    pub fn job_is_idle(&self) -> bool {
        !self.jobs.is_busy()
    }

    /// Whether F3 is unpacking a file out of an archive.
    ///
    /// Read by the end-to-end tests, which cannot reach the private field.
    pub fn is_extracting_for_test(&self) -> bool {
        self.ops.extracting.is_some()
    }

    /// Whether a transfer is waiting on the overwrite dialog.
    ///
    /// Read by the end-to-end tests, which cannot reach the private field.
    pub fn conflict_is_pending(&self) -> bool {
        self.ops.conflict_is_open()
    }

    /// The originals the open trash view lists, newest first.
    pub fn trash_view_originals(&self) -> Option<Vec<PathBuf>> {
        self.ops.trash_view.as_ref().map(|view| {
            view.items
                .iter()
                .map(|item| item.original.clone())
                .collect()
        })
    }

    /// The trash view is up and its panel is the active one.
    pub fn trash_view_has_keys(&self) -> bool {
        self.trash_has_keys()
    }

    /// Whether the restore is done and its dialog gone.
    pub fn restore_is_idle(&self) -> bool {
        self.ops.restoring.is_none() && self.ops.restore_conflict.is_none()
    }

    /// Tags the row at `index`, addressed by position the way a keypress is.
    pub fn toggle_tag_for_test(&mut self, index: usize) {
        let Some(path) = self.panels.left.entries.get(index).map(|e| e.path.clone()) else {
            return;
        };
        self.panels.left.selection.toggle(&path);
    }

    pub fn tag_all_for_test(&mut self) {
        let mut selection = crate::selection::SelectionSet::new();
        selection.tag_all(&self.panels.left.entries);
        self.panels.left.selection = selection;
    }

    pub fn left_panel_selection_tagged_for_test(&self, index: usize) -> bool {
        self.panels
            .left
            .entries
            .get(index)
            .is_some_and(|e| self.panels.left.selection.is_tagged(&e.path))
    }

    pub fn move_selection_for_test(&mut self, delta: isize) {
        let rows = self.visible_rows;
        self.panels.left.move_selection(delta, rows);
    }

    /// Whether the delete confirmation is up.
    pub fn delete_dialog_is_open(&self) -> bool {
        self.ops.delete_dialog.is_some()
    }

    /// Replaces the clock type-ahead reads.
    pub fn path_field_for_test(&self) -> Option<&str> {
        self.panels.path_field.as_deref()
    }

    pub fn command_line_for_test(&self) -> &CommandLine {
        &self.command_line
    }

    /// A config warning in the status bar, for the snapshot tests.
    pub fn with_config_warning(mut self, text: &str) -> Self {
        self.config_error = Some(text.to_owned());
        self
    }

    /// The status bar's notice, for the snapshot tests.
    pub fn notice_for_test(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn with_clock(
        mut self,
        clock: impl Fn() -> std::time::Instant + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Replaces the launcher, so a test never starts a real program.
    pub fn with_launcher(mut self, launcher: Arc<dyn fs::Launcher>) -> Self {
        self.launcher = launcher;
        self
    }

    /// Opens the folder F3 unpacks archive files into at `root`, so a test
    /// unpacks into its own.
    #[cfg(test)]
    pub fn with_view_copies_in(mut self, root: &Path) -> Self {
        self.ops.view_copies = fs::archive::ViewCopies::open(root).ok();
        self
    }

    /// Puts connections of the test's own in: no test reaches a network.
    pub fn with_remote(mut self, pool: Arc<Pool>) -> Self {
        self.remote = pool;
        self
    }

    /// Puts a password store of the test's own in; none is the user's keychain.
    pub fn with_passwords(mut self, passwords: Arc<dyn PasswordStore>) -> Self {
        self.passwords = passwords;
        self
    }

    /// Replaces the drive list, so a test offers its own directories.
    pub fn with_volumes(
        mut self,
        list: impl Fn() -> Vec<fs::Volume> + Send + Sync + 'static,
    ) -> Self {
        self.list_volumes = Arc::new(list);
        self
    }

    /// Whether the drive menu is up.
    pub fn volume_menu_is_open(&self) -> bool {
        self.panels.volume_menu.is_some()
    }

    /// The highlighted row of the open drive menu.
    pub fn volume_menu_selected(&self) -> Option<usize> {
        self.panels.volume_menu.as_ref().map(|menu| menu.selected)
    }

    /// Puts the drive menu in front of the user with a fixed list, for the
    /// snapshot tests.
    pub fn open_volume_menu_for_test(&mut self, side: PanelSide, volumes: Vec<fs::Volume>) {
        let current = self.panel(side).path.clone();
        self.panels.volume_menu = Some(VolumeMenu::new(side, volumes, &current));
    }

    /// `open_volume_menu_for_test` with favourite rows, each with the
    /// `present` flag the menu would have read from the disk.
    pub fn open_volume_menu_with_favourites_for_test(
        &mut self,
        side: PanelSide,
        volumes: Vec<fs::Volume>,
        favourites: &[(&str, bool)],
        selected: usize,
    ) {
        let current = self.panel(side).path.clone();
        let rows = favourites
            .iter()
            .map(|(path, present)| FavouriteRow {
                path: PathBuf::from(path),
                present: *present,
            })
            .collect();
        let mut menu = VolumeMenu::new(side, volumes, &current).with_favourites(rows);
        menu.selected = selected;
        self.panels.volume_menu = Some(menu);
    }

    /// Whether the context menu is up.
    pub fn context_menu_is_open(&self) -> bool {
        self.panels.context_menu.is_some()
    }

    /// The entry Enter would run in the open context menu.
    pub fn context_menu_selected_action(&self) -> Option<ContextAction> {
        self.panels
            .context_menu
            .as_ref()
            .and_then(ContextMenu::selected_action)
    }

    pub fn context_menu_for_test(&self) -> Option<&ContextMenu> {
        self.panels.context_menu.as_ref()
    }

    /// Where the pointer is for the next right click, as the raw event
    /// subscription would have recorded it.
    pub fn set_pointer_for_test(&self, position: Point) {
        self.input.lock().pointer = position;
    }

    pub fn set_modifiers_for_test(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn hermes_overview_is_open_for_test(&self) -> bool {
        self.hermes.overview.is_some()
    }

    pub fn active_panel_side_for_test(&self) -> PanelSide {
        self.panels.active
    }

    /// Replaces the trash, so a test never fills the developer's real one.
    pub fn with_trash(mut self, trash: Arc<dyn fs::Trash>) -> Self {
        self.trash = trash;
        self
    }

    /// Starts a transfer of what F5 would offer, to the other panel and asking
    /// per conflict, without the confirmation. For the tests of what happens
    /// once a transfer runs, which have no real folders to confirm against.
    pub fn start_transfer(&mut self, kind: TransferKind) -> Task<Message> {
        let sources = self.action_sources();
        if sources.is_empty() {
            return Task::none();
        }
        let target = self.panel(self.source_side().other()).path.clone();
        self.with_ops(|ops, ctx| {
            ncrs_ops::begin_transfer(ops, kind, sources, target, Policy::Ask, ctx)
        })
    }

    /// Puts the delete dialog in front of the user for `count` fixed entries,
    /// for the snapshot tests.
    pub fn open_delete_dialog_for_test(&mut self, count: usize, permanent: bool) {
        self.dialog_focus = delete_dialog_view::default_focus(permanent);
        self.ops.delete_dialog = Some(Deletion {
            sources: (0..count)
                .map(|i| PathBuf::from("/left").join(format!("entry-{}.txt", i + 1)))
                .collect(),
            permanent,
            side: PanelSide::Left,
        });
    }

    /// Puts the copy confirmation in front of the user for `count` fixed
    /// entries with a finished scan and three taken names, for the snapshot
    /// tests.
    pub fn open_transfer_dialog_for_test(&mut self, count: usize) {
        let sources = (0..count)
            .map(|i| PathBuf::from("/home/test/links").join(format!("entry-{}.txt", i + 1)))
            .collect();
        let mut dialog = TransferDialog::new(
            1,
            TransferKind::Copy,
            sources,
            self.panels.right.path.clone(),
        );
        dialog.set_scan(Some(transfer_dialog::Scan {
            files: count * 4,
            folders: count.div_ceil(2),
            bytes: 48_300_000,
            unreadable: 0,
        }));
        dialog.set_conflicts(dialog.edit(), 3);
        self.ops.transfer_dialog = Some(dialog);
    }

    /// The search view with four fixed hits for "Telekom", so the buttons that
    /// open them as a list are enabled, for the snapshot tests.
    pub fn with_fixed_search_hits() -> Self {
        let mut app = Self::with_fixed_panels();
        app.panels.left.path = fixed_hit_scope();
        app.show_search_results_for_test("Telekom", fixed_hit_search_results());
        app
    }

    /// The left panel as a hit list of the four fixed Telekom files, for the
    /// snapshot tests. Nothing is read from disk.
    pub fn with_fixed_hit_list() -> Self {
        let mut app = Self::with_fixed_panels();
        let rows = app.visible_rows;
        let scope = fixed_hit_scope();
        let panel = app.panel_mut(PanelSide::Left);
        let return_to = panel.path.clone();
        panel.begin_hit_list(HitList {
            query: "Telekom".to_owned(),
            scope,
            return_to,
        });
        panel.apply_hit_entries(fixed_hit_entries(), rows);
        app
    }

    /// The hit list with its context menu open on the second row.
    pub fn with_fixed_hit_list_menu() -> Self {
        let mut app = Self::with_fixed_hit_list();
        app.open_context_menu(PanelSide::Left, 1, Some(Point::new(260.0, 120.0)));
        app
    }

    /// The search results with the context menu open on the third row.
    pub fn with_fixed_search_hits_menu() -> Self {
        let mut app = Self::with_fixed_search_hits();
        app.set_pointer_for_test(Point::new(260.0, 380.0));
        drop(app.update(Message::Search(SearchMessage::ContextAt(2))));
        app
    }

    /// The pack prompt for the two `invoice.pdf` of the fixed hit list.
    pub fn with_fixed_hit_list_pack() -> Self {
        let mut app = Self::with_fixed_hit_list();
        let scope = fixed_hit_scope();
        let panel = app.panel_mut(PanelSide::Left);
        for year in ["2024", "2025"] {
            panel
                .selection
                .toggle(&scope.join(format!("Telekom/{year}/invoice.pdf")));
        }
        drop(app.open_pack_prompt());
        app
    }

    /// Puts the left panel inside a fixed archive and asks for a move there,
    /// so the notice that archives are read-only shows, for the snapshot tests.
    pub fn show_archive_read_only_for_test(&mut self) {
        let folder = PathBuf::from("/home/test/fotos.zip/2024");
        let entry = |name: &str, is_dir: bool, is_parent: bool, size: u64| fs::FileEntry {
            name: std::ffi::OsString::from(name),
            path: folder.join(name),
            is_dir,
            is_symlink: false,
            is_parent,
            size,
            modified: None,
            folder_size: None,
        };
        let mut panel = PanelState::new(folder.clone());
        panel.archive = Some(PathBuf::from("/home/test/fotos.zip"));
        panel.entries = vec![
            entry("..", true, true, 0),
            entry("urlaub", true, false, 0),
            entry("strand.jpg", false, false, 2_400_000),
            entry("sonnenuntergang.jpg", false, false, 3_100_000),
            entry("notizen.txt", false, false, 812),
        ];
        self.panels.left = panel;
        drop(self.apply_unless_refused(Message::Transfer(TransferKind::Move)));
    }

    /// Shows the progress mask of a fixed copy that is `done` of `total`
    /// items in, for the snapshot tests.
    pub fn show_progress_for_test(&mut self, done: usize, total: usize) {
        self.ops.transfer = Some(Transfer {
            kind: TransferKind::Copy,
            sources: vec![PathBuf::from("/home/test/links/entry-1")],
            target: self.panels.right.path.clone(),
            names: Vec::new(),
        });
        self.job_progress = Some(fs::transfer::Tick { done, total });
        self.ops.transfer_mask_visible = true;
    }

    /// The text of the status-line error, for the end-to-end tests.
    pub fn job_error_for_test(&self) -> Option<&str> {
        self.job_error.as_deref()
    }

    /// The config error alone, for the end-to-end tests.
    pub fn config_error_for_test(&self) -> Option<&str> {
        self.config_error.as_deref()
    }

    /// Puts a conflict in front of the user, for the snapshot tests.
    ///
    /// The state is set up rather than reached through a real copy: that would
    /// make the rendered image depend on what happens to be in the directory,
    /// and the test is about the dialog, not about the copy.
    pub fn open_conflict_for_test(&mut self, target: PathBuf, name: &str) {
        self.ops.pending_conflict = Some(PendingConflict {
            transfer: Transfer {
                kind: TransferKind::Copy,
                sources: vec![PathBuf::from("/left").join(name)],
                target,
                names: Vec::new(),
            },
            index: 0,
        });
    }
}
