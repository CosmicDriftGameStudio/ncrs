use std::collections::BTreeMap;

use ncrs_index::KindCatalog;

use super::types::StructureDocument;
use crate::archive_profile::{is_month, name_tokens, year_of, NameToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super) enum FolderType {
    Year,
    Month,
    Date,
    Number,
    Sender,
    Kind,
    Literal,
}

impl FolderType {
    /// Years, months, dates and numbers are always placeholders: they are
    /// never a constant and never part of a closed list.
    pub fn is_counting(self) -> bool {
        matches!(self, Self::Year | Self::Month | Self::Date | Self::Number)
    }
}

const MONTH_NAMES: [&str; 23] = [
    "januar",
    "februar",
    "märz",
    "april",
    "mai",
    "juni",
    "juli",
    "august",
    "september",
    "oktober",
    "november",
    "dezember",
    "january",
    "february",
    "march",
    "may",
    "june",
    "july",
    "october",
    "december",
    "jan",
    "feb",
    "mar",
];

/// What a folder name is, judged from the documents below the folder.
pub(in super) fn folder_type(
    name: &str,
    documents: &[&StructureDocument],
    below_year: bool,
    kinds: &KindCatalog,
) -> FolderType {
    if year_of(name).is_some() {
        FolderType::Year
    } else if below_year && (is_month(name) || is_month_name(name)) {
        FolderType::Month
    } else if matches!(name_tokens(name).as_slice(), [NameToken::Date(_)]) {
        FolderType::Date
    } else if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()) {
        FolderType::Number
    } else if names_the_sender(name, documents) {
        FolderType::Sender
    } else if kinds.resolve(name).is_some() {
        FolderType::Kind
    } else {
        FolderType::Literal
    }
}

fn is_month_name(name: &str) -> bool {
    MONTH_NAMES.contains(&name.to_lowercase().as_str())
}

/// Lower case without dashes and whitespace.
fn folded(text: &str) -> String {
    text.chars()
        .filter(|character| *character != '-' && !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn names_the_sender(name: &str, documents: &[&StructureDocument]) -> bool {
    let Some(sender) = dominant_sender(documents) else {
        return false;
    };
    let (folder, from_documents) = (folded(name), folded(&sender));
    !folder.is_empty()
        && !from_documents.is_empty()
        && (from_documents.contains(&folder) || folder.contains(&from_documents))
}

fn dominant_sender(documents: &[&StructureDocument]) -> Option<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for sender in documents
        .iter()
        .filter_map(|document| document.sender.as_deref())
    {
        *counts.entry(sender).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(sender, _)| sender.to_owned())
}
