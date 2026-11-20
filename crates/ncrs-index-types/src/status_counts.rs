#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub new: u64,
    pub planned: u64,
    pub in_review: u64,
    pub filed: u64,
}
