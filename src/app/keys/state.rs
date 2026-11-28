use crate::app::*;

impl App {
    /// Writes the current routing state into the cell the keyboard reads.
    ///
    /// A no-op before the first `subscription()` call, which is the only time
    /// there is no cell — the subscription creates it.
    pub(in crate::app) fn publish_key_state(&mut self) {
        let state = self.prompt_key_state();
        // An overlay answers Escape itself.
        let search_open = self.search_has_keys() && !self.overlay_is_open();
        let job_running = state.job_running;
        let mut input = self.input.lock();
        input.path_editing = state.path_editing;
        input.command_line = state.command_line;
        input.search_open = search_open;
        input.hermes_chat = state.hermes_chat;
        input.job_running = job_running;
        input.quick_open = state.quick_open;
        input.command_palette = state.command_palette;
        input.key_bindings = self
            .settings
            .key_bindings
            .as_ref()
            .is_some_and(|shortcuts| !shortcuts.is_recording());
        input.theme_fetch = state.theme_fetch;
        input.transfer_dialog = state.transfer_dialog;
        input.permissions_dialog = state.permissions_dialog;
        input.batch_rename_dialog = state.batch_rename_dialog;
        input.settings = self.settings_shown() && !self.blocking_overlay_is_open();
        input.monitor = state.monitor;
        input.viewer = state.viewer;
        input.notes = self.notes_shown() && !self.blocking_overlay_is_open();
        input.filing = self.filing_shown() && !self.blocking_overlay_is_open();
        input.filing_renaming = input.filing && state.filing_keys.renaming;
        input.filing_question = input.filing && state.filing_keys.question;
        input.repos = self.repos_shown() && !self.blocking_overlay_is_open();
        input.space = self.space_shown() && !self.blocking_overlay_is_open();
        input.git = self
            .shown_git_tab()
            .filter(|_| !self.blocking_overlay_is_open());
        input.repos_menu_key = input.repos && !state.repos_text && !state.repos_menu;
        input.space_menu_key =
            input.space && !state.space_filter && !state.space_menu && !state.space_confirm;
        input.git_menu_key = input.git.is_some() && state.git_context.takes_menu_key();
        input.monitor_filter = self.monitor_shown()
            && self.monitor.filter_has_focus()
            && !self.blocking_overlay_is_open();
        input.monitor_menu = self.monitor_shown()
            && !self.monitor.filter_has_focus()
            && !self.blocking_overlay_is_open();
        drop(input);
        // `get` rather than `get_or_init`: the cell is created by the
        // subscription, and `update` must not create it — a cell nothing reads
        // would just be a value nobody sees. Before the first `subscription()`
        // call there is nothing to publish to, and that is fine: no keys have
        // been pressed yet.
        if let Some(cell) = self.key_state.get() {
            *cell
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
        }
    }

    /// The state the key routing needs, cheap to clone.
    pub(in crate::app) fn prompt_key_state(&self) -> PromptKeyState {
        let job_running = self.jobs.is_busy() || self.panels.folder_size_run.is_some();
        let config_error = self.config_error.is_some();
        let volume_menu = self.panels.volume_menu.is_some();
        let header_menu = self.panels.header_menu.is_some();
        let context_menu = self.panels.context_menu.is_some();
        let favourite_menu = self.favourite_menu.is_some();
        let context_submenu = self
            .panels
            .context_menu
            .as_ref()
            .is_some_and(ContextMenu::submenu_is_open);
        let confirm_again = self.hermes.confirm_again.is_some();
        let hermes_dialog = self.hermes_dialog_keys();
        let path_editing =
            self.panels.path_field.is_some() || self.panels.renaming_side().is_some();
        let command_line = self.command_line.is_focused();
        let search = self.search_has_keys();
        let hermes_chat = self.hermes_chat_shown();
        let trash = self.trash_has_keys();
        let hit_list = self.active_panel().is_hit_list();
        let quick_open = self.quick_open.is_some();
        let command_palette = self.command_palette.is_some();
        let key_bindings = self.settings.key_bindings.is_some();
        let key_bindings_recording = self
            .settings
            .key_bindings
            .as_ref()
            .is_some_and(KeyBindings::is_recording);
        let filing_card = self
            .hermes
            .filing
            .as_ref()
            .is_some_and(FilingModule::card_can_be_dismissed);
        let theme_fetch = self.settings.theme_fetch.is_some();
        let about = self.about.is_some();
        let ocr_models = self.settings.ocr_models.is_some();
        let ai_model = self.settings.ai_card_is_shown();
        let grant_dialog = self.grant_dialog;
        let connect_dialog = self.remote_ui.dialog.is_some();
        let compare_dialog = self.compare.dialog_is_open();
        let settings = self.settings_shown();
        let monitor = self.monitor_shown();
        let monitor_filter = monitor && self.monitor.filter_has_focus();
        let monitor_dialog = monitor && self.monitor.dialog_is_open();
        let monitor_menu = monitor && self.monitor.menu_is_open();
        let monitor_drive_focused = monitor && self.monitor.drive_list_has_focus();
        let monitor_drives_listed = monitor && self.monitor.drives_are_listed();
        let viewer = self.viewer_shown();
        let viewer_markdown = viewer && self.viewer.shows_markdown();
        let notes = self.notes_shown();
        let notes_editing = notes && self.notes.editing();
        let filing = self.filing_shown();
        let filing_keys = ncrs_filing_view::key_facts(&self.filing_view);
        let repos = self.repos_shown();
        let repos_text = repos && self.repos.as_ref().is_some_and(|state| state.takes_text());
        let repos_menu = repos && self.repos.as_ref().is_some_and(|state| state.menu_open());
        let space = self.space_shown();
        let space_filter = space
            && self
                .space
                .as_ref()
                .is_some_and(|state| state.filter_focused());
        let space_menu = space && self.space.as_ref().is_some_and(|state| state.menu_open());
        let space_confirm = space
            && self
                .space
                .as_ref()
                .is_some_and(|state| state.confirm().is_some());
        let git = self.shown_git_tab();
        let git_state = git.and_then(|id| self.git_tabs.get(id));
        let git_context = git_state
            .map(|state| state.key_context())
            .unwrap_or_default();
        let transfer_dialog = self.ops.transfer_dialog.is_some();
        let transfer_progress = self.ops.progress_mask_is_open();
        let permissions_dialog = self.ops.permissions_dialog.is_some();
        let batch_rename_dialog = self.ops.batch_rename.is_some();
        let start_over_dialog = self.start_over_dialog;
        let open_file_dialog = self.ops.open_file_dialog.is_some();
        let quit_dialog = self.ops.quit_dialog;
        let delete_dialog = match &self.ops.delete_dialog {
            None => DeleteKeys::Closed,
            Some(deletion) if deletion.permanent => DeleteKeys::Permanent,
            Some(_) => DeleteKeys::Trash,
        };
        let shared = PromptKeyState {
            keymap: Arc::clone(&self.keymap),
            git_keys: Arc::clone(&self.git_keys),
            repos_keys: Arc::clone(&self.repos_keys),
            space_keys: Arc::clone(&self.space_keys),
            job_running,
            config_error,
            delete_dialog,
            open_file_dialog,
            quit_dialog,
            start_over_dialog,
            volume_menu,
            header_menu,
            context_menu,
            favourite_menu,
            context_submenu,
            confirm_again,
            hermes_dialog,
            path_editing,
            command_line,
            search,
            hermes_chat,
            trash,
            hit_list,
            quick_open,
            command_palette,
            key_bindings,
            key_bindings_recording,
            filing_card,
            theme_fetch,
            about,
            ocr_models,
            ai_model,
            grant_dialog,
            connect_dialog,
            compare_dialog,
            settings,
            monitor,
            monitor_filter,
            monitor_dialog,
            monitor_menu,
            monitor_drive_focused,
            monitor_drives_listed,
            viewer,
            viewer_markdown,
            notes,
            notes_editing,
            filing,
            filing_keys,
            git,
            repos,
            repos_text,
            repos_menu,
            space,
            space_filter,
            space_menu,
            space_confirm,
            git_context,
            // A conflict is asked while no prompt is open — the prompt is
            // gone by then, the transfer is what is running. Without this
            // the dialog's keys would fall through to the panels.
            conflict_open: self.ops.conflict_is_open(),
            transfer_dialog,
            transfer_progress,
            permissions_dialog,
            batch_rename_dialog,
            ..PromptKeyState::default()
        };
        match &self.ops.prompt {
            None => shared,
            Some(prompt) => PromptKeyState {
                open: true,
                busy: prompt.busy(),
                typed: prompt.name().into(),
                ..shared
            },
        }
    }
}
