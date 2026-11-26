use crate::app::*;
use crate::messages::MenuMessage;

/// Where a shell command of this panel runs. A path inside an archive is no
/// directory a process can start in, so it runs in the folder the archive
/// lies in.
pub(in crate::app) fn shell_directory(panel: &PanelState) -> PathBuf {
    panel
        .archive
        .as_deref()
        .and_then(archive_file_place)
        .map_or_else(|| panel.path.clone(), |(folder, _)| folder.to_path_buf())
}

/// The folder an archive lies in and its file name.
pub(in crate::app) fn archive_file_place(archive: &Path) -> Option<(&Path, String)> {
    Some((archive.parent()?, archive.file_name()?.to_str()?.to_owned()))
}

pub(in crate::app) fn folder_size_task(
    walk: Option<ncrs_files::folder_size::FolderWalk>,
) -> Task<Message> {
    ncrs_files::folder_size::walk_task(walk).map(Message::Panel)
}

pub(in crate::app) fn panel_facts(panels: &Panels, side: PanelSide) -> ncrs_ops::PanelFacts<'_> {
    let panel = panels.panel(side);
    ncrs_ops::PanelFacts {
        side,
        path: &panel.path,
        is_virtual: panel.is_virtual(),
        hit_list: panel.hit_list.as_ref().map(|list| ncrs_ops::HitListFacts {
            scope: &list.scope,
            archive_name: ncrs_files::hit_list::hit_archive_name(&list.query),
        }),
        entries: &panel.entries,
    }
}

pub(in crate::app) fn selection_facts(
    panels: &Panels,
    source_side: PanelSide,
    sources: Vec<PathBuf>,
) -> ncrs_ops::SelectionFacts<'_> {
    ncrs_ops::SelectionFacts {
        source: panel_facts(panels, source_side),
        other: panel_facts(panels, source_side.other()),
        sources,
    }
}

pub(in crate::app) fn rows_of(panel: &PanelState) -> PanelRows<'_> {
    PanelRows {
        path: &panel.path,
        entries: &panel.entries,
        selection: &panel.selection,
        cursor: panel.selected_entry(),
    }
}

impl App {
    /// Both panels read their folder again and keep the cursor on its file.
    pub(in crate::app) fn reload_panels(&mut self) -> Task<Message> {
        let tasks =
            [PanelSide::Left, PanelSide::Right].map(|each| self.reload_keeping_cursor(each));
        Task::batch(tasks)
    }

    pub(in crate::app) fn reload_keeping_cursor(&mut self, side: PanelSide) -> Task<Message> {
        let panel = self.panel(side);
        let path = panel.path.clone();
        let cursor = panel.selected_entry().map(|entry| entry.name.clone());
        self.load(side, path, cursor)
    }

    pub(in crate::app) fn update_path_field(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ClickedOutside => {
                self.close_context_menu();
                self.panels.set_path_field(None);
                self.panels.end_rename();
                self.command_line.leave();
                self.monitor_clicked_away()
            }
            _ => Task::none(),
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub fn panel(&self, side: PanelSide) -> &PanelState {
        self.panels.panel(side)
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelState {
        self.panels.panel_mut(side)
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panels.active_panel()
    }

    /// The classic rule for F5, F6 and F8: tagged rows win, otherwise the row under
    /// the cursor. The `..` entry is never a thing to act on. In the search
    /// view the same holds for its files, and never for all files found.
    pub(in crate::app) fn action_sources(&self) -> Vec<PathBuf> {
        if let Some(search) = self.open_search_view() {
            return search.action_files();
        }
        self.action_rows()
            .into_iter()
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// The rows of the active panel the F5/F6/F8 rule acts on.
    pub(in crate::app) fn action_rows(&self) -> Vec<&fs::FileEntry> {
        self.active_panel().action_entries()
    }

    /// Puts the path or name of the target on the clipboard and says so in the
    /// status bar. Nothing happens under an overlay or with nothing to copy.
    pub(in crate::app) fn copy_to_clipboard(&mut self, kind: ClipboardKind) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let from_results = self
            .open_search_view()
            .filter(|_| kind != ClipboardKind::Directory)
            .map(|search| ncrs_ops::clipboard_text_of_files(&search.action_files(), kind));
        let Some(copied) = from_results
            .unwrap_or_else(|| ncrs_ops::clipboard_text(rows_of(self.active_panel()), kind))
        else {
            return Task::none();
        };
        self.job_error = None;
        let shown = self.show_notice(ncrs_ops::copied_notice(self.lang, kind, copied.count));
        Task::batch([iced::clipboard::write(copied.text), shown])
    }

    fn open_ctx(&self) -> ncrs_files::open_flow::OpenCtx<'_> {
        let label = |opener| self.keymap.shortcut_label(&Message::OpenExternal(opener));
        ncrs_files::open_flow::OpenCtx {
            lang: self.lang,
            programs: &self.open_programs,
            launcher: self.launcher.as_ref(),
            view_key: label(fs::OpenKind::View),
            edit_key: label(fs::OpenKind::Edit),
        }
    }

    /// F3 / F4: the row under the cursor goes to the external program. Tags
    /// are ignored, as in classic dual-panel file managers.
    pub(in crate::app) fn open_external(&mut self, kind: fs::OpenKind) -> Task<Message> {
        let source = match self.open_search_view() {
            Some(search) => {
                OpenSource::Search(search.cursor_target().filter(|(path, _)| !path.is_dir()))
            }
            None => OpenSource::ActivePanel,
        };
        let actions =
            ncrs_files::open_flow::open_external(&self.panels, kind, source, &self.open_ctx());
        self.run_panel_actions(actions)
    }

    /// Enter or a double click on a file.
    pub(in crate::app) fn open_like_the_finder(&mut self) -> Task<Message> {
        let actions = ncrs_files::open_flow::open_like_the_finder(&self.panels, &self.open_ctx());
        self.run_panel_actions(actions)
    }

    /// "Open terminal here".
    pub(in crate::app) fn open_terminal(&mut self) -> Task<Message> {
        let actions = ncrs_files::open_flow::open_terminal(
            &self.panels,
            self.open_search_view().is_some(),
            &self.open_ctx(),
        );
        self.run_panel_actions(actions)
    }

    /// "Open terminal here" for `folder`, whatever the panels show.
    pub(in crate::app) fn open_terminal_in(&mut self, folder: &Path) -> Task<Message> {
        let actions = ncrs_files::open_flow::open_terminal_in(folder, &self.open_ctx());
        self.run_panel_actions(actions)
    }

    /// Puts `path` on the clipboard the way "Copy path" does in the panels.
    pub(in crate::app) fn copy_path_of(&mut self, path: &Path) -> Task<Message> {
        let Some(copied) =
            ncrs_ops::clipboard_text_of_files(&[path.to_path_buf()], ClipboardKind::Path)
        else {
            return Task::none();
        };
        self.job_error = None;
        let shown = self.show_notice(ncrs_ops::copied_notice(
            self.lang,
            ClipboardKind::Path,
            copied.count,
        ));
        Task::batch([iced::clipboard::write(copied.text), shown])
    }

    /// Hands `path` to the system's own opener, never to a configured program.
    pub(in crate::app) fn open_with_system(&mut self, path: &Path) {
        let actions = ncrs_files::open_flow::open_with_system(path, &self.open_ctx());
        self.apply_message_actions(actions);
    }

    /// Hands `path` to the program for `kind`; `was_executable` says that the
    /// file `path` is a copy of could run, which the copy no longer shows.
    pub(in crate::app) fn launch_external(
        &mut self,
        kind: fs::OpenKind,
        path: PathBuf,
        line: Option<u64>,
        from: CopyOf,
        was_executable: bool,
    ) {
        let executable = was_executable || fs::could_execute(&path);
        let actions = ncrs_files::open_flow::launch_external(
            kind,
            path,
            line,
            from,
            executable,
            &self.open_ctx(),
        );
        self.apply_message_actions(actions);
    }

    /// For flows that only clear or set the status line, where the caller has
    /// no task to hand on.
    fn apply_message_actions(&mut self, actions: Vec<ncrs_files::Action>) {
        drop(self.run_panel_actions(actions));
    }

    /// Starts unpacking `source`, a file in `archive`, for the viewer. The
    /// viewer starts when the copy is there, see `view_extracted`.
    fn view_from_archive(&mut self, source: PathBuf, archive: PathBuf) -> Task<Message> {
        let shown = source.clone();
        self.with_ops(move |ops, ctx| {
            ncrs_ops::start_view_copy(
                ops,
                archive,
                shown,
                jobs::JobKind::Extract,
                move |slot, tx| {
                    let mut progress = fs::transfer::Reporting::new(tx);
                    fs::archive::extract_for_view(&source, slot, &mut progress)
                },
                ctx,
            )
        })
    }

    /// Starts fetching `source`, a file on a server, for the viewer. The copies
    /// of one server are kept together under its root address, as those of an
    /// archive are under the archive.
    fn view_from_server(&mut self, source: RemotePath) -> Task<Message> {
        let pool = Arc::clone(&self.remote);
        let (key, shown) = (source.server().root_path(), source.to_path_buf());
        self.with_ops(move |ops, ctx| {
            ncrs_ops::start_view_copy(
                ops,
                key,
                shown,
                jobs::JobKind::Copy,
                move |slot, tx| {
                    let mut progress = fs::transfer::Reporting::new(tx);
                    fs::remote::transfer::download_for_view(&pool, &source, slot, &mut progress)
                        .map(|path| fs::archive::ViewCopy {
                            path,
                            was_executable: false,
                        })
                },
                ctx,
            )
        })
    }

    pub(in crate::app) fn reload_local_panels(&mut self) -> Task<Message> {
        let folders = self.panels.local_folders();
        Task::batch(
            folders
                .into_iter()
                .map(|(side, path)| self.load(side, path, None)),
        )
    }

    /// A panel has left `archive`: the copies made for the viewer go, unless
    /// the other panel is still in it. A running unpacking stops first.
    fn drop_view_copies_of(&mut self, archive: &Path) {
        if self.panels.shows(archive) {
            return;
        }
        if self
            .ops
            .extracting
            .as_ref()
            .is_some_and(|extracting| extracting.archive == archive)
        {
            self.jobs.abort_running();
            self.drop_progress_channel();
            ncrs_ops::end_extraction(&mut self.ops);
        }
        if let Some(copies) = self.ops.view_copies.as_mut() {
            copies.release(archive);
        }
    }

    pub(in crate::app) fn copy_external_files_into_active_panel(
        &mut self,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        let active = self.panels.active;
        self.with_ops_and_panels(|ops, ctx, panels| {
            ncrs_ops::copy_external_files(ops, paths, panel_facts(panels, active), ctx)
        })
    }

    /// Cmd+V / Ctrl+V: reads the system clipboard, then `paste_content` decides.
    pub(in crate::app) fn paste_from_clipboard(&mut self) -> Task<Message> {
        if self
            .ops
            .transfer_into_panel_is_blocked(self.ops.transfer_dialog.is_some())
        {
            let text = self.lang.text(Msg::StatusPasteBusy).to_owned();
            return self.show_notice(text);
        }
        (self.read_clipboard)().map(Message::ClipboardRead)
    }

    pub(in crate::app) fn paste_content(
        &mut self,
        content: Option<PastedContent>,
    ) -> Task<Message> {
        let active = self.panels.active;
        self.with_ops_and_panels(|ops, ctx, panels| {
            ncrs_ops::paste_content(ops, content, panel_facts(panels, active), ctx)
        })
    }

    /// Files from another app held over the window outline the active panel;
    /// a row dragged over the other panel outlines that one.
    pub(in crate::app) fn is_drop_target(&self, side: PanelSide) -> bool {
        (self.ops.drop_hover && side == self.panels.active) || self.panels.drag_hovers_side(side)
    }

    /// A cursor only where a drag from a panel is over a panel; files held over
    /// the window from another app keep the OS cursor.
    pub(in crate::app) fn drag_cursor(&self, side: PanelSide) -> Option<mouse::Interaction> {
        self.panels.drag_cursor(side, self.modifiers)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(in crate::app) fn drag_out_started(
        &mut self,
        started: Result<(), String>,
    ) -> Task<Message> {
        let failure = ncrs_ops::drag_out::drag_out_failure(self.lang, started);
        self.run_ops_actions(failure.into_iter().collect())
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn end_panel_drag(&mut self) -> Option<crate::panel_drag::PanelDrag> {
        self.input.lock().panel_drag_armed = false;
        self.panels.drag.take()
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn start_drag_out(&mut self) -> Task<Message> {
        self.end_panel_drag();
        let sources = self.action_sources();
        let all_on_local_disk = sources
            .iter()
            .all(|path| ncrs_files::panel::is_file_on_local_disk(path));
        let (started, actions) = ncrs_ops::drag_out::start_drag_out(
            self.lang,
            sources,
            all_on_local_disk,
            ncrs_ops::drag_out::DragIcon {
                size: crate::WINDOW_ICON_SIZE,
                rgba: crate::WINDOW_ICON_RGBA,
            },
        );
        Task::batch([
            started.map(Message::DragOutStarted),
            self.run_ops_actions(actions),
        ])
    }

    pub(in crate::app) fn copy_files_to_clipboard(&mut self) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let sources = self.action_sources();
        let all_on_local_disk = sources
            .iter()
            .all(|path| ncrs_files::panel::is_file_on_local_disk(path));
        let actions = ncrs_ops::copy_files_to_clipboard(
            self.lang,
            &sources,
            all_on_local_disk,
            &mut *self.write_clipboard_files,
        );
        self.run_ops_actions(actions)
    }

    /// The panel that file actions take their files from: the one the search
    /// view stands in for, else the active one.
    pub(in crate::app) fn source_side(&self) -> PanelSide {
        if self.search_has_keys() {
            self.search.side
        } else if self.trash_has_keys() {
            self.ops.trash_side
        } else {
            self.panels.active
        }
    }

    pub(in crate::app) fn update_panel(&mut self, message: PanelMessage) -> Task<Message> {
        let rename_has_no_row =
            self.blocking_overlay_is_open() || self.search_has_keys() || self.trash_has_keys();
        if message == PanelMessage::RenameStart && rename_has_no_row {
            return Task::none();
        }
        if message == PanelMessage::RenameStart {
            let tagged = self.tagged_entries_to_rename();
            if tagged.len() >= 2 && self.active_panel().hit_list.is_some() {
                // The names of the hits are resolved against one folder, which
                // is not where each of them lives.
                let text = self.lang.text(Msg::StatusHitListBatchRename).to_owned();
                return self.show_notice(text);
            }
            if tagged.len() >= 2 {
                return self.open_batch_rename_dialog(tagged);
            }
        }
        let overlay_is_open = self.overlay_is_open();
        let context_menu_is_open = self.panels.context_menu.is_some();
        let pointer = self.input.lock().pointer;
        let saved_servers = self.saved_server_ids();
        let remote = &self.remote;
        let session_home =
            |server: &fs::remote::ServerId| remote.session(server).map(|s| s.home().to_owned());
        let ctx = ncrs_files::Ctx {
            home: &self.home,
            session_home: &session_home,
            columns: column_settings_of(&self.columns),
            has_hermes_column: self.hermes.is_active()
                || self.compare.marks(PanelSide::Left).is_some(),
            visible_rows: self.visible_rows,
            window_size: self.window_size,
            modifiers: self.modifiers,
            is_context_click: ncrs_files::ctx::is_context_click(self.modifiers),
            pointer,
            overlay_is_open,
            context_menu_is_open,
            clock: &*self.clock,
            nearest_existing_directory: session_state::nearest_existing_directory,
            lang: self.lang,
            saved_servers: &saved_servers,
        };
        let roots_before = self.panels_git_roots();
        let (task, actions) = ncrs_files::update(&mut self.panels, message, &ctx);
        let mapped = task.map(Message::Panel);
        let acted = self.run_panel_actions(actions);
        let learned: Vec<Task<Message>> = self
            .panels_git_roots()
            .into_iter()
            .zip(roots_before)
            .filter(|(now, before)| now != before)
            .map(|(now, _)| self.learn_repo_of_panel(now))
            .collect();
        Task::batch([mapped, acted, Task::batch(learned)])
    }

    /// The repository each panel shows, left then right.
    fn panels_git_roots(&self) -> [Option<PathBuf>; 2] {
        [
            self.panels.left.git.root.clone(),
            self.panels.right.git.root.clone(),
        ]
    }

    pub(in crate::app) fn run_panel_actions(
        &mut self,
        actions: Vec<ncrs_files::Action>,
    ) -> Task<Message> {
        let acted: Vec<_> = actions
            .into_iter()
            .map(|action| self.run_panel_action(action))
            .collect();
        Task::batch(acted)
    }

    /// Alt+F1 / Alt+F2 and the favourites key: reads what the menu lists on the
    /// blocking pool, since listing the drives and looking at each favourite
    /// touch the disks. The answer opens the menu.
    pub(in crate::app) fn request_volume_menu(
        &mut self,
        side: PanelSide,
        on_favourites: bool,
    ) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let request = self.panels.next_volume_menu_request();
        let read = self.volume_facts_source();
        Task::perform(tokio::task::spawn_blocking(read), move |facts| {
            Message::Files(MenuMessage::VolumeMenuFacts {
                request,
                side,
                on_favourites,
                facts: facts.unwrap_or_default(),
            })
        })
    }

    /// What the drive menu lists.
    fn volume_facts_source(&self) -> impl FnOnce() -> ncrs_files::VolumeFacts + Send + 'static {
        let list_volumes = Arc::clone(&self.list_volumes);
        let remote = Arc::clone(&self.remote);
        let trash = Arc::clone(&self.trash);
        let saved_servers: Vec<ServerRow> = self
            .saved_servers
            .iter()
            .map(|entry| ServerRow {
                label: entry.label(),
                url: entry.url(),
                server: entry.server_id(),
                connected: false,
            })
            .collect();
        let favourites = self.panels.favourites.clone();
        move || ncrs_files::VolumeFacts {
            volumes: list_volumes(),
            saved_servers,
            connected: remote.connected(),
            trash: ncrs_ops::trash_menu_row(trash.reach()),
            favourites: favourites
                .into_iter()
                .map(|path| ncrs_files::volumes_view::FavouriteRow {
                    present: path.is_dir(),
                    path,
                })
                .collect(),
        }
    }

    pub(in crate::app) fn saved_server_ids(&self) -> Vec<fs::remote::ServerId> {
        self.saved_servers
            .iter()
            .map(ServerEntry::server_id)
            .collect()
    }

    /// What the panels asked for because it reaches outside the panels.
    fn run_panel_action(&mut self, action: ncrs_files::Action) -> Task<Message> {
        use ncrs_files::Action;
        match action {
            Action::Load { side, path, select } => self.load(side, path, select),
            Action::MarkHistoryStep(side) => {
                self.panel_mut(side).mark_history_step();
                Task::none()
            }
            Action::ClearMessages => {
                self.job_error = None;
                self.notice = None;
                Task::none()
            }
            Action::OpenContextMenu {
                side,
                index,
                pointer,
            } => {
                self.open_context_menu(side, index, Some(pointer));
                Task::none()
            }
            Action::ContextMenuAt { side, index } => {
                self.update(Message::Files(MenuMessage::ContextMenuAt { side, index }))
            }
            Action::CloseContextMenu => {
                self.close_context_menu();
                Task::none()
            }
            Action::RunContextEntry(ContextAction::AddFavourite) => {
                let Some(target) = self.panels.local_folder_target() else {
                    return Task::none();
                };
                let added = self.panels.add_favourite_at(self.lang, target);
                self.run_panel_actions(vec![added])
            }
            Action::RunContextEntry(ContextAction::RemoveFavourite) => {
                let Some(target) = self.panels.local_folder_target() else {
                    return Task::none();
                };
                let removed = self.panels.remove_favourite_at(self.lang, &target);
                self.run_panel_actions(vec![removed])
            }
            Action::SubmitRename {
                side,
                from,
                new_name,
            } => self.with_ops(|ops, ctx| ncrs_ops::start_rename(ops, side, from, new_name, ctx)),
            Action::RunContextEntry(entry) => self.update(entry.message()),
            Action::RunHermesEntry(entry) => self.update(entry.message()),
            Action::ActivateSearchResult => self.update(Message::Search(SearchMessage::Activate)),
            Action::CloseMenuOfReloaded(side) => {
                self.panels.close_menu_of_reloaded(side);
                Task::none()
            }
            Action::DropViewCopies(place) => {
                self.drop_view_copies_of(&place);
                Task::none()
            }
            Action::StopFolderSizeRunIfLeft(side) => {
                self.panels.stop_folder_size_run_if_left(side);
                Task::none()
            }
            Action::DragArmed(armed) => {
                self.input.lock().panel_drag_armed = armed;
                Task::none()
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Action::StartDragOut => self.start_drag_out(),
            Action::OpenTransferDialog(kind) => self.open_transfer_dialog_to(kind, None),
            Action::OpenSelected => self.update(Message::Panel(PanelMessage::OpenSelected)),
            Action::OpenFile => self.open_like_the_finder(),
            Action::OpenBundle(path) => {
                self.with_ops(|ops, _| ncrs_ops::open_open_file_dialog(ops, path))
            }
            Action::ViewFromServer(remote) => self.view_from_server(remote),
            Action::ViewFromArchive { source, archive } => self.view_from_archive(source, archive),
            Action::OpenInViewer(path) => self.open_in_viewer(path),
            Action::ShowPanels => {
                self.shown_tab = ShownTab::Panels;
                Task::none()
            }
            Action::ReleaseFocus => release_focus(),
            Action::GoToPathAnswer(answer) => {
                let asks =
                    self.ops.prompt.as_ref().is_some_and(|prompt| {
                        prompt.kind() == ncrs_ops::prompt::PromptKind::GoToPath
                    });
                match answer {
                    Ok(()) if asks => {
                        self.ops.prompt = None;
                        release_focus()
                    }
                    Err(text) if asks => {
                        self.ops.set_prompt_error_text(&text);
                        Task::none()
                    }
                    Ok(()) | Err(_) => Task::none(),
                }
            }
            Action::LeaveCommandLine => {
                self.command_line.leave();
                Task::none()
            }
            Action::OpenConnectDialog(url) => {
                let outcome = self.with_connect_feature(|remote, ctx| {
                    ncrs_remote::open_connect_dialog_for(remote, ctx, &url)
                });
                self.run_remote_outcome(outcome)
            }
            Action::ShowNotice(text) => self.show_notice(text),
            Action::ShowError(text) => {
                self.set_status_error(&text);
                Task::none()
            }
            Action::OpenTrash(side) => self.open_trash_on(side),
            Action::Disconnect(server) => {
                Task::done(Message::Remote(RemoteMessage::Disconnect(server)))
            }
            Action::ColumnsChanged { side, columns } => self.panel_columns_changed(side, columns),
            Action::ColumnWidth { column, width } => self.panel_column_resized(column, width),
            Action::SameColumnsInBoth(same) => self.panel_columns_same_in_both(same),
            Action::RestoreColumnDefaults(side) => self.panel_column_defaults_restored(side),
            Action::OpenColumnSettings => self.open_column_settings(),
        }
    }

    /// The active panel shows the file's directory with the cursor on the file.
    /// Only navigates: the file is never opened or run.
    pub(in crate::app) fn reveal_file(&mut self, path: &Path) -> Task<Message> {
        let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
            return Task::none();
        };
        let directory = parent.to_path_buf();
        let name = file.to_os_string();
        let left = self.leave_settings_page();
        let shown = self.show_folder_in_active_panel(directory, Some(name));
        Task::batch([left, shown])
    }

    /// Switches to the panels and shows the file's directory in the active one, cursor on the file.
    pub(in crate::app) fn reveal_in_active_panel(&mut self, file: &Path) -> Task<Message> {
        let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = file.file_name().map(OsStr::to_os_string);
        self.shown_tab = ShownTab::Panels;
        self.show_folder_in_active_panel(folder, name)
    }

    /// Starts an async directory read for `side`.
    pub(in crate::app) fn load(
        &mut self,
        side: PanelSide,
        path: PathBuf,
        select: Option<OsString>,
    ) -> Task<Message> {
        match self.panels.start_load(side, path, select.clone()) {
            LoadStart::RefreshHitList => self.refresh_hit_list(side),
            LoadStart::Remote(remote) => self.load_remote(side, remote, select),
            LoadStart::Local(read) => read.map(Message::Panel),
        }
    }

    /// Opens a folder on a server in `side`. With a connection the listing is
    /// read; without one the keychain is asked for the password first, so a
    /// server whose password was remembered reconnects without a question.
    fn load_remote(
        &mut self,
        side: PanelSide,
        remote: RemotePath,
        select: Option<OsString>,
    ) -> Task<Message> {
        let request_id = self.panel_mut(side).begin_load();
        let outcome = self.with_connect_feature(|_, ctx| {
            ncrs_remote::load_folder(ctx, (side, request_id, remote, select))
        });
        self.run_remote_outcome(outcome)
    }
}
