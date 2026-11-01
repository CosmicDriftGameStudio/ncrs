//! The filing logic of ncrs: the filing module that opens the index and starts
//! the pipeline, the work behind the review, rules, tags, query and
//! reorganisation pages, and the reading of the Hermes column. Knows nothing
//! about the UI.

pub mod column_reading;
pub mod execute;
pub mod filing;
pub mod filing_work;
pub mod query_work;
pub mod reorganisation_work;
pub mod replan_work;
pub mod review_work;
pub mod rules_work;
pub mod suggestion_work;
pub mod tags_work;
