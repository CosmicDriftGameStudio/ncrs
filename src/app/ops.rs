use crate::app::*;

/// Runs one job and reports what happens as messages.
///
/// Separate from `App` so the queue can hand it to `Task::abortable` without
/// borrowing the app. The real work of F5/F6/F8 runs per row in `App`; a job
/// that reaches the queue's `start_next` here has nothing left to do.
pub(in crate::app) fn run_job(_job: jobs::Job) -> Task<Message> {
    Task::done(Message::JobFinished(JobEvent::Done))
}

impl App {
    /// F8 / Shift+F8: ask before deleting.
    pub(in crate::app) fn open_delete_dialog(&mut self, permanent: bool) -> Task<Message> {
        let side = self.source_side();
        let request = ncrs_ops::DeleteRequest {
            permanent,
            sources: self.action_sources(),
            side,
            in_archive: self.panel(side).archive.is_some(),
        };
        self.with_ops(|ops, ctx| ncrs_ops::open_delete(ops, request, ctx))
    }

    pub(in crate::app) fn confirm_delete(&mut self) -> Task<Message> {
        self.with_ops(ncrs_ops::confirm_delete)
    }

    fn end_delete(&mut self) -> Task<Message> {
        self.with_ops(ncrs_ops::end_delete)
    }

    pub(in crate::app) fn end_restore(&mut self) -> Task<Message> {
        self.with_ops(|ops, ctx| ncrs_ops::end_restore(ops, ctx, false))
    }

    /// Runs `step` on the file operation flows and then what they ask of the app.
    pub(in crate::app) fn with_ops(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>) -> ncrs_ops::Outcome,
    ) -> Task<Message> {
        let outcome = self.with_ops_ctx(step);
        self.run_ops_outcome(outcome)
    }

    /// Runs `step` on the flows' state, taken out of the app for it since the
    /// context borrows the rest.
    pub(in crate::app) fn with_ops_ctx<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>) -> R,
    ) -> R {
        self.with_ops_and_panels_ctx(|ops, flow, _| step(ops, flow))
    }

    /// As [`Self::with_ops_ctx`], with the panels to read from.
    fn with_ops_and_panels_ctx<R>(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>, &Panels) -> R,
    ) -> R {
        let start_blocked =
            self.jobs.is_busy() || self.ops.prompt.is_some() || self.ops.conflict_is_open();
        let transfer_dialog_open = self.ops.transfer_dialog.is_some();
        let permanent_delete_key = self
            .keymap
            .shortcut_label(&Message::Delete { permanent: true });
        let restore_last_key = self
            .keymap
            .shortcut_label(&KeyAction::RestoreLastDeletion.message());
        let mut ops = std::mem::take(&mut self.ops);
        let outcome = step(
            &mut ops,
            &ncrs_ops::FlowCtx {
                lang: self.lang,
                trash: &self.trash,
                jobs_busy: self.jobs.is_busy(),
                start_blocked,
                transfer_dialog_open,
                has_job_error: self.job_error.is_some(),
                sandbox: &self.sandbox,
                source_side: self.source_side(),
                left_archive: self.panel(PanelSide::Left).archive.as_deref(),
                right_archive: self.panel(PanelSide::Right).archive.as_deref(),
                left_server: self.panel(PanelSide::Left).remote().map(|s| s.root_path()),
                right_server: self.panel(PanelSide::Right).remote().map(|s| s.root_path()),
                remote: &self.remote,
                permanent_delete_key,
                restore_last_key,
            },
            &self.panels,
        );
        self.ops = ops;
        outcome
    }

    /// As [`Self::with_ops`], with the panels to read from.
    pub(in crate::app) fn with_ops_and_panels(
        &mut self,
        step: impl FnOnce(&mut ncrs_ops::Ops, &ncrs_ops::FlowCtx<'_>, &Panels) -> ncrs_ops::Outcome,
    ) -> Task<Message> {
        let outcome = self.with_ops_and_panels_ctx(step);
        self.run_ops_outcome(outcome)
    }

    pub(in crate::app) fn run_ops_actions(
        &mut self,
        actions: Vec<ncrs_ops::Action>,
    ) -> Task<Message> {
        self.run_ops_outcome((Task::none(), actions))
    }

    fn run_ops_outcome(&mut self, (task, actions): ncrs_ops::Outcome) -> Task<Message> {
        let mut tasks = vec![task.map(Message::from)];
        for action in actions {
            tasks.push(self.run_ops_action(action));
        }
        Task::batch(tasks)
    }

    fn run_ops_action(&mut self, action: ncrs_ops::Action) -> Task<Message> {
        use ncrs_ops::Action as Ask;
        match action {
            Ask::ClearStatus => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Ask::ShowNotice(text) => self.show_notice(text),
            Ask::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Ask::FocusDialogButton(button) => {
                self.dialog_focus = button;
                Task::none()
            }
            Ask::StartJob(start) => self.start_job(start.job, start.build),
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
            Ask::ClearSelection(side) => {
                self.panel_mut(side).selection.clear();
                Task::none()
            }
            Ask::ReloadSourceThenTarget => {
                let source_side = self.source_side();
                let sides = [source_side, source_side.other()];
                Task::batch(sides.map(|each| {
                    let path = self.panel(each).path.clone();
                    self.load(each, path, None)
                }))
            }
            Ask::ReloadPanelsSelecting { folder, name } => {
                let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
                    let path = self.panel(each).path.clone();
                    let select = (path == folder).then(|| name.clone());
                    self.load(each, path, select)
                });
                Task::batch(tasks)
            }
            Ask::CountJobDone => {
                self.job_done += 1;
                Task::none()
            }
            Ask::ResetJobDone => {
                self.job_done = 0;
                Task::none()
            }
            Ask::LaunchViewCopy { path, origin } => {
                let from = match origin {
                    ncrs_ops::ViewOrigin::Archive => CopyOf::Archive,
                    ncrs_ops::ViewOrigin::Server => CopyOf::Server,
                };
                self.launch_external(fs::OpenKind::View, path, None, from);
                Task::none()
            }
            Ask::ReloadPanels => {
                let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
                    let path = self.panel(each).path.clone();
                    self.load(each, path, None)
                });
                Task::batch(tasks)
            }
            Ask::RestartSearch => self.restart_search(),
            Ask::ListTrash => self.list_trash(),
            Ask::CloseQuickOpenAndPalette => {
                Task::batch([self.close_quick_open(), self.close_command_palette()])
            }
            Ask::OpenTrash(side) => self.open_trash_on(side),
            Ask::OpenTrashInFinder => self.open_trash_in_finder(),
            Ask::ShowErrorWithFinderPress(text) => {
                self.set_status_error_with_press(&text, Message::Trash(TrashMessage::OpenInFinder));
                Task::none()
            }
        }
    }

    // --- the trash ---

    pub(in crate::app) fn update_trash(&mut self, message: TrashMessage) -> Task<Message> {
        let rows = self.trash_rows();
        self.with_ops(|ops, flow| ncrs_ops::update_trash_flow(ops, message, rows, flow))
    }

    /// Rows of the trash view that fit the window.
    pub(in crate::app) fn trash_rows(&self) -> usize {
        layout::trash_visible_rows(self.window_size)
    }

    /// Shows the trash in place of the panel on `side` and reads it. Where the
    /// app cannot read the trash, the Finder opens it instead.
    pub(in crate::app) fn open_trash_on(&mut self, side: PanelSide) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        let reach = self.trash.reach();
        if reach == fs::TrashReach::FinderOnly {
            return self.open_trash_in_finder();
        }
        self.panels.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let closed_search = if self.search.open && self.search.side == side {
            self.update_search(SearchMessage::Close)
        } else {
            Task::none()
        };
        self.panels.active = side;
        let listing = self.ops.show_trash(side, reach, &self.trash);
        Task::batch([closed_search, listing.map(Message::Trash)])
    }

    /// Reads the trash on the blocking pool; the answer carries the generation,
    /// so a late one finds the view closed or read again.
    fn list_trash(&mut self) -> Task<Message> {
        self.ops.list_trash(&self.trash).map(Message::Trash)
    }

    pub(in crate::app) fn trash_panel(&self, side: PanelSide) -> Element<'_, Message> {
        let Some(view) = self.ops.trash_view.as_ref() else {
            return Space::new().into();
        };
        trash_ui::view(trash_ui::Props {
            view,
            visible_rows: self.trash_rows(),
            is_active: self.panels.active == side,
            panel_width: self.window_size.width / 2.0,
            lang: self.lang,
            palette: &self.palette,
            finder_key: self
                .keymap
                .shortcut_label(&KeyAction::OpenTrashInFinder.message()),
        })
        .map(Message::from)
    }

    /// Shows the trash folder in the Finder, where the app cannot list it.
    #[cfg(target_os = "macos")]
    fn open_trash_in_finder(&mut self) -> Task<Message> {
        if let Err(reason) = self.launcher.launch(&fs::trash_in_finder_command()) {
            let text = fill_template(
                self.lang.text(Msg::TrashFinderFailed),
                &[("{reason}", &reason)],
            );
            self.set_status_error(&text);
        }
        Task::none()
    }

    /// Only the Finder has a trash the app cannot list; no other platform
    /// offers the action.
    #[cfg(not(target_os = "macos"))]
    fn open_trash_in_finder(&mut self) -> Task<Message> {
        Task::none()
    }

    pub(in crate::app) fn open_pack_prompt(&mut self) -> Task<Message> {
        let (sources, source_side) = (self.action_sources(), self.source_side());
        self.with_ops_and_panels(|ops, ctx, panels| {
            let facts = selection_facts(panels, source_side, sources);
            ncrs_ops::open_pack_prompt(ops, facts, ctx)
        })
    }

    pub(in crate::app) fn toggle_keep_folders(&mut self) -> Task<Message> {
        self.ops.toggle_keep_folders();
        Task::none()
    }

    /// Stops the running job, see `Message::AbortJob`.
    pub(in crate::app) fn abort_job(&mut self) -> Task<Message> {
        if !self.jobs.abort_running() {
            return Task::none();
        }
        self.drop_progress_channel();
        if self.ops.extracting.is_some() {
            ncrs_ops::end_extraction(&mut self.ops);
            Task::none()
        } else if let Some(outcome) = ncrs_ops::abort_archive_write(&mut self.ops) {
            self.run_ops_outcome(outcome)
        } else if self.ops.deleting.is_some() {
            self.end_delete()
        } else if self.ops.restoring.is_some() {
            self.end_restore()
        } else {
            let outcome = ncrs_ops::end_transfer(&mut self.ops);
            self.run_ops_outcome(outcome)
        }
    }

    /// Both halves of the progress channel go together: a sender kept without
    /// its receiver makes every later transfer fail on a closed channel.
    /// Dropping the receiver is also what stops a blocking copy, whose next
    /// tick cannot be sent.
    pub(in crate::app) fn drop_progress_channel(&mut self) {
        self.job_done = 0;
        self.job_progress = None;
        self.progress_rx = None;
        self.progress_tx = None;
    }

    /// F5 / F6: resolve what to transfer and ask before anything starts.
    ///
    /// The rows are resolved here rather than at the confirmation because a
    /// conflict dialog pauses the operation, and by then the selection may
    /// have moved. With a transfer already running the key brings its progress
    /// back instead: one transfer at a time owns the conflict state.
    pub(in crate::app) fn open_transfer_dialog(&mut self, kind: TransferKind) -> Task<Message> {
        self.open_transfer_dialog_to(kind, None)
    }

    pub(in crate::app) fn open_transfer_dialog_to(
        &mut self,
        kind: TransferKind,
        preset: Option<PathBuf>,
    ) -> Task<Message> {
        let (sources, source_side) = (self.action_sources(), self.source_side());
        self.with_ops_and_panels(|ops, ctx, panels| {
            let facts = selection_facts(panels, source_side, sources);
            ncrs_ops::open_transfer_dialog(ops, kind, preset, facts, ctx)
        })
    }

    pub(in crate::app) fn update_transfer_dialog(
        &mut self,
        message: TransferDialogMessage,
    ) -> Task<Message> {
        self.with_ops(|ops, ctx| ncrs_ops::update_transfer_dialog_flow(ops, message, ctx))
    }

    #[cfg(test)]
    pub(in crate::app) fn prompt_error(&self, err: &CreateDirError) -> String {
        self.ops.prompt_error(self.lang, err)
    }

    pub(in crate::app) fn start_prompt_job(&mut self) -> Task<Message> {
        let active = self.panels.active;
        self.with_ops_and_panels(|ops, ctx, panels| {
            ncrs_ops::start_prompt_job(ops, panel_facts(panels, active), ctx)
        })
    }

    fn progress_sender(&mut self) -> mpsc::UnboundedSender<fs::transfer::Tick> {
        match &self.progress_tx {
            Some(tx) => tx.clone(),
            None => {
                let (created, receiver) = tokio::sync::mpsc::unbounded_channel();
                self.progress_tx = Some(created.clone());
                self.progress_rx = Some(Arc::new(tokio::sync::Mutex::new(receiver)));
                created
            }
        }
    }

    /// The search view is up and its panel is the active one: the keys and the
    /// file actions are the search's. After Tab or a click on the other panel
    /// they are that panel's again, while the search stays on screen.
    /// The trash view is up and its panel is the active one: its keys are
    /// the cursor's.
    pub(in crate::app) fn trash_has_keys(&self) -> bool {
        self.ops.trash_view.is_some() && self.panels.active == self.ops.trash_side
    }

    /// Puts the job into the queue and starts it if nothing runs.
    fn start_job<M>(
        &mut self,
        job: jobs::Job,
        build: impl FnOnce() -> Task<M> + 'static,
    ) -> Task<Message>
    where
        Message: From<M>,
        M: Send + 'static,
    {
        self.jobs.enqueue(job);
        self.jobs
            .start_next(move |_| build().map(Message::from))
            .unwrap_or_else(Task::none)
    }

    /// As `start_job`, for work that sends its progress through the channel,
    /// which is made first.
    pub(in crate::app) fn start_job_with_progress<M>(
        &mut self,
        job: jobs::Job,
        build: impl FnOnce(mpsc::UnboundedSender<fs::transfer::Tick>) -> Task<M> + 'static,
    ) -> Task<Message>
    where
        Message: From<M>,
        M: Send + 'static,
    {
        let progress_tx = self.progress_sender();
        self.jobs.enqueue(job);
        self.jobs
            .start_next(move |_| build(progress_tx).map(Message::from))
            .unwrap_or_else(Task::none)
    }
}
