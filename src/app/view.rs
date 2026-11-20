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
                    compare_marks: self.compare.marks(side),
                    hermes_cell,
                    columns: self.column_settings(),
                    special_folders: &self.special_folders,
                    noted: &self.notes.noted,
                    header_drag: self.panels.header_drag_view(side),
                    size_run: self.panels.folder_size_run.as_ref(),
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
                        .style(theme::outlined_in(self.palette.border))
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

        let below_title_bar = if self.filing_shown() {
            self.filing_area()
        } else if self.git_shown() {
            self.git_area()
        } else if self.notes_shown() {
            self.notes_area()
        } else if self.viewer_shown() {
            self.viewer_area()
        } else if self.monitor_shown() {
            self.monitor_area()
        } else if self.hermes_chat_shown() {
            self.hermes_chat_area()
        } else if let Some(state) = self
            .settings
            .page
            .as_ref()
            .filter(|_| self.settings_shown())
        {
            self.settings_area(state)
        } else {
            let bars: Vec<Element<'_, Message>> = self
                .favourites_bar_view()
                .into_iter()
                .chain(
                    ncrs_compare::view::bar(&self.compare, self.lang, &self.palette)
                        .map(|bar| bar.map(Message::from)),
                )
                .collect();
            let panels_area: Element<'_, Message> = if bars.is_empty() {
                panels.into()
            } else {
                iced::widget::Column::with_children(bars)
                    .push(panels)
                    .height(Length::Fill)
                    .into()
            };
            container(column![
                panels_area,
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
                        job_press: self.ops.transfer.is_some().then_some(Message::Ops(
                            OpsMessage::TransferDialog(TransferDialogMessage::ShowProgress,)
                        )),
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
                        note_line: self.note_line(),
                        git: self.git_line(),
                        version: Some(statusbar::VersionBadge {
                            text: self.app_version.to_owned(),
                            on_press: Message::VersionPressed,
                            update: self.newer_version_text(),
                        }),
                        monitor: self.monitor_badge(),
                        test_folder: config::data_dir_override().map(|dir| {
                            ncrs_i18n::fill_template(
                                self.lang.text(Msg::StatusTestFolder),
                                &[("{dir}", &dir.display().to_string())],
                            )
                        }),
                        date_format: self.columns.date_format,
                    },
                    self.status_error(),
                    self.search_status(),
                    statusbar::PathBar {
                        editing: self.panels.path_field.as_deref(),
                        on_input: |typed| Message::Panel(PanelMessage::PathFieldInput(typed)),
                        on_submit: Message::Panel(PanelMessage::PathFieldSubmit),
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
            Stack::with_children([window.into(), ncrs_ui_kit::kit::scrim(card.into())]).into()
        };
        let root = if cfg!(target_os = "macos") {
            base
        } else {
            titlebar::with_resize_edges(base)
        };

        let layer = self
            .prompt_key_state()
            .modal_stack()
            .into_iter()
            .find_map(|modal| self.modal_layer(modal))
            .or_else(|| self.page_dialog_layer());
        match layer {
            Some(on_top) => Stack::with_children([root, on_top]).into(),
            None => root,
        }
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
