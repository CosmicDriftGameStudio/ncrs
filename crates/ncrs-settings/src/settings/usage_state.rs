//! What the Usage page remembers: the sums of the log for each period it
//! offers, read when the page is opened, and the period shown.

use ncrs_ai::UsageSummary;

use crate::settings::SettingsState;

/// The periods of the page, in days.
pub const USAGE_PERIODS: [u32; 3] = [7, 30, 90];
pub const DEFAULT_USAGE_DAYS: u32 = 30;

impl SettingsState {
    /// The sums for the period shown; none until the log was read.
    pub fn usage_summary(&self) -> Option<&UsageSummary> {
        self.usage
            .as_ref()?
            .iter()
            .find(|(days, _)| *days == self.usage_days)
            .map(|(_, summary)| summary)
    }
}
