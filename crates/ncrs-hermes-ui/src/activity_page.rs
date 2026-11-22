//! State of the activity page: the latest runs that moved files. No widgets
//! and no tasks; the view is `ui::activity`.

use ncrs_filing::filing::ActivityBatch;

/// How many runs the page lists.
pub const MAX_BATCHES: usize = 20;

#[derive(Debug, Clone, PartialEq)]
enum Reading {
    Loading,
    Loaded(Vec<ActivityBatch>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityPage {
    reading: Reading,
    utc_times: bool,
}

impl ActivityPage {
    pub fn new() -> Self {
        Self {
            reading: Reading::Loading,
            utc_times: false,
        }
    }

    /// Writes the times of the runs in UTC instead of the local zone, for
    /// pictures that must not depend on the zone of the machine.
    pub fn in_utc(mut self) -> Self {
        self.utc_times = true;
        self
    }

    pub fn time_text(&self, at: Option<std::time::SystemTime>) -> String {
        if !self.utc_times {
            return ncrs_ui_kit::format::time(at);
        }
        at.map(|moment| {
            chrono::DateTime::<chrono::Utc>::from(moment)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
    }

    pub fn loaded(&mut self, batches: Vec<ActivityBatch>) {
        self.reading = Reading::Loaded(batches);
    }

    pub fn failed(&mut self, detail: String) {
        self.reading = Reading::Failed(detail);
    }

    pub fn is_loading(&self) -> bool {
        self.reading == Reading::Loading
    }

    pub fn problem(&self) -> Option<&str> {
        match &self.reading {
            Reading::Failed(detail) => Some(detail),
            Reading::Loading | Reading::Loaded(_) => None,
        }
    }

    pub fn batches(&self) -> &[ActivityBatch] {
        match &self.reading {
            Reading::Loaded(batches) => batches,
            Reading::Loading | Reading::Failed(_) => &[],
        }
    }
}

impl Default for ActivityPage {
    fn default() -> Self {
        Self::new()
    }
}
