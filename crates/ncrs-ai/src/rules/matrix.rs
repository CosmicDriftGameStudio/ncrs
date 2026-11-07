//! Every rule the matrix asks for, written out from the constants of the rule
//! language: the number of cases follows from the placeholders and the
//! condition kinds, and each case carries the rule or pattern that is right.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]

use std::collections::BTreeSet;

use ncrs_index::{
    Combinator, Condition, ConditionKind, KindLanguage, Kinds, RuleConditions, RuleSpec,
};
use ncrs_intake::{render_pattern_in, FILING_PLACEHOLDERS};

pub use super::fixtures::{archive_kinds, fixtures, spaced, Fixture};
use super::vocabulary::{COMBINATORS, MODEL_CONDITIONS};

/// The file name separators; case number `n` of the file name axis takes the
/// `n`th, round and round.
pub const SEPARATORS: [char; 4] = ['-', '_', ' ', '.'];
/// A word every fixture text has: the one condition of the folder and file
/// name axes, so that every document is filed by the pattern under test.
pub const COMMON_WORD: &str = "Kundennummer";
const LONGEST_SELECTION: usize = 3;
/// How many fixtures a recognize task shows as paths.
const SHOWN_PATHS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    Folder,
    FileName,
    Condition,
    Recognize,
    /// Recognize tasks whose file name holds a fixed word.
    FixedName,
    /// Sentences that say nothing about the structure: the default applies.
    Default,
    Holdout,
}

impl Axis {
    pub const ALL: [Self; 7] = [
        Self::Folder,
        Self::FileName,
        Self::Condition,
        Self::Recognize,
        Self::FixedName,
        Self::Default,
        Self::Holdout,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    German,
    English,
}

impl Language {
    /// German and English take turns, case by case.
    pub fn alternating(index: usize) -> Self {
        if index.is_multiple_of(2) {
            Self::German
        } else {
            Self::English
        }
    }

    fn kind_language(self) -> KindLanguage {
        match self {
            Self::German => KindLanguage::German,
            Self::English => KindLanguage::English,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Describe(String),
    Paths {
        question: String,
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Rule(RuleSpec),
    Pattern(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub axis: Axis,
    pub language: Language,
    pub ask: Ask,
    pub expected: Expected,
    /// The placeholders of the expected pattern.
    pub placeholders: Vec<&'static str>,
    /// The kinds of the expected conditions.
    pub conditions: Vec<ConditionKind>,
}

impl Case {
    pub fn asked(&self) -> String {
        match &self.ask {
            Ask::Describe(description) => description.clone(),
            Ask::Paths {
                paths,
                question: _ignored,
            } => paths.join(" | "),
        }
    }

    pub fn expected_text(&self) -> String {
        match &self.expected {
            Expected::Pattern(pattern) => pattern.clone(),
            Expected::Rule(spec) => rule_text(spec),
        }
    }
}

pub fn rule_text(spec: &RuleSpec) -> String {
    let conditions = spec
        .conditions
        .list
        .iter()
        .map(|condition| {
            let negation = if condition.negated { "not " } else { "" };
            format!("{negation}{}={}", condition.kind.as_str(), condition.value)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} [{conditions}] -> {}",
        spec.conditions.combinator.as_str(),
        spec.filing_pattern
    )
}

fn word_of(placeholder: &str, language: Language) -> &'static str {
    const WORDS: [(&str, &str, &str); 8] = [
        ("year", "Jahr", "year"),
        ("month", "Monat", "month"),
        ("day", "Tag des Monats", "day"),
        ("date", "Datum", "date"),
        ("sender", "Absender", "sender"),
        ("recipient", "Empfänger", "recipient"),
        ("number", "Nummer", "number"),
        ("kind", "Dokumentart", "document kind"),
    ];
    let (_, german, english) = WORDS
        .iter()
        .find(|(name, _, _)| *name == placeholder)
        .unwrap_or_else(|| panic!("no words for {{{placeholder}}}"));
    match language {
        Language::German => german,
        Language::English => english,
    }
}

/// Ordered selections of one to `LONGEST_SELECTION` different placeholders.
pub fn placeholder_selections() -> Vec<Vec<&'static str>> {
    let mut all = Vec::new();
    let mut layer: Vec<Vec<&'static str>> = vec![Vec::new()];
    for _ in 0..LONGEST_SELECTION {
        layer = layer
            .iter()
            .flat_map(|chosen| {
                FILING_PLACEHOLDERS
                    .iter()
                    .filter(|name| !chosen.contains(name))
                    .map(|name| [chosen.as_slice(), &[*name]].concat())
                    .collect::<Vec<_>>()
            })
            .collect();
        all.extend(layer.iter().cloned());
    }
    all
}

fn block(name: &str) -> String {
    format!("{{{name}}}")
}

pub fn rule(list: Vec<Condition>, combinator: Combinator, pattern: &str) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions { combinator, list },
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: pattern.to_owned(),
        confirmed: true,
        enabled: true,
    }
}

fn common_condition_sentence(language: Language) -> String {
    match language {
        Language::German => format!("Für Dokumente mit dem Text \"{COMMON_WORD}\": "),
        Language::English => format!("For documents with the text \"{COMMON_WORD}\": "),
    }
}

fn list_with_and(words: &[&str], language: Language) -> String {
    let and = match language {
        Language::German => "und",
        Language::English => "and",
    };
    match words.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} {and} {last}", rest.join(", ")),
        None => String::new(),
    }
}

fn folder_description(names: &[&str], language: Language, variant: usize) -> String {
    let words: Vec<&str> = names.iter().map(|name| word_of(name, language)).collect();
    let (lead, glue, tail) = match (language, variant % 2) {
        (Language::German, 0) => ("Lege sie in Ordner nach ", ", dann nach ", " ab."),
        (Language::German, _) => ("Sortiere sie in Ordner, oben ", ", darunter ", "."),
        (Language::English, 0) => ("File them into folders by ", ", then by ", "."),
        (Language::English, _) => ("Sort them into folders, top level ", ", below it ", "."),
    };
    format!(
        "{}{lead}{}{tail}",
        common_condition_sentence(language),
        words.join(glue)
    )
}

fn file_name_description(names: &[&str], language: Language, separator: char) -> String {
    let words: Vec<&str> = names.iter().map(|name| word_of(name, language)).collect();
    let (separator_german, separator_english) = match separator {
        '-' => ("einen Bindestrich", "a hyphen"),
        '_' => ("einen Unterstrich", "an underscore"),
        ' ' => ("ein Leerzeichen", "a space"),
        _ => ("einen Punkt", "a dot"),
    };
    let lead = common_condition_sentence(language);
    match (language, words.as_slice()) {
        (Language::German, [only]) => format!("{lead}Der Dateiname ist nur {only}."),
        (Language::English, [only]) => format!("{lead}The file name is just the {only}."),
        (Language::German, _) => format!(
            "{lead}Der Dateiname besteht aus {}, getrennt durch {separator_german}.",
            list_with_and(&words, language)
        ),
        (Language::English, _) => format!(
            "{lead}The file name consists of {}, separated by {separator_english}.",
            list_with_and(&words, language)
        ),
    }
}

fn common_rule(pattern: &str) -> RuleSpec {
    rule(
        vec![Condition::new(ConditionKind::Substring, COMMON_WORD)],
        Combinator::AllOf,
        pattern,
    )
}

fn folder_pattern(names: &[&str]) -> String {
    let parts: Vec<String> = names.iter().map(|name| block(name)).collect();
    format!("{}/", parts.join("/"))
}

fn file_name_pattern(names: &[&str], separator: char) -> String {
    let parts: Vec<String> = names.iter().map(|name| block(name)).collect();
    parts.join(&separator.to_string())
}

fn folder_cases() -> Vec<Case> {
    placeholder_selections()
        .into_iter()
        .enumerate()
        .map(|(index, names)| {
            let language = Language::alternating(index);
            Case {
                axis: Axis::Folder,
                language,
                ask: Ask::Describe(folder_description(&names, language, index / 2)),
                expected: Expected::Rule(common_rule(&folder_pattern(&names))),
                placeholders: names,
                conditions: vec![ConditionKind::Substring],
            }
        })
        .collect()
}

fn file_name_cases() -> Vec<Case> {
    placeholder_selections()
        .into_iter()
        .enumerate()
        .map(|(index, names)| {
            let language = Language::alternating(index);
            let separator = SEPARATORS[index % SEPARATORS.len()];
            Case {
                axis: Axis::FileName,
                language,
                ask: Ask::Describe(file_name_description(&names, language, separator)),
                expected: Expected::Rule(common_rule(&file_name_pattern(&names, separator))),
                placeholders: names,
                conditions: vec![ConditionKind::Substring],
            }
        })
        .collect()
}

fn condition_value(kind: ConditionKind, fixture: &Fixture, language: Language) -> String {
    let sender = fixture.fields.sender.clone().unwrap_or_default();
    let recipient = fixture.fields.recipient.clone().unwrap_or_default();
    let number = fixture.fields.number.clone().unwrap_or_default();
    match kind {
        ConditionKind::Substring => fixture.keyword.to_owned(),
        ConditionKind::Word => sender.split(' ').next().unwrap_or_default().to_owned(),
        ConditionKind::Regex => {
            let prefix: String = number
                .chars()
                .take_while(|ch| !ch.is_ascii_digit())
                .collect();
            format!("{prefix}[0-9]+")
        }
        ConditionKind::Sender => sender,
        ConditionKind::SenderAddress => fixture.domain.to_owned(),
        ConditionKind::Iban => spaced(fixture.iban),
        ConditionKind::TaxId => fixture.tax_id.to_owned(),
        ConditionKind::Recipient => recipient.rsplit(' ').next().unwrap_or_default().to_owned(),
        ConditionKind::Folder => fixture
            .folder
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned(),
        ConditionKind::Tag => fixture.tag.to_owned(),
        ConditionKind::ReadFolder => fixture.folder.to_owned(),
        ConditionKind::Kind => {
            let stored = fixture.fields.kind.clone().unwrap_or_default();
            Kinds::built_in(language.kind_language())
                .kind_word(&stored)
                .into_owned()
        }
    }
}

/// The clause "..., where <this> ..." in German and English, for a condition
/// that holds and for one that does not.
fn condition_phrase(kind: ConditionKind, negated: bool, language: Language, value: &str) -> String {
    let [german, german_not, english, english_not] = match kind {
        ConditionKind::Substring => [
            "im Text \"V\" vorkommt",
            "im Text \"V\" nicht vorkommt",
            "the text contains \"V\"",
            "the text does not contain \"V\"",
        ],
        ConditionKind::Word => [
            "das ganze Wort \"V\" im Text steht",
            "das ganze Wort \"V\" nicht im Text steht",
            "the whole word \"V\" appears in the text",
            "the whole word \"V\" does not appear in the text",
        ],
        ConditionKind::Regex => [
            "der Text zum regulären Ausdruck V passt",
            "der Text nicht zum regulären Ausdruck V passt",
            "the text matches the regular expression V",
            "the text does not match the regular expression V",
        ],
        ConditionKind::Sender => [
            "der Absender \"V\" ist",
            "der Absender nicht \"V\" ist",
            "the sender is \"V\"",
            "the sender is not \"V\"",
        ],
        ConditionKind::SenderAddress => [
            "die Absenderadresse die Domain V hat",
            "die Absenderadresse nicht die Domain V hat",
            "the sender address has the domain V",
            "the sender address does not have the domain V",
        ],
        ConditionKind::Iban => [
            "die IBAN V im Text steht",
            "die IBAN V nicht im Text steht",
            "the IBAN V appears in the text",
            "the IBAN V does not appear in the text",
        ],
        ConditionKind::TaxId => [
            "die Steuernummer V im Text steht",
            "die Steuernummer V nicht im Text steht",
            "the tax number V appears in the text",
            "the tax number V does not appear in the text",
        ],
        ConditionKind::Recipient => [
            "im Adressfeld \"V\" steht",
            "im Adressfeld \"V\" nicht steht",
            "the address block contains \"V\"",
            "the address block does not contain \"V\"",
        ],
        ConditionKind::Folder => [
            "das Dokument im Ordner \"V\" liegt",
            "das Dokument nicht im Ordner \"V\" liegt",
            "the document lies in the folder \"V\"",
            "the document does not lie in the folder \"V\"",
        ],
        ConditionKind::Tag => [
            "das Dokument den Tag \"V\" hat",
            "das Dokument den Tag \"V\" nicht hat",
            "the document has the tag \"V\"",
            "the document does not have the tag \"V\"",
        ],
        ConditionKind::Kind => [
            "die Art \"V\" ist",
            "die Art nicht \"V\" ist",
            "the kind is \"V\"",
            "the kind is not \"V\"",
        ],
        ConditionKind::ReadFolder => [
            "das Dokument im Lesen-Ordner \"V\" liegt",
            "das Dokument nicht im Lesen-Ordner \"V\" liegt",
            "the document lies in the read folder \"V\"",
            "the document does not lie in the read folder \"V\"",
        ],
    };
    let template = match (language, negated) {
        (Language::German, false) => german,
        (Language::German, true) => german_not,
        (Language::English, false) => english,
        (Language::English, true) => english_not,
    };
    template.replace('V', value)
}

fn condition_description(
    conditions: &[Condition],
    combinator: Combinator,
    language: Language,
) -> String {
    let phrases: Vec<String> = conditions
        .iter()
        .map(|condition| {
            condition_phrase(
                condition.kind,
                condition.negated,
                language,
                &condition.value,
            )
        })
        .collect();
    let (joiner, lead, tail) = match (language, combinator) {
        (Language::German, Combinator::AllOf) => (
            " und ",
            "Dokumente, bei denen ",
            ", kommen in einen Ordner pro Absender.",
        ),
        (Language::German, Combinator::AnyOf) => (
            " oder ",
            "Dokumente, bei denen ",
            ", kommen in einen Ordner pro Absender.",
        ),
        (Language::English, Combinator::AllOf) => (
            " and ",
            "Documents where ",
            " go into one folder per sender.",
        ),
        (Language::English, Combinator::AnyOf) => (
            " or ",
            "Documents where ",
            " go into one folder per sender.",
        ),
    };
    format!("{lead}{}{tail}", phrases.join(joiner))
}

fn condition_case(index: usize, list: Vec<Condition>, combinator: Combinator) -> Case {
    let language = Language::alternating(index);
    Case {
        axis: Axis::Condition,
        language,
        ask: Ask::Describe(condition_description(&list, combinator, language)),
        placeholders: vec!["sender"],
        conditions: list.iter().map(|condition| condition.kind).collect(),
        expected: Expected::Rule(rule(list, combinator, "{sender}/")),
    }
}

fn condition_cases() -> Vec<Case> {
    let documents = fixtures();
    let mut cases = Vec::new();
    for kind in MODEL_CONDITIONS {
        for negated in [false, true] {
            let index = cases.len();
            let language = Language::alternating(index);
            let value = condition_value(kind, &documents[index % documents.len()], language);
            let condition = Condition {
                kind,
                value,
                negated,
            };
            cases.push(condition_case(index, vec![condition], Combinator::AllOf));
        }
    }
    for (first_position, first) in MODEL_CONDITIONS.into_iter().enumerate() {
        for second in MODEL_CONDITIONS.into_iter().skip(first_position + 1) {
            for combinator in COMBINATORS {
                let index = cases.len();
                let language = Language::alternating(index);
                let document = &documents[index % documents.len()];
                let list = [first, second]
                    .into_iter()
                    .map(|kind| Condition::new(kind, condition_value(kind, document, language)))
                    .collect();
                cases.push(condition_case(index, list, combinator));
            }
        }
    }
    cases
}

/// Twelve patterns with a fixed word at the front or the back of the file
/// name, with and without folder levels. The words are no field values.
fn fixed_name_patterns() -> Vec<(String, Vec<&'static str>)> {
    [
        ("Beleg_{date}_{sender}", vec!["date", "sender"]),
        ("Scan-{sender}-{date}", vec!["sender", "date"]),
        ("Eingang {year} {sender}", vec!["year", "sender"]),
        ("{date}_{sender}_Kopie", vec!["date", "sender"]),
        ("{sender}-{kind}-final", vec!["sender", "kind"]),
        ("{year}-{month}_Ablage", vec!["year", "month"]),
        (
            "Post/{year}/Scan_{date}_{sender}",
            vec!["year", "date", "sender"],
        ),
        ("{year}/Eingang_{sender}", vec!["year", "sender"]),
        ("Bank/{sender}/{date}_Kopie", vec!["sender", "date"]),
        (
            "Archiv/{year}/{month}/{sender}_Beleg",
            vec!["year", "month", "sender"],
        ),
        (
            "{sender}/Beleg_{date}_{kind}",
            vec!["sender", "date", "kind"],
        ),
        (
            "Bank/{year}/{kind}_{sender}_Ablage",
            vec!["year", "kind", "sender"],
        ),
    ]
    .into_iter()
    .map(|(pattern, placeholders)| (pattern.to_owned(), placeholders))
    .collect()
}

fn recognize_cases(axis: Axis, patterns: Vec<(String, Vec<&'static str>)>) -> Vec<Case> {
    let kinds = archive_kinds();
    let documents = fixtures();
    patterns
        .into_iter()
        .enumerate()
        .map(|(index, (pattern, placeholders))| {
            let language = Language::alternating(index);
            let paths = documents
                .iter()
                .take(SHOWN_PATHS)
                .map(|document| {
                    let rendered = render_pattern_in(&pattern, &document.fields, &kinds)
                        .unwrap_or_else(|| panic!("{pattern} does not render"));
                    format!("{}.pdf", rendered.display())
                })
                .collect();
            let question = match language {
                Language::German => "Welches Muster hat diese Pfade geschrieben?",
                Language::English => "Which pattern wrote these paths?",
            };
            Case {
                axis,
                language,
                ask: Ask::Paths {
                    question: question.to_owned(),
                    paths,
                },
                expected: Expected::Pattern(pattern),
                placeholders,
                conditions: Vec::new(),
            }
        })
        .collect()
}

/// The places the default sentences send documents to.
const AREAS: [&str; 8] = [
    "Bank",
    "Versicherung",
    "Steuer",
    "Strom",
    "Arzt",
    "Einkäufe",
    "Verträge",
    "Archiv",
];
const DEFAULT_CASES: usize = 24;
/// Folder, file name and area-less default of the instructions.
const DEFAULT_TAIL: &str = "{year}/{date}_{sender}_{kind}";

/// 24 sentences that name a condition and a place, never a folder or a file
/// name: the answer is `<area>/{year}/{date}_{sender}_{kind}`.
fn default_cases() -> Vec<Case> {
    // Six conditions with the words for them in both languages; with eight
    // areas, 24 cases use every pair once.
    const CONDITIONS: usize = 6;
    (0..DEFAULT_CASES)
        .map(|index| {
            let language = Language::alternating(index);
            let area = AREAS[index % AREAS.len()];
            let german = language == Language::German;
            let invoice = Kinds::built_in(language.kind_language())
                .kind_word("invoice")
                .into_owned();
            let (condition, sentence) = match index % CONDITIONS {
                0 => (
                    Condition::new(ConditionKind::Sender, "Sparkasse Leipzig"),
                    if german {
                        format!("Kontoauszüge der Sparkasse Leipzig kommen nach {area}.")
                    } else {
                        format!("Statements from Sparkasse Leipzig go to {area}.")
                    },
                ),
                1 => (
                    Condition::new(ConditionKind::Kind, invoice),
                    if german {
                        format!("Alle Rechnungen kommen nach {area}.")
                    } else {
                        format!("All invoices go to {area}.")
                    },
                ),
                2 => (
                    Condition::new(ConditionKind::Tag, "steuer"),
                    if german {
                        format!("Was das Tag steuer hat, gehört nach {area}.")
                    } else {
                        format!("Anything tagged steuer goes to {area}.")
                    },
                ),
                3 => (
                    Condition::new(ConditionKind::Word, "Haftpflicht"),
                    if german {
                        format!("Dokumente mit dem Wort Haftpflicht im Text kommen nach {area}.")
                    } else {
                        format!("Documents with the word Haftpflicht in the text go to {area}.")
                    },
                ),
                4 => (
                    Condition::not(ConditionKind::Sender, "Zalando SE"),
                    if german {
                        format!("Alles außer von Zalando SE kommt nach {area}.")
                    } else {
                        format!("Everything except from Zalando SE goes to {area}.")
                    },
                ),
                _ => (
                    Condition::new(ConditionKind::SenderAddress, "stromio.de"),
                    if german {
                        format!("Mails von stromio.de kommen nach {area}.")
                    } else {
                        format!("Mail from stromio.de goes to {area}.")
                    },
                ),
            };
            let kind = condition.kind;
            Case {
                axis: Axis::Default,
                language,
                ask: Ask::Describe(sentence),
                expected: Expected::Rule(rule(
                    vec![condition],
                    Combinator::AllOf,
                    &format!("{area}/{DEFAULT_TAIL}"),
                )),
                placeholders: vec!["year", "date", "sender", "kind"],
                conditions: vec![kind],
            }
        })
        .collect()
}

pub fn all_cases() -> Vec<Case> {
    let mut cases = folder_cases();
    cases.extend(file_name_cases());
    cases.extend(condition_cases());
    let patterns = cases
        .iter()
        .filter(|case| matches!(case.axis, Axis::Folder | Axis::FileName))
        .filter_map(|case| match &case.expected {
            Expected::Rule(spec) => Some((spec.filing_pattern.clone(), case.placeholders.clone())),
            Expected::Pattern(_) => None,
        })
        .collect();
    cases.extend(recognize_cases(Axis::Recognize, patterns));
    cases.extend(recognize_cases(Axis::FixedName, fixed_name_patterns()));
    cases.extend(default_cases());
    cases
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ncrs_index::iban_is_valid;

    use super::*;
    use crate::rules::prompt::system_prompt;

    fn count(axis: Axis) -> usize {
        all_cases().iter().filter(|case| case.axis == axis).count()
    }

    #[test]
    fn the_counts_follow_from_the_constants() {
        let placeholders = FILING_PLACEHOLDERS.len();
        let kinds = MODEL_CONDITIONS.len();
        let selections = placeholders
            + placeholders * (placeholders - 1)
            + placeholders * (placeholders - 1) * (placeholders - 2);
        assert_eq!(count(Axis::Folder), selections);
        assert_eq!(count(Axis::FileName), selections);
        assert_eq!(count(Axis::Condition), 2 * kinds + kinds * (kinds - 1));
        assert_eq!(count(Axis::Recognize), 2 * selections);
        assert_eq!(
            (
                count(Axis::Folder),
                count(Axis::FileName),
                count(Axis::Condition),
                count(Axis::Recognize)
            ),
            (400, 400, 132, 800)
        );
        assert_eq!(count(Axis::FixedName), 12);
        assert_eq!(count(Axis::Default), 24);
        let generate = all_cases()
            .iter()
            .filter(|case| !matches!(case.axis, Axis::Recognize | Axis::FixedName | Axis::Default))
            .count();
        assert_eq!(generate, 932);
    }

    #[test]
    fn every_case_asks_something_new_and_not_what_the_prompt_shows() {
        let prompt = system_prompt();
        let mut seen = HashSet::new();
        for case in all_cases() {
            let asked = case.asked();
            // `{date}` and `{year}-{month}-{day}` write the same paths, so a
            // recognize case is told apart by its answer as well.
            let identity = format!("{:?}{asked}{}", case.axis, case.expected_text());
            assert!(seen.insert(identity), "{asked}");
            if let Ask::Describe(description) = &case.ask {
                assert!(!prompt.contains(description.as_str()), "{description}");
            }
        }
    }

    #[test]
    fn languages_take_turns_inside_each_axis() {
        for axis in Axis::ALL {
            let languages: Vec<Language> = all_cases()
                .into_iter()
                .filter(|case| case.axis == axis)
                .map(|case| case.language)
                .collect();
            assert!(languages
                .iter()
                .enumerate()
                .all(|(index, language)| *language == Language::alternating(index)));
        }
    }

    #[test]
    fn the_file_name_separators_take_turns() {
        let patterns: Vec<String> = file_name_cases()
            .iter()
            .filter(|case| case.placeholders.len() == 2)
            .take(4)
            .map(|case| match &case.expected {
                Expected::Rule(spec) => spec.filing_pattern.clone(),
                Expected::Pattern(pattern) => pattern.clone(),
            })
            .collect();
        let used: HashSet<char> = patterns
            .iter()
            .filter_map(|pattern| pattern.split(['{', '}']).nth(2)?.chars().next())
            .collect();
        assert_eq!(used.len(), SEPARATORS.len(), "{patterns:?}");
    }

    #[test]
    fn the_fixtures_have_valid_ibans_and_every_placeholder_filled() {
        for fixture in fixtures() {
            assert!(iban_is_valid(fixture.iban), "{}", fixture.iban);
            let fields = &fixture.fields;
            assert!(fields.sender.is_some() && fields.recipient.is_some());
            assert!(fields.kind.is_some() && fields.date.is_some() && fields.number.is_some());
        }
    }
}
