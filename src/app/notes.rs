use crate::app::*;
use ncrs_notes::Action as Ask;

/// Longest part of a note's first line the status bar shows.
const NOTE_LINE_CHARS: usize = 60;

impl App {
    fn notes_ctx(&self) -> ncrs_notes::Ctx {
        ncrs_notes::Ctx { lang: self.lang }
    }

    pub(in crate::app) fn notes_area(&self) -> Container<'_, Message> {
        let new_note_key = ncrs_config::keymap::binding_label(&ncrs_config::keymap::Binding {
            key: iced::keyboard::Key::Character("n".into()),
            modifiers: iced::keyboard::Modifiers::COMMAND,
        })
        .unwrap_or_default();
        let ctx = ncrs_notes::ViewCtx {
            disk_warning: self.disk_warning_percent(),
            lang: self.lang,
            palette: &self.palette,
            home: &self.home,
            date_format: self.columns.date_format,
        };
        container(ncrs_notes::view(&self.notes, ctx, &new_note_key).map(Message::Notes))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// The first read of the notes folder, at start, so the file rows know
    /// which files have a note before the tab is ever opened.
    pub(in crate::app) fn start_notes(&mut self) -> Task<Message> {
        ncrs_notes::reload(&mut self.notes).map(Message::Notes)
    }

    /// The command list's "Notes": shows the tab and reads the folder again.
    pub(in crate::app) fn open_notes(&mut self) -> Task<Message> {
        let was_open = self.open_pages();
        let task = ncrs_notes::open(&mut self.notes).map(Message::Notes);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Notes;
        task
    }

    /// The file under the cursor of the active panel, when it is a local file.
    pub(in crate::app) fn file_under_cursor(&self) -> Option<PathBuf> {
        let panel = self.active_panel();
        if panel.is_virtual() {
            return None;
        }
        panel
            .selected_entry()
            .filter(|entry| !entry.is_dir && !entry.is_parent)
            .map(|entry| panel.path.join(&entry.name))
    }

    /// ⌘⇧N: the note on the file under the cursor, ready to type.
    pub(in crate::app) fn note_on_file(&mut self) -> Task<Message> {
        if self.overlay_is_open() {
            return Task::none();
        }
        let Some(file) = self.file_under_cursor() else {
            return Task::none();
        };
        let task = self.update_notes(ncrs_notes::Message::EditNoteOn(file));
        self.shown_tab = ShownTab::Notes;
        if self.notes.loaded || self.notes.loading {
            task
        } else {
            let reload = ncrs_notes::reload(&mut self.notes).map(Message::Notes);
            Task::batch([task, reload])
        }
    }

    pub(in crate::app) fn update_notes(&mut self, message: ncrs_notes::Message) -> Task<Message> {
        let was_open = self.open_pages();
        if matches!(message, ncrs_notes::Message::OpenMenu(_)) {
            self.notes.menu_pointer = self.input.lock().pointer;
        }
        let ctx = self.notes_ctx();
        let (task, asked) = ncrs_notes::update(&mut self.notes, message, &ctx);
        let action_task = match asked {
            Some(action) => self.run_notes_action(action),
            None => Task::none(),
        };
        self.follow_opened_pages(was_open);
        self.show_notes_state_in_settings();
        Task::batch([task.map(Message::Notes), action_task])
    }

    /// What the settings say about the notes. Turning "encrypt all" on also
    /// encrypts the notes that exist.
    pub(in crate::app) fn apply_notes_settings(
        &mut self,
        settings: &ncrs_core::notes_sync::NotesSettings,
    ) -> Task<Message> {
        let was_encrypting = self.notes.options.encrypt_all;
        ncrs_notes::apply_settings(&mut self.notes, settings);
        if settings.encrypt_all && !was_encrypting {
            return self.update_notes(ncrs_notes::Message::Seal(
                ncrs_notes::Seal::Everything,
                true,
            ));
        }
        Task::none()
    }

    /// The sync and lock state the Notes page of the settings shows.
    fn show_notes_state_in_settings(&mut self) {
        let last = self
            .notes
            .sync
            .last
            .as_ref()
            .and_then(|report| report.finished);
        let line = last.map(|finished| {
            ncrs_ui_kit::format::time_in(Some(finished), self.columns.date_format, self.lang)
        });
        let (syncing, unlocked) = (self.notes.sync.running, self.notes.vault.is_unlocked());
        if let Some(page) = self.settings.page.as_mut() {
            page.notes.last_sync = line;
            page.notes.syncing = syncing;
            page.notes.unlocked = unlocked;
        }
    }

    fn run_notes_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::AskWhyDiskFull => self.update_hermes_chat(ncrs_hermes_chat::Message::SpaceAsked),
            Ask::Close => Task::none(),
            Ask::OpenUrl(url) => {
                self.open_link(&url);
                Task::none()
            }
            Ask::Reveal(file) => self.reveal_in_active_panel(&file),
            Ask::ResolveLinks(links) => {
                let Some(finder) = self
                    .hermes
                    .filing
                    .as_ref()
                    .and_then(FilingModule::note_link_finder)
                else {
                    return Task::none();
                };
                let asked: Vec<PathBuf> = links.iter().map(|link| link.file.clone()).collect();
                Task::perform(
                    tokio::task::spawn_blocking(move || finder.resolve(&links)),
                    move |joined| {
                        Message::Notes(ncrs_notes::Message::LinksResolved(
                            asked.clone(),
                            joined.unwrap_or_else(|err| Err(err.to_string())),
                        ))
                    },
                )
            }
            Ask::Notice(text) => self.show_notice(text),
            Ask::RecoveryKey(key) => {
                if let Some(page) = self.settings.page.as_mut() {
                    page.notes.recovery = Some(key);
                }
                Task::none()
            }
            Ask::Error(reason) => {
                self.set_status_error(&reason);
                Task::none()
            }
        }
    }
}

impl App {
    /// What the status bar says about the note on the file under the cursor.
    pub(in crate::app) fn note_line(&self) -> Option<String> {
        let file = self.file_under_cursor()?;
        let first = self.notes.note_on(&file)?.first_line().unwrap_or_default();
        let short = ncrs_ui_kit::format::ellipsize_end(first, NOTE_LINE_CHARS);
        Some(ncrs_i18n::fill_template(
            self.lang.text(Msg::NotesStatusNote),
            &[("{line}", &short)],
        ))
    }
}
