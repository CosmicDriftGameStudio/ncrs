use crate::app::*;

impl App {
    /// The model of the chat and the background filing, instead of the local
    /// one; it stays while the AI setting stays the same.
    pub fn set_chat_model_for_test(&mut self, model: Arc<dyn ncrs_ai::ChatModel>) {
        self.chat_model = Some(model);
        self.chat_model_setting = Some(self.hermes.filing_config.ai.clone());
        self.chat_model_error = None;
    }

    /// The flow that writes "files continuously" for a read folder into the
    /// config, as the end of the filing tab runs it.
    pub fn switch_continuous_for_test(
        &mut self,
        chosen: &std::path::Path,
        transfer: ncrs_config::config::LocationTransfer,
    ) -> Task<Message> {
        let folder = chosen.to_path_buf();
        self.run_hermes_flow(Vec::new(), move |hermes, ctx| {
            ncrs_hermes_ui::set_continuous_filing(hermes, &folder, Some(transfer), ctx)
        })
    }

    /// What the folder dialog of the reviews answers.
    pub fn set_folder_picker_for_test(&mut self, pick: fn(&Path) -> Task<Option<PathBuf>>) {
        self.pick_folder = pick;
    }

    pub fn reviews_page_for_test(&self) -> Option<&ncrs_hermes_ui::reviews_page::ReviewsPage> {
        self.hermes.reviews.as_ref()
    }

    pub fn activity_page_for_test(&self) -> Option<&ncrs_hermes_ui::activity_page::ActivityPage> {
        self.hermes.activity.as_ref()
    }

    /// The pipeline is up, idle, and no file waits in any step; candidates the
    /// continuous filing is unsure of stay waiting for good.
    pub fn pipeline_is_quiet_for_test(&self) -> bool {
        self.hermes.live_pipeline().is_some_and(|runner| {
            let snapshot = runner.snapshot();
            snapshot.state == ncrs_pipeline::RunnerState::Idle
                && snapshot.steps.iter().all(|step| step.waiting == 0)
        })
    }
}
