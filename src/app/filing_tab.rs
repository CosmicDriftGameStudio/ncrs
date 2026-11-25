//! The filing tab: what the rules make of the files of one source.

use crate::app::*;
use ncrs_filing_view::Action as Ask;
use ncrs_intake::SortSource;

impl App {
    fn filing_ctx(&self) -> ncrs_filing_view::Ctx {
        ncrs_filing_view::Ctx {
            lang: self.lang,
            access: self
                .hermes
                .filing
                .as_ref()
                .and_then(|module| module.sorting_access(Arc::clone(&self.trash))),
        }
    }

    pub(in crate::app) fn filing_area(&self) -> Container<'_, Message> {
        let ctx = ncrs_filing_view::ViewCtx {
            lang: self.lang,
            palette: &self.palette,
            window: self.window_size,
            home: &self.home,
        };
        container(ncrs_filing_view::view(&self.filing_view, ctx).map(Message::FilingView))
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// Opens the tab on the plan of `source`; `once` for a folder that is filed
    /// one time, otherwise the tab shows what its filing left open.
    pub fn open_filing_plan(&mut self, source: SortSource, once: bool) -> Task<Message> {
        let was_open = self.open_pages();
        let ctx = self.filing_ctx();
        let task = ncrs_filing_view::open(&mut self.filing_view, source, once, &ctx)
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

    pub(in crate::app) fn run_filing_action(&mut self, action: Ask) -> Task<Message> {
        match action {
            Ask::Close => Task::none(),
            Ask::PickFolder { start } => self.pick_granted_folder(Some(start), |folder| {
                Message::FilingView(ncrs_filing_view::Message::FolderPicked(folder))
            }),
            Ask::OpenRuleForm {
                target,
                document,
                path,
            } => self.run_hermes_flow(|hermes, ctx| {
                ncrs_hermes_ui::open_rule_form(hermes, target, document, Some(&path), ctx)
            }),
            Ask::SuggestRule { source, path } => {
                let Some(source) = self
                    .filing_source()
                    .filter(|shown| shown.path == source)
                    .cloned()
                else {
                    return Task::none();
                };
                self.run_hermes_flow(|hermes, ctx| {
                    ncrs_hermes_ui::open_suggest_rule(hermes, source, path, ctx)
                })
            }
            Ask::Filed { .. } => self.reload_panels(),
        }
    }

    fn filing_source(&self) -> Option<&SortSource> {
        self.filing_view
            .session
            .as_ref()
            .map(|session| &session.source)
    }
}
