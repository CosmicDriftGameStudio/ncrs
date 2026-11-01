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
        input.job_running = job_running;
        input.quick_open = self.quick_open.is_some();
        input.command_palette = self.command_palette.is_some();
        input.key_bindings = self
            .settings
            .key_bindings
            .as_ref()
            .is_some_and(|shortcuts| !shortcuts.is_recording());
        input.theme_fetch = self.settings.theme_fetch.is_some();
        input.rule_suggestions = self.hermes.rule_suggestions.is_some();
        input.rules = self.hermes.rules.is_some();
        input.review_list = self.hermes.review_list.is_some();
        input.saved_queries = self.hermes.saved_queries.is_some();
        input.tags = self.hermes.tags.is_some();
        input.hermes_sidebar = state.hermes_sidebar;
        input.transfer_dialog = self.ops.transfer_dialog.is_some();
        input.reorganisation_plan = self.hermes.reorganisation.is_some();
        input.settings = self.settings.page.is_some()
            && self.hermes.filing_setup.is_none()
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
        let job_running = self.jobs.is_busy();
        let config_error = self.config_error.is_some();
        let volume_menu = self.panels.volume_menu.is_some();
        let context_menu = self.panels.context_menu.is_some();
        let context_submenu = self
            .panels
            .context_menu
            .as_ref()
            .is_some_and(ContextMenu::submenu_is_open);
        let quick_fix = self.hermes.quick_fix.is_some();
        let path_editing = self.panels.path_field.is_some();
        let command_line = self.command_line.is_focused();
        let search = self.search_has_keys();
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
        let filing_status = self.hermes.filing_status.page_open();
        let hermes_overview = self.hermes.overview.is_some();
        let hermes_overview_editing = self
            .hermes
            .overview
            .as_ref()
            .is_some_and(OverviewState::is_editing);
        let filing_setup = self.hermes.filing_setup.is_some();
        let hermes_setup = self.hermes.hermes_setup.is_some();
        let filing_card = self
            .hermes
            .filing
            .as_ref()
            .is_some_and(FilingModule::card_can_be_dismissed);
        let theme_fetch = self.settings.theme_fetch.is_some();
        let rule_suggestions = self.hermes.rule_suggestions.is_some();
        let rules = self.hermes.rules.is_some();
        let rules_form = self
            .hermes
            .rules
            .as_ref()
            .is_some_and(RulesPage::is_editing);
        let rules_confirming_delete = self
            .hermes
            .rules
            .as_ref()
            .is_some_and(RulesPage::is_confirming_delete);
        let review_list = self.hermes.review_list.is_some();
        let review_list_editing = self
            .hermes
            .review_list
            .as_ref()
            .is_some_and(ReviewList::is_editing);
        let review_list_replanning = self
            .hermes
            .review_list
            .as_ref()
            .is_some_and(|view| view.replan().is_some());
        let saved_queries = self.hermes.saved_queries.is_some();
        let tags = self.hermes.tags.is_some();
        let hermes_sidebar =
            self.hermes_focus == HermesFocus::Sidebar && self.open_hermes_page().is_some();
        let reorganisation_plan = self.hermes.reorganisation.is_some();
        let about = self.about.is_some();
        let ocr_models = self.settings.ocr_models.is_some();
        let ai_model = self.settings.ai_card_is_shown();
        let grant_dialog = self.grant_dialog;
        let connect_dialog = self.remote_ui.dialog.is_some();
        let settings = self.settings.page.is_some();
        let transfer_dialog = self.ops.transfer_dialog.is_some();
        let transfer_progress = self.ops.progress_mask_is_open();
        let start_over_dialog = self.start_over_dialog;
        let delete_dialog = match &self.ops.delete_dialog {
            None => DeleteKeys::Closed,
            Some(deletion) if deletion.permanent => DeleteKeys::Permanent,
            Some(_) => DeleteKeys::Trash,
        };
        match &self.ops.prompt {
            None => PromptKeyState {
                keymap: Arc::clone(&self.keymap),
                job_running,
                config_error,
                delete_dialog,
                start_over_dialog,
                volume_menu,
                context_menu,
                context_submenu,
                quick_fix,
                path_editing,
                command_line,
                search,
                trash,
                hit_list,
                quick_open,
                command_palette,
                key_bindings,
                key_bindings_recording,
                filing_status,
                hermes_overview,
                hermes_overview_editing,
                filing_setup,
                hermes_setup,
                filing_card,
                theme_fetch,
                rule_suggestions,
                rules,
                rules_form,
                rules_confirming_delete,
                review_list,
                review_list_editing,
                review_list_replanning,
                saved_queries,
                tags,
                hermes_sidebar,
                reorganisation_plan,
                about,
                ocr_models,
                ai_model,
                grant_dialog,
                connect_dialog,
                settings,
                // A conflict is asked while no prompt is open — the prompt is
                // gone by then, the transfer is what is running. Without this
                // the dialog's keys would fall through to the panels.
                conflict_open: self.ops.conflict_is_open(),
                transfer_dialog,
                transfer_progress,
                ..PromptKeyState::default()
            },
            Some(prompt) => PromptKeyState {
                keymap: Arc::clone(&self.keymap),
                open: true,
                job_running,
                config_error,
                delete_dialog,
                start_over_dialog,
                volume_menu,
                context_menu,
                context_submenu,
                quick_fix,
                path_editing,
                command_line,
                search,
                trash,
                hit_list,
                quick_open,
                command_palette,
                key_bindings,
                key_bindings_recording,
                filing_status,
                hermes_overview,
                hermes_overview_editing,
                filing_setup,
                hermes_setup,
                filing_card,
                theme_fetch,
                rule_suggestions,
                rules,
                rules_form,
                rules_confirming_delete,
                review_list,
                review_list_editing,
                review_list_replanning,
                saved_queries,
                tags,
                hermes_sidebar,
                reorganisation_plan,
                about,
                ocr_models,
                ai_model,
                grant_dialog,
                connect_dialog,
                settings,
                conflict_open: self.ops.conflict_is_open(),
                transfer_dialog,
                transfer_progress,
                busy: prompt.busy(),
                typed: prompt.name().into(),
            },
        }
    }
}
