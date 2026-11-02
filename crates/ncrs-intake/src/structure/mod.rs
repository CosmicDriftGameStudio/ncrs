//! Learns the structure of an archive location per area: which folders are
//! constants, which are placeholders for a year, a sender or a subfolder, which
//! are a closed list of choices, and how the files in them are named. Pure:
//! nothing here touches a file or the index.
//!
//! An area is a top-level folder and learns on its own; nothing is learned
//! across areas. A flat folder that follows no naming scheme, and files lying
//! directly in the location root, are unordered rather than an area.

mod branches;
mod describe;
mod folder_type;
mod learn;
mod name_scheme;
mod tree;
mod types;

#[cfg(test)]
mod tests;

pub use branches::folder_label;
pub use learn::learn_structure;
pub use types::{
    Area, Branch, DateFormat, Level, LocationStructure, NamePart, NameScheme, Placeholder,
    PlaceholderKind, PlaceholderLabel, StructureDocument, UnorderedFolder,
};
