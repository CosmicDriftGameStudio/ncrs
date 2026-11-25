use crate::app::*;
use crate::messages::GitFileView;

/// What a list of search hits has no use for: it is not a folder, so nothing is
/// made, pasted or measured in it.
fn is_not_offered_in_hit_list(message: &Message) -> bool {
    matches!(
        message,
        Message::CreateDirPrompt
            | Message::CreateFilePrompt
            | Message::Paste
            | Message::ComputeFolderSizes
            | Message::OpenTerminal
            | Message::Panel(PanelMessage::PathFieldInput(_))
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
        Message::OpenGitView | Message::OpenGitFile(_) => Some(Msg::GitViewNotARepository),
        Message::OpenTerminal => Some(Msg::StatusTerminalNeedsLocalFolder),
        Message::Transfer(TransferKind::Move)
        | Message::PermissionsPrompt
        | Message::Panel(PanelMessage::RenameStart)
        | Message::Panel(PanelMessage::ShowPackageContents)
        | Message::OpenExternal(fs::OpenKind::Edit)
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::ComputeFolderSizes
        | Message::OpenSearch
        | Message::NoteOnFile
        | Message::ExtractHere
        | Message::AddFavourite
        | Message::RemoveFavourite
        | Message::HermesFile(
            HermesFileMessage::ProcessAgain
            | HermesFileMessage::FileOnce
            | HermesFileMessage::WatchSource
            | HermesFileMessage::AddTarget
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

/// What the panel refuses, whether it shows an archive or an SFTP server.
pub(in crate::app) fn refusal_for(panel: &PanelState, message: &Message) -> Option<Msg> {
    archive_refusal(panel, message)
        .or_else(|| remote_refusal(panel, message))
        .or_else(|| git_refusal(panel, message))
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
        Message::OpenTerminal => Some(Msg::StatusTerminalNeedsLocalFolder),
        Message::PackPrompt => Some(Msg::StatusPackUnavailable),
        Message::OpenGitView | Message::OpenGitFile(_) => Some(Msg::GitViewNotARepository),
        Message::Transfer(TransferKind::Move)
        | Message::PermissionsPrompt
        | Message::OpenExternal(fs::OpenKind::Edit)
        | Message::Delete { .. }
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::ComputeFolderSizes
        | Message::OpenSearch
        | Message::Panel(PanelMessage::ShowPackageContents)
        | Message::NoteOnFile
        | Message::ExtractHere
        | Message::AddFavourite
        | Message::RemoveFavourite
        | Message::HermesFile(
            HermesFileMessage::ProcessAgain
            | HermesFileMessage::FileOnce
            | HermesFileMessage::WatchSource
            | HermesFileMessage::AddTarget
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
            HermesFileMessage::ProcessAgain
                | HermesFileMessage::WatchSource
                | HermesFileMessage::AddTarget
                | HermesFileMessage::ToggleExclusion
                | HermesFileMessage::ShowInInspection
        ) | Message::HermesPage(HermesPage::Sorting | HermesPage::SavedQueries)
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
            | Message::HermesChat(HermesChatMessage::Open)
            | Message::QuickOpen(QuickOpenMessage::Open)
            | Message::CommandPalette(CommandPaletteMessage::Toggle)
            | Message::KeyBindings(KeyBindingsMessage::Open)
            | Message::ThemeFetch(ThemeFetchMessage::Open)
            | Message::HermesPage(_)
            | Message::OcrModels(OcrModelsMessage::Open)
            | Message::About(AboutMessage::Open)
            | Message::VersionPressed
            | Message::AiModel(AiModelMessage::Open)
            | Message::Grant(GrantMessage::Open)
            | Message::Remote(RemoteMessage::OpenConnect)
            | Message::Compare(CompareMessage::Open)
            | Message::Settings(_)
            | Message::FilingStatus(FilingStatusMessage::Open)
            | Message::Files(MenuMessage::ContextMenuAt { .. })
            | Message::Files(MenuMessage::ContextMenuKey)
            | Message::CopyToClipboard(_)
            | Message::CopyFilesToClipboard
            | Message::Panel(PanelMessage::SwitchPanel)
            | Message::NextTab
            | Message::PreviousTab
            | Message::Panel(PanelMessage::ActivatePanel(_))
            | Message::Panel(PanelMessage::SortBy { .. })
            | Message::Panel(PanelMessage::HeaderPressed { .. })
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
        TitleBarMessage::TabsFrom(_) => Task::none(),
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
        let was_open = self.open_pages();
        let task = self.update_message(message);
        let compare = self.follow_compare();
        self.follow_opened_pages(was_open);
        self.refit_for_favourites_bar();
        self.refresh_special_folders();
        let chat = self.tell_chat_hermes_is_ready();
        // The keyboard subscription cannot see `&self`, so it reads the routing
        // state from a cell kept current here. This runs last: published any
        // earlier, the cell would describe the state from before the message
        // just handled, or from before a page was brought to the front, and
        // the keys that follow would be routed to the wrong place.
        self.publish_key_state();
        #[cfg(target_os = "macos")]
        native_menu::sync(self.lang, &self.keymap, self.menu_state());
        Task::batch([task, compare, chat])
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
            self.panels.end_rename();
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
        let session_unchanged = cannot_change_session_state(&message);
        let applied = self.apply_unless_refused(message);
        let task = Task::batch([
            applied,
            self.with_hermes(ncrs_hermes_ui::request_hermes_column)
                .map(Message::from),
            if session_unchanged {
                Task::none()
            } else {
                self.schedule_session_save()
            },
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

    /// Toggling a page that is open but behind the other tab shows it.
    fn show_hidden_tab(&mut self, message: &Message) -> bool {
        let tab = match message {
            Message::Monitor(MonitorMessage::Toggle) if self.monitor.is_open() => ShownTab::Monitor,
            Message::Settings(SettingsMessage::Toggle) if self.settings.page.is_some() => {
                ShownTab::Settings
            }
            _ => return false,
        };
        let hidden = self.shown_tab != tab;
        if hidden && !self.dialog_is_open() {
            self.shown_tab = tab;
        }
        hidden
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        if self.show_hidden_tab(&message) {
            return Task::none();
        }
        match message {
            // A raw key press. Routed here rather than in the subscription,
            // because that one cannot see whether a prompt is open.
            // --- copy / move ---
            Message::Transfer(kind) => self.open_transfer_dialog(kind),

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
            Message::HitList(hit_message) => self.update_hit_list(hit_message),

            Message::OpenExternal(kind) => self.open_external(kind),
            Message::OpenTerminal => self.open_terminal(),

            #[cfg(target_os = "macos")]
            Message::OpenPrivacySettings => {
                self.open_privacy_settings();
                Task::none()
            }
            #[cfg(not(target_os = "macos"))]
            Message::OpenPrivacySettings => Task::none(),

            // --- drive menu and context menu ---
            Message::Files(menu_message) => self.update_files_menu(menu_message),
            Message::AddFavourite => {
                let action = self.panels.add_favourite(self.lang);
                self.run_panel_actions(vec![action])
            }
            Message::RemoveFavourite => {
                let action = self.panels.remove_favourite(self.lang);
                self.run_panel_actions(vec![action])
            }
            Message::OpenFavourites => match self.panels.no_favourites_notice(self.lang) {
                Some(notice) => self.run_panel_actions(vec![notice]),
                None => self.request_volume_menu(self.panels.active, true),
            },
            Message::OpenFavourite(index) => self.open_favourite_by_key(index),
            Message::FavouritesBar(bar_message) => self.update_favourites_bar(bar_message),

            Message::HermesFile(file_message) => self.update_hermes_file(file_message),

            // --- path field ---
            Message::ClickedOutside => self.update_path_field(Message::ClickedOutside),

            Message::CommandLine(line_message) => self.update_command_line(line_message),

            // --- delete ---
            Message::Delete { permanent } => self.open_delete_dialog(permanent),
            Message::CopyToClipboard(kind) => self.copy_to_clipboard(kind),
            // The field that has the focus pastes by itself when the key
            // reaches the window; a file is made only from the panels.
            Message::Paste if self.text_field_has_keys() => Task::none(),
            Message::Paste => self.paste_from_clipboard(),
            Message::ClipboardRead(content) => self.paste_content(content),
            Message::CopyFilesToClipboard => self.copy_files_to_clipboard(),

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
            Message::TagPatternPrompt { untag } => self.open_tag_pattern_prompt(untag),
            Message::GoToPath => self.open_go_to_path_prompt(),
            Message::ExtractHere => self.extract_here(),
            Message::PermissionsPrompt => self.open_permissions_dialog(),
            Message::Ops(ops_message) => self.update_ops(ops_message),

            Message::PreviousTab => self.switch_tab(tab_stops::TabDirection::Backward),
            Message::NextTab => self.switch_tab(tab_stops::TabDirection::Forward),
            Message::TabInPage { backward } => {
                ncrs_ui_kit::focus::field_has_focus().map(move |field_focused| {
                    Message::TabInPageAnswered {
                        backward,
                        field_focused,
                    }
                })
            }
            Message::FocusPageThen(row, inner) => {
                let focused = self.update_settings(SettingsMessage::FocusThen(row, None));
                match inner {
                    Some(then) => Task::batch([focused, self.update(*then)]),
                    None => focused,
                }
            }
            Message::TabInPageAnswered {
                backward,
                field_focused,
            } => self.tab_inside_page(backward, field_focused),
            Message::ShowTab(tab) => {
                self.shown_tab = tab;
                Task::none()
            }
            Message::Panel(panel_message) => self.update_panel(panel_message),
            Message::TitleBar(TitleBarMessage::TabsFrom(first)) => {
                self.title_tabs_first = first;
                Task::none()
            }
            Message::TitleBar(title_bar) => update_title_bar(title_bar),
            Message::ComputeFolderSizes => {
                let walk = self.panels.start_folder_size_run();
                folder_size_task(walk)
            }
            Message::SwitchLanguage => {
                self.switch_language();
                self.load_documents_facts()
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Message::DragOutStarted(started) => self.drag_out_started(started),

            Message::WindowResized(size) => {
                self.apply_window_size(size);
                Task::none()
            }
            Message::WindowFocused => self.reload_local_panels(),
            Message::OpenSearch => self.open_search(),
            Message::Search(search) => self.update_search(search),
            Message::HermesChat(chat) => self.update_hermes_chat(chat),
            Message::HermesChatToCommandLine => self.hermes_chat_to_command_line(),
            Message::QuickOpen(quick_open) => self.update_quick_open(quick_open),
            Message::CommandPalette(command_palette) => {
                self.update_command_palette(command_palette)
            }
            Message::KeyBindings(shortcuts) => self.update_key_bindings(shortcuts),
            Message::ThemeFetch(fetch_message) => self.update_theme_fetch(fetch_message),
            plain @ (Message::Rules(_)
            | Message::Sorting(_)
            | Message::Targets(_)
            | Message::Suggest(_)
            | Message::SavedQueries(_)
            | Message::Tags(_)
            | Message::Activity(_)) => self.update_hermes_plain(plain),
            Message::Monitor(monitor_message) => self.update_monitor(monitor_message),
            Message::Viewer(viewer_message) => self.update_viewer(viewer_message),
            Message::Notes(notes_message) => self.update_notes(notes_message),
            Message::OpenNotes => self.open_notes(),
            Message::Repos(repos_message) => self.update_repos(repos_message),
            Message::OpenRepos => self.open_repos(),
            Message::CloseRepos => {
                self.close_repos();
                Task::none()
            }
            Message::GitView(id, git_message) => self.update_git_tab(id, git_message),
            Message::GitAsked(request) => self.route_git_request(request),
            Message::GitFetchDue => self.fetch_git_tabs_in_background(),
            Message::OpenGitView => self.open_git_view(),
            Message::OpenGitFile(view) => self.open_git_file(view),
            Message::FilingView(filing_message) => self.update_filing_tab(filing_message),
            Message::NoteOnFile => self.note_on_file(),
            Message::HandedOver(requests) => self.open_handed_over(requests),
            Message::Updates(updates_message) => self.update_updates(updates_message),
            Message::Store(store_message) => self.update_store(store_message),
            Message::VersionPressed => self.open_version_info(),
            Message::HermesPage(page) => self.show_hermes_page(page),
            plain @ Message::ContinuousFiled(_) => self.update_hermes_plain(plain),
            Message::About(about_message) => self.update_about(about_message),
            Message::OcrModels(models_message) => self.update_ocr_models(models_message),
            Message::AiModel(model_message) => self.update_ai_model(model_message),
            Message::Grant(grant_message) => self.update_grant(grant_message),
            Message::Compare(compare_message) => self.update_compare(compare_message),
            Message::Remote(remote_message) => {
                let outcome = self.with_connect_feature(|remote, ctx| {
                    ncrs_remote::update(remote, remote_message, ctx)
                });
                self.run_remote_outcome(outcome)
            }
            Message::Settings(SettingsMessage::Close) => {
                match ncrs_hermes_ui::close_dialog(&mut self.hermes) {
                    Some(closed) => closed.map(Message::from),
                    None => self.update_settings(SettingsMessage::Close),
                }
            }
            Message::Settings(SettingsMessage::FocusKey(key)) => self.settings_focus_key(key),
            Message::Settings(settings_message) => self.update_settings(settings_message),
            plain @ Message::Filing(_) => self.update_hermes_plain(plain),
            plain @ Message::FilingStatus(_) => self.update_hermes_plain(plain),
            plain @ Message::HermesColumn(_) => self.update_hermes_plain(plain),
            plain @ Message::HermesOverview(_) => self.update_hermes_plain(plain),
            #[cfg(any(target_os = "macos", test))]
            Message::RunCommand(action) => self.run_command(action),
            #[cfg(target_os = "macos")]
            Message::InstallNativeMenu => {
                native_menu::install(self.lang, &self.keymap, self.menu_state());
                Task::none()
            }
            Message::Quit if self.jobs.is_busy() && !self.ops.quit_confirmed => {
                self.ops.quit_dialog = true;
                self.dialog_focus = ncrs_ops::quit_dialog::CANCEL_FOCUS;
                Task::none()
            }
            #[cfg(feature = "local-ai")]
            Message::QuitReady => iced::exit(),
            Message::Quit => {
                self.ops.quit_confirmed = false;
                self.quit_after_settings_page()
            }
            // Handled in `update`, which owns the timer's generation check.
            Message::NoticeExpired(_) | Message::SaveSessionState(_) => Task::none(),
        }
    }

    /// A panel inside an archive can be read and copied from, nothing else.
    pub(in crate::app) fn apply_unless_refused(&mut self, message: Message) -> Task<Message> {
        if let Some(refused) = self.refuse_if_locked(&message) {
            return refused;
        }
        if asks_for_hermes(&message) {
            if self.hermes.is_restarting() {
                let text = self.lang.text(Msg::StatusHermesRestarting).to_owned();
                return self.show_notice(text);
            }
            if self.hermes.is_off() {
                return self.explain_hermes_is_off();
            }
        }
        if let (Message::OpenGitFile(view), Some(id)) = (&message, self.shown_git_tab()) {
            let file = match view {
                GitFileView::History => ncrs_git_view::Message::HistoryKey,
                GitFileView::Blame => ncrs_git_view::Message::BlameKey,
            };
            return self.update_git_tab(id, file);
        }
        let panel = self.panel(self.source_side());
        if panel.is_hit_list() && is_not_offered_in_hit_list(&message) {
            return Task::none();
        }
        match refusal_for(panel, &message) {
            Some(refusal) => {
                let text = self.lang.text(refusal).to_string();
                self.set_status_error(&text);
                Task::none()
            }
            None => self.apply(message),
        }
    }
}

/// The Git view needs a folder of this computer that lies inside a repository.
fn git_refusal(panel: &PanelState, message: &Message) -> Option<Msg> {
    if !matches!(message, Message::OpenGitView) {
        return None;
    }
    git_repository_of(panel)
        .is_none()
        .then_some(Msg::GitViewNotARepository)
}

/// The repository root of the folder under the cursor, or of the folder the
/// panel shows.
pub(in crate::app) fn git_repository_of(panel: &PanelState) -> Option<std::path::PathBuf> {
    if panel.is_virtual() || panel.archive.is_some() || panel.remote().is_some() {
        return None;
    }
    let under_cursor = panel
        .selected_entry()
        .filter(|entry| entry.is_dir && !entry.is_parent)
        .map(|entry| panel.path.join(&entry.name));
    under_cursor
        .and_then(|folder| ncrs_git::repository_root_of(&folder))
        .or_else(|| ncrs_git::repository_root_of(&panel.path))
}
