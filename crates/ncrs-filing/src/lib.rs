//! The filing logic of ncrs: the filing module that opens the index and starts
//! the pipeline, the work behind the review, rules, tags, query and
//! reorganisation pages, and the reading of the Hermes column. Knows nothing
//! about the UI.

pub mod chat_work;
pub mod column_reading;
pub mod execute;
pub mod filing;
pub mod filing_work;
mod index_slot;
pub mod index_work;
pub mod query_work;
mod reading_support;
pub mod review_work;
pub mod rules_work;
pub mod sorting;
pub mod tags_work;
