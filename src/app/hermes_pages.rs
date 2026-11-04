use crate::app::*;

impl App {
    /// The tags page, from the settings or the command list: in place of the
    /// page of Hermes that is up, if any.
    pub(in crate::app) fn show_tags_page(&mut self) -> Task<Message> {
        let left = self.leave_settings_page();
        let opened = if self.open_hermes_page().is_some() {
            self.go_to_hermes_page(HermesPage::Tags)
        } else {
            self.open_tags()
        };
        Task::batch([left, opened])
    }

    pub(in crate::app) fn overview_facts(&self, ocr_models_present: bool) -> OverviewFacts {
        ncrs_hermes_ui::overview_facts(&self.hermes, &self.hermes_view_ctx(), ocr_models_present)
    }

    pub(in crate::app) fn update_hermes_overview(
        &mut self,
        message: HermesOverviewMessage,
    ) -> Task<Message> {
        let before = match &message {
            HermesOverviewMessage::EnterRecoveryKey => {
                self.update_filing(FilingMessage::AskKeyAgain)
            }
            _ => Task::none(),
        };
        let after = self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_hermes_overview(hermes, message, ctx)
        });
        Task::batch([before, after])
    }

    /// The Hermes card that is up, if any.
    pub(in crate::app) fn open_hermes_page(&self) -> Option<HermesPage> {
        self.hermes
            .filing_status
            .page_open()
            .then_some(HermesPage::Inspection)
            .or_else(|| self.hermes.open_page())
    }

    /// The card that is up closes without its Escape steps, the chosen page
    /// opens. Nothing happens when it is the open one,
    /// or when no Hermes card is up.
    pub(in crate::app) fn go_to_hermes_page(&mut self, page: HermesPage) -> Task<Message> {
        if page != HermesPage::Inspection {
            return self.run_hermes_flow(Vec::new(), |hermes, ctx| {
                ncrs_hermes_ui::go_to_hermes_page(hermes, page, ctx)
            });
        }
        match self.open_hermes_page() {
            None => return Task::none(),
            Some(open) if open == page => return Task::none(),
            Some(_) => {}
        }
        self.hermes.close_pages();
        self.hermes.filing_status.close_page();
        let open = self.open_filing_status();
        Task::batch([release_focus(), open])
    }

    pub(in crate::app) fn update_quick_fix(&mut self, message: QuickFixMessage) -> Task<Message> {
        let inquiry_paths = if matches!(message, QuickFixMessage::Open) {
            self.inquiry_paths()
        } else {
            Vec::new()
        };
        let outcome = self.with_hermes(inquiry_paths, |hermes, ctx| {
            ncrs_hermes_ui::update_quick_fix(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    pub(in crate::app) fn update_rules(&mut self, message: RulesMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_rules(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    pub(in crate::app) fn update_saved_queries(
        &mut self,
        message: SavedQueriesMessage,
    ) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_saved_queries(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    pub(in crate::app) fn update_tags(&mut self, message: TagsMessage) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_tags(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    pub(in crate::app) fn open_saved_queries(&mut self) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), ncrs_hermes_ui::open_saved_queries);
        self.run_hermes_outcome(outcome)
    }

    fn open_tags(&mut self) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), ncrs_hermes_ui::open_tags);
        self.run_hermes_outcome(outcome)
    }

    /// The full-text lookup of Hermes, when its module is ready. Without an
    /// index it answers with an error, which the box shows as no section.
    pub(in crate::app) fn document_finder(&self) -> Option<DocumentFinder> {
        self.hermes
            .filing
            .as_ref()
            .and_then(FilingModule::document_finder)
    }

    /// Sends the user to the Hermes settings, where Hermes is switched on,
    /// with the reason in the status bar.
    pub(in crate::app) fn explain_hermes_is_off(&mut self) -> Task<Message> {
        let shown = self.show_setting(crate::messages::SettingsFeature::Hermes, None);
        let reason = self.lang.text(Msg::StatusHermesIsOff).to_owned();
        self.set_status_error(&reason);
        shown
    }

    pub(in crate::app) fn open_filing_status(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        let left_settings = self.leave_settings_page();
        if self.settings.page.is_some() {
            return left_settings;
        }
        let opened = self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::open_filing_status_page);
        Task::batch([left_settings, opened])
    }
}
