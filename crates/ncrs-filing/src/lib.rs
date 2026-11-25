//! The filing logic of ncrs: the filing module that opens the index and starts
//! the pipeline, sorting by sources, targets and rules, the work behind the
//! rules, tags and query pages, and the reading of the Hermes column. Knows
//! nothing about the UI.

pub mod chat_work;
pub mod column_reading;
pub mod execute;
pub mod filing;
mod index_slot;
pub mod index_work;
pub mod paths_work;
pub mod query_work;
mod reading_support;
pub mod rules_work;
pub mod sorting;
pub mod tags_work;
