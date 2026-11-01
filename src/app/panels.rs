use crate::app::*;

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

/// The walk of one folder, off the UI thread; nothing when the run is over.
pub(in crate::app) fn folder_size_task(
    walk: Option<ncrs_files::folder_size::FolderWalk>,
) -> Task<Message> {
    let Some(ncrs_files::folder_size::FolderWalk { run, folder, stop }) = walk else {
        return Task::none();
    };
    let walked = folder.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || fs::measure_folder(&walked, &stop)),
        move |done| Message::FolderSizeMeasured {
            run,
            folder: folder.clone(),
            size: done.ok().flatten(),
        },
    )
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
        let tasks = [PanelSide::Left, PanelSide::Right].map(|each| {
            let panel = self.panel(each);
            let path = panel.path.clone();
            let cursor = panel
                .selected_entry()
                .map(|entry| entry.name.to_string_lossy().into_owned());
            self.load(each, path, cursor)
        });
        Task::batch(tasks)
    }

    pub(in crate::app) fn update_path_field(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PathFieldInput(typed) => {
                self.command_line.leave();
                self.panels.set_path_field(Some(typed));
                Task::none()
            }
            Message::PathFieldSubmit => {
                let remote = &self.remote;
                let session_home = |server: &fs::remote::ServerId| {
                    remote.session(server).map(|s| s.home().to_owned())
                };
                let ctx = ncrs_files::path_field::TypedPathCtx {
                    lang: self.lang,
                    home: &self.home,
                    session_home: &session_home,
                };
                let (check, actions) = self.panels.go_to_typed_path(&ctx);
                let located = check.map(|checked| Message::PathChecked {
                    request_id: checked.request_id,
                    side: checked.side,
                    target: checked.target,
                    result: checked.result,
                });
                Task::batch([located, self.run_panel_actions(actions)])
            }
            Message::PathChecked {
                request_id,
                side,
                target,
                result,
            } => {
                let checked = ncrs_files::path_field::PathChecked {
                    request_id,
                    side,
                    target,
                    result,
                };
                let actions = self.panels.finish_path_check(self.lang, checked);
                self.run_panel_actions(actions)
            }
            Message::PathFieldCancel => {
                self.panels.set_path_field(None);
                release_focus()
            }
            Message::ClickedOutside => {
                self.panels.context_menu = None;
                self.panels.set_path_field(None);
                self.command_line.leave();
                Task::none()
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

    #[allow(dead_code)] // Used by upcoming two-panel operations (copy/move).
    pub fn inactive_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.panels.active.other())
    }

    /// NC's rule for F5, F6 and F8: tagged rows win, otherwise the row under
    /// the cursor. The `..` entry is never a thing to act on. In the search
    /// view the same holds for its files, and never for all files found.
    pub(in crate::app) fn action_sources(&self) -> Vec<PathBuf> {
        if let Some(search) = self.open_search_view() {
            return search.action_files();
        }
        let panel = self.active_panel();
        if panel.selection.any_tagged() {
            panel
                .selection
                .ordered(&panel.entries)
                .into_iter()
                .map(|e| e.path.clone())
                .collect()
        } else {
            panel
                .selected_entry()
                .filter(|e| !e.is_parent)
                .map(|e| vec![e.path.clone()])
                .unwrap_or_default()
        }
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

    /// F3 / F4: the row under the cursor goes to the external program. Tags
    /// are ignored, as in Norton Commander; `..` and directories have nothing
    /// to view or edit. A file inside an archive is unpacked first, and only
    /// viewing works: the copy is thrown away, so an edit would be lost.
    pub(in crate::app) fn open_external(&mut self, kind: fs::OpenKind) -> Task<Message> {
        let source = match self.open_search_view() {
            Some(search) => {
                OpenSource::Search(search.cursor_target().filter(|(path, _)| !path.is_dir()))
            }
            None => OpenSource::ActivePanel,
        };
        match self.panels.plan_open(kind, source) {
            OpenPlan::Nothing => Task::none(),
            OpenPlan::Refused(msg) => {
                let text = self.lang.text(msg).to_owned();
                self.set_status_error(&text);
                Task::none()
            }
            OpenPlan::ViewFromServer(remote) => self.view_from_server(remote),
            OpenPlan::ViewFromArchive {
                source: packed,
                archive,
            } => self.view_from_archive(packed, archive),
            OpenPlan::Launch { path, line } => {
                self.launch_external(kind, path, line, CopyOf::Nothing);
                Task::none()
            }
        }
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

    /// Hands `path` to the program for `kind`, at `line` when the program
    /// takes one. `from` says whether `path` is a temporary copy made for
    /// viewing, and of what.
    pub(in crate::app) fn launch_external(
        &mut self,
        kind: fs::OpenKind,
        path: PathBuf,
        line: Option<u64>,
        from: CopyOf,
    ) {
        self.job_error = None;
        self.notice = None;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let executable = fs::could_execute(&path);
        let Some(command) = fs::open_command(&self.open_programs, kind, &path, executable, line)
        else {
            let label = |opener| self.keymap.shortcut_label(&Message::OpenExternal(opener));
            let message = open_refused_text(
                self.lang,
                &name,
                from,
                label(fs::OpenKind::View),
                label(fs::OpenKind::Edit),
            );
            self.set_status_error(&message);
            return;
        };
        if let Err(reason) = self.launcher.launch(&command) {
            let message = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", &name)
                .replace("{reason}", &reason);
            self.set_status_error(&message);
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
        let overlay_is_open = self.overlay_is_open();
        let context_menu_is_open = self.panels.context_menu.is_some();
        let pointer = self.input.lock().pointer;
        let ctx = ncrs_files::Ctx {
            visible_rows: self.visible_rows,
            window_size: self.window_size,
            modifiers: self.modifiers,
            is_context_click: ncrs_files::ctx::is_context_click(self.modifiers),
            pointer,
            overlay_is_open,
            context_menu_is_open,
            clock: &*self.clock,
            nearest_existing_directory: session_state::nearest_existing_directory,
        };
        let (task, actions) = ncrs_files::update(&mut self.panels, message, &ctx);
        let mapped = task.map(Message::Panel);
        let acted = self.run_panel_actions(actions);
        Task::batch([mapped, acted])
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

    /// What the drive menu lists, read when it opens.
    pub(in crate::app) fn volume_facts_source(&self) -> impl FnOnce() -> ncrs_files::VolumeFacts {
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
        move || ncrs_files::VolumeFacts {
            volumes: list_volumes(),
            saved_servers,
            connected: remote.connected(),
            trash: ncrs_ops::trash_menu_row(trash.reach()),
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
                self.update(Message::ContextMenuAt { side, index })
            }
            Action::CloseContextMenu => {
                self.panels.context_menu = None;
                Task::none()
            }
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
            Action::CloseSettings => self.close_settings(),
            Action::ReleaseFocus => release_focus(),
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
        }
    }

    /// The active panel shows the file's directory with the cursor on the file.
    /// Only navigates: the file is never opened or run.
    pub(in crate::app) fn reveal_file(&mut self, path: &Path) -> Task<Message> {
        let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
            return Task::none();
        };
        let directory = parent.to_path_buf();
        let name = file.to_string_lossy().into_owned();
        self.load(self.panels.active, directory, Some(name))
    }

    /// Starts an async directory read for `side`.
    pub(in crate::app) fn load(
        &mut self,
        side: PanelSide,
        path: PathBuf,
        select: Option<String>,
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
        select: Option<String>,
    ) -> Task<Message> {
        let sort = self.panel(side).sort;
        let request_id = self.panel_mut(side).begin_load();
        if self.remote.is_connected(remote.server()) {
            return remote_listing(
                Arc::clone(&self.remote),
                side,
                request_id,
                remote,
                sort,
                select,
            );
        }
        let passwords = Arc::clone(&self.passwords);
        let server = remote.server().clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || passwords.get(&server))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            move |result| {
                Message::Remote(RemoteMessage::PasswordLoaded {
                    side,
                    request_id,
                    remote: remote.clone(),
                    select: select.clone(),
                    result,
                })
            },
        )
    }
}
