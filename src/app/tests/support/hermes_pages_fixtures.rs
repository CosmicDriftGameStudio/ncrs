use crate::app::*;
use ncrs_hermes_ui::activity_page::ActivityPage;
use ncrs_hermes_ui::reviews_page::ReviewsPage;

impl App {
    /// Fixed panels with the reviews page up, in the state of `page`.
    pub fn with_reviews(page: ReviewsPage) -> Self {
        let mut app = Self::with_fixed_panels();
        app.hermes.reviews = Some(page);
        app.showing_setting(crate::messages::SettingsFeature::Reviews)
    }

    /// Fixed panels with the activity page up, after the pipeline was seen
    /// in `snapshot`.
    pub fn with_activity(
        page: ActivityPage,
        snapshot: ncrs_pipeline::Snapshot,
        taken_at: std::time::SystemTime,
    ) -> Self {
        let mut app = Self::with_filing_snapshot(snapshot, taken_at);
        app.hermes.activity = Some(page);
        app.showing_setting(crate::messages::SettingsFeature::Activity)
    }
}
