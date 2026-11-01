use crate::app::*;

impl App {
    /// The tags page, from the settings or the command list: in place of the
    /// page of the Ablage tab that is up, if any.
    pub(in crate::app) fn show_tags_page(&mut self) -> Task<Message> {
        let left = self.leave_settings_page();
        let opened = if self.open_hermes_page().is_some() {
            self.show_hermes_page(HermesPage::Tags)
        } else {
            self.open_tags()
        };
        Task::batch([left, opened])
    }

    pub(in crate::app) fn overview_facts(&self, ocr_models_present: bool) -> OverviewFacts {
        ncrs_hermes_ui::overview_facts(&self.hermes, &self.hermes_view_ctx(), ocr_models_present)
    }

    pub(in crate::app) fn open_hermes_overview(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        let left_settings = self.leave_settings_page();
        if self.settings.page.is_some() {
            return left_settings;
        }
        let opened = self.run_hermes_flow(Vec::new(), ncrs_hermes_ui::open_overview);
        Task::batch([left_settings, opened])
    }

    pub(in crate::app) fn leave_hermes_overview(&mut self) -> Task<Message> {
        ncrs_hermes_ui::leave_overview(&mut self.hermes).map(Message::from)
    }

    pub(in crate::app) fn update_hermes_overview(
        &mut self,
        message: HermesOverviewMessage,
    ) -> Task<Message> {
        let before = match &message {
            HermesOverviewMessage::EnterRecoveryKey => {
                self.update_filing(FilingMessage::AskKeyAgain)
            }
            HermesOverviewMessage::ChangeFolders => {
                self.update_filing_setup(FilingSetupMessage::Toggle)
            }
            HermesOverviewMessage::SwitchHermesOn => {
                self.update_settings(SettingsMessage::Change(SettingChange::HermesEnabled(true)))
            }
            HermesOverviewMessage::PatternWritten(Ok(config)) => {
                self.hermes.filing_config.default_pattern = config.filing.default_pattern.clone();
                Task::none()
            }
            _ => Task::none(),
        };
        let write_failure = match &message {
            HermesOverviewMessage::PatternWritten(Err(error)) => Some(
                ncrs_settings::write::write_failure(error, &self.settings_ctx()),
            ),
            _ => None,
        };
        let after = self.run_hermes_flow(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_hermes_overview(hermes, message, ctx, write_failure)
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

    /// A click in the sidebar: the card that is up closes without its Escape
    /// steps, the chosen page opens. Nothing happens when it is the open one,
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

    pub(in crate::app) fn update_review_list(
        &mut self,
        message: ReviewListMessage,
    ) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_review_list(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
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

    pub(in crate::app) fn update_reorganisation(
        &mut self,
        message: ReorganisationPlanMessage,
    ) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_reorganisation(hermes, message, ctx)
        });
        self.run_hermes_outcome(outcome)
    }

    pub(in crate::app) fn update_rule_suggestions(
        &mut self,
        message: RuleSuggestionsMessage,
    ) -> Task<Message> {
        let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
            ncrs_hermes_ui::update_rule_suggestions(hermes, message, ctx)
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

    pub(in crate::app) fn request_review_preview(&mut self) -> Task<Message> {
        ncrs_hermes_ui::request_review_preview(&mut self.hermes).map(Message::from)
    }

    pub(in crate::app) fn show_hermes_page(&mut self, page: HermesPage) -> Task<Message> {
        self.hermes_focus = HermesFocus::Content;
        self.go_to_hermes_page(page)
    }

    pub(in crate::app) fn focus_hermes(&mut self, focus: HermesFocus) -> Task<Message> {
        if self.open_hermes_page().is_some() {
            self.hermes_focus = focus;
        }
        Task::none()
    }

    pub(in crate::app) fn step_hermes_page(&mut self, step: isize) -> Task<Message> {
        let next = self
            .open_hermes_page()
            .and_then(|page| crate::ui::hermes::neighbour(page, step));
        next.map_or_else(Task::none, |page| self.go_to_hermes_page(page))
    }

    /// The column shows only while the filing module is ready and has places
    /// to look at; otherwise the panels are drawn as they always were.
    /// The full-text lookup of Hermes, when its module is ready. Without an
    /// index it answers with an error, which the box shows as no section.
    pub(in crate::app) fn document_finder(&self) -> Option<DocumentFinder> {
        self.hermes
            .filing
            .as_ref()
            .and_then(FilingModule::document_finder)
    }

    /// Sends the user to the overview, where Hermes is switched on, with the
    /// reason in the status bar.
    pub(in crate::app) fn explain_hermes_is_off(&mut self) -> Task<Message> {
        let shown = self.open_hermes_overview();
        if self.hermes.overview.is_some() {
            let reason = self.lang.text(Msg::StatusHermesIsOff).to_owned();
            self.set_status_error(&reason);
        }
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
