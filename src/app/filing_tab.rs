//! The filing tab: a source folder planned live into the write folder.

use crate::app::*;
use ncrs_filing_view::Action as Ask;

impl App {
    fn filing_ctx(&mut self) -> ncrs_filing_view::Ctx {
        self.ensure_chat_model();
        let model = self.chat_ai_on().then(|| self.chat_model.clone()).flatten();
        let access = match self.chat_availability() {
            ncrs_hermes_chat::Availability::Ready(ready) => {
                Some((*ready).with_model(model.clone()))
            }
            _ => None,
        };
        ncrs_filing_view::Ctx {
            lang: self.lang,
            access,
            model,
        }
    }

    pub(in crate::app) fn filing_area(&self) -> Container<'_, Message> {
        let ctx = ncrs_filing_view::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            window: self.window_size,
        };
        container(ncrs_filing_view::view(&self.filing_view, ctx).map(Message::FilingView))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// Opens the tab and starts planning `source` into `target`.
    pub fn open_filing(&mut self, source: PathBuf, target: PathBuf) -> Task<Message> {
        let was_open = self.open_pages();
        let ctx = self.filing_ctx();
        let task = ncrs_filing_view::start(&mut self.filing_view, source, target, &ctx)
            .map(Message::FilingView);
        self.follow_opened_pages(was_open);
        self.shown_tab = ShownTab::Filing;
        task
    }

    pub(in crate::app) fn update_filing_tab(
        &mut self,
        message: ncrs_filing_view::Message,
    ) -> Task<Message> {
        let was_open = self.open_pages();
        let ctx = self.filing_ctx();
        let (task, asked) = ncrs_filing_view::update(&mut self.filing_view, message, &ctx);
        let action_task = match asked {
            Some(action) => self.run_filing_action(action),
            None => Task::none(),
        };
        self.follow_opened_pages(was_open);
        Task::batch([task.map(Message::FilingView), action_task])
    }

    fn run_filing_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::PickFolder { start } => (self.pick_folder)(&start)
                .map(|folder| Message::FilingView(ncrs_filing_view::Message::FolderPicked(folder))),
            Ask::Finished {
                source,
                transfer,
                filed,
                left,
                batch,
            } => {
                self.filing_finished(source, transfer, filed, left.len(), batch);
                Task::none()
            }
        }
    }

    /// The chat says what the run did; the user stays where they are.
    fn filing_finished(
        &mut self,
        source: PathBuf,
        transfer: ncrs_index::Transfer,
        filed: usize,
        left: usize,
        batch: ncrs_index::BatchId,
    ) {
        self.ensure_chat_model();
        let ctx = self.hermes_chat_ctx();
        ncrs_hermes_chat::filed_in_tab(
            &mut self.hermes_chat,
            &ctx,
            ncrs_hermes_chat::FiledInTab {
                source,
                transfer,
                filed,
                left,
                batch,
            },
        );
    }
}
