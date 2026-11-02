use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::tree::NodeFile;
use super::types::{NamePart, NameScheme};
use crate::archive_profile::{is_pattern_safe, shape, year_of, NameToken};

/// Fewer files have no scheme, and a scheme needs this share of them.
const MIN_FILES: usize = 3;
const MIN_FOLLOWING_PERCENT: usize = 60;
/// Letters of this many senders under counting names are a dump.
pub(in super) const MIN_SENDERS_OF_A_DUMP: usize = 2;

/// The name scheme most of the files follow and how many do.
pub(in super) fn learn_scheme(files: &[&NodeFile<'_>]) -> (Option<NameScheme>, usize) {
    let Some(members) = dominant_group(files) else {
        return (None, 0);
    };
    if files.len() < MIN_FILES || members.len() * 100 < files.len() * MIN_FOLLOWING_PERCENT {
        return (None, 0);
    }
    let Some(first) = members.first() else {
        return (None, 0);
    };
    let mut parts = Vec::new();
    for (position, token) in first.tokens.iter().enumerate() {
        let mut same_place = members
            .iter()
            .filter_map(|member| member.tokens.get(position));
        let part = match token {
            NameToken::Date(format) => NamePart::Date(*format),
            NameToken::Separator(character) => NamePart::Separator(*character),
            NameToken::Number(_) => {
                let all_years = same_place.all(
                    |other| matches!(other, NameToken::Number(digits) if year_of(digits).is_some()),
                );
                if all_years {
                    NamePart::Year
                } else {
                    NamePart::Number
                }
            }
            NameToken::Word(word) => {
                let everywhere = same_place.all(|other| other == token);
                if everywhere && is_pattern_safe(word) {
                    NamePart::Literal(word.clone())
                } else {
                    NamePart::Free
                }
            }
        };
        push_merging_free_words(&mut parts, part);
    }
    (Some(NameScheme { parts }), members.len())
}

/// The files that share the most common shape of file name.
fn dominant_group<'f, 'a>(files: &[&'f NodeFile<'a>]) -> Option<Vec<&'f NodeFile<'a>>> {
    let mut groups: BTreeMap<String, Vec<&'f NodeFile<'a>>> = BTreeMap::new();
    for file in files {
        groups.entry(shape(&file.tokens)).or_default().push(file);
    }
    groups
        .into_iter()
        .max_by_key(|(key, members)| (members.len(), Reverse(key.clone())))
        .map(|(_, members)| members)
}

/// The shape of file name most of the files have, to compare folders.
pub(in super) fn dominant_shape(files: &[&NodeFile<'_>]) -> Option<String> {
    let members = dominant_group(files)?;
    members.first().map(|first| shape(&first.tokens))
}

/// Free words next to each other, however they are separated, are one name.
fn push_merging_free_words(parts: &mut Vec<NamePart>, part: NamePart) {
    if part == NamePart::Free {
        let separators = parts
            .iter()
            .rev()
            .take_while(|earlier| matches!(earlier, NamePart::Separator(_)))
            .count();
        let before = parts.len().saturating_sub(separators);
        let after_free = before
            .checked_sub(1)
            .and_then(|at| parts.get(at))
            .is_some_and(|earlier| *earlier == NamePart::Free);
        if after_free {
            parts.truncate(before);
            return;
        }
    }
    parts.push(part);
}
