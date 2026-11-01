use crate::quick_fix::QuickFix;
use crate::reorganisation_plan::ReorganisationView;
use crate::review_list::ReviewList;

/// What only the Hermes pages use: the pages that are up. Each page has a
/// generation, so the answer of work that was started for a page that was
/// closed and opened again is not taken for the answer of the new one.
#[derive(Debug, Default)]
pub struct Hermes {
    pub review_list: Option<ReviewList>,
    pub review_list_generation: u64,
    /// The fixes for an inquiry; at most one of it and the context menu.
    pub quick_fix: Option<QuickFix>,
    pub quick_fix_generation: u64,
    pub reorganisation: Option<ReorganisationView>,
    pub reorganisation_generation: u64,
}

impl Hermes {
    /// A page of this state stands in for the panels.
    pub fn page_is_open(&self) -> bool {
        self.review_list.is_some() || self.reorganisation.is_some()
    }

    pub fn close_pages(&mut self) {
        self.review_list = None;
        self.reorganisation = None;
    }
}
