use crate::app::*;

impl App {
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

    /// Opens the settings on the page of Settings that shows this Hermes page.
    pub(in crate::app) fn show_hermes_page(&mut self, page: HermesPage) -> Task<Message> {
        self.show_setting(settings_feature_of(page), None)
    }

    /// Brings the Hermes state in line with the settings page that is up: the
    /// page is loaded when it is shown and let go when the settings leave it.
    pub(in crate::app) fn sync_hermes_page(&mut self) -> Task<Message> {
        let wanted = self.settings.page.as_ref().and_then(|state| {
            settings_ui::shown_feature(&self.settings_view(state)).and_then(hermes_page_of)
        });
        if wanted == self.hermes.open_page() {
            return Task::none();
        }
        match wanted {
            Some(page) => {
                let outcome = self.with_hermes(Vec::new(), |hermes, ctx| {
                    ncrs_hermes_ui::show_page(hermes, page, ctx)
                });
                self.run_hermes_outcome(outcome)
            }
            None => ncrs_hermes_ui::leave_pages(&mut self.hermes).map(Message::from),
        }
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
        let outcome = self.with_hermes(Vec::new(), |hermes, _ctx| {
            ncrs_hermes_ui::update_rules(hermes, message)
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
        self.show_hermes_page(HermesPage::SavedQueries)
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
}

fn settings_feature_of(page: HermesPage) -> crate::messages::SettingsFeature {
    use crate::messages::SettingsFeature;
    match page {
        HermesPage::Rules => SettingsFeature::Rules,
        HermesPage::SavedQueries => SettingsFeature::Reports,
        HermesPage::Tags => SettingsFeature::Tags,
        HermesPage::Inspection => SettingsFeature::ReadErrors,
    }
}

pub(in crate::app) fn hermes_page_of(
    feature: crate::messages::SettingsFeature,
) -> Option<HermesPage> {
    use crate::messages::SettingsFeature;
    match feature {
        SettingsFeature::Rules => Some(HermesPage::Rules),
        SettingsFeature::Reports => Some(HermesPage::SavedQueries),
        SettingsFeature::Tags => Some(HermesPage::Tags),
        SettingsFeature::ReadErrors => Some(HermesPage::Inspection),
        _ => None,
    }
}
