use crate::app::*;

impl App {
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

    /// The Git actions, while a Git tab is shown that offers them.
    pub(in crate::app) fn command_palette_git_commands(&self) -> Vec<GitCommandEntry> {
        let offered = self
            .shown_git_tab()
            .and_then(|id| self.git_tabs.get(id))
            .is_some_and(ncrs_git_view::actions_available);
        if !offered {
            return Vec::new();
        }
        ncrs_git_view::commands(self.lang, &self.git_keys)
            .into_iter()
            .map(|(label, shortcut)| GitCommandEntry {
                label,
                shortcut: Some(shortcut).filter(|key| !key.is_empty()),
            })
            .collect()
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

    /// The commands whose key the active panel would refuse with a note,
    /// as in an archive.
    pub(in crate::app) fn commands_the_panel_refuses(&self) -> Vec<KeyAction> {
        let panel = self.active_panel();
        ncrs_config::commands::all()
            .map(|command| command.action)
            .filter(|action| {
                refusal_for(panel, &action.message()).is_some()
                    || (*action == KeyAction::UndoRename && !self.ops.can_undo_rename())
            })
            .collect()
    }

    /// The commands on a folder that the row under the cursor cannot take now.
    fn commands_without_a_folder_to_file(&self) -> Vec<KeyAction> {
        [
            (KeyAction::HermesFileOnce, HermesAction::FileOnce),
            (
                KeyAction::HermesFileContinuously,
                HermesAction::FileContinuously,
            ),
            (
                KeyAction::HermesStopContinuous,
                HermesAction::StopContinuous,
            ),
        ]
        .into_iter()
        .filter(|(_, action)| !self.hermes_action_enabled(*action))
        .map(|(key, _)| key)
        .collect()
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
        let blocked = opening && (self.blocking_overlay_is_open() || self.page_dialog_is_open());
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
            unavailable: if opening && !blocked && scope == KeyScope::Panel {
                let mut refused = self.commands_the_panel_refuses();
                refused.extend(self.commands_without_a_folder_to_file());
                refused
            } else {
                Vec::new()
            },
            git_commands: if opening && !blocked {
                self.command_palette_git_commands()
            } else {
                Vec::new()
            },
            settings_targets,
        };
        let (inner, action) = command_list::update(&mut self.command_palette, message, &ctx);
        let task = inner.map(Message::CommandPalette);
        match action {
            None => task,
            Some(CommandPaletteAction::CloseContextMenu) => {
                self.close_context_menu();
                task
            }
            // The closing task comes first and is chained, not batched: a
            // prompt the action opens takes the focus after the list gave it
            // up.
            Some(CommandPaletteAction::Run(command)) => task.chain(self.run_command(command)),
            Some(CommandPaletteAction::RunGit(place)) => {
                let sent = self
                    .shown_git_tab()
                    .zip(ncrs_git_view::message_of_command(place));
                match sent {
                    Some((id, git_message)) => task.chain(self.update_git_tab(id, git_message)),
                    None => task,
                }
            }
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
        let task = self.with_settings(|settings, ctx| {
            ncrs_settings::show_setting(settings, ctx, feature, row)
        });
        if self.settings.page.is_some() && !self.dialog_is_open() {
            self.shown_tab = ShownTab::Settings;
        }
        self.run_settings_actions(task, Vec::new())
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
        quick_open::close(&mut self.quick_open)
    }

    pub(in crate::app) fn update_quick_open(&mut self, message: QuickOpenMessage) -> Task<Message> {
        let may_open = !(self.blocking_overlay_is_open() || self.page_dialog_is_open());
        if matches!(message, QuickOpenMessage::Open) && self.quick_open.is_none() && may_open {
            self.close_context_menu();
        }
        let ctx = QuickOpenCtx {
            root: self.active_panel().path.clone(),
            may_open,
            documents_available: self.document_finder().is_some(),
        };
        let (inner, action) = quick_open::update(&mut self.quick_open, message, &ctx);
        let task = inner.map(Message::QuickOpen);
        match action {
            None => task,
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
