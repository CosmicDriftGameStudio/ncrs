use crate::app::*;

impl App {
    /// The model of the chat and the background filing, instead of the local
    /// one; it stays while the AI setting stays the same.
    pub fn set_chat_model_for_test(&mut self, model: Arc<dyn ncrs_ai::ChatModel>) {
        self.chat_model = Some(model);
        self.chat_model_setting = Some(self.hermes.filing_config.ai.clone());
        self.chat_model_error = None;
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
