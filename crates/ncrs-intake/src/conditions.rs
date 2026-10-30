//! The matchers behind the condition kinds that read the text or the place of
//! a document. Each takes a value that is not empty.

use std::path::{Component, Path};
use std::sync::LazyLock;

use ncrs_index::{compile_bounded_regex, is_iban_separator, is_tax_id_separator};
use regex::Regex;

/// A text prepared once per planning run and searched by many rules.
pub struct SearchText {
    pub lowered: String,
    pub words: String,
}

impl SearchText {
    pub fn new(text: &str) -> Self {
        Self {
            lowered: text.to_lowercase(),
            words: normalise_words(text),
        }
    }
}

const ADDRESSEE_SEARCH_LINES: usize = 25;

pub static EMAIL_ADDRESS: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_bounded_regex(r"[a-z0-9._%+-]+@([a-z0-9.-]+\.[a-z]{2,})").ok());
pub static WEB_DOMAIN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_bounded_regex(r"(?:https?://|www\.)([a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,})").ok()
});
static POSTAL_CODE_LINE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_bounded_regex(r"^(?:[A-Z]{1,3}-)?[0-9]{4,5}\s+\p{L}").ok());

/// Whitespace runs become one space, and case is dropped.
pub fn normalise_words(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `value` is already normalised.
pub fn words_hold(normalised_text: &str, value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    let mut from = 0;
    while let Some(offset) = normalised_text[from..].find(value) {
        let start = from + offset;
        let end = start + value.len();
        let before = normalised_text[..start].chars().next_back();
        let after = normalised_text[end..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = start + first.len_utf8();
    }
    false
}

/// Whether the word or phrase `value` stands in `text` between word
/// boundaries, without regard to case or the kind of whitespace.
pub fn word_condition_holds(text: &str, value: &str) -> bool {
    words_hold(&normalise_words(text), &normalise_words(value))
}

pub fn word_holds(text: &SearchText, value: &str) -> bool {
    words_hold(&text.words, &normalise_words(value))
}

pub fn domains_found(pattern: &Option<Regex>, lowered_text: &str) -> Vec<String> {
    pattern
        .iter()
        .flat_map(|regex| regex.captures_iter(lowered_text))
        .filter_map(|captures| captures.get(1).map(|found| found.as_str().to_owned()))
        .collect()
}

/// `lowered_text` is already lowercase.
pub fn sender_address_holds(lowered_text: &str, value: &str) -> bool {
    let wanted = value.trim().to_lowercase();
    if wanted.trim_start_matches('@').contains('@') {
        return EMAIL_ADDRESS.iter().any(|regex| {
            regex
                .find_iter(lowered_text)
                .any(|address| address.as_str() == wanted)
        });
    }
    let domain = wanted.trim_start_matches('@');
    let subdomain_suffix = format!(".{domain}");
    domains_found(&EMAIL_ADDRESS, lowered_text)
        .into_iter()
        .chain(domains_found(&WEB_DOMAIN, lowered_text))
        .any(|found| found == domain || found.ends_with(&subdomain_suffix))
}

pub fn iban_holds(text: &str, value: &str) -> bool {
    separated_number_holds(text, value, is_iban_separator)
}

pub fn tax_id_holds(text: &str, value: &str) -> bool {
    separated_number_holds(text, value, is_tax_id_separator)
}

/// The value occurs in the text between characters that are not alphanumeric,
/// ignoring case and the separators in either of them.
fn separated_number_holds(text: &str, value: &str, is_separator: fn(char) -> bool) -> bool {
    let wanted: Vec<char> = value.chars().filter(|ch| !is_separator(*ch)).collect();
    let Some((first_wanted, remaining)) = wanted.split_first() else {
        return false;
    };
    let same_char = |a: char, b: char| a.to_uppercase().eq(b.to_uppercase());
    text.char_indices().any(|(start, first)| {
        if !same_char(first, *first_wanted) {
            return false;
        }
        if text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
        {
            return false;
        }
        let mut rest = text[start + first.len_utf8()..].chars().peekable();
        for expected in remaining {
            while rest.next_if(|ch| is_separator(*ch)).is_some() {}
            match rest.next() {
                Some(ch) if same_char(ch, *expected) => {}
                _ => return false,
            }
        }
        !rest.next().is_some_and(char::is_alphanumeric)
    })
}

/// The addressee block is the first block after the letterhead that ends in a
/// line starting with a postal code.
pub fn addressee_block_lines(text: &str) -> Option<Vec<&str>> {
    let postal_code = POSTAL_CODE_LINE.as_ref()?;
    let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
    for line in text.lines().take(ADDRESSEE_SEARCH_LINES) {
        if line.trim().is_empty() {
            if blocks.last().is_some_and(|block| !block.is_empty()) {
                blocks.push(Vec::new());
            }
        } else if let Some(block) = blocks.last_mut() {
            block.push(line.trim());
        }
    }
    blocks
        .into_iter()
        .skip(1)
        .find(|block| block.last().is_some_and(|last| postal_code.is_match(last)))
}

fn addressee_block(text: &str) -> Option<String> {
    addressee_block_lines(text).map(|lines| lines.join("\n"))
}

pub fn recipient_holds(text: &str, value: &str) -> bool {
    addressee_block(text).is_some_and(|block| word_condition_holds(&block, value))
}

/// `None` when the value names no folder at all.
pub fn folder_holds(path: &Path, value: &str) -> Option<bool> {
    let wanted: Vec<String> = value
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let folders: Vec<String> = path
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    Some(folders.windows(wanted.len()).any(|window| window == wanted))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_word_needs_word_boundaries() {
        assert!(word_condition_holds(
            "Fahrkarte Deutsche Bahn, Berlin",
            "bahn"
        ));
        assert!(word_condition_holds("DEUTSCHE BAHN", "Bahn"));
        assert!(word_condition_holds("bahn", "bahn"));
        assert!(!word_condition_holds("Autobahn", "bahn"));
        assert!(!word_condition_holds("Bahnhof", "bahn"));
        assert!(!word_condition_holds("anything", "  "));
    }

    #[test]
    fn a_word_is_found_after_an_occurrence_inside_another_word() {
        assert!(word_condition_holds("Autobahn und Bahn", "bahn"));
    }

    #[test]
    fn a_phrase_matches_across_line_breaks_and_spaces() {
        assert!(word_condition_holds(
            "Deutsche\n   Bahn AG",
            "deutsche bahn"
        ));
        assert!(word_condition_holds("Deutsche Bahn AG", "Deutsche \t Bahn"));
        assert!(!word_condition_holds("Deutsche Bahnhof", "deutsche bahn"));
    }

    #[test]
    fn a_sender_address_matches_a_domain_its_subdomains_and_a_full_address() {
        let mixed_case = "Rechnung\nrechnung@billing.nordlicht.de\nIm Netz: www.example.org";
        let lowered = mixed_case.to_lowercase();
        let text = lowered.as_str();

        assert!(sender_address_holds(text, "nordlicht.de"));
        assert!(sender_address_holds(text, " Billing.Nordlicht.de "));
        assert!(sender_address_holds(text, "@nordlicht.de"));
        assert!(sender_address_holds(text, "Rechnung@Billing.Nordlicht.de"));
        assert!(sender_address_holds(text, "example.org"));
        assert!(!sender_address_holds(text, "licht.de"));
        assert!(!sender_address_holds(text, "info@nordlicht.de"));
        assert!(!sender_address_holds(text, "nordlicht.com"));
    }

    #[test]
    fn a_sender_address_domain_is_not_matched_inside_a_longer_domain() {
        assert!(!sender_address_holds(
            "info@notnordlicht.de",
            "nordlicht.de"
        ));
        assert!(sender_address_holds(
            "see https://shop.nordlicht.de/cart",
            "nordlicht.de"
        ));
    }

    #[test]
    fn an_iban_matches_with_and_without_spaces_on_either_side() {
        let spaced = "IBAN: DE00 0000 0000 0000 0000 00\nBIC";
        let joined = "IBAN DE00000000000000000000.";

        assert!(iban_holds(spaced, "DE00 0000 0000 0000 0000 00"));
        assert!(iban_holds(spaced, "DE00000000000000000000"));
        assert!(iban_holds(joined, "de00 0000 0000 0000 0000 00"));
        assert!(iban_holds(joined, "DE00000000000000000000"));
        assert!(!iban_holds("DE0012345", "DE001"));
        assert!(!iban_holds(
            "XDE00 0000 0000 0000 0000 00",
            "DE00000000000000000000"
        ));
        assert!(!iban_holds(spaced, "DE00 0000 0000 0000 0000 01"));
    }

    #[test]
    fn a_tax_id_ignores_spaces_slashes_dashes_and_dots() {
        assert!(tax_id_holds("USt-IdNr. DE 123 456 789", "DE123456789"));
        assert!(tax_id_holds("USt-IdNr. DE123456789", "DE 123 456 789"));
        assert!(tax_id_holds("Steuernummer 12/345/67890", "1234567890"));
        assert!(tax_id_holds("Steuernummer 1234567890", "12/345/67890"));
        assert!(!tax_id_holds("Steuernummer 12/345/678901", "12/345/67890"));
        assert!(!tax_id_holds("DE 123 456 780", "DE123456789"));
    }

    const LETTER: &str = "Nordlicht GmbH\nHafenstraße 1\n20457 Hamburg\n\n\
        Frau Erika Mustermann\nMusterweg 5\nD-10115 Berlin\n\n\
        Rechnung\nSehr geehrte Frau Mustermann, Nordlicht dankt.";

    #[test]
    fn the_recipient_is_read_from_the_addressee_block_only() {
        assert!(recipient_holds(LETTER, "Erika Mustermann"));
        assert!(recipient_holds(LETTER, "musterweg 5"));
        assert!(!recipient_holds(LETTER, "Nordlicht"));
        assert!(!recipient_holds(LETTER, "Rechnung"));
        assert!(!recipient_holds(
            "Frau Erika Mustermann",
            "Erika Mustermann"
        ));
    }

    #[test]
    fn a_folder_matches_consecutive_parent_components_without_case() {
        let path = Path::new("/archive/Steuer/2025/Rechnungen/bill.pdf");

        assert_eq!(folder_holds(path, "rechnungen"), Some(true));
        assert_eq!(folder_holds(path, "steuer/2025"), Some(true));
        assert_eq!(folder_holds(path, "Steuer\\2025\\Rechnungen"), Some(true));
        assert_eq!(folder_holds(path, "Steuer/Rechnungen"), Some(false));
        assert_eq!(folder_holds(path, "bill.pdf"), Some(false));
        assert_eq!(folder_holds(path, "/"), None);
    }
}
