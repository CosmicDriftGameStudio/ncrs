//! The matchers behind the condition kinds that read the text or the place of
//! a document. Each takes a value that is not empty.

use std::ops::Range;
use std::path::{Component, Path};
use std::sync::LazyLock;

use ncrs_index::{
    compile_bounded_regex, is_iban_separator, is_tax_id_separator, Condition, ConditionKind,
};
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

static LETTERHEAD_MARKER: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_bounded_regex(concat!(
        r"(?i)@|\bwww\.|https?://",
        r"|\b(?:tel|telefon|fax|iban|hrb|hra|amtsgericht|registergericht|ust)\b",
        r"|\bsteuernummer",
        r"|\be\.\s?v\.",
        r"|\b(?:gmbh|mbh)\b",
        // Short forms only in capitals: "kg" and "se" are ordinary lowercase text.
        r"|\b(?:(?-i:AG|KG|OHG|GbR|UG|SE))\b",
    ))
    .ok()
});

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

fn separated_number_holds(text: &str, value: &str, is_separator: fn(char) -> bool) -> bool {
    separated_number_ranges(text, value, is_separator)
        .next()
        .is_some()
}

/// Where the value occurs in the text between characters that are not
/// alphanumeric, ignoring case and the separators in either of them.
fn separated_number_ranges<'text>(
    text: &'text str,
    value: &str,
    is_separator: fn(char) -> bool,
) -> impl Iterator<Item = Range<usize>> + 'text {
    let wanted: Vec<char> = value.chars().filter(|ch| !is_separator(*ch)).collect();
    let same_char = |a: char, b: char| a.to_uppercase().eq(b.to_uppercase());
    text.char_indices().filter_map(move |(start, first)| {
        let (first_wanted, remaining) = wanted.split_first()?;
        if !same_char(first, *first_wanted) {
            return None;
        }
        if text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
        {
            return None;
        }
        let mut end = start + first.len_utf8();
        let mut rest = text[end..].chars().peekable();
        for expected in remaining {
            while let Some(separator) = rest.next_if(|ch| is_separator(*ch)) {
                end += separator.len_utf8();
            }
            match rest.next() {
                Some(ch) if same_char(ch, *expected) => end += ch.len_utf8(),
                _ => return None,
            }
        }
        (!rest.next().is_some_and(char::is_alphanumeric)).then_some(start..end)
    })
}

/// Most passages one condition marks in a text.
pub const MAX_MATCH_RANGES: usize = 24;

/// The byte ranges of `text` the condition holds because of, for marking them
/// in a preview. Negated conditions and the kinds that read no passage of the
/// text (sender, recipient, folder) mark nothing, and neither does a condition
/// that does not hold.
pub fn condition_match_ranges(condition: &Condition, text: &str) -> Vec<Range<usize>> {
    let value = condition.value.trim();
    if condition.negated || value.is_empty() {
        return Vec::new();
    }
    let mut ranges = match condition.kind {
        ConditionKind::Substring => literal_ranges(text, &condition.value),
        ConditionKind::Word => word_ranges(text, value),
        ConditionKind::Regex => regex_ranges(text, &condition.value),
        ConditionKind::SenderAddress => {
            if sender_address_holds(&text.to_lowercase(), value) {
                literal_ranges(text, value.trim_start_matches('@'))
            } else {
                Vec::new()
            }
        }
        ConditionKind::Iban => separated_number_ranges(text, value, is_iban_separator).collect(),
        ConditionKind::TaxId => separated_number_ranges(text, value, is_tax_id_separator).collect(),
        ConditionKind::Sender | ConditionKind::Recipient | ConditionKind::Folder => Vec::new(),
    };
    ranges.truncate(MAX_MATCH_RANGES);
    ranges
}

fn literal_ranges(text: &str, value: &str) -> Vec<Range<usize>> {
    let Ok(regex) = compile_bounded_regex(&format!("(?i){}", regex::escape(value))) else {
        return Vec::new();
    };
    regex
        .find_iter(text)
        .take(MAX_MATCH_RANGES)
        .map(|found| found.range())
        .collect()
}

fn regex_ranges(text: &str, source: &str) -> Vec<Range<usize>> {
    let Ok(regex) = compile_bounded_regex(source) else {
        return Vec::new();
    };
    regex
        .find_iter(text)
        .filter(|found| !found.is_empty())
        .take(MAX_MATCH_RANGES)
        .map(|found| found.range())
        .collect()
}

/// Like [`words_hold`] on the original text: the words of `value` with any
/// whitespace between them, between characters that are not alphanumeric.
fn word_ranges(text: &str, value: &str) -> Vec<Range<usize>> {
    let words: Vec<String> = value.split_whitespace().map(regex::escape).collect();
    let Ok(regex) = compile_bounded_regex(&format!("(?i){}", words.join(r"\s+"))) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(found) = regex.find_at(text, from) {
        let before = text[..found.start()].chars().next_back();
        let after = text[found.end()..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            ranges.push(found.range());
            if ranges.len() == MAX_MATCH_RANGES {
                break;
            }
            from = found.end();
        } else {
            let Some(first) = text[found.start()..].chars().next() else {
                break;
            };
            from = found.start() + first.len_utf8();
        }
        if found.is_empty() {
            break;
        }
    }
    ranges
}

fn looks_like_letterhead_line(line: &str) -> bool {
    LETTERHEAD_MARKER
        .as_ref()
        .is_some_and(|marker| marker.is_match(line))
}

/// The addressee block is the first block after the letterhead that ends in a
/// line starting with a postal code. When the letterhead is only a logo, the
/// first text block is the addressee block: it counts when it ends in such a
/// line and no line of it looks like a letterhead. A company addressee with a
/// legal form in the first block is therefore still taken for the letterhead.
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
    let ends_in_postal_code =
        |block: &Vec<&str>| block.last().is_some_and(|last| postal_code.is_match(last));
    let first_is_addressee = blocks.first().is_some_and(|first| {
        ends_in_postal_code(first) && !first.iter().any(|line| looks_like_letterhead_line(line))
    });
    blocks
        .into_iter()
        .enumerate()
        .find(|(position, block)| {
            (*position > 0 || first_is_addressee) && ends_in_postal_code(block)
        })
        .map(|(_, block)| block)
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
    fn a_first_text_block_without_letterhead_lines_is_the_addressee() {
        let text = "Max Mustermann\nMusterweg 3\n12345 Musterstadt\n\n\
            Rechnung 4711\n\nStadtwerke Musterstadt GmbH, Hauptstraße 1, 12345 Musterstadt";

        assert_eq!(
            addressee_block_lines(text),
            Some(vec!["Max Mustermann", "Musterweg 3", "12345 Musterstadt"])
        );
        assert!(recipient_holds(text, "Max Mustermann"));
    }

    #[test]
    fn a_first_block_with_letterhead_lines_is_skipped() {
        for letterhead_line in [
            "Stadtwerke Musterstadt GmbH",
            "Muster e. V.",
            "Tel. 0123 456",
            "info@stadtwerke.example",
            "www.stadtwerke.example",
            "Fax 0123 457",
            "IBAN DE89 3704 0044 0532 0130 00",
            "Amtsgericht Musterstadt HRB 12",
            "USt-IdNr. DE123456789",
            "Steuernummer 12/345/67890",
            "Muster & Söhne OHG",
            "Muster AG",
        ] {
            let text = format!(
                "Stadtwerke Musterstadt\n{letterhead_line}\n12345 Musterstadt\n\n\
                 Max Mustermann\nMusterweg 3\n54321 Andernorts"
            );
            assert_eq!(
                addressee_block_lines(&text),
                Some(vec!["Max Mustermann", "Musterweg 3", "54321 Andernorts"]),
                "{letterhead_line}"
            );
        }
    }

    #[test]
    fn ordinary_words_are_not_letterhead_lines() {
        assert!(!looks_like_letterhead_line("Max Mustermann"));
        assert!(!looks_like_letterhead_line("Musterweg 3"));
        assert!(!looks_like_letterhead_line("5 kg Se Telegram"));
        assert!(!looks_like_letterhead_line("Ludwigstraße 3a"));
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

    fn text_of<'a>(condition: &Condition, text: &'a str) -> Vec<&'a str> {
        condition_match_ranges(condition, text)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    fn condition(kind: ConditionKind, value: &str) -> Condition {
        Condition::new(kind, value)
    }

    #[test]
    fn a_substring_is_marked_wherever_it_stands_regardless_of_case() {
        let text = "Rechnung der Company A Ltd. company a wieder";
        assert_eq!(
            text_of(&condition(ConditionKind::Substring, "COMPANY A"), text),
            ["Company A", "company a"]
        );
        assert!(text_of(&condition(ConditionKind::Substring, "agency"), text).is_empty());
    }

    #[test]
    fn a_word_is_marked_only_between_word_boundaries_and_across_whitespace() {
        let text = "Autobahn und Deutsche\n  Bahn AG, Bahnhof";
        assert_eq!(
            text_of(&condition(ConditionKind::Word, "deutsche bahn"), text),
            ["Deutsche\n  Bahn"]
        );
        assert_eq!(
            text_of(&condition(ConditionKind::Word, "bahn"), text),
            ["Bahn"]
        );
    }

    #[test]
    fn a_word_after_an_occurrence_inside_another_word_is_found() {
        let text = "Autobahn und Bahn";
        let ranges = condition_match_ranges(&condition(ConditionKind::Word, "bahn"), text);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges.first(), Some(&(13..17)));
    }

    #[test]
    fn a_regex_marks_its_matches_as_it_is_written() {
        let text = "Invoice 4711 and invoice 42";
        assert_eq!(
            text_of(&condition(ConditionKind::Regex, r"[Ii]nvoice \d+"), text),
            ["Invoice 4711", "invoice 42"]
        );
        assert!(text_of(&condition(ConditionKind::Regex, "INVOICE"), text).is_empty());
        assert!(text_of(&condition(ConditionKind::Regex, "("), text).is_empty());
    }

    #[test]
    fn a_sender_address_marks_the_domain_where_the_address_holds() {
        let text = "Mail: Rechnung@Billing.Nordlicht.de";
        assert_eq!(
            text_of(
                &condition(ConditionKind::SenderAddress, "nordlicht.de"),
                text
            ),
            ["Nordlicht.de"]
        );
        assert!(text_of(&condition(ConditionKind::SenderAddress, "other.org"), text).is_empty());
    }

    #[test]
    fn an_iban_and_a_tax_id_are_marked_with_their_separators() {
        let text = "IBAN DE89 3704 0044 0532 0130 00 und St-Nr. 12/345/67890.";
        assert_eq!(
            text_of(
                &condition(ConditionKind::Iban, "DE89370400440532013000"),
                text
            ),
            ["DE89 3704 0044 0532 0130 00"]
        );
        assert_eq!(
            text_of(&condition(ConditionKind::TaxId, "1234567890"), text),
            ["12/345/67890"]
        );
        assert!(text_of(&condition(ConditionKind::Iban, "DE89370400440532013"), text).is_empty());
    }

    #[test]
    fn kinds_without_a_passage_and_negated_conditions_mark_nothing() {
        let text = "Company A, Berlin";
        for kind in [
            ConditionKind::Sender,
            ConditionKind::Recipient,
            ConditionKind::Folder,
        ] {
            assert!(text_of(&condition(kind, "Company A"), text).is_empty());
        }
        let negated = Condition::not(ConditionKind::Substring, "Company");
        assert!(text_of(&negated, text).is_empty());
    }

    #[test]
    fn the_marks_of_one_condition_are_limited() {
        let text = "a ".repeat(MAX_MATCH_RANGES * 2);
        let ranges = condition_match_ranges(&condition(ConditionKind::Word, "a"), &text);
        assert_eq!(ranges.len(), MAX_MATCH_RANGES);
    }

    #[test]
    fn ranges_stay_on_characters_where_lowercasing_changes_the_length() {
        let text = "İstanbul Company A";
        assert_eq!(
            text_of(&condition(ConditionKind::Substring, "company a"), text),
            ["Company A"]
        );
    }
}
