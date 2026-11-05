use crate::app::*;

impl App {
    /// Replaces nothing: the caller has dropped any module and pipeline before.
    pub(in crate::app) fn start_filing_module(&mut self, filing: &Filing) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::start_filing_module(hermes, filing, ctx)
        })
    }

    pub(in crate::app) fn update_filing(&mut self, message: FilingMessage) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_filing(hermes, message, ctx)
        })
    }

    pub(in crate::app) fn apply_saved_filing(
        &mut self,
        filing: Filing,
    ) -> (SavedFilingApplies, Task<Message>) {
        let (applies, outcome) = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::apply_saved_filing(hermes, filing, ctx)
        });
        (applies, self.run_hermes_outcome(outcome))
    }

    pub(in crate::app) fn apply_ai_change(
        &mut self,
        filing: Filing,
    ) -> (SavedFilingApplies, Task<Message>) {
        let (applies, outcome) = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::apply_ai_change(hermes, filing, ctx)
        });
        (applies, self.run_hermes_outcome(outcome))
    }

    pub(in crate::app) fn filing_restarted(&mut self, generation: u64) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::filing_restarted(hermes, generation, ctx)
        })
    }

    /// The file or folder under the cursor of the active panel, for the Hermes
    /// commands: not the `..` row.
    fn hermes_cursor_entry(&self) -> Option<&fs::FileEntry> {
        self.active_panel()
            .selected_entry()
            .filter(|entry| !entry.is_parent)
    }

    fn cursor_is_excluded_from_hermes(&self) -> bool {
        self.hermes_cursor_entry().is_some_and(|entry| {
            if entry.is_dir {
                self.hermes
                    .hermes_column
                    .summary_of(&entry.path)
                    .is_some_and(|summary| summary.excluded)
            } else {
                self.hermes.hermes_column.state_of(&entry.path) == Some(HermesFileState::Excluded)
            }
        })
    }

    fn inbox_location(&self) -> Option<&Path> {
        self.hermes
            .filing_config
            .locations
            .iter()
            .find(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.as_path())
    }

    /// What the Hermes entry of the context menu checks for the cursor row.
    pub(in crate::app) fn hermes_facts(&self) -> HermesFacts {
        let locations = &self.hermes.filing_config.locations;
        let cursor = self.hermes_cursor_entry().map(|entry| {
            let role_free = |role| role_of(&entry.path, locations) != Some(role);
            HermesCursor {
                is_dir: entry.is_dir,
                excluded: self.cursor_is_excluded_from_hermes(),
                can_be_handed_to_inbox: self
                    .inbox_location()
                    .is_some_and(|inbox| !entry.path.starts_with(inbox)),
                lies_in_a_filing_location: locations
                    .iter()
                    .any(|location| entry.path.starts_with(&location.path)),
                inbox_role_is_free: role_free(FolderRole::Inbox),
                filing_location_role_is_free: role_free(FolderRole::FilingLocation),
                archive_role_is_free: role_free(FolderRole::Archive),
            }
        });
        HermesFacts {
            ready: self.hermes.is_ready(),
            active: self.hermes.is_active(),
            has_inquiry_paths: !self.inquiry_paths().is_empty(),
            has_live_pipeline: self.hermes.live_pipeline().is_some(),
            cursor,
        }
    }

    pub(in crate::app) fn hermes_action_enabled(&self, action: HermesAction) -> bool {
        self.hermes_facts().action_enabled(action)
    }

    pub(in crate::app) fn update_hermes_file(
        &mut self,
        message: HermesFileMessage,
    ) -> Task<Message> {
        match message {
            HermesFileMessage::RoleSaved(result) => {
                return self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::hermes_role_saved(hermes, result, ctx)
                })
            }
            HermesFileMessage::Included { path, result } => {
                return self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::hermes_included(hermes, &path, result, ctx)
                })
            }
            HermesFileMessage::HandToInbox
            | HermesFileMessage::ProcessAgain
            | HermesFileMessage::SetRole(_)
            | HermesFileMessage::ToggleExclusion
            | HermesFileMessage::ShowInInspection => {}
        }
        let available = if matches!(message, HermesFileMessage::SetRole(_)) {
            self.hermes.is_ready()
        } else {
            self.hermes.is_active()
        };
        if self.overlay_is_open() || self.search.open || !available {
            return Task::none();
        }
        let cursor = self
            .hermes_cursor_entry()
            .map(|entry| (entry.path.clone(), entry.is_dir));
        let Some((path, is_dir)) = cursor else {
            return Task::none();
        };
        match message {
            HermesFileMessage::HandToInbox => {
                let Some(inbox) = self.inbox_location().map(Path::to_path_buf) else {
                    let text = self.lang.text(Msg::StatusHermesNoInbox).to_owned();
                    return self.show_notice(text);
                };
                self.open_transfer_dialog_to(TransferKind::Move, Some(inbox))
            }
            HermesFileMessage::ProcessAgain => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::process_again(hermes, &path, ctx)
            }),
            HermesFileMessage::SetRole(role) if is_dir => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::set_folder_role(hermes, &path, role, ctx)
                }),
            HermesFileMessage::ToggleExclusion => {
                let excluded = self.cursor_is_excluded_from_hermes();
                self.run_hermes_flow(Vec::new(), |hermes, _| {
                    ncrs_hermes_ui::toggle_hermes_exclusion(hermes, path, excluded)
                })
            }
            HermesFileMessage::ShowInInspection => {
                let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::show_in_inspection(hermes, &path, ctx)
                });
                self.run_hermes_outcome(outcome)
            }
            HermesFileMessage::SetRole(_)
            | HermesFileMessage::RoleSaved(_)
            | HermesFileMessage::Included { .. } => Task::none(),
        }
    }

    pub(in crate::app) fn documents_filed(
        &mut self,
        result: Result<FilingSummary, String>,
    ) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |_, ctx| {
            ncrs_hermes_ui::documents_filed(result, ctx)
        })
    }

    fn hermes_ctx(
        &self,
        hermes: &ncrs_hermes_ui::Hermes,
        inquiry_paths: Vec<PathBuf>,
    ) -> ncrs_hermes_ui::Ctx<'_> {
        let side = self.panels.active;
        let panel = self.panel(side);
        ncrs_hermes_ui::Ctx {
            lang: self.lang,
            kinds: &self.kinds,
            status_page_open: hermes.filing_status.page_open(),
            overlay_apart_from_ablage_tab_open: self.dialog_apart_from_hermes_is_open(),
            start_over_dialog_open: self.start_over_dialog,
            hermes_is_restarting: hermes.is_restarting(),
            hermes_has_open_reviews: hermes
                .filing_status
                .hermes_counts()
                .is_some_and(|counts| counts.open_reviews > 0),
            file_list_tags: &self.file_list_tags,
            filing_status_shortcut: self
                .keymap
                .shortcut_label(&Message::FilingStatus(FilingStatusMessage::Toggle)),
            active_panel_path: &panel.path,
            other_panel_path: &self.panel(side.other()).path,
            context_menu_open: self.panels.context_menu.is_some(),
            search_open: self.search.open,
            hermes_active: hermes.is_active(),
            hermes_counts_missing: hermes.filing_status.hermes_counts().is_none(),
            jobs_busy: self.jobs.is_busy(),
            window_size: self.window_size,
            config_path: self.config_path.as_deref(),
            home: &self.home,
            quick_fix_anchor: layout::cursor_row_anchor(
                side == PanelSide::Right,
                panel.selected.saturating_sub(panel.scroll_offset),
                panel.row_height(),
                self.window_size,
            ),
            inquiry_paths,
            now: self.wall_clock.now(),
            panels: [
                self.panel_facts(PanelSide::Left),
                self.panel_facts(PanelSide::Right),
            ],
            cursor_entry: Some(self.active_panel())
                .filter(|active| !active.is_virtual())
                .and_then(PanelState::selected_entry)
                .map(|entry| entry.path.as_path()),
        }
    }

    fn panel_facts(&self, side: PanelSide) -> ncrs_hermes_ui::ctx::PanelFacts<'_> {
        let panel = self.panel(side);
        ncrs_hermes_ui::ctx::PanelFacts {
            side,
            directory: &panel.path,
            request_id: panel.request_id,
            readable: !panel.loading && !panel.is_virtual(),
            visible: panel
                .entries
                .iter()
                .skip(panel.scroll_offset)
                .take(panel.rows_in_view(self.visible_rows))
                .map(|entry| ncrs_hermes_ui::ctx::VisibleEntry {
                    path: &entry.path,
                    is_dir: entry.is_dir,
                    is_parent: entry.is_parent,
                })
                .collect(),
        }
    }

    /// Runs `step` on the Hermes pages. The state is taken out for it, since
    /// the context borrows the rest of the app. The context reads nothing of
    /// the pages themselves: the update looks at those as they are while it
    /// runs.
    pub(in crate::app) fn with_hermes<R>(
        &mut self,
        inquiry_paths: Vec<PathBuf>,
        step: impl FnOnce(&mut ncrs_hermes_ui::Hermes, &ncrs_hermes_ui::Ctx<'_>) -> R,
    ) -> R {
        let mut hermes = std::mem::take(&mut self.hermes);
        let ctx = self.hermes_ctx(&hermes, inquiry_paths);
        let outcome = step(&mut hermes, &ctx);
        self.hermes = hermes;
        outcome
    }

    /// Runs a flow of the Hermes pages and then the actions it asked for.
    pub(in crate::app) fn run_hermes_flow(
        &mut self,
        inquiry_paths: Vec<PathBuf>,
        step: impl FnOnce(
            &mut ncrs_hermes_ui::Hermes,
            &ncrs_hermes_ui::Ctx<'_>,
        ) -> ncrs_hermes_ui::Outcome,
    ) -> Task<Message> {
        let outcome = self.with_hermes(inquiry_paths, step);
        self.run_hermes_outcome(outcome)
    }

    pub(in crate::app) fn run_hermes_outcome(
        &mut self,
        (task, actions): ncrs_hermes_ui::Outcome,
    ) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_hermes_action(action));
        }
        Task::batch(tasks)
    }

    fn run_hermes_action(&mut self, action: ncrs_hermes_ui::Action) -> Task<Message> {
        use ncrs_hermes_ui::Action as Ask;
        match action {
            Ask::OpenChat => self.update_hermes_chat(HermesChatMessage::Open),
            Ask::OpenFilingStatus => self.open_filing_status(),
            Ask::RevealFile(path) => self.reveal_file(&path),
            Ask::FilingRestarted(generation) => self.filing_restarted(generation),
            Ask::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
            Ask::ClearStatus => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::ShowError(detail) => {
                self.set_status_error(&detail);
                Task::none()
            }
            Ask::RequestHermesCounts => self
                .with_hermes(Vec::new(), |hermes, _| {
                    ncrs_hermes_ui::request_hermes_counts(hermes)
                })
                .map(Message::from),
            Ask::ReadPathsAgain(paths) => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::read_paths_again(hermes, paths, ctx)
            }),
            Ask::CloseStatusPage => {
                self.hermes.filing_status.close_page();
                Task::none()
            }
            Ask::GoToHermesPage(page) => self.go_to_hermes_page(page),
            Ask::RefreshFilingSnapshot => {
                self.hermes.refresh_filing_snapshot(self.wall_clock.now());
                Task::none()
            }
            Ask::ChangeProtection(begin) => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::change_protection(hermes, begin, ctx)
            }),
            Ask::OpenStartOverDialog(focus) => {
                self.dialog_focus = focus;
                self.start_over_dialog = true;
                Task::none()
            }
            Ask::CloseStartOverDialog => {
                self.start_over_dialog = false;
                Task::none()
            }
            Ask::SetLoad(load) => {
                self.update_settings(SettingsMessage::Change(SettingChange::Load(load)))
            }
            Ask::RestartHermesModule => {
                self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::restart_hermes_module)
            }
            Ask::SetTagExceptions(exceptions) => {
                self.tag_exceptions = exceptions;
                Task::none()
            }
            Ask::ChangeFileListTags(tags) => {
                self.update_settings(SettingsMessage::Change(SettingChange::FileListTags(tags)))
            }
            Ask::OpenSearchFor { spec, report } => self.open_search_for(&spec, report),
            Ask::PickExportFolder(start_in) => (self.pick_folder)(&start_in)
                .map(|folder| Message::SavedQueries(SavedQueriesMessage::FolderPicked(folder))),
            Ask::StartJobWithProgress(start) => {
                self.start_job_with_progress(start.job, start.build)
            }
            Ask::FinishJob => {
                self.jobs.finish();
                Task::none()
            }
            Ask::SetJobProgress(tick) => {
                self.job_progress = Some(tick);
                Task::none()
            }
            Ask::ClearJobProgress => {
                self.job_progress = None;
                Task::none()
            }
            Ask::ReloadPanels => self.reload_panels(),
            Ask::SelectErrorOf(path) => {
                self.hermes.filing_status.select_error_of(&path);
                Task::none()
            }
        }
    }

    pub(in crate::app) fn open_and_download_ai_model(&mut self) -> Task<Message> {
        let open = self.update_ai_model(AiModelMessage::Open);
        let download = self.update_ai_model(AiModelMessage::Download);
        Task::batch([open, download])
    }
}
