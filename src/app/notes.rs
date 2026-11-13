use crate::app::*;
use ncrs_notes::Action as Ask;

/// Longest part of a note's first line the status bar shows.
const NOTE_LINE_CHARS: usize = 60;

impl App {
    fn notes_ctx(&self) -> ncrs_notes::Ctx {
        ncrs_notes::Ctx { lang: self.lang }
    }

    pub(in crate::app) fn notes_area(&self) -> Container<'_, Message> {
        let ctx = ncrs_notes::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            home: &self.home,
        };
        container(ncrs_notes::view(&self.notes, ctx).map(Message::Notes))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// The first read of the notes folder, at start, so the file rows know
    /// which files have a note before the tab is ever opened.
    pub(in crate::app) fn start_notes(&mut self) -> Task<Message> {
        ncrs_notes::load(self.notes.root.clone()).map(Message::Notes)
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
        let reload = ncrs_notes::load(self.notes.root.clone()).map(Message::Notes);
        if self.notes.loaded {
            task
        } else {
            Task::batch([task, reload])
        }
    }

    pub(in crate::app) fn update_notes(&mut self, message: ncrs_notes::Message) -> Task<Message> {
        let was_open = self.open_pages();
        let ctx = self.notes_ctx();
        let (task, asked) = ncrs_notes::update(&mut self.notes, message, &ctx);
        self.noted = self.notes.noted_files();
        let action_task = match asked {
            Some(action) => self.run_notes_action(action),
            None => Task::none(),
        };
        self.follow_opened_pages(was_open);
        Task::batch([task.map(Message::Notes), action_task])
    }

    fn run_notes_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::OpenUrl(url) => {
                self.open_release_page(&url);
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
                let asked: Vec<PathBuf> = links.iter().map(|(_, path)| path.clone()).collect();
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
