use std::path::{Component, Path, PathBuf};

use chrono::NaiveDate;

pub use crate::archive_profile::DateFormat;

/// A document of the location with the fields found for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureDocument {
    /// Below the root of the archive location.
    pub relative_path: PathBuf,
    pub date: Option<NaiveDate>,
    pub sender: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationStructure {
    /// Most documents first, then by name.
    pub areas: Vec<Area>,
    /// Sorted by folder.
    pub unordered: Vec<UnorderedFolder>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    pub folder: String,
    pub documents: u32,
    /// Most documents first.
    pub branches: Vec<Branch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// The folders below the area folder; empty for files directly in it.
    pub levels: Vec<Level>,
    pub name: Option<NameScheme>,
    /// Files directly in the last level.
    pub documents: u32,
    /// Of those, the files that follow the name scheme.
    pub following: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Literal(String),
    /// A closed list of folder names, sorted.
    Choice(Vec<String>),
    Placeholder(Placeholder),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    pub kind: PlaceholderKind,
    pub label: PlaceholderLabel,
    /// Up to three folder names, the ones with most documents first.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    /// `follows_date`: more than half of the dated documents below lie in the
    /// folder of their own year.
    Year {
        follows_date: bool,
    },
    Month,
    Date,
    Number,
    Sender,
    Kind,
    Subfolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderLabel {
    Year,
    Month,
    Date,
    Number,
    Sender,
    Kind,
    Bank,
    Insurer,
    Vehicle,
    Child,
    Property,
    Subfolder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameScheme {
    pub parts: Vec<NamePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePart {
    Literal(String),
    Separator(char),
    Date(DateFormat),
    Year,
    Number,
    Free,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnorderedFolder {
    /// Empty for the files directly in the location root.
    pub folder: String,
    pub documents: u32,
}

impl LocationStructure {
    pub fn is_unordered(&self, relative_path: &Path) -> bool {
        let mut components = relative_path.components();
        let first = components.next();
        let folder = if components.next().is_some() {
            match first {
                Some(Component::Normal(name)) => name.to_str(),
                _ => None,
            }
        } else {
            Some("")
        };
        folder.is_some_and(|name| self.unordered.iter().any(|entry| entry.folder == name))
    }
}
