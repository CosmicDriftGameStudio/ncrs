use crate::app::*;
use ncrs_ui_kit::kit::scrim;
use ncrs_ui_kit::popup_menu::{popup_layer, RightPress};

impl App {
    /// The layer of `modal` over the window, if it has something to show.
    pub(in crate::app) fn modal_layer(&self, modal: Modal) -> Option<Element<'_, Message>> {
        let (window, lang, palette) = (self.window_size, self.lang, &self.palette);
        let focus = self.dialog_focus;
        let layer = match modal {
            Modal::QuickFix => popup_layer(
                quick_fix_view::view(self.hermes.quick_fix.as_ref()?, lang, palette)
                    .map(Message::from),
                Some(Message::QuickFix(QuickFixMessage::Close)),
                RightPress::PassThrough,
            ),
            Modal::HeaderMenu => ncrs_files::header_menu_view(
                self.panels.header_menu.as_ref()?,
                self.column_settings(),
                lang,
                palette,
            )
            .map(Message::Panel),
            Modal::FavouriteMenu => self.favourite_menu_view()?,
            Modal::ContextMenu => {
                let menu = self.panels.context_menu.as_ref()?;
                // A left press anywhere beside the menu closes it, the path
                // field and the column titles included: those take their own
                // clicks and would never report one. A right press passes
                // through, so another row's menu replaces this one.
                // A Ctrl+click on a Mac is a right click and passes through too.
                let on_dismiss = (!ncrs_files::ctx::is_context_click(self.modifiers))
                    .then_some(Message::ContextMenuClose);
                popup_layer(
                    context_menu_view::view(
                        menu,
                        lang,
                        &context_menu_view::EntryKeys {
                            context: &|action| self.keymap.shortcut_label(&action.message()),
                            hermes: &|action| self.keymap.shortcut_label(&action.message()),
                        },
                        palette,
                    )
                    .map(Message::from),
                    on_dismiss,
                    RightPress::PassThrough,
                )
            }
            Modal::VolumeMenu => {
                let menu = self.panels.volume_menu.as_ref()?;
                scrim(volumes_view::view(
                    menu.side,
                    volumes_view::MenuRows {
                        volumes: &menu.volumes,
                        favourites: &menu.favourites,
                        servers: &menu.servers,
                        trash: menu.trash,
                    },
                    menu.selected,
                    window,
                    lang,
                    palette,
                    volumes_view::MenuMessages {
                        on_click: Box::new(|index| {
                            Message::Panel(PanelMessage::VolumeMenuClick(index))
                        }),
                        on_close: Message::Panel(PanelMessage::VolumeMenuClose),
                    },
                ))
            }
            Modal::Delete => {
                let deletion = self.ops.delete_dialog.as_ref()?;
                scrim(
                    delete_dialog_view::view(
                        &ncrs_ops::delete_subject(lang, deletion),
                        deletion.permanent,
                        focus,
                        window,
                        lang,
                        palette,
                    )
                    .map(Message::from),
                )
            }
            Modal::Quit => {
                scrim(ncrs_ops::quit_dialog::view(focus, window, lang, palette).map(Message::from))
            }
            Modal::OpenFile => {
                let name = self
                    .ops
                    .open_file_dialog
                    .as_ref()?
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                scrim(
                    ncrs_ops::open_file::view(&name, focus, window, lang, palette)
                        .map(Message::from),
                )
            }
            Modal::BatchRename => scrim(
                ncrs_ops::batch_rename::view::view(
                    self.ops.batch_rename.as_ref()?,
                    window,
                    lang,
                    palette,
                )
                .map(Message::from),
            ),
            Modal::Permissions => scrim(
                ncrs_ops::permissions::view::view(
                    self.ops.permissions_dialog.as_ref()?,
                    window,
                    lang,
                    palette,
                )
                .map(Message::from),
            ),
            Modal::About => scrim(
                about_ui::view(
                    self.about.as_ref()?,
                    self.app_version,
                    focus,
                    window,
                    lang,
                    palette,
                )
                .map(Message::About),
            ),
            Modal::OcrModels => scrim(
                ocr_models_ui::view(
                    self.settings.ocr_models.as_ref()?,
                    focus,
                    window,
                    lang,
                    palette,
                )
                .map(Message::OcrModels),
            ),
            Modal::AiModel => scrim(
                ai_model_ui::view(
                    self.settings.ai_model.as_ref()?,
                    focus,
                    window,
                    lang,
                    palette,
                )
                .map(Message::AiModel),
            ),
            Modal::ConfirmAgain => scrim(
                ncrs_hermes_ui::confirm_again_dialog(&self.hermes, &self.hermes_view_ctx())?
                    .map(Message::from),
            ),
            Modal::StartOver => scrim(
                hermes_overview_ui::start_over_dialog(
                    self.hermes
                        .filing
                        .as_ref()
                        .is_some_and(crate::filing::FilingModule::index_is_plain),
                    focus,
                    window,
                    lang,
                    palette,
                )
                .map(Message::from),
            ),
            Modal::Compare => scrim(
                ncrs_compare::view::dialog(&self.compare, window, lang, palette)?
                    .map(Message::from),
            ),
            Modal::Connect => scrim(
                remote_ui::view(self.remote_ui.dialog.as_ref()?, window, lang, palette)
                    .map(Message::from),
            ),
            Modal::Grant => scrim(grant_ui::view(focus, window, lang, palette)),
            Modal::Conflict => scrim(
                conflict::view(
                    &self.ops.conflict_name(),
                    lang,
                    palette,
                    self.ops.conflict_all,
                    self.ops.conflict_folder,
                    focus,
                    window,
                )
                .map(Message::from),
            ),
            Modal::TransferDialog => scrim(
                transfer_ui::confirm(self.ops.transfer_dialog.as_ref()?, window, lang, palette)
                    .map(Message::from),
            ),
            Modal::Progress => scrim(
                transfer_ui::progress(
                    &self.ops.progress_mask(self.job_progress)?,
                    focus,
                    window,
                    lang,
                    palette,
                )
                .map(Message::from),
            ),
            Modal::QuickOpen => scrim(
                quick_open_ui::view(self.quick_open.as_ref()?, window, lang, palette)
                    .map(Message::QuickOpen),
            ),
            Modal::KeyBindings => scrim(
                key_bindings_ui::view(self.settings.key_bindings.as_ref()?, window, lang, palette)
                    .map(Message::KeyBindings),
            ),
            Modal::CommandPalette => scrim(
                command_palette_ui::view(self.command_palette.as_ref()?, window, lang, palette)
                    .map(Message::CommandPalette),
            ),
            Modal::ThemeFetch => scrim(
                theme_fetch_ui::view(self.settings.theme_fetch.as_ref()?, window, lang, palette)
                    .map(Message::ThemeFetch),
            ),
            // The prompt takes every keystroke; the scrim swallows clicks
            // meant for the panels underneath.
            Modal::Prompt => scrim(dialog::view(
                self.ops.prompt.as_ref()?,
                window,
                lang,
                palette,
                PromptMessages {
                    on_input: Box::new(Message::PromptInput),
                    on_submit: Message::PromptSubmit,
                    on_cancel: Message::PromptCancel,
                    on_toggle_keep_folders: Message::PromptToggleKeepFolders,
                    on_format: Box::new(Message::PromptFormat),
                },
            )?),
        };
        Some(layer)
    }

    /// The menu or dialog of the page that is up, below every overlay of the
    /// modal stack.
    pub(in crate::app) fn page_dialog_layer(&self) -> Option<Element<'_, Message>> {
        if let Some(menu) = ncrs_monitor::menu_view(&self.monitor, self.monitor_view_ctx()) {
            return Some(menu.map(Message::Monitor));
        }
        if let Some(dialog) = ncrs_monitor::dialog_view(&self.monitor, self.monitor_view_ctx()) {
            return Some(scrim(dialog.map(Message::Monitor)));
        }
        if let Some(dialog) = self
            .settings
            .page
            .as_ref()
            .filter(|_| self.settings_shown())
            .and_then(|state| {
                ncrs_settings::dialogs::open_dialog(&self.settings_view(state), self.window_size)
            })
        {
            return Some(scrim(dialog.map(Message::Settings)));
        }
        let dialog = ncrs_hermes_ui::dialogs(&self.hermes, &self.hermes_view_ctx())
            .into_iter()
            .next()?;
        Some(scrim(dialog.map(Message::from)))
    }
}
