use crate::app::*;
use iced::widget::column;

impl App {
    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let panel_view = |side: PanelSide| {
            let hermes_cell: Option<ncrs_files::HermesCellOf<'_>> =
                self.hermes.is_active().then(|| {
                    let column = ColumnView::new(
                        &self.hermes.hermes_column,
                        &self.hermes.filing_config.locations,
                        self.hermes.hermes_spinner_angle,
                        &self.file_list_tags,
                        &self.tag_exceptions,
                        self.window_size.width / 2.0,
                        &self.kinds,
                    );
                    let (lang, palette) = (self.lang, self.palette);
                    Box::new(move |entry: &fs::FileEntry| {
                        panel::hermes_cell_of(entry, column, lang, &palette)
                    }) as ncrs_files::HermesCellOf<'_>
                });
            ncrs_files::view(
                self.panel(side),
                ncrs_files::ViewCtx {
                    side,
                    lang: self.lang,
                    palette: self.palette,
                    is_active: self.panels.active == side,
                    visible_rows: self.visible_rows,
                    hermes_cell,
                    columns: self.column_settings(),
                    special_folders: &self.special_folders,
                    header_drag: self.panels.header_drag_view(side),
                },
            )
            .map(Message::Panel)
        };

        let side_view_plain = |side: PanelSide| match self.search.view.as_ref() {
            _ if self.ops.trash_view.is_some() && side == self.ops.trash_side => {
                self.trash_panel(side)
            }
            Some(search) if self.search.open && side == self.search.side => {
                let keymap = &self.keymap;
                ncrs_search::view(
                    search,
                    self.search.rows,
                    ncrs_search::ViewCtx {
                        lang: self.lang,
                        palette: &self.palette,
                        is_active: self.panels.active == side,
                        side_is_remote: self.panel(side).remote().is_some(),
                        shortcuts: self.toggle_shortcuts(),
                        toggle_for_press: Rc::new(move |key, physical_key, modifiers| {
                            search_toggle_for_press(keymap, key, physical_key, modifiers)
                        }),
                    },
                )
                .map(Message::Search)
            }
            _ => panel_view(side),
        };

        let side_view = |side: PanelSide| {
            let plain = side_view_plain(side);
            let view: Element<'_, Message> = if self.is_drop_target(side) {
                Stack::with_children([
                    plain,
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(theme::outlined_in(self.palette.accent))
                        .into(),
                ])
                .into()
            } else {
                plain
            };
            // The cursor goes on a wrapper, not on the overlay: a Stack layer
            // with its own cursor lifts the pointer off the layers below, so
            // the panel would report the pointer as gone and end the hover.
            // The wrapper stays in place without a drag so the panel keeps its
            // widget state when a drag starts.
            let wrapper = mouse_area(view);
            Element::from(match self.drag_cursor(side) {
                Some(interaction) => wrapper.interaction(interaction),
                None => wrapper,
            })
        };

        let panels = row![
            side_view(PanelSide::Left),
            theme::vertical_rule(self.palette.border),
            side_view(PanelSide::Right),
        ]
        .height(Length::Fill);

        let below_title_bar =
            if self.monitor_shown() {
                self.monitor_area()
            } else if self.hermes_chat.open {
                self.hermes_chat_area()
            } else if let Some(state) = self
                .settings
                .page
                .as_ref()
                .filter(|_| self.settings_shown())
            {
                self.settings_area(state)
            } else {
                container(column![
                    panels,
                    command_line_ui::view(command_line_ui::Props {
                        prompt: self.active_panel().path.display().to_string(),
                        typed: self.command_line.typed(),
                        focused: self.command_line.is_focused(),
                        focusable: !self.command_line_is_blocked(),
                        running: self.command_line.running(),
                        key_label: self
                            .keymap
                            .shortcut_label(&KeyAction::CommandLine.message()),
                        lang: self.lang,
                        palette: &self.palette,
                    })
                    .map(Message::CommandLine),
                    fkeys::view_dimmed(
                        &self.function_key_bar(),
                        &self.function_key_dimmed(),
                        &self.palette,
                    ),
                    statusbar::view(
                        self.active_panel(),
                        self.lang,
                        &self.palette,
                        statusbar::Activity {
                            side: self.panels.active,
                            theme_name: self.theme_name(),
                            job: self.job_status(),
                            job_press: self.ops.transfer.is_some().then_some(
                                Message::TransferDialog(TransferDialogMessage::ShowProgress,)
                            ),
                            filing: self.filing_badge(),
                            on_blocked_press: blocked_access_press(
                                self.sandbox.active,
                                &self.active_panel().path,
                            ),
                            on_failure_press: self
                                .job_error
                                .is_some()
                                .then(|| self.job_error_press.clone())
                                .flatten(),
                            blocked_by_sandbox: cfg!(target_os = "macos") && self.sandbox.active,
                            notice: self.notice.as_deref(),
                            hermes_line: self.hermes_line(),
                            version: Some(statusbar::VersionBadge {
                                text: self.app_version.to_owned(),
                                on_press: Message::VersionPressed,
                                update: self.newer_version_text(),
                            }),
                            monitor: self.monitor_badge(),
                            test_folder: config::data_dir_override().map(|dir| {
                                ncrs_i18n::fill_template(
                                    self.lang.text(Msg::StatusTestFolder),
                                    &[("dir", &dir.display().to_string())],
                                )
                            }),
                        },
                        self.status_error(),
                        self.search_status(),
                        statusbar::PathBar {
                            editing: self.panels.path_field.as_deref(),
                            on_input: Message::PathFieldInput,
                            on_submit: Message::PathFieldSubmit,
                            on_copy: Message::CopyToClipboard(ClipboardKind::Directory),
                            hit_line: self.hit_line(),
                        },
                    ),
                ])
                .width(Length::Fill)
                .height(Length::Fill)
            };

        let window = container(column![self.title_bar_view(), below_title_bar])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::root(&self.palette));

        // The cards sit on the same scrim as the modals; the panels keep their
        // keys. Every modal below stacks on this base, so it stays above them.
        let module_card = self
            .hermes
            .filing
            .as_ref()
            .filter(|module| module.has_card())
            .and_then(|module| filing_ui::view(module, self.window_size, self.lang, &self.palette))
            .map(|card| card.map(Message::from));
        let cards: Vec<Element<'_, Message>> = module_card.into_iter().collect();
        let base: Element<'_, Message> = if cards.is_empty() {
            window.into()
        } else {
            let card = Column::with_children(cards)
                .spacing(theme::spacing::LINE_GAP)
                .align_x(iced::Alignment::Center);
            Stack::with_children([window.into(), dialog::scrim(card.into())]).into()
        };
        let root = if cfg!(target_os = "macos") {
            base
        } else {
            titlebar::with_resize_edges(base)
        };

        if let Some(menu) = self.hermes.quick_fix.as_ref() {
            let click_catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::QuickFix(QuickFixMessage::Close));
            return Stack::with_children([
                root,
                click_catcher.into(),
                quick_fix_view::view(menu, self.lang, &self.palette).map(Message::from),
            ])
            .into();
        }

        if let Some(menu) = ncrs_monitor::menu_view(&self.monitor, self.monitor_view_ctx()) {
            return Stack::with_children([root, menu.map(Message::Monitor)]).into();
        }

        if let Some(dialog_element) =
            ncrs_monitor::dialog_view(&self.monitor, self.monitor_view_ctx())
        {
            return Stack::with_children([
                root,
                dialog::scrim(dialog_element.map(Message::Monitor)),
            ])
            .into();
        }

        if let Some(menu) = self.panels.header_menu.as_ref() {
            return Stack::with_children([
                root,
                ncrs_files::header_menu_view(
                    menu,
                    self.column_settings(),
                    self.lang,
                    &self.palette,
                )
                .map(Message::Panel),
            ])
            .into();
        }

        if let Some(menu) = self.panels.context_menu.as_ref() {
            // A left press anywhere beside the menu closes it, the path field
            // and the column titles included: those take their own clicks and
            // would never report one. A right press passes through, so another
            // row's menu replaces this one.
            // A Ctrl+click on a Mac is a right click and passes through too.
            let mut click_catcher =
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill));
            if !ncrs_files::ctx::is_context_click(self.modifiers) {
                click_catcher = click_catcher.on_press(Message::ContextMenuClose);
            }
            return Stack::with_children([
                root,
                click_catcher.into(),
                context_menu_view::view(
                    menu,
                    self.lang,
                    &context_menu_view::EntryKeys {
                        context: &|action| self.keymap.shortcut_label(&action.message()),
                        hermes: &|action| self.keymap.shortcut_label(&action.message()),
                    },
                    &self.palette,
                )
                .map(Message::from),
            ])
            .into();
        }

        if let Some(menu) = self.panels.volume_menu.as_ref() {
            let menu_element = volumes_view::view(
                menu.side,
                volumes_view::MenuRows {
                    volumes: &menu.volumes,
                    favourites: &menu.favourites,
                    servers: &menu.servers,
                    trash: menu.trash,
                },
                menu.selected,
                self.window_size,
                self.lang,
                &self.palette,
                volumes_view::MenuMessages {
                    on_click: Box::new(Message::VolumeMenuClick),
                    on_close: Message::VolumeMenuClose,
                },
            );
            return Stack::with_children([root, dialog::scrim(menu_element)]).into();
        }

        if let Some(deletion) = self.ops.delete_dialog.as_ref() {
            let dialog_element = delete_dialog_view::view(
                &ncrs_ops::delete_subject(self.lang, deletion),
                deletion.permanent,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        if let Some(dialog) = self.ops.permissions_dialog.as_ref() {
            let dialog_element = ncrs_ops::permissions::view::view(
                dialog,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        if let Some(card) = self.about.as_ref() {
            let card_element = about_ui::view(
                card,
                self.app_version,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element)]).into();
        }

        if let Some(card) = self.settings.ocr_models.as_ref() {
            let card_element = ocr_models_ui::view(
                card,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([
                root,
                dialog::scrim(card_element.map(Message::OcrModels)),
            ])
            .into();
        }

        if let Some(card) = self.settings.ai_model.as_ref() {
            let card_element = ai_model_ui::view(
                card,
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(card_element.map(Message::AiModel))])
                .into();
        }

        if let Some(dialog_element) = self
            .settings
            .page
            .as_ref()
            .filter(|_| self.settings_shown())
            .and_then(|state| {
                ncrs_settings::dialogs::open_dialog(&self.settings_view(state), self.window_size)
            })
        {
            return Stack::with_children([
                root,
                dialog::scrim(dialog_element.map(Message::Settings)),
            ])
            .into();
        }

        if let Some(dialog_element) = ncrs_hermes_ui::dialogs(&self.hermes, &self.hermes_view_ctx())
            .into_iter()
            .next()
        {
            return Stack::with_children([root, dialog::scrim(dialog_element.map(Message::from))])
                .into();
        }

        if self.start_over_dialog {
            let question = hermes_overview_ui::start_over_dialog(
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(question)]).into();
        }

        if let Some(dialog) = self.remote_ui.dialog.as_ref() {
            let dialog_element =
                remote_ui::view(dialog, self.window_size, self.lang, &self.palette)
                    .map(Message::from);
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        if self.grant_dialog {
            let dialog_element = grant_ui::view(
                self.dialog_focus,
                self.window_size,
                self.lang,
                &self.palette,
            );
            return Stack::with_children([root, dialog::scrim(dialog_element)]).into();
        }

        // The conflict dialog comes first, before the prompt. A conflict is
        // asked *after* the prompt is gone — the user submitted, and the name
        // turned out to be taken — so behind the prompt's early return the
        // dialog was never drawn. That was a real bug, and the snapshot test is
        // what found it.
        if self.ops.conflict_is_open() {
            let name = self.ops.conflict_name();
            let conflict_element = conflict::view(
                &name,
                self.lang,
                &self.palette,
                self.ops.conflict_all,
                self.dialog_focus,
                self.window_size,
            )
            .map(Message::from);
            return Stack::with_children([root, dialog::scrim(conflict_element)]).into();
        }

        if let Some(confirm) = self.ops.transfer_dialog.as_ref() {
            let confirm_element =
                transfer_ui::confirm(confirm, self.window_size, self.lang, &self.palette)
                    .map(Message::from);
            return Stack::with_children([root, dialog::scrim(confirm_element)]).into();
        }

        if let Some(running) = self.ops.progress_mask(self.job_progress) {
            let progress_element =
                transfer_ui::progress(&running, self.window_size, self.lang, &self.palette)
                    .map(Message::from);
            return Stack::with_children([root, dialog::scrim(progress_element)]).into();
        }

        if let Some(quick_open) = self.quick_open.as_ref() {
            let box_element =
                quick_open_ui::view(quick_open, self.window_size, self.lang, &self.palette)
                    .map(Message::QuickOpen);
            return Stack::with_children([root, dialog::scrim(box_element)]).into();
        }

        if let Some(shortcuts) = self.settings.key_bindings.as_ref() {
            let dialog_element =
                key_bindings_ui::view(shortcuts, self.window_size, self.lang, &self.palette);
            return Stack::with_children([
                root,
                dialog::scrim(dialog_element.map(Message::KeyBindings)),
            ])
            .into();
        }

        if let Some(command_palette) = self.command_palette.as_ref() {
            let box_element = command_palette_ui::view(
                command_palette,
                self.window_size,
                self.lang,
                &self.palette,
            )
            .map(Message::CommandPalette);
            return Stack::with_children([root, dialog::scrim(box_element)]).into();
        }

        if let Some(card) = self.settings.theme_fetch.as_ref() {
            let card_element =
                theme_fetch_ui::view(card, self.window_size, self.lang, &self.palette);
            return Stack::with_children([
                root,
                dialog::scrim(card_element.map(Message::ThemeFetch)),
            ])
            .into();
        }

        // Then the create-directory prompt, for F7.
        let Some(prompt) = self.ops.prompt.as_ref() else {
            return root;
        };
        let overlay = dialog::view(
            prompt,
            self.window_size,
            self.lang,
            &self.palette,
            PromptMessages {
                on_input: Box::new(Message::PromptInput),
                on_submit: Message::PromptSubmit,
                on_cancel: Message::PromptCancel,
                on_toggle_keep_folders: Message::PromptToggleKeepFolders,
            },
        );
        let Some(prompt_element) = overlay else {
            return root;
        };

        // The prompt sits above everything and takes every keystroke; the scrim
        // swallows clicks meant for the panels underneath.
        Stack::with_children([root, dialog::scrim(prompt_element)]).into()
    }

    pub(in crate::app) fn hermes_view_ctx(&self) -> ncrs_hermes_ui::ViewCtx<'_> {
        ncrs_hermes_ui::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            window_size: self.window_size,
            file_list_tags: &self.file_list_tags,
            jobs_busy: self.jobs.is_busy(),
            ai_download: self
                .settings
                .ai_model
                .as_ref()
                .filter(|card| card.is_downloading())
                .and_then(AiModelCard::progress),
            system_reads_scans: crate::ocr_models::system_reads_scans(),
        }
    }
}
