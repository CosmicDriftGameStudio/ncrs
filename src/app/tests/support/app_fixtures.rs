use super::fixed::*;
use crate::app::*;

#[cfg(test)]
// Test-only constructors and helpers. One block rather than four: they were
// appended as tests were written, and a reader had to hunt for them.
impl App {
    /// An app with the create-directory prompt open, for the UI tests. The
    /// prompt field is private to this module, so the setup lives here.
    /// An app whose panels are filled from fixed data, for snapshot tests.
    ///
    /// `App::new()` would read the working directory, so the rendered image
    /// would depend on where the test runner happens to be started — a fresh CI
    /// container and a laptop would produce different references for the same
    /// code. This one renders the same bytes everywhere.
    pub fn with_fixed_panels() -> Self {
        let mut app = Self {
            panels: Panels::new(
                PanelState::new(PathBuf::from("/home/test/links")),
                PanelState::new(PathBuf::from("/home/test/files")),
                PanelSide::Left,
            ),
            visible_rows: 12,
            lang: Language::default(),
            function_keys: Keymap::built_in().function_keys(Language::default(), KeyScope::Panel),
            search_function_keys: Keymap::built_in()
                .function_keys(Language::default(), KeyScope::Search),
            keymap: Keymap::built_in(),
            jobs: jobs::Queue::new(),
            start_over_dialog: false,
            dialog_focus: 0,
            modifiers: Modifiers::default(),
            trash: fs::system_trash(None),
            launcher: Arc::new(fs::SystemLauncher),
            open_programs: config::OpenPrograms::default(),
            config_error: None,
            config_problem: None,
            palette: ThemePreset::Cosmic.palette(),
            job_error: None,
            job_error_press: None,
            notice: None,
            notice_generation: 0,
            job_done: 0,
            ops: ncrs_ops::Ops::default(),
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            command_line: CommandLine::default(),
            home: PathBuf::from("/home/test"),
            clock: Arc::new(std::time::Instant::now),
            window_size: layout::INITIAL_WINDOW_SIZE,
            state_path: None,
            session_scheduled: SessionState::default(),
            session_save_generation: 0,
            session_writes: Arc::default(),
            input: Arc::default(),
            list_volumes: Arc::new(fs::list_volumes),
            search: ncrs_search::Search::new(layout::INITIAL_WINDOW_SIZE),
            quick_open: None,
            quick_open_walk_generation: 0,
            quick_open_walk: None,
            command_palette: None,
            recent_commands: Vec::new(),
            wall_clock: Arc::new(ncrs_pipeline::SystemClock),
            config_path: None,
            file_list_tags: FileListTags::default(),
            kinds: Kinds::default(),
            hermes_flow_step: 0,
            settings: Settings::default(),
            hermes: ncrs_hermes_ui::Hermes::default(),
            theme_preset: ThemePreset::Cosmic,
            vscode_theme: None,
            resolve_folder: |folder| Ok(folder.to_path_buf()),
            pick_folder: |_| Task::none(),
            pick_file: |_| Task::none(),
            read_clipboard: || Task::done(None),
            write_clipboard_files: Box::new(|_| Ok(())),
            tag_exceptions: TagExceptions::new(),
            hermes_focus: HermesFocus::default(),
            fetch_extension: |_| Err(FetchError::NoConnection("no network in tests".to_owned())),
            install_theme: |_, _| Err(FetchError::NoConfigPath),
            about: None,
            find_third_party_licenses: || None,
            app_version: env!("CARGO_PKG_VERSION"),
            download_ocr_models: |_, _| Err(DownloadError::NoDataFolder),
            download_ai_model: |_| Err(AiDownloadError::NoDataFolder),
            grant_dialog: false,
            sandbox: fs::SandboxAccess::inactive(),
            remote: Arc::new(Pool::over_tcp(KnownHostsFiles::default())),
            passwords: Arc::new(fs::remote::credentials::InMemoryPasswords::default()),
            remote_ui: ncrs_remote::Remote::default(),
            saved_servers: Vec::new(),
        };
        for (side, names) in [
            (PanelSide::Left, ["..", "Documents", "Projects", "Desktop"]),
            (
                PanelSide::Right,
                ["..", "notes.txt", "report.pdf", "photo.jpg"],
            ),
        ] {
            let panel = app.panel_mut(side);
            panel.entries = names
                .iter()
                .enumerate()
                .map(|(i, name)| fs::FileEntry {
                    name: (*name).into(),
                    path: PathBuf::from("/home/test").join(name),
                    is_dir: i == 0 || name.ends_with('s') && i < 2,
                    is_symlink: false,
                    is_parent: i == 0,
                    size: if i == 0 { 0 } else { 1024 * (i as u64 * 4096) },
                    modified: None,
                    folder_size: None,
                })
                .collect();
        }
        app
    }

    /// Fixed panels with the filing card of `phase` up. The module is not
    /// running: no work, an in-memory store, no file anywhere.
    pub fn with_fixed_filing(phase: Phase) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.filing = Some(FilingModule::in_phase(phase));
        app
    }

    /// Fixed panels with the filing setup up, holding `chosen`. With
    /// `index_is_plain` a module stands behind it that opened an unencrypted
    /// index.
    pub fn with_filing_setup(chosen: &[config::FilingLocation], index_is_plain: bool) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.filing_setup = Some(FilingSetup::new(chosen));
        if index_is_plain {
            app.hermes.filing = Some(FilingModule::in_phase_with_index(Phase::Ready, false));
        }
        app
    }

    /// Fixed panels with the first-time setup of Hermes up, prefilled from
    /// `configured`.
    pub fn with_hermes_setup(configured: &[config::FilingLocation], offer: SetupOffer) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.filing_config.locations = configured.to_vec();
        let setup = HermesSetup::new(configured, offer);
        app.dialog_focus = setup.first_focus(setup.can_start());
        app.hermes.hermes_setup = Some(setup);
        app
    }

    /// Fixed panels with the Hermes overview up, over a
    /// module of `phase` (`None`: Hermes is off), the `config` and the
    /// `counts` read from the index.
    pub fn with_hermes_overview(
        phase: Option<Phase>,
        config: config::Filing,
        counts: Option<ncrs_index::HermesCounts>,
    ) -> Self {
        let mut app = Self::with_fixed_panels();
        let module_exists = phase.is_some();
        app.hermes.filing =
            phase.map(|in_phase| FilingModule::in_phase_with_ai(in_phase, &config.ai));
        if module_exists {
            let mut idle = ncrs_pipeline::Snapshot::off();
            idle.state = ncrs_pipeline::RunnerState::Idle;
            app.hermes
                .filing_status
                .set_snapshot(idle, app.wall_clock.now());
        }
        if let Some(read) = counts {
            app.hermes.filing_status.counts_arrived(Ok(read));
        }
        app.hermes.overview = Some(OverviewState::new(config.default_pattern.as_deref(), false));
        app.hermes.filing_config = config;
        app
    }

    /// A model download of `percent` of the model's size under the overview.
    pub fn with_ai_download_at_for_test(mut self, percent: u64) -> Self {
        let mut card = AiModelCard::new(Some(PathBuf::from("models")), false);
        drop(card.start());
        card.set_received_for_test(ncrs_ai::LOCAL_MODEL.size.div_euclid(100) * percent);
        self.settings.ai_model = Some(card);
        self.settings.ai_card_hidden = true;
        self
    }

    /// The module's key card was put off, so the overview shows the key as missing.
    pub fn with_missing_key_for_test(mut self) -> Self {
        self.hermes.filing = self
            .hermes
            .filing
            .take()
            .map(|module| module.dismissed_over(crate::filing::KeyProblem::Missing));
        self
    }

    /// An open index with or without encryption under the ready module.
    pub fn with_index_for_test(mut self, encrypted: bool) -> Self {
        self.hermes.filing = Some(FilingModule::in_phase_with_index(Phase::Ready, encrypted));
        self
    }

    /// The settings tab on the filing page, over a module in `phase`.
    pub fn with_hermes_settings(phase: Option<Phase>, config: config::Filing) -> Self {
        let mut app = Self::with_settings_open(crate::messages::SettingsFeature::Hermes, config);
        app.hermes.filing = phase
            .map(|in_phase| FilingModule::in_phase_with_ai(in_phase, &app.hermes.filing_config.ai));
        app
    }

    /// The question whether to remove the index up.
    pub fn with_start_over_question_for_test(mut self) -> Self {
        drop(self.update(Message::HermesOverview(HermesOverviewMessage::AskStartOver)));
        self
    }

    /// The same app, worded in `lang`.
    pub fn in_language(mut self, lang: Language) -> Self {
        self.set_language(lang);
        self
    }

    /// Fixed panels that have been shown `snapshot` of a pipeline at
    /// `taken_at`; no runner exists.
    pub fn with_filing_snapshot(
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.filing_status.set_snapshot(snapshot, taken_at);
        app
    }

    /// The sample column with tags after the names, as the setting says.
    pub fn with_file_list_tags_for_test(mut self, setting: FileListTags) -> Self {
        self.file_list_tags = setting;
        self
    }

    pub fn with_window_size_for_test(mut self, size: Size) -> Self {
        self.apply_window_size(size);
        self
    }

    /// Fixed panels with the Hermes column up and filled by hand: folders in
    /// every role on the left, files in every state on the right with the
    /// cursor on an inquiry.
    pub fn with_hermes_column_sample() -> Self {
        use crate::hermes_column::{ColumnReading, ColumnStage, DirectoryStates, FileDetail};
        use ncrs_filing::column_reading::FolderReading;
        use ncrs_index::{DocumentStatus, PathState, ReviewReason, StatusCounts};

        let home = PathBuf::from("/home/test");
        let files_dir = home.join("files");
        let location = |name: &str, mode, is_target| config::FilingLocation {
            path: home.join(name),
            mode,
            images: false,
            is_target,
        };
        let mut app = Self::with_fixed_panels();
        app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
        app.hermes.filing_config.locations = vec![
            location("Scans", ncrs_intake::LocationMode::Inbox, false),
            location("Filed", ncrs_intake::LocationMode::Archive, true),
            location("Archive", ncrs_intake::LocationMode::Archive, false),
        ];
        app.panels.active = PanelSide::Right;
        app.panels.left.path = home.clone();
        app.panels.right.path = files_dir.clone();

        let entry = |directory: &Path, name: &str, is_dir: bool, is_parent: bool| fs::FileEntry {
            name: name.into(),
            path: directory.join(name),
            is_dir,
            is_symlink: false,
            is_parent,
            size: if is_dir { 0 } else { 183_000 },
            modified: None,
            folder_size: None,
        };
        app.panels.left.entries = [".."]
            .into_iter()
            .map(|name| entry(&home, name, true, true))
            .chain(
                ["Scans", "Filed", "Archive", "Projects", "Private"]
                    .into_iter()
                    .map(|name| entry(&home, name, true, false)),
            )
            .collect();
        let files = [
            ("2025-03 utility bill.pdf", DocumentStatus::New),
            ("2025-03 scan 0412.pdf", DocumentStatus::Planned),
            ("2025-02 insurance letter.pdf", DocumentStatus::Filed),
            ("2025-03 receipt market.pdf", DocumentStatus::InReview),
            ("2025-01 damaged scan.pdf", DocumentStatus::New),
            ("2024-12 private diary.pdf", DocumentStatus::New),
        ];
        app.panels.right.entries = [entry(&files_dir, "..", true, true)]
            .into_iter()
            .chain(
                files
                    .iter()
                    .map(|(name, _)| entry(&files_dir, name, false, false)),
            )
            .collect();
        let inquiry = 4;
        app.panels.right.selected = inquiry;

        let counted = |path: &str, counts: StatusCounts| FolderReading {
            path: home.join(path),
            excluded: false,
            direct: Some(counts),
            deep: Some(counts),
        };
        let states = DirectoryStates::sample(
            files
                .iter()
                .enumerate()
                .map(|(position, (name, status))| PathState {
                    path: files_dir.join(name),
                    status: *status,
                    tags: ncrs_index::FileTags {
                        kind: Some(
                            ["rechnung", "brief", "versicherung", "beleg"]
                                .get(position % 4)
                                .copied()
                                .unwrap_or_default()
                                .to_owned(),
                        ),
                        recipient: Some("firma-a-gmbh".to_owned()),
                        sender: Some("telekom".to_owned()),
                        year: Some("2025".to_owned()),
                        own: [
                            vec!["steuer".to_owned()],
                            Vec::new(),
                            vec!["steuer".to_owned(), "büro".to_owned(), "privat".to_owned()],
                        ]
                        .get(position % 3)
                        .cloned()
                        .unwrap_or_default()
                        .clone(),
                    },
                    review: (*status == DocumentStatus::InReview)
                        .then_some(ReviewReason::NoMatchingRule),
                })
                .collect(),
            vec![files_dir.join("2024-12 private diary.pdf")],
        );
        app.hermes.hermes_column.mark_for_test(
            vec![files_dir.join("2025-03 scan 0412.pdf")],
            vec![(
                files_dir.join("2025-01 damaged scan.pdf"),
                "the PDF could not be opened".to_owned(),
            )],
        );
        let readings = [
            ColumnReading {
                generation: 0,
                epoch: 0,
                directory: files_dir.clone(),
                stage: ColumnStage::Listing,
                states: Some(states),
                folders: Vec::new(),
            },
            ColumnReading {
                generation: 0,
                epoch: 0,
                directory: home.clone(),
                stage: ColumnStage::Deep,
                states: None,
                folders: vec![
                    counted(
                        "Scans",
                        StatusCounts {
                            new: 3,
                            in_review: 2,
                            ..StatusCounts::default()
                        },
                    ),
                    counted(
                        "Filed",
                        StatusCounts {
                            filed: 12_400,
                            ..StatusCounts::default()
                        },
                    ),
                    counted(
                        "Archive",
                        StatusCounts {
                            filed: 31_000,
                            ..StatusCounts::default()
                        },
                    ),
                    FolderReading {
                        excluded: true,
                        ..counted("Private", StatusCounts::default())
                    },
                ],
            },
        ];
        for reading in readings {
            app.hermes.hermes_column.apply(reading);
        }
        app.hermes.hermes_column.apply_detail(
            files_dir.join("2025-03 receipt market.pdf"),
            FileDetail {
                text_state: crate::review_work::TextState::Read,
                suggestion: Some(PathBuf::from("Receipts/2025/Market/2025-03 receipt.pdf")),
            },
        );
        app
    }

    /// The Hermes column sample with the quick-fix menu open on its inquiry:
    /// one file whose sender was not read and whose suggestion is known.
    pub fn with_hermes_quick_fix_sample() -> Self {
        let mut app = Self::with_hermes_column_sample();
        let path = app.panels.right.path.join("2025-03 receipt market.pdf");
        let mut item = crate::review_list::fixtures::loaded_list()
            .rows()
            .to_vec()
            .swap_remove(0);
        item.path = path;
        item.fields.sender = None;
        item.fields.kind = Some("receipt".to_owned());
        item.fields.date = chrono::NaiveDate::from_ymd_opt(2025, 3, 14);
        item.proposed_pattern = Some("Receipts/2025/Market/2025-03 {kind}.pdf".to_owned());
        let anchor = layout::cursor_row_anchor(
            true,
            app.panels
                .right
                .selected
                .saturating_sub(app.panels.right.scroll_offset),
            layout::ROW_HEIGHT,
            app.window_size,
        );
        let mut menu = QuickFix::new(anchor, app.window_size);
        menu.loaded(vec![item]);
        app.hermes.quick_fix = Some(menu);
        app
    }

    /// Fixed panels with the quick-fix menu open on `item` alone.
    pub fn with_quick_fix_menu(item: ReviewItem) -> Self {
        let mut app = Self::with_fixed_panels();
        let mut menu = QuickFix::new(iced::Point::ORIGIN, app.window_size);
        menu.loaded(vec![item]);
        app.hermes.quick_fix = Some(menu);
        app
    }

    /// The column sample with the context menu open on the inquiry row and the
    /// Hermes submenu out.
    pub fn with_hermes_context_menu_sample() -> Self {
        let mut app = Self::with_hermes_column_sample();
        let index = app.panels.right.selected;
        app.open_context_menu(PanelSide::Right, index, None);
        if let Some(menu) = app.panels.context_menu.as_mut() {
            menu.select_last();
            menu.open_submenu();
        }
        app
    }

    /// Fixed panels that saw `began` and then `now`, so progress has moved.
    pub fn with_filing_progress(
        began: ncrs_pipeline::Snapshot,
        now: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_filing_snapshot(began, taken_at);
        app.hermes.filing_status.set_snapshot(now, taken_at);
        app
    }

    /// Like [`Self::with_filing_snapshot`], with the status page open.
    pub fn with_filing_status_page(
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_filing_snapshot(snapshot, taken_at);
        app.hermes.filing_status.open_page();
        app
    }

    /// Like [`Self::with_filing_status_page`], with the index open and
    /// `figures` read from it.
    pub fn with_filing_inspection(
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
        figures: crate::filing::IndexFigures,
    ) -> Self {
        let mut app = Self::with_filing_status_page(snapshot, taken_at);
        app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
        app.hermes.filing_status.figures_arrived(Ok(figures));
        app
    }

    /// A prompt on fixed data, for the prompt's own reference image.
    pub fn with_fixed_prompt() -> Self {
        let mut app = Self::with_fixed_panels();
        app.ops.prompt = Some(Prompt::create_dir(PathBuf::from("/home/test/links")));
        if let Some(prompt) = app.ops.prompt.as_mut() {
            prompt.set_name("neuer ordner".to_string());
        }
        app
    }

    /// The new-file prompt on fixed data, for its reference image.
    pub fn with_fixed_new_file_prompt() -> Self {
        let mut app = Self::with_fixed_panels();
        let mut prompt = Prompt::create_file(PathBuf::from("/home/test/links"));
        prompt.set_name("notes.txt".to_string());
        app.ops.prompt = Some(prompt);
        app
    }

    /// The pack prompt on fixed data, for its reference image.
    pub fn with_fixed_pack_prompt() -> Self {
        let mut app = Self::with_fixed_panels();
        app.ops.prompt = Some(Prompt::pack(
            PathBuf::from("/home/test/links"),
            vec![PathBuf::from("/home/test/fotos")],
            "fotos.zip",
        ));
        app
    }

    /// Opens the prompt the way F7 does — through `update` — so the focus Task
    /// is produced the same way it is in the running app. Setting `prompt`
    /// directly would skip it.
    ///
    /// No parent argument: `update` takes the active panel's path, as it does
    /// in the app.
    pub fn with_prompt_open() -> Self {
        let mut app = Self::with_fixed_panels();
        let _task = app.update(crate::messages::Message::CreateDirPrompt);
        app
    }

    pub fn set_prompt(&mut self, name: &str, error: Option<&str>) {
        let Some(open) = self.ops.prompt.as_mut() else {
            return;
        };
        open.set_name(name.to_string());
        if let Some(message) = error {
            open.set_error(message.to_string());
        }
    }
    /// An app with the trash view up on two fixed entries, for the snapshot.
    /// Without a deletion time: a date would show the time zone of the machine.
    pub fn with_fixed_trash() -> Self {
        let mut app = Self::with_fixed_panels();
        let entry = |original: &str, key: &str| fs::TrashedItem {
            original: PathBuf::from(original),
            deleted_at: 0,
            key: key.into(),
        };
        let mut view = TrashView::new(fs::TrashReach::OwnDeletions);
        view.set_items(
            Ok(vec![
                entry("/home/test/files/report.pdf", "a"),
                entry("/home/test/links/photos", "b"),
            ]),
            app.trash_rows(),
        );
        view.toggle_tag();
        view.move_cursor(1, app.trash_rows());
        app.ops.trash_view = Some(view);
        app
    }

    /// An app with the search view up on fixed results, for the snapshot.
    pub fn with_fixed_search() -> Self {
        let mut app = Self::with_fixed_panels();
        let root = app.panel(PanelSide::Left).path.clone();
        app.show_search_results_for_test("hello", fixed_search_results(&root));
        // One tagged file next to the cursor, so the reference shows the tint.
        if let Some(view) = app.search.view.as_mut() {
            view.tagged.insert(root.join("notes.txt"));
        }
        app
    }

    /// Opens the search view the way Alt+F7 does and fills it with `files`, as
    /// a finished search for `query` would, with the cursor on the second row.
    pub fn show_search_results_for_test(
        &mut self,
        query: &str,
        files: Vec<(fs::FileEntry, Vec<crate::fs::search::LineMatch>)>,
    ) {
        use crate::fs::search::{SearchEvent, SearchTotals};
        drop(self.update(Message::OpenSearch));
        let totals = SearchTotals {
            scanned: 14,
            files: files.len(),
            matches: files.iter().map(|(_, lines)| lines.len()).sum(),
            skipped: 0,
            truncated: false,
        };
        if let Some(view) = self.search.view.as_mut() {
            view.query = query.to_string();
            view.begin(1);
            for (entry, lines) in files {
                view.apply_event(SearchEvent::File {
                    path: entry.path,
                    lines,
                    size: entry.size,
                    modified: entry.modified,
                });
            }
            view.apply_event(SearchEvent::Finished(totals));
            view.select_row(2, self.search.rows);
        }
    }

    pub fn search_for_test(&self) -> Option<&SearchView> {
        self.search.view.as_ref()
    }

    pub fn search_is_open_for_test(&self) -> bool {
        self.search.open
    }

    pub fn settings_for_test(&self) -> Option<&settings_ui::SettingsState> {
        self.settings.page.as_ref()
    }

    pub fn command_palette_for_test(&self) -> Option<&CommandPalette> {
        self.command_palette.as_ref()
    }

    pub fn theme_fetch_for_test(&self) -> Option<&ThemeFetch> {
        self.settings.theme_fetch.as_ref()
    }

    pub fn about_for_test(&self) -> Option<&AboutCard> {
        self.about.as_ref()
    }

    /// Fixed panels with the about card up, showing a fixed version so a
    /// release never changes a snapshot.
    pub fn with_about_card(card: AboutCard) -> Self {
        let mut app = Self::with_fixed_panels();
        app.dialog_focus = about_ui::CLOSE_FOCUS;
        app.app_version = "1.2.3";
        app.about = Some(card);
        app
    }

    /// Makes the licence lookup find (or not find) a file.
    pub fn with_licenses_finder(mut self, finder: fn() -> Option<PathBuf>) -> Self {
        self.find_third_party_licenses = finder;
        self
    }

    pub fn ocr_models_for_test(&self) -> Option<&OcrModelsCard> {
        self.settings.ocr_models.as_ref()
    }

    /// Fixed panels with the text recognition models card up.
    pub fn with_ocr_models_card(card: OcrModelsCard) -> Self {
        let mut app = Self::with_fixed_panels();
        app.dialog_focus = ocr_models_ui::CANCEL_FOCUS;
        app.settings.ocr_models = Some(card);
        app
    }

    pub fn ai_model_for_test(&self) -> Option<&AiModelCard> {
        self.settings.ai_model.as_ref()
    }

    /// Fixed panels with the AI model card up.
    pub fn with_ai_model_card(card: AiModelCard) -> Self {
        let mut app = Self::with_fixed_panels();
        app.dialog_focus = ai_model_ui::CANCEL_FOCUS;
        app.settings.ai_model = Some(card);
        app
    }
}
