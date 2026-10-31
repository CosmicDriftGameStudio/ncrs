//! Reads two fields from the look of a letter: to whom it is addressed (the
//! first name line of the addressee block) and what kind of letter it is (a
//! heading near the top).

use ncrs_index::KindCatalog;

use crate::conditions::addressee_block_lines;

/// Words that address a name or deliver a letter but are no part of it.
const SALUTATIONS: [&str; 11] = [
    "herrn",
    "herr",
    "frau",
    "an",
    "z. hd.",
    "z.hd.",
    "zu händen",
    "mr",
    "mrs",
    "ms",
    "attn",
];

const COMPANY_LINE: &str = "firma";

const HEADING_SEARCH_LINES: usize = 15;

fn without_leading_salutation(line: &str) -> &str {
    let mut rest = line.trim();
    'again: loop {
        for salutation in SALUTATIONS {
            let length = salutation.chars().count();
            let starts_with_it = rest
                .chars()
                .take(length)
                .flat_map(char::to_lowercase)
                .eq(salutation.chars());
            if !starts_with_it {
                continue;
            }
            let after = rest
                .char_indices()
                .nth(length)
                .map_or("", |(index, _)| &rest[index..]);
            let ends_the_word =
                after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || c == ':');
            if ends_the_word {
                rest = after.trim_start_matches(|c: char| c.is_whitespace() || c == ':');
                continue 'again;
            }
        }
        return rest;
    }
}

/// The first line of the addressee block that is a name: lines with digits
/// (street, postal code) and lines that are only a salutation are skipped, a
/// salutation in front of a name is cut off.
pub fn detect_recipient(text: &str) -> Option<String> {
    addressee_block_lines(text)?.into_iter().find_map(|line| {
        let name = without_leading_salutation(line).trim_end_matches([',', ';']);
        // "Firma" in front of a name is often part of it ("Firma A GmbH"), so
        // it only falls away as a line of its own.
        let usable = !name.is_empty()
            && !name.eq_ignore_ascii_case(COMPANY_LINE)
            && !name.chars().any(|c| c.is_ascii_digit());
        usable.then(|| name.to_owned())
    })
}

/// The kind a heading names: one of the first lines that starts with a
/// word like "Rechnung" or "Invoice". Running text never counts, only the
/// start of a line.
pub fn kind_from_heading(text: &str, kinds: &KindCatalog) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(HEADING_SEARCH_LINES)
        .find_map(|line| kinds.heading_kind(line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_line_of_a_german_address_block_is_the_recipient() {
        let text = "Stadtwerke Musterstadt GmbH, Hauptstraße 1, 12345 Musterstadt\n\nFrau Erika Beispiel\nMusterweg 3\n12345 Musterstadt\n\nRechnung 4711";

        assert_eq!(detect_recipient(text).as_deref(), Some("Erika Beispiel"));
    }

    #[test]
    fn the_name_line_of_an_english_address_block_is_the_recipient() {
        let text = "Ms Example\n12 Sample Street\n12345 Sampletown\n\nInvoice 7";

        assert_eq!(detect_recipient(text).as_deref(), Some("Example"));
    }

    #[test]
    fn a_line_with_only_a_salutation_is_skipped() {
        let text = "Firma\nBeispiel Handel\nMusterweg 3\n12345 Musterstadt\n";

        assert_eq!(detect_recipient(text).as_deref(), Some("Beispiel Handel"));
    }

    #[test]
    fn firma_in_front_of_a_company_name_stays_part_of_it() {
        let text = "Beispiel Handel, Hauptstraße 1, 12345 Musterstadt\n\nAn: Firma A GmbH\nMusterweg 3\n12345 Musterstadt\n\nRechnung 4711";

        assert_eq!(detect_recipient(text).as_deref(), Some("Firma A GmbH"));
    }

    #[test]
    fn a_name_that_only_starts_like_a_salutation_is_kept() {
        let text = "Anna Beispiel\nMusterweg 3\n12345 Musterstadt\n";

        assert_eq!(detect_recipient(text).as_deref(), Some("Anna Beispiel"));
    }

    #[test]
    fn without_an_address_block_there_is_no_recipient() {
        assert_eq!(detect_recipient("Rechnung 4711\nBetrag 12,00 EUR\n"), None);
        assert_eq!(detect_recipient(""), None);
    }

    #[test]
    fn a_heading_names_the_kind() {
        for (text, kind) in [
            ("Stadtwerke\nRechnung Nr. 4711\n", "invoice"),
            ("Firma X\n\nMAHNUNG\n", "reminder"),
            ("Bescheid über die Grundsteuer", "notice"),
            ("Finanzamt\nEinkommensteuerbescheid 2025", "notice"),
            ("Shop\nInvoice: 12\n", "invoice"),
            ("Shop\nCredit note 12\n", "credit_note"),
            ("Kündigung des Vertrags", "termination"),
        ] {
            assert_eq!(
                kind_from_heading(text, &KindCatalog::built_in()).as_deref(),
                Some(kind),
                "{text}"
            );
        }
    }

    #[test]
    fn running_text_and_late_lines_are_no_heading() {
        assert_eq!(
            kind_from_heading(
                "Wir senden Ihnen die Rechnung in der Anlage.",
                &KindCatalog::built_in()
            ),
            None
        );
        assert_eq!(
            kind_from_heading("Credit card statement", &KindCatalog::built_in()),
            None
        );
        let late = format!("{}Rechnung\n", "Zeile\n".repeat(HEADING_SEARCH_LINES));
        assert_eq!(kind_from_heading(&late, &KindCatalog::built_in()), None);
    }
}
