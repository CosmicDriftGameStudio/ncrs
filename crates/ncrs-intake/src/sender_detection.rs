//! Finds the sender of a document without an AI: from identifiers a confirmed
//! alias knows, else from the letterhead or the footer. Pure; nothing is
//! stored here, new identifiers and name variants come back as offers.

use std::sync::LazyLock;

use ncrs_index::{compile_bounded_regex, SenderAliasKind, SenderAliases};
use regex::Regex;

use crate::conditions::{addressee_block_lines, domains_found, EMAIL_ADDRESS, WEB_DOMAIN};

const LETTERHEAD_LINES: usize = 6;
const FOOTER_LINES: usize = 8;
const MAX_NAME_LINE_CHARS: usize = 80;
const IBAN_PAYLOAD_CHARS: std::ops::RangeInclusive<usize> = 11..=30;
const MIN_TAX_ID_CHARS: usize = 6;

const LEGAL_FORMS: [&str; 24] = [
    "GmbH", "mbH", "AG", "KG", "KGaA", "UG", "e.K.", "e.V.", "GbR", "OHG", "SE", "Ltd", "Ltd.",
    "Limited", "Inc", "Inc.", "LLC", "PLC", "LLP", "Corp", "S.A.", "SAS", "SARL", "B.V.",
];
const LEGAL_FORMS_WITH_DOTS: [&str; 1] = ["N.V."];
const AUTHORITY_WORDS: [&str; 9] = [
    "finanzamt",
    "amtsgericht",
    "landratsamt",
    "stadt",
    "gemeinde",
    "bundeszentralamt",
    "bundesagentur",
    "jobcenter",
    "krankenkasse",
];

static IBAN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_bounded_regex(r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,4})?\b").ok()
});
static TAX_ID: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_bounded_regex(
        r"(?i)\b(?:ust-?id-?nr|ust-?id|uid|vat[ -]?id|vat[ -]?no|vat[ -]?number|steuernummer|st\.-?nr)\b\.?[ \t]*:?[ \t]*((?:[a-z]{2,3} ?)?[0-9][0-9/ ]*)",
    )
    .ok()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderSource {
    Identifier(SenderAliasKind),
    Letterhead,
    Footer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedSender {
    pub name: String,
    pub source: SenderSource,
}

/// A value the user may confirm as another name of `sender`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasOffer {
    pub sender: String,
    pub kind: SenderAliasKind,
    /// Normalised.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderDetection {
    pub sender: Option<DetectedSender>,
    /// Empty when no sender was detected.
    pub offers: Vec<AliasOffer>,
}

struct Identifiers {
    ibans: Vec<String>,
    tax_ids: Vec<String>,
    domains: Vec<String>,
}

impl Identifiers {
    fn find(text: &str) -> Self {
        let lowered = text.to_lowercase();
        Self {
            ibans: found_ibans(text),
            tax_ids: found_tax_ids(text),
            domains: domains_found(&EMAIL_ADDRESS, &lowered)
                .into_iter()
                .chain(domains_found(&WEB_DOMAIN, &lowered))
                .collect(),
        }
    }

    fn of_kind(&self) -> [(SenderAliasKind, &[String]); 3] {
        [
            (SenderAliasKind::Iban, &self.ibans),
            (SenderAliasKind::TaxId, &self.tax_ids),
            (SenderAliasKind::Domain, &self.domains),
        ]
    }
}

fn found_ibans(text: &str) -> Vec<String> {
    IBAN.iter()
        .flat_map(|regex| regex.find_iter(text))
        .map(|found| found.as_str())
        .filter(|candidate| {
            let payload = SenderAliasKind::Iban
                .normalise(candidate)
                .chars()
                .count()
                .saturating_sub(4);
            IBAN_PAYLOAD_CHARS.contains(&payload)
        })
        .map(str::to_owned)
        .collect()
}

fn found_tax_ids(text: &str) -> Vec<String> {
    TAX_ID
        .iter()
        .flat_map(|regex| regex.captures_iter(text))
        .filter_map(|captures| captures.get(1))
        .map(|value| value.as_str().split("  ").next().unwrap_or_default())
        .filter(|value| SenderAliasKind::TaxId.normalise(value).chars().count() >= MIN_TAX_ID_CHARS)
        .map(str::to_owned)
        .collect()
}

pub fn detect_sender(
    text: &str,
    aliases: &SenderAliases,
    known_senders: &[String],
) -> SenderDetection {
    let identifiers = Identifiers::find(text);
    // The recipient stands in the addressee block; it is never the sender.
    let addressee = addressee_block_lines(text).unwrap_or_default();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !addressee.contains(line))
        .collect();
    let detected = by_identifier(&identifiers, aliases)
        .or_else(|| {
            name_in(lines.iter().take(LETTERHEAD_LINES), aliases)
                .map(|name| (name, SenderSource::Letterhead))
        })
        .or_else(|| {
            let from = lines.len().saturating_sub(FOOTER_LINES);
            name_in(lines.iter().skip(from), aliases).map(|name| (name, SenderSource::Footer))
        });
    let Some((name, source)) = detected else {
        return SenderDetection {
            sender: None,
            offers: Vec::new(),
        };
    };
    let canonical = aliases.canonical_sender(&name).to_owned();
    let offers = offers_for(&canonical, &identifiers, aliases, known_senders);
    SenderDetection {
        sender: Some(DetectedSender {
            name: canonical,
            source,
        }),
        offers,
    }
}

fn by_identifier(
    identifiers: &Identifiers,
    aliases: &SenderAliases,
) -> Option<(String, SenderSource)> {
    identifiers
        .of_kind()
        .into_iter()
        .find_map(|(kind, values)| {
            values.iter().find_map(|value| {
                aliases
                    .sender_for(kind, value)
                    .map(|sender| (sender.to_owned(), SenderSource::Identifier(kind)))
            })
        })
}

fn offers_for(
    sender: &str,
    identifiers: &Identifiers,
    aliases: &SenderAliases,
    known_senders: &[String],
) -> Vec<AliasOffer> {
    let mut offers: Vec<AliasOffer> = Vec::new();
    for (kind, values) in identifiers.of_kind() {
        for value in values {
            let normalised = kind.normalise(value);
            let new = aliases.sender_for(kind, value).is_none()
                && !offers
                    .iter()
                    .any(|offer| offer.kind == kind && offer.value == normalised);
            if new {
                offers.push(AliasOffer {
                    sender: sender.to_owned(),
                    kind,
                    value: normalised,
                });
            }
        }
    }
    offers.extend(name_variant_offer(sender, known_senders));
    offers
}

/// "telekom" is a word prefix of "telekom deutschland gmbh".
fn name_variant_offer(detected: &str, known_senders: &[String]) -> Option<AliasOffer> {
    let kind = SenderAliasKind::Name;
    let detected_key = kind.normalise(detected);
    let known_keys: Vec<(String, &String)> = known_senders
        .iter()
        .map(|known| (kind.normalise(known), known))
        .collect();
    if known_keys.iter().any(|(key, _)| *key == detected_key) {
        return None;
    }
    known_keys
        .into_iter()
        .filter(|(key, _)| {
            detected_key
                .strip_prefix(key.as_str())
                .is_some_and(|rest| !key.is_empty() && rest.starts_with(' '))
        })
        .max_by_key(|(key, _)| key.len())
        .map(|(_, known)| AliasOffer {
            sender: known.clone(),
            kind,
            value: detected_key,
        })
}

fn name_in<'a>(
    mut lines: impl Iterator<Item = &'a &'a str>,
    aliases: &SenderAliases,
) -> Option<String> {
    lines.find_map(|line| company_name(line, aliases))
}

fn company_name(line: &str, aliases: &SenderAliases) -> Option<String> {
    if line.chars().count() > MAX_NAME_LINE_CHARS || line.ends_with(',') || is_register_line(line) {
        return None;
    }
    let name = line
        .split([',', '\t'])
        .next()
        .unwrap_or_default()
        .split(" · ")
        .next()
        .unwrap_or_default()
        .split(" | ")
        .next()
        .unwrap_or_default()
        .trim();
    let is_name = has_legal_form(name)
        || starts_with_authority_word(name)
        || aliases.sender_for(SenderAliasKind::Name, name).is_some();
    if !is_name || (line.ends_with('.') && !ends_with_legal_form(line)) {
        return None;
    }
    Some(name.to_owned())
}

fn is_legal_form(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| matches!(ch, '(' | ')' | ';' | ':' | '&'));
    let spelled = |form: &str| {
        let bare = |text: &str| text.trim_end_matches('.').to_owned();
        bare(trimmed) == bare(form) || bare(trimmed) == bare(form).to_uppercase()
    };
    LEGAL_FORMS
        .iter()
        .chain(LEGAL_FORMS_WITH_DOTS.iter())
        .any(|form| spelled(form))
}

fn has_legal_form(name: &str) -> bool {
    name.split_whitespace().any(is_legal_form)
}

fn ends_with_legal_form(line: &str) -> bool {
    line.split_whitespace()
        .next_back()
        .is_some_and(is_legal_form)
}

fn starts_with_authority_word(name: &str) -> bool {
    name.split_whitespace().next().is_some_and(|first| {
        let word = first.trim_matches(|ch: char| !ch.is_alphanumeric());
        AUTHORITY_WORDS
            .iter()
            .any(|authority| word.eq_ignore_ascii_case(authority))
    })
}

/// A register line ("Amtsgericht München HRB 12345") names a court, not the sender.
fn is_register_line(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let word = token.trim_matches(|ch: char| !ch.is_alphanumeric());
        word.eq_ignore_ascii_case("HRB") || word.eq_ignore_ascii_case("HRA")
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn confirmed(entries: &[(SenderAliasKind, &str, &str)]) -> SenderAliases {
        SenderAliases::from_confirmed(entries.iter().copied())
    }

    const LETTER: &str = "Nordlicht Energie GmbH · Hafenstraße 4 · 20457 Hamburg\n\nMax Muster\nMusterweg 1\n12345 Berlin\n\nRechnung 2024-17\nKunden-Nr. 4711\n";

    #[test]
    fn a_letterhead_with_a_legal_form_names_the_sender() {
        let detection = detect_sender(LETTER, &SenderAliases::default(), &[]);

        let sender = detection.sender.unwrap();
        assert_eq!(sender.name, "Nordlicht Energie GmbH");
        assert_eq!(sender.source, SenderSource::Letterhead);
    }

    #[test]
    fn a_letterhead_with_an_authority_word_names_the_sender() {
        let text = "Finanzamt Berlin-Mitte\nKlosterstraße 59\n\nBescheid über Einkommensteuer\n";

        let sender = detect_sender(text, &SenderAliases::default(), &[])
            .sender
            .unwrap();

        assert_eq!(sender.name, "Finanzamt Berlin-Mitte");
        assert_eq!(sender.source, SenderSource::Letterhead);
    }

    #[test]
    fn the_footer_is_used_when_the_top_has_no_name() {
        let mut text = String::from("Rechnung\nfür Ihre Bestellung\n");
        for number in 0..10 {
            text.push_str(&format!("Position {number}\n"));
        }
        text.push_str("Pixelwerk Studio Ltd | London\n");

        let sender = detect_sender(&text, &SenderAliases::default(), &[])
            .sender
            .unwrap();

        assert_eq!(sender.name, "Pixelwerk Studio Ltd");
        assert_eq!(sender.source, SenderSource::Footer);
    }

    #[test]
    fn the_top_company_wins_over_a_later_one() {
        let text = "Alpha Handels GmbH\nMusterweg 1\nBeta Logistik AG\n";

        let sender = detect_sender(text, &SenderAliases::default(), &[])
            .sender
            .unwrap();

        assert_eq!(sender.name, "Alpha Handels GmbH");
    }

    #[test]
    fn the_addressee_block_is_never_the_sender() {
        let text = "Lieferant\nMusterstraße 5\n\nMusterfirma GmbH\nHauptstr. 1\n12345 Berlin\n\nRechnung 4711\nPosition 1\nPosition 2\nPosition 3\nPosition 4\nPosition 5\n\nLieferant Handels AG, Weg 1, 54321 Ort\n";

        let sender = detect_sender(text, &SenderAliases::default(), &[])
            .sender
            .unwrap();

        assert_eq!(sender.name, "Lieferant Handels AG");
        assert_eq!(sender.source, SenderSource::Footer);
    }

    #[test]
    fn an_authority_word_must_be_a_whole_word() {
        let street = "Bäckerei Schmidt\nStadtweg 12, 12345 Ort\n";
        assert_eq!(
            detect_sender(street, &SenderAliases::default(), &[]).sender,
            None
        );

        let town = "Stadt Musterstadt\nBürgeramt\n";
        let sender = detect_sender(town, &SenderAliases::default(), &[])
            .sender
            .unwrap();
        assert_eq!(sender.name, "Stadt Musterstadt");
    }

    #[test]
    fn a_register_line_is_not_a_sender() {
        let mut footer = String::from("Rechnung\n");
        for number in 0..10 {
            footer.push_str(&format!("Position {number}\n"));
        }
        let only_register = format!("{footer}Amtsgericht München HRB 12345\n");
        assert_eq!(
            detect_sender(&only_register, &SenderAliases::default(), &[]).sender,
            None
        );

        let with_company = format!("{footer}Lieferant Handels AG\nAmtsgericht München HRB 12345\n");
        let sender = detect_sender(&with_company, &SenderAliases::default(), &[])
            .sender
            .unwrap();
        assert_eq!(sender.name, "Lieferant Handels AG");
    }

    #[test]
    fn sentences_and_long_lines_are_not_names() {
        let text = "Wir danken der Muster GmbH für den Auftrag.\nHallo\n";

        assert_eq!(
            detect_sender(text, &SenderAliases::default(), &[]).sender,
            None
        );
    }

    #[test]
    fn a_confirmed_iban_alias_decides_before_the_letterhead() {
        let aliases = confirmed(&[(
            SenderAliasKind::Iban,
            "DE89370400440532013000",
            "Stadtwerke Nord",
        )]);
        let text = "Irgendein Absender GmbH\nIBAN: DE89 3704 0044 0532 0130 00\n";

        let sender = detect_sender(text, &aliases, &[]).sender.unwrap();

        assert_eq!(sender.name, "Stadtwerke Nord");
        assert_eq!(
            sender.source,
            SenderSource::Identifier(SenderAliasKind::Iban)
        );
    }

    #[test]
    fn a_confirmed_tax_id_alias_resolves_the_sender() {
        let aliases = confirmed(&[(
            SenderAliasKind::TaxId,
            "DE123456789",
            "Kranich Versicherung AG",
        )]);
        let text = "Seite 1\nUSt-IdNr.: DE 123 456 789  Amtsgericht Köln\n";

        let sender = detect_sender(text, &aliases, &[]).sender.unwrap();

        assert_eq!(sender.name, "Kranich Versicherung AG");
        assert_eq!(
            sender.source,
            SenderSource::Identifier(SenderAliasKind::TaxId)
        );
    }

    #[test]
    fn a_confirmed_domain_alias_resolves_the_sender() {
        let aliases = confirmed(&[(
            SenderAliasKind::Domain,
            "pixelwerk.example",
            "Pixelwerk Studio Ltd",
        )]);
        let text = "Hallo\nKontakt: billing@Pixelwerk.example oder www.pixelwerk.example\n";

        let sender = detect_sender(text, &aliases, &[]).sender.unwrap();

        assert_eq!(sender.name, "Pixelwerk Studio Ltd");
        assert_eq!(
            sender.source,
            SenderSource::Identifier(SenderAliasKind::Domain)
        );
    }

    #[test]
    fn an_unconfirmed_identifier_does_not_resolve_but_is_offered() {
        let text = "Nordlicht Energie GmbH\nIBAN DE89 3704 0044 0532 0130 00\nSteuernummer: 12/345/67890\nwww.nordlicht.example\n";

        let detection = detect_sender(text, &SenderAliases::default(), &[]);

        assert_eq!(
            detection.sender.as_ref().unwrap().source,
            SenderSource::Letterhead
        );
        let offered: Vec<_> = detection
            .offers
            .iter()
            .map(|offer| (offer.kind, offer.value.as_str()))
            .collect();
        assert_eq!(
            offered,
            [
                (SenderAliasKind::Iban, "DE89370400440532013000"),
                (
                    SenderAliasKind::TaxId,
                    "12345 67890".replace(' ', "").as_str()
                ),
                (SenderAliasKind::Domain, "nordlicht.example"),
            ]
        );
        assert!(detection
            .offers
            .iter()
            .all(|offer| offer.sender == "Nordlicht Energie GmbH"));
    }

    #[test]
    fn a_confirmed_name_alias_gives_the_canonical_name() {
        let aliases = confirmed(&[(SenderAliasKind::Name, "kranich", "Kranich Versicherung AG")]);
        let text = "Kranich\nPolice 4711\n";

        let sender = detect_sender(text, &aliases, &[]).sender.unwrap();

        assert_eq!(sender.name, "Kranich Versicherung AG");
        assert_eq!(sender.source, SenderSource::Letterhead);
    }

    #[test]
    fn a_name_variant_of_a_known_sender_is_offered_not_merged() {
        let text = "Telekom Deutschland GmbH\nRechnung\n";
        let known = vec!["Telekom".to_owned(), "Tele".to_owned()];

        let detection = detect_sender(text, &SenderAliases::default(), &known);

        assert_eq!(detection.sender.unwrap().name, "Telekom Deutschland GmbH");
        assert_eq!(
            detection.offers,
            [AliasOffer {
                sender: "Telekom".to_owned(),
                kind: SenderAliasKind::Name,
                value: "telekom deutschland gmbh".to_owned(),
            }]
        );
    }

    #[test]
    fn a_text_without_a_sender_gives_nothing() {
        let detection = detect_sender("Hallo Welt\nBis bald\n", &SenderAliases::default(), &[]);

        assert_eq!(detection.sender, None);
        assert!(detection.offers.is_empty());
    }
}
