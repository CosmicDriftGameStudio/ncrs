//! The plain data types of the document index that other crates show or
//! compare: amounts, document kinds, rule condition kinds, status counts. No
//! database: `ncrs-index` re-exports them, so the UI crates need not build it.

mod amount;
mod condition_kind;
pub mod kinds;
mod names;
mod status_counts;

pub use amount::{amount_digits, amount_text, Amount, Currency, MINOR_UNITS_PER_UNIT};
pub use condition_kind::ConditionKind;
pub use kinds::{
    CustomKind, KindCatalog, KindEntry, KindLanguage, KindSkip, Kinds, MAX_FREE_KIND_CHARS,
};
pub use names::{name_key, tag_name};
pub use status_counts::StatusCounts;
