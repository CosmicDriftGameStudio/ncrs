use crate::app::*;

fn quick_open_batch(generation: u64, events: Vec<fs::search::SearchEvent>) -> Message {
    let walk_events = events
        .into_iter()
        .filter_map(|event| match event {
            fs::search::SearchEvent::File { path, .. } => Some(WalkEvent::File { path }),
            fs::search::SearchEvent::Progress { .. } => None,
            fs::search::SearchEvent::Finished(totals) => Some(WalkEvent::Finished {
                truncated: totals.truncated,
            }),
        })
        .collect();
    Message::QuickOpen(QuickOpenMessage::Events {
        generation,
        events: walk_events,
    })
}

impl App {
    /// Cmd/Ctrl+P: lists the files below the active panel's directory. Pressed
    /// while the box is up, it only gives the field the focus again.
    fn open_quick_open(&mut self) -> Task<Message> {
        let focus = operation::focus(quick_open_ui::QUICK_OPEN_ID);
        if self.quick_open.is_some() {
            return focus;
        }
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        self.panels.context_menu = None;
        let root = self.active_panel().path.clone();
        self.quick_open_walk_generation = self.quick_open_walk_generation.wrapping_add(1);
        let generation = self.quick_open_walk_generation;
        let walk = fs::search::all_files(
            root.clone(),
            stays_on_file_system(&root),
            QUICK_OPEN_FILE_LIMIT,
        );
        let (task, handle) = Task::run(
            search_events(walk, generation, quick_open_batch),
            std::convert::identity,
        )
        .abortable();
        self.quick_open_walk = Some(handle.abort_on_drop());
        let documents_enabled = self.document_finder().is_some();
        self.quick_open = Some(QuickOpen::new(root, generation).with_documents(documents_enabled));
        Task::batch([focus, task])
    }

    /// Cmd/Ctrl+Shift+P: lists the actions that work where the keys are now.
    fn open_command_palette(&mut self) -> Task<Message> {
        self.update_command_palette(CommandPaletteMessage::Toggle)
    }

    /// Which Hermes commands on a file the command list offers.
    fn command_palette_hermes(&self) -> HermesCommands {
        HermesCommands::from_facts(self.hermes.is_ready(), self.hermes.is_active(), || {
            self.hermes_action_enabled(HermesAction::ProcessAgain)
        })
    }

    /// The settings pages and settings the command list finds by a query.
    fn command_palette_settings_targets(&self) -> Vec<ncrs_config::SettingsTarget> {
        let fresh;
        let state = match &self.settings.page {
            Some(open) => open,
            None => {
                fresh = ncrs_settings::fresh_settings_state(&self.settings_ctx());
                &fresh
            }
        };
        settings_ui::search_targets(&self.settings_view(state))
    }

    pub(in crate::app) fn resize_command_palette(&mut self, window: Size) {
        command_list::resize(&mut self.command_palette, window);
    }

    pub(in crate::app) fn close_command_palette(&mut self) -> Task<Message> {
        command_list::close(&mut self.command_palette).map(Message::CommandPalette)
    }

    pub(in crate::app) fn update_command_palette(
        &mut self,
        message: CommandPaletteMessage,
    ) -> Task<Message> {
        let opening =
            matches!(message, CommandPaletteMessage::Toggle) && self.command_palette.is_none();
        let blocked = opening && self.blocking_overlay_is_open();
        let (hermes, settings_targets) = if opening && !blocked {
            (
                self.command_palette_hermes(),
                self.command_palette_settings_targets(),
            )
        } else {
            (HermesCommands::All, Vec::new())
        };
        let scope = if self.search_has_keys() {
            KeyScope::Search
        } else {
            KeyScope::Panel
        };
        let ctx = CommandPaletteCtx {
            lang: self.lang,
            keymap: &self.keymap,
            recent: &self.recent_commands,
            window: self.window_size,
            blocked,
            scope,
            hermes,
            settings_targets,
        };
        let (inner, action) = command_list::update(&mut self.command_palette, message, &ctx);
        let task = inner.map(Message::CommandPalette);
        match action {
            None => task,
            Some(CommandPaletteAction::CloseContextMenu) => {
                self.panels.context_menu = None;
                task
            }
            // The closing task comes first and is chained, not batched: a
            // prompt the action opens takes the focus after the list gave it
            // up.
            Some(CommandPaletteAction::Run(command)) => task.chain(self.run_command(command)),
            Some(CommandPaletteAction::ShowSetting { feature, row }) => {
                Task::batch([task, self.show_setting(feature, row)])
            }
        }
    }

    /// Opens the settings tab, unless something blocks it, on `feature`, with
    /// `row` in view.
    pub(in crate::app) fn show_setting(
        &mut self,
        feature: crate::messages::SettingsFeature,
        row: Option<String>,
    ) -> Task<Message> {
        let mut actions = Vec::new();
        let task = self.with_settings(|settings, ctx| {
            ncrs_settings::show_setting(settings, ctx, feature, row, &mut actions)
        });
        self.run_settings_actions(task, actions)
    }

    /// Runs a command chosen from the command list or the menu bar: what its
    /// key would do, and it counts among the recent commands.
    pub(in crate::app) fn run_command(&mut self, action: KeyAction) -> Task<Message> {
        self.remember_command(action);
        self.update(action.message())
    }

    /// Puts `action` first among the recent commands, once.
    fn remember_command(&mut self, action: KeyAction) {
        self.recent_commands.retain(|&recent| recent != action);
        self.recent_commands.insert(0, action);
        self.recent_commands.truncate(RECENT_LIMIT);
    }

    /// Closes the box and stops its walk.
    pub(in crate::app) fn close_quick_open(&mut self) -> Task<Message> {
        self.quick_open_walk = None;
        quick_open::close(&mut self.quick_open)
    }

    pub(in crate::app) fn update_quick_open(&mut self, message: QuickOpenMessage) -> Task<Message> {
        let ctx = QuickOpenCtx {
            documents_available: self.document_finder().is_some(),
        };
        let (inner, action) = quick_open::update(&mut self.quick_open, message, &ctx);
        let task = inner.map(Message::QuickOpen);
        match action {
            None => task,
            Some(QuickOpenAction::Open) => self.open_quick_open(),
            Some(QuickOpenAction::Close) => self.close_quick_open(),
            Some(QuickOpenAction::SwitchToCommands(query)) => {
                self.switch_quick_open_to_commands(query)
            }
            Some(QuickOpenAction::FindDocuments(wanted)) => {
                let documents = self.document_finder().map_or_else(Task::none, |finder| {
                    quick_open::find_documents(wanted, move |words, limit| {
                        finder.find(words, limit)
                    })
                    .map(Message::QuickOpen)
                });
                Task::batch([task, documents])
            }
            Some(QuickOpenAction::WalkFinished) => {
                self.quick_open_walk = None;
                task
            }
            // Enter on the last row.
            Some(QuickOpenAction::OpenSearch(text)) => {
                let closed = self.close_quick_open();
                Task::batch([closed, self.open_search_with_words(text)])
            }
            // Enter on a hit: the active panel shows the file's directory with
            // the cursor on the file.
            Some(QuickOpenAction::Reveal(path)) => {
                let closed = self.close_quick_open();
                let revealed = self.reveal_file(&path);
                Task::batch([closed, revealed])
            }
        }
    }

    /// A `>` typed first: the box gives way to the command list, with the rest
    /// of the text as its query.
    fn switch_quick_open_to_commands(&mut self, query: String) -> Task<Message> {
        let closed = self.close_quick_open();
        let opened = self.open_command_palette();
        if let Some(palette) = self.command_palette.as_mut() {
            palette.set_query(query);
        }
        Task::batch([closed, opened])
    }
}
