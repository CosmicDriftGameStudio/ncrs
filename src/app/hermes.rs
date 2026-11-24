use crate::app::*;

impl App {
    /// Replaces nothing: the caller has dropped any module and pipeline before.
    pub(in crate::app) fn start_filing_module(&mut self, filing: &Filing) -> Task<Message> {
        self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::start_filing_module(hermes, filing, ctx)
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

    /// What the Hermes entry of the context menu checks for the cursor row.
    pub(in crate::app) fn hermes_facts(&self) -> HermesFacts {
        let cursor = self.hermes_cursor_entry().map(|entry| {
            let roles = self.hermes.cursor_roles(&entry.path);
            HermesCursor {
                is_dir: entry.is_dir,
                excluded: self.hermes.cursor_is_excluded(&entry.path, entry.is_dir),
                can_be_handed_to_inbox: roles.can_be_handed_to_inbox,
                lies_in_a_filing_location: roles.lies_in_a_filing_location,
                is_write_folder: roles.is_write_folder,
                lies_in_write_folder: roles.lies_in_write_folder,
                is_continuous: roles.is_continuous,
                filing_location_role_is_free: roles.filing_location_role_is_free,
                archive_role_is_free: roles.archive_role_is_free,
            }
        });
        HermesFacts {
            ready: self.hermes.is_ready(),
            active: self.hermes.is_active(),
            has_inquiry_paths: !self.inquiry_paths().is_empty(),
            has_live_pipeline: self.hermes.live_pipeline().is_some(),
            has_write_folder: self.hermes.write_folder().is_some(),
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
        if let HermesFileMessage::FileOnce | HermesFileMessage::FileContinuously = message {
            return self.file_folder_in_the_tab(message == HermesFileMessage::FileContinuously);
        }
        let action_sources = if matches!(message, HermesFileMessage::ProcessAgain) {
            self.action_sources()
        } else {
            Vec::new()
        };
        // Read before Hermes is taken out: the tools come from its module.
        let continuous = self.continuous_tools();
        let mut hermes = std::mem::take(&mut self.hermes);
        let mut ctx = self.hermes_ctx(&hermes, Vec::new(), continuous);
        ctx.action_sources = action_sources;
        let asked = ncrs_hermes_ui::update(
            &mut hermes,
            ncrs_hermes_ui::Message::HermesFile(message),
            &ctx,
        );
        drop(ctx);
        self.hermes = hermes;
        asked.map_or_else(Task::none, |outcome| self.run_hermes_outcome(outcome))
    }

    /// Opens the filing tab for the folder under the cursor, into the write
    /// folder; `continuously` keeps the folder filing by itself once the run
    /// is filed.
    fn file_folder_in_the_tab(&mut self, continuously: bool) -> Task<Message> {
        let action = if continuously {
            HermesAction::FileContinuously
        } else {
            HermesAction::FileOnce
        };
        if !self.hermes_action_enabled(action) {
            return Task::none();
        }
        let Some(source) = self.hermes_cursor_entry().map(|entry| entry.path.clone()) else {
            return Task::none();
        };
        let Some(target) = self.hermes.write_folder().map(Path::to_path_buf) else {
            return Task::none();
        };
        let task = self.open_filing(source, target);
        if let Some(session) = self.filing_view.session.as_mut() {
            session.then_continuous = continuously;
        }
        task
    }

    /// The handle and the model of the background filing, when Hermes is
    /// ready and its AI is on.
    pub(in crate::app) fn continuous_tools(&self) -> Option<ncrs_hermes_ui::ContinuousTools> {
        // Locked, the background filing waits without a word.
        if !self.changes_allowed() {
            return None;
        }
        let model = self
            .chat_ai_on()
            .then(|| self.chat_model.clone())
            .flatten()?;
        match self.chat_availability() {
            ncrs_hermes_chat::Availability::Ready(ready) => Some(ncrs_hermes_ui::ContinuousTools {
                access: (*ready).with_model(Some(Arc::clone(&model))),
                model,
            }),
            _ => None,
        }
    }

    fn hermes_ctx(
        &self,
        hermes: &ncrs_hermes_ui::Hermes,
        inquiry_paths: Vec<PathBuf>,
        continuous: Option<ncrs_hermes_ui::ContinuousTools>,
    ) -> ncrs_hermes_ui::Ctx<'_> {
        let side = self.panels.active;
        let panel = self.panel(side);
        ncrs_hermes_ui::Ctx {
            lang: self.lang,
            kinds: &self.kinds,
            start_over_dialog_open: self.start_over_dialog,
            hermes_is_restarting: hermes.is_restarting(),
            hermes_has_open_reviews: hermes
                .filing_status
                .hermes_counts()
                .is_some_and(|counts| counts.open_reviews > 0),
            file_list_tags: &self.file_list_tags,
            filing_status_shortcut: self
                .keymap
                .shortcut_label(&Message::FilingStatus(FilingStatusMessage::Open)),
            active_panel_path: &panel.path,
            other_panel_path: &self.panel(side.other()).path,
            context_menu_open: self.panels.context_menu.is_some(),
            search_open: self.search.open,
            hermes_active: hermes.is_active(),
            chat_open: self.hermes_chat.open,
            hermes_counts_missing: hermes.filing_status.hermes_counts().is_none(),
            jobs_busy: self.jobs.is_busy(),
            continuous,
            trash: ncrs_hermes_ui::Bin(Arc::clone(&self.trash)),
            filing_tab_source: self
                .filing_view
                .session
                .as_ref()
                .filter(|session| session.is_working())
                .map(|session| session.source.as_path()),
            window_size: self.window_size,
            config_path: self.config_path.as_deref(),
            home: &self.home,
            quick_fix_anchor: layout::cursor_row_anchor(
                side == PanelSide::Right,
                panel.selected.saturating_sub(panel.scroll_offset),
                panel.row_height(),
                self.window_size,
                self.favourites_bar_height(),
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
            cursor_row: self
                .hermes_cursor_entry()
                .map(|entry| (entry.path.as_path(), entry.is_dir)),
            overlay_open: self.overlay_is_open(),
            action_sources: Vec::new(),
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
        // Read before Hermes is taken out: the tools come from its module.
        let continuous = self.continuous_tools();
        let mut hermes = std::mem::take(&mut self.hermes);
        let ctx = self.hermes_ctx(&hermes, inquiry_paths, continuous);
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
            Ask::OpenChatAbout(items) => {
                self.update_hermes_chat(HermesChatMessage::OpenAbout(items))
            }
            Ask::IndexReplaced => self.update_hermes_chat(HermesChatMessage::IndexReplaced),
            Ask::RevealFile(path) => self.reveal_file(&path),
            Ask::FilingRestarted(generation) => self.filing_restarted(generation),
            Ask::CloseContextMenu => {
                self.close_context_menu();
                Task::none()
            }
            Ask::ClearStatus => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::MoveToInbox(inbox) => {
                self.open_transfer_dialog_to(TransferKind::Move, Some(inbox))
            }
            Ask::ShowError(detail) => {
                let shown = if detail == ncrs_core::INDEX_NOT_OPEN {
                    self.lang.text(Msg::IndexNotOpen).to_owned()
                } else {
                    detail
                };
                self.set_status_error(&shown);
                Task::none()
            }
            Ask::RefreshDocumentsFacts => self.refresh_documents_facts(),
            Ask::EnsureChatModel => {
                self.ensure_chat_model();
                Task::none()
            }
            Ask::CheckChatWaiting => self.update_hermes_chat(HermesChatMessage::WaitingCheck),
            Ask::ReadingSettled(sequence) => {
                self.update_hermes_chat(HermesChatMessage::ReadingSettled(sequence))
            }
            Ask::RequestHermesCounts => self
                .with_hermes(Vec::new(), |hermes, _| {
                    ncrs_hermes_ui::request_hermes_counts(hermes)
                })
                .map(Message::from),
            Ask::ReadPathsAgain(paths) => self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::read_paths_again(hermes, paths, ctx)
            }),
            Ask::ShowPage(page) => self.show_hermes_page(page),
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
            Ask::PickReviewFolder(start_in) => {
                let sandbox = self
                    .sandbox
                    .grants_dir
                    .clone()
                    .map(|dir| (dir, self.sandbox.grant));
                (self.pick_folder)(&start_in)
                    .then(move |folder| review_folder_chosen(folder, sandbox.clone()))
            }
            Ask::PreviewFile(path) => {
                let left = self.leave_settings_page();
                let opened = self.open_in_viewer(path);
                Task::batch([left, opened])
            }
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
        }
    }

    pub(in crate::app) fn open_and_download_ai_model(&mut self) -> Task<Message> {
        let open = self.update_ai_model(AiModelMessage::Open);
        let download = self.update_ai_model(AiModelMessage::Download);
        Task::batch([open, download])
    }
}

/// The folder chosen for a review answer keeps its access for the next start,
/// like a folder chosen in the chat; a failure to save it is shown, the answer
/// goes on with the access the dialog just gave.
type SaveGrant = fn(&Path, &Path) -> Result<(), String>;

fn review_folder_chosen(
    folder: Option<PathBuf>,
    sandbox: Option<(PathBuf, SaveGrant)>,
) -> Task<Message> {
    let picked = |answer_folder| {
        Message::Reviews(ncrs_hermes_ui::ReviewsMessage::FolderPicked(answer_folder))
    };
    let (Some(chosen), Some((dir, grant))) = (folder.clone(), sandbox) else {
        return Task::done(picked(folder));
    };
    let saved = chosen.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || grant(&dir, &saved))
                .await
                .unwrap_or_else(|failure| Err(failure.to_string()))
        },
        move |result| (chosen.clone(), result),
    )
    .then(move |(granted, result)| match result {
        Ok(()) => Task::done(picked(Some(granted))),
        Err(reason) => Task::batch([
            Task::done(Message::Grant(GrantMessage::Saved {
                folder: granted.clone(),
                result: Err(reason),
            })),
            Task::done(picked(Some(granted))),
        ]),
    })
}
