use crate::app::*;

/// What a list of search hits has no use for: it is not a folder, so nothing is
/// made, pasted or measured in it.
fn is_not_offered_in_hit_list(message: &Message) -> bool {
    matches!(
        message,
        Message::CreateDirPrompt
            | Message::CreateFilePrompt
            | Message::Paste
            | Message::ComputeFolderSizes
            | Message::PathFieldInput(_)
    )
}

/// What a panel inside an archive refuses: everything that would change or
/// hand over the files in it except deleting and copying into it (both
/// rewrite the archive), anything that takes the panel's path or selection
/// into the config or to Hermes, and an archive inside an archive. Viewing is
/// allowed: it works on a copy, see `open_external`.
/// Keys, the command list and the context menu all end up here; the lists of
/// what may run are `allowed_in_archive` on the three action enums.
pub(in crate::app) fn archive_refusal(panel: &PanelState, message: &Message) -> Option<Msg> {
    panel.archive.as_ref()?;
    match message {
        Message::PackPrompt => Some(Msg::StatusPackUnavailable),
        Message::Transfer(TransferKind::Move)
        | Message::OpenExternal(fs::OpenKind::Edit)
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::ComputeFolderSizes
        | Message::OpenSearch
        | Message::AddFavourite
        | Message::RemoveFavourite
        | Message::QuickFix(QuickFixMessage::Open)
        | Message::FilingSetup(FilingSetupMessage::AddPanelFolder(_))
        | Message::HermesFile(
            HermesFileMessage::HandToInbox
            | HermesFileMessage::ProcessAgain
            | HermesFileMessage::ReprocessFolder
            | HermesFileMessage::SetRole(_)
            | HermesFileMessage::ToggleExclusion
            | HermesFileMessage::ShowInInspection,
        )
        | Message::QuickOpen(QuickOpenMessage::Open) => Some(Msg::StatusArchiveReadOnly),
        Message::Panel(PanelMessage::OpenSelected)
            if panel.selected_entry().is_some_and(|entry| {
                !entry.is_dir && !entry.is_parent && fs::archive::is_archive_name(&entry.name)
            }) =>
        {
            Some(Msg::StatusArchiveNested)
        }
        _ => None,
    }
}

/// What a panel on an SFTP server refuses: what would change something there,
/// the command line, which runs on this computer, and whatever would take the
/// folder into the config or to Hermes. Browsing, viewing and copying are what
/// it does; the lists of what may run are `allowed_on_server` on the three
/// action enums.
pub(in crate::app) fn remote_refusal(panel: &PanelState, message: &Message) -> Option<Msg> {
    panel.remote()?;
    match message {
        Message::CommandLine(CommandLineMessage::Focus) => Some(Msg::StatusRemoteNoShell),
        Message::PackPrompt => Some(Msg::StatusPackUnavailable),
        Message::Transfer(TransferKind::Move)
        | Message::OpenExternal(fs::OpenKind::Edit)
        | Message::Delete { .. }
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::ComputeFolderSizes
        | Message::OpenSearch
        | Message::AddFavourite
        | Message::RemoveFavourite
        | Message::QuickFix(QuickFixMessage::Open)
        | Message::FilingSetup(FilingSetupMessage::AddPanelFolder(_))
        | Message::HermesFile(
            HermesFileMessage::HandToInbox
            | HermesFileMessage::ProcessAgain
            | HermesFileMessage::ReprocessFolder
            | HermesFileMessage::SetRole(_)
            | HermesFileMessage::ToggleExclusion
            | HermesFileMessage::ShowInInspection,
        )
        | Message::QuickOpen(QuickOpenMessage::Open) => Some(Msg::StatusRemoteReadOnly),
        _ => None,
    }
}

/// The user asks for something only Hermes can do, as opposed to the answers
/// to what it was asked before.
fn asks_for_hermes(message: &Message) -> bool {
    matches!(
        message,
        Message::HermesFile(
            HermesFileMessage::HandToInbox
                | HermesFileMessage::ProcessAgain
                | HermesFileMessage::ReprocessFolder
                | HermesFileMessage::SetRole(_)
                | HermesFileMessage::ToggleExclusion
                | HermesFileMessage::ShowInInspection
        ) | Message::QuickFix(QuickFixMessage::Open)
            | Message::RuleSuggestions(RuleSuggestionsMessage::Open)
            | Message::Rules(RulesMessage::Open)
            | Message::ReviewList(ReviewListMessage::Open)
            | Message::SavedQueries(SavedQueriesMessage::Open)
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Open)
            | Message::UndoReorganisation
    )
}

/// Other things the user does end the editing of the path or the command line:
/// the text field loses the focus to them without telling the app.
fn leaves_bar_fields(message: &Message) -> bool {
    matches!(
        message,
        Message::Panel(PanelMessage::RowClicked { .. })
            | Message::Panel(PanelMessage::RowActivated { .. })
            | Message::Transfer(_)
            | Message::OpenSearch
            | Message::QuickOpen(QuickOpenMessage::Open)
            | Message::CommandPalette(CommandPaletteMessage::Toggle)
            | Message::KeyBindings(KeyBindingsMessage::Open)
            | Message::ThemeFetch(ThemeFetchMessage::Open)
            | Message::RuleSuggestions(RuleSuggestionsMessage::Open)
            | Message::Rules(RulesMessage::Open)
            | Message::ReviewList(ReviewListMessage::Open)
            | Message::SavedQueries(SavedQueriesMessage::Open)
            | Message::HermesPage(_)
            | Message::ReorganisationPlan(ReorganisationPlanMessage::Open)
            | Message::OcrModels(OcrModelsMessage::Open)
            | Message::About(AboutMessage::Open)
            | Message::AiModel(AiModelMessage::Open)
            | Message::Grant(GrantMessage::Open)
            | Message::Remote(RemoteMessage::OpenConnect)
            | Message::Settings(_)
            | Message::FilingStatus(FilingStatusMessage::Open)
            | Message::FilingSetup(
                FilingSetupMessage::Toggle
                    | FilingSetupMessage::Close
                    | FilingSetupMessage::PickFolder(_)
                    | FilingSetupMessage::AddPanelFolder(_)
                    | FilingSetupMessage::SetTarget(_)
                    | FilingSetupMessage::ToggleImages(_)
                    | FilingSetupMessage::Remove(_)
                    | FilingSetupMessage::Save
            )
            | Message::ContextMenuAt { .. }
            | Message::ContextMenuKey
            | Message::CopyToClipboard(_)
            | Message::Paste
            | Message::CopyFilesToClipboard
            | Message::Panel(PanelMessage::SwitchPanel)
            | Message::Panel(PanelMessage::ActivatePanel(_))
            | Message::Panel(PanelMessage::SortBy { .. })
    )
}

/// The window is the runtime's to move and resize: the id comes from the
/// runtime, since the app opens exactly one window.
fn update_title_bar(message: TitleBarMessage) -> Task<Message> {
    let window = iced::window::latest();
    match message {
        TitleBarMessage::Drag => window.and_then(iced::window::drag),
        TitleBarMessage::ToggleMaximize => window.and_then(iced::window::toggle_maximize),
        TitleBarMessage::Minimize => window.and_then(|id| iced::window::minimize(id, true)),
        TitleBarMessage::Resize(edge) => {
            window.and_then(move |id| iced::window::drag_resize(id, edge.direction()))
        }
    }
}

impl App {
    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_message(message);
        let back = self.return_to_overview_when_its_card_is_gone();
        Task::batch([task, back])
    }

    fn return_to_overview_when_its_card_is_gone(&mut self) -> Task<Message> {
        if !self.hermes.return_to_overview
            || self.hermes.filing_setup.is_some()
            || self.hermes.hermes_setup.is_some()
            || self
                .hermes
                .filing
                .as_ref()
                .is_some_and(FilingModule::has_card)
        {
            return Task::none();
        }
        self.hermes.return_to_overview = false;
        self.open_hermes_overview()
    }

    fn update_message(&mut self, message: Message) -> Task<Message> {
        if let Message::Panel(panel_message) = &message {
            if panel_message.resets_type_ahead() {
                self.panels.type_ahead.reset();
            }
        }
        if let Message::NoticeExpired(generation) = message {
            if generation == self.notice_generation {
                self.notice = None;
            }
            return Task::none();
        }
        if leaves_bar_fields(&message) {
            self.panels.set_path_field(None);
            self.command_line.leave();
        }
        if let Message::SaveSessionState(generation) = message {
            return match self.due_session_state(generation) {
                Some((path, state)) => {
                    let order = Arc::clone(&self.session_writes);
                    Task::future(async move {
                        // A failed write was already reported to stderr.
                        drop(
                            tokio::task::spawn_blocking(move || {
                                write_session_state(&order, generation, &path, &state);
                            })
                            .await,
                        );
                    })
                    .discard()
                }
                None => Task::none(),
            };
        }
        let had_search_keys = self.search_has_keys();
        let applied = self.apply_unless_refused(message);
        let task = Task::batch([
            applied,
            self.with_hermes(Vec::new(), ncrs_hermes_ui::request_hermes_column)
                .map(Message::from),
            self.schedule_session_save(),
        ]);
        // A dialog that opens while a line is being typed gets the keys, not the field.
        if self.command_line.is_focused() && self.command_line_is_blocked() {
            self.command_line.leave();
            self.publish_key_state();
            return Task::batch([task, release_focus()]);
        }
        // The text field keeps typing into the query until it is told to let
        // go, so leaving the search's panel releases it.
        if had_search_keys && !self.search_has_keys() {
            return Task::batch([task, release_focus()]);
        }
        task
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            // A raw key press. Routed here rather than in the subscription,
            // because that one cannot see whether a prompt is open.
            // --- copy / move ---
            Message::Transfer(kind) => self.open_transfer_dialog(kind),
            Message::TransferDialog(dialog_message) => self.update_transfer_dialog(dialog_message),

            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Message::DialogFocus(step) => {
                let buttons = self.dialog_button_count();
                if buttons > 0 {
                    // `buttons` is 2 or 3 and `step` is -1 or 1.
                    self.dialog_focus =
                        (self.dialog_focus + buttons).wrapping_add_signed(step) % buttons;
                }
                Task::none()
            }
            Message::DialogActivate => self.activate_dialog_button(),
            Message::ToggleConflictAll => {
                self.ops.conflict_all = !self.ops.conflict_all;
                Task::none()
            }

            // The user answered a conflict. The queue's pending job carries
            // what was asked, so the answer lands on the right file.
            Message::TransferConflict(choice) => {
                self.with_ops(|ops, ctx| ncrs_ops::answer_conflict(ops, choice, ctx))
            }

            // A tick from the copy on the blocking thread. The only thing the
            // status bar needs, and nothing more.
            Message::JobProgress(tick) => {
                self.job_progress = Some(tick);
                Task::none()
            }

            Message::DismissConfigError => {
                self.config_error = None;
                self.config_problem = None;
                Task::none()
            }

            // Stop the running job. The abort key (Ctrl+C on a Mac unless
            // rebound) and Escape both, because a long copy that cannot be
            // stopped is the case a queue was supposed to fix.
            //
            // A delete in flight finishes its current entry even so: it runs in
            // `spawn_blocking`, which cannot be cancelled, so the panel may
            // still list that entry for a moment after the abort.
            Message::AbortJob => self.abort_job(),
            Message::Trash(trash_message) => self.update_trash(trash_message),
            Message::HitList(hit_message) => self.update_hit_list(hit_message),

            // One row of a transfer finished. Continue with the next, or ask
            // about a name that is in the way.
            Message::TransferRowDone {
                result,
                index,
                generation,
            } => self.with_ops(|ops, ctx| {
                ncrs_ops::transfer_row_done(ops, result, index, generation, ctx)
            }),

            Message::OpenExternal(kind) => self.open_external(kind),

            Message::ViewExtracted {
                generation,
                slot,
                result,
            } => self
                .with_ops(|ops, ctx| ncrs_ops::view_extracted(ops, generation, slot, result, ctx)),

            #[cfg(target_os = "macos")]
            Message::OpenPrivacySettings => {
                self.open_privacy_settings();
                Task::none()
            }

            // --- drive menu ---
            Message::VolumeMenu(side) => {
                let overlay_is_open = self.overlay_is_open();
                let facts = self.volume_facts_source();
                self.panels
                    .open_volume_menu_on(side, false, overlay_is_open, facts);
                Task::none()
            }
            Message::VolumeMenuMove(delta) => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.move_by(delta);
                }
                Task::none()
            }
            Message::VolumeMenuFirst => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.selected = 0;
                }
                Task::none()
            }
            Message::VolumeMenuLast => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.selected = menu.last_row();
                }
                Task::none()
            }
            Message::VolumeMenuRemove => {
                let saved = self.saved_server_ids();
                let actions = self.panels.remove_highlighted_row(&saved);
                self.run_panel_actions(actions)
            }
            Message::AddFavourite => {
                let action = self.panels.add_favourite(self.lang);
                self.run_panel_actions(vec![action])
            }
            Message::RemoveFavourite => {
                let action = self.panels.remove_favourite(self.lang);
                self.run_panel_actions(vec![action])
            }
            Message::OpenFavourites => {
                let overlay_is_open = self.overlay_is_open();
                let facts = self.volume_facts_source();
                let actions = self
                    .panels
                    .open_favourites(self.lang, overlay_is_open, facts);
                self.run_panel_actions(actions)
            }
            Message::VolumeMenuActivate => {
                let actions = self.panels.go_to_selected_volume(self.lang);
                self.run_panel_actions(actions)
            }
            Message::VolumeMenuClick(index) => {
                if let Some(menu) = self.panels.volume_menu.as_mut() {
                    menu.selected = index;
                }
                let actions = self.panels.go_to_selected_volume(self.lang);
                self.run_panel_actions(actions)
            }
            Message::VolumeMenuClose => {
                self.panels.volume_menu = None;
                Task::none()
            }

            // --- context menu ---
            menu_message @ (Message::ContextMenuAt { .. }
            | Message::ContextMenuKey
            | Message::ContextMenuMove(_)
            | Message::ContextMenuFirst
            | Message::ContextMenuLast
            | Message::ContextMenuHover(_)
            | Message::ContextMenuActivate
            | Message::ContextMenuClick(_)
            | Message::ContextMenuOpenSub
            | Message::ContextMenuCloseSub
            | Message::ContextMenuSubHover(_)
            | Message::ContextMenuSubClick(_)
            | Message::ContextMenuClose) => self.update_context_menu(menu_message),

            Message::HermesFile(file_message) => self.update_hermes_file(file_message),

            Message::QuickFix(fix_message) => self.update_quick_fix(fix_message),

            // --- path field ---
            path_message @ (Message::PathFieldInput(_)
            | Message::PathFieldSubmit
            | Message::PathFieldCancel
            | Message::PathChecked { .. }
            | Message::ClickedOutside) => self.update_path_field(path_message),

            Message::CommandLine(line_message) => self.update_command_line(line_message),

            // --- delete ---
            Message::Delete { permanent } => self.open_delete_dialog(permanent),
            Message::CopyToClipboard(kind) => self.copy_to_clipboard(kind),
            Message::Paste => self.paste_from_clipboard(),
            Message::ClipboardRead(content) => self.paste_content(content),
            Message::CopyFilesToClipboard => self.copy_files_to_clipboard(),
            Message::DeleteCancel => {
                self.ops.delete_dialog = None;
                Task::none()
            }
            Message::DeleteConfirm => self.confirm_delete(),
            Message::DeleteRowDone {
                result,
                index,
                generation,
            } => self.with_ops(|ops, ctx| {
                ncrs_ops::delete_row_done(ops, result, index, generation, ctx)
            }),

            // --- job queue ---
            Message::JobFinished(event) => {
                // Progress arrives mid-run; only the terminal events free the
                // slot for the next job.
                let finished = matches!(event, JobEvent::Done);
                if finished {
                    self.jobs.finish();
                }
                if let Some(next) = self.jobs.start_next(run_job) {
                    return next;
                }
                Task::none()
            }

            // --- modal prompt ---
            // Handled before the panels: while a prompt is open every
            // keystroke belongs to it, and Enter must not open a directory.
            Message::CreateDirPrompt => {
                let parent = self.active_panel().path.clone();
                self.open_prompt(Prompt::create_dir(parent))
            }
            Message::CreateFilePrompt => {
                let parent = self.active_panel().path.clone();
                self.open_prompt(Prompt::create_file(parent))
            }
            Message::PackPrompt => self.open_pack_prompt(),
            Message::ArchiveWritten { result, generation } => {
                self.with_ops(|ops, ctx| ncrs_ops::archive_written(ops, generation, result, ctx))
            }
            Message::PromptInput(text) => {
                if let Some(prompt) = self.ops.prompt.as_mut() {
                    prompt.set_name(text);
                }
                Task::none()
            }
            Message::PromptToggleKeepFolders => self.toggle_keep_folders(),
            // Its own arm, not an early return: starting the job changes what the
            // keys mean, and the routing state is published after the match.
            Message::PromptSubmit if self.ops.valid_prompt_has_its_own_job() => {
                self.start_prompt_job()
            }
            Message::PromptSubmit => {
                let Some(prompt) = self.ops.prompt.as_ref() else {
                    return Task::none();
                };
                if let Err(reason) = prompt.validate() {
                    self.ops.set_prompt_error(reason);
                    return Task::none();
                }
                let kind = prompt.kind();
                let side = self.ops.prompt_side.unwrap_or(self.panels.active);
                let parent = self.panel(side).path.clone();
                let new_name = prompt.name().trim().to_string();
                let request_id = self.ops.prompt_request_id.wrapping_add(1);
                self.ops.prompt_request_id = request_id;
                self.ops.set_prompt_busy(true);

                // The new entry's name, carried through the task so the
                // reload can select it.
                let target = parent.join(&new_name);
                let finished = move |result| Message::PromptFinished {
                    prompt: kind,
                    request_id,
                    result,
                };
                let create = match kind {
                    PromptKind::CreateDir => Task::perform(fs::create_dir(target), finished),
                    PromptKind::CreateFile => Task::perform(fs::create_file(target), finished),
                    // Taken out above: a pack and a paste have their own jobs.
                    PromptKind::Pack | PromptKind::Paste => Task::none(),
                };

                // The reload belongs *after* the create, not beside it.
                // Running both at once races: a read that finishes first sees a
                // directory that does not contain the new entry yet, and the
                // selection then points at whatever is there instead.
                self.panels.pending_reload = Some(PendingReload {
                    side,
                    name: new_name,
                });
                create
            }
            Message::PromptCancel => {
                self.ops.prompt = None;
                self.ops.pending_paste = None;
                self.ops.prompt_side = None;
                Task::none()
            }
            Message::PromptFinished {
                prompt: kind,
                request_id,
                result,
            } => self.finish_prompt(kind, request_id, result),

            Message::Panel(panel_message) => self.update_panel(panel_message),
            Message::TitleBar(title_bar) => update_title_bar(title_bar),
            Message::ComputeFolderSizes => {
                let walk = self.panels.start_folder_size_run();
                folder_size_task(walk)
            }
            Message::FolderSizeMeasured { run, folder, size } => {
                let walk = self
                    .panels
                    .folder_size_measured(run, folder, size, self.visible_rows);
                folder_size_task(walk)
            }
            Message::SwitchLanguage => {
                self.switch_language();
                Task::none()
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Message::DragOutStarted(started) => self.drag_out_started(started),

            Message::WindowResized(size) => {
                self.apply_window_size(size);
                Task::none()
            }
            Message::WindowFocused => self.reload_local_panels(),
            Message::FilesHovered => {
                self.ops.drop_hover = true;
                Task::none()
            }
            Message::FilesHoverLeft => {
                self.ops.drop_hover = false;
                Task::none()
            }
            Message::FileDropped(path) => {
                self.ops.queue_dropped_file(path).map(Message::DropSettled)
            }
            Message::DropSettled(generation) => match self.ops.settled_drop(generation) {
                Some(paths) => self.copy_external_files_into_active_panel(paths),
                None => Task::none(),
            },
            Message::OpenSearch => self.open_search(),
            Message::Search(search) => self.update_search(search),
            Message::QuickOpen(quick_open) => self.update_quick_open(quick_open),
            Message::CommandPalette(command_palette) => {
                self.update_command_palette(command_palette)
            }
            Message::KeyBindings(shortcuts) => self.update_key_bindings(shortcuts),
            Message::ThemeFetch(fetch_message) => self.update_theme_fetch(fetch_message),
            Message::RuleSuggestions(suggestions) => self.update_rule_suggestions(suggestions),
            Message::Rules(rules) => self.update_rules(rules),
            Message::ReviewList(review) => {
                let update = self.update_review_list(review);
                Task::batch([update, self.request_review_preview()])
            }
            Message::SavedQueries(queries) => self.update_saved_queries(queries),
            Message::Tags(tags) => self.update_tags(tags),
            Message::HermesPage(page) => self.show_hermes_page(page),
            Message::HermesFocus(focus) => self.focus_hermes(focus),
            Message::HermesStep(step) => self.step_hermes_page(step),
            Message::ReorganisationPlan(plan) => self.update_reorganisation(plan),
            // #73 hangs the executor here; until then the selection goes nowhere.
            Message::ExecuteReorganisation(selection) => self.run_reorganisation(selection),
            Message::ReorganisationExecuted(result) => self.reorganisation_executed(result),
            Message::UndoReorganisation => self.undo_reorganisation(),
            Message::DocumentsFiled(result) => self.documents_filed(result),
            Message::DocumentsRefiled(result) => self.documents_refiled(result),
            Message::ReorganisationUndone(result) => self.reorganisation_undone(result),
            Message::About(about_message) => self.update_about(about_message),
            Message::OcrModels(models_message) => self.update_ocr_models(models_message),
            Message::AiModel(model_message) => self.update_ai_model(model_message),
            Message::Grant(grant_message) => self.update_grant(grant_message),
            Message::Remote(remote_message) => {
                let outcome = self.with_connect_feature(|remote, ctx| {
                    ncrs_remote::update(remote, remote_message, ctx)
                });
                self.run_remote_outcome(outcome)
            }
            Message::Settings(settings_message) => self.update_settings(settings_message),
            Message::Filing(filing_message) => self.update_filing(filing_message),
            Message::FilingStatus(page_message) => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::update_filing_status(hermes, page_message, ctx)
                }),
            Message::HermesColumn(column_message) => self
                .run_hermes_flow(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::update_hermes_column(hermes, column_message, ctx)
                }),
            Message::FilingSetup(setup_message) => self.update_filing_setup(setup_message),
            Message::HermesSetup(setup_message) => self.update_hermes_setup(setup_message),
            Message::HermesOverview(overview_message) => {
                self.update_hermes_overview(overview_message)
            }
            Message::HermesFlowTick => {
                self.hermes_flow_step = self.hermes_flow_step.wrapping_add(1);
                Task::none()
            }
            #[cfg(any(target_os = "macos", test))]
            Message::RunCommand(action) => self.run_command(action),
            #[cfg(target_os = "macos")]
            Message::InstallNativeMenu => {
                native_menu::install(self.lang, &self.keymap, self.menu_availability());
                Task::none()
            }
            Message::Quit => self.quit(),
            // Handled in `update`, which owns the timer's generation check.
            Message::NoticeExpired(_) | Message::SaveSessionState(_) => Task::none(),
        };

        // The keyboard subscription cannot see `&self`, so it reads the routing
        // state from a cell this method keeps current.
        //
        // This runs *after* the match, and that ordering is the point. Published
        // before it, the cell would always describe the state from before the
        // message that has just been handled — one step behind — so the keys
        // that follow a conflict would still be routed as panel keys and the
        // dialog could not be answered at all.
        self.publish_key_state();
        #[cfg(target_os = "macos")]
        native_menu::sync(self.lang, &self.keymap, self.menu_availability());
        task
    }

    /// A panel inside an archive can be read and copied from, nothing else.
    pub(in crate::app) fn apply_unless_refused(&mut self, message: Message) -> Task<Message> {
        if asks_for_hermes(&message) {
            if self.hermes.is_restarting() {
                let text = self.lang.text(Msg::StatusHermesRestarting).to_owned();
                return self.show_notice(text);
            }
            if self.hermes.is_off() {
                return self.explain_hermes_is_off();
            }
        }
        let panel = self.panel(self.source_side());
        if panel.is_hit_list() && is_not_offered_in_hit_list(&message) {
            return Task::none();
        }
        match archive_refusal(panel, &message).or_else(|| remote_refusal(panel, &message)) {
            Some(refusal) => {
                let text = self.lang.text(refusal).to_string();
                self.set_status_error(&text);
                Task::none()
            }
            None => self.apply(message),
        }
    }
}
