//! The words of the rule language that the grammar, the prompt and the parser
//! share: one place decides what a condition kind or a combinator is called.

use ncrs_index::{Combinator, ConditionKind};

pub const COMBINATORS: [Combinator; 2] = [Combinator::AllOf, Combinator::AnyOf];

/// A new `ConditionKind` variant stops compiling here until the prompt can
/// explain it.
pub fn condition_meaning(kind: ConditionKind) -> &'static str {
    match kind {
        ConditionKind::Substring => "the text of the document contains the value, in any case",
        ConditionKind::Word => "the text contains the value as a whole word or phrase",
        ConditionKind::Regex => "the text matches the regular expression in the value",
        ConditionKind::Sender => "the sender of the document is the value",
        ConditionKind::SenderAddress => {
            "the text has an e-mail address or web domain; the value is the domain or the address"
        }
        ConditionKind::Iban => "the text contains this IBAN",
        ConditionKind::TaxId => "the text contains this tax number",
        ConditionKind::Recipient => "the address block of the letter contains the value",
        ConditionKind::Folder => "the document lies in the folder with this name",
        ConditionKind::Tag => "the document has this tag",
        ConditionKind::Kind => "the document is of this kind (invoice, contract, ...)",
    }
}

pub fn condition_word(kind: ConditionKind) -> &'static str {
    kind.as_str()
}

pub fn condition_from_word(word: &str) -> Option<ConditionKind> {
    ConditionKind::ALL
        .into_iter()
        .find(|kind| condition_word(*kind) == word)
}

pub fn combinator_from_word(word: &str) -> Option<Combinator> {
    COMBINATORS
        .into_iter()
        .find(|combinator| combinator.as_str() == word)
}

/// `None` for a placeholder this table does not know yet; a test keeps it in
/// step with `FILING_PLACEHOLDERS`.
pub fn placeholder_meaning(name: &str) -> Option<&'static str> {
    Some(match name {
        "year" => "the year of the document date, four digits (2023)",
        "month" => "the month of the document date, two digits (03)",
        "day" => "the day of the document date, two digits (14)",
        "date" => "the whole document date as YYYY-MM-DD, the same as {year}-{month}-{day}",
        "sender" => "who sent the document",
        "recipient" => "who the document is addressed to",
        "number" => "the number of the document (invoice, reference or customer number)",
        "kind" => "the kind of the document as a word (Invoice, Contract, ...)",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use ncrs_intake::FILING_PLACEHOLDERS;

    use super::*;

    #[test]
    fn every_placeholder_has_a_meaning() {
        for name in FILING_PLACEHOLDERS {
            assert!(placeholder_meaning(name).is_some(), "{name}");
        }
    }

    #[test]
    fn words_map_back_to_their_kinds_and_combinators() {
        for kind in ConditionKind::ALL {
            assert_eq!(condition_from_word(condition_word(kind)), Some(kind));
        }
        for combinator in COMBINATORS {
            assert_eq!(combinator_from_word(combinator.as_str()), Some(combinator));
        }
        assert_eq!(condition_from_word("nonsense"), None);
    }
}
