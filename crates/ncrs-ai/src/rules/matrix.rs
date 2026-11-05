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
use std::path::PathBuf;

use ncrs_index::{
    Combinator, Condition, ConditionKind, DocumentFields, KindLanguage, Kinds, RuleConditions,
    RuleSpec,
};
use ncrs_intake::{parse_date, render_pattern_in, FILING_PLACEHOLDERS};

use super::vocabulary::COMBINATORS;

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
}

impl Axis {
    pub const ALL: [Self; 4] = [
        Self::Folder,
        Self::FileName,
        Self::Condition,
        Self::Recognize,
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
            Ask::Paths { paths, question: _ } => paths.join(" | "),
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

struct Row {
    sender: &'static str,
    domain: &'static str,
    recipient: &'static str,
    address: &'static str,
    kind: &'static str,
    date: &'static str,
    number: &'static str,
    folder: &'static str,
    tag: &'static str,
    keyword: &'static str,
    iban: &'static str,
    tax_id: &'static str,
}

/// Days above 12 and different from the month, years and numbers all
/// different: no two placeholders render the same on these.
const ROWS: [Row; 8] = [
    Row {
        sender: "Telekom Deutschland GmbH",
        domain: "telekom.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "invoice",
        date: "2023-03-14",
        number: "R-2023-0314",
        folder: "Posteingang/Telefon",
        tag: "steuer",
        keyword: "Mobilfunk",
        iban: "DE89370400440532013000",
        tax_id: "12/345/67890",
    },
    Row {
        sender: "Allianz Versicherung",
        domain: "allianz.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "contract",
        date: "2022-11-28",
        number: "V-88231",
        folder: "Versicherungen",
        tag: "privat",
        keyword: "Haftpflicht",
        iban: "DE02120300000000202051",
        tax_id: "21/987/65432",
    },
    Row {
        sender: "Stadtwerke Müller",
        domain: "stadtwerke-mueller.de",
        recipient: "Jürgen Köhler",
        address: "Gartenweg 5\n04109 Leipzig",
        kind: "reminder",
        date: "2024-01-31",
        number: "M-4412",
        folder: "Posteingang/Energie",
        tag: "energie",
        keyword: "Stromlieferung",
        iban: "DE75512108001245126199",
        tax_id: "33/222/11110",
    },
    Row {
        sender: "Finanzamt Chemnitz",
        domain: "finanzamt-chemnitz.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "notice",
        date: "2021-06-25",
        number: "St-2021-77",
        folder: "Steuer",
        tag: "steuer",
        keyword: "Einkommensteuer",
        iban: "DE12500105170648489890",
        tax_id: "44/555/66670",
    },
    Row {
        sender: "Sparkasse Leipzig",
        domain: "sparkasse-leipzig.de",
        recipient: "Anna Weiß",
        address: "Lindenallee 7\n80331 München",
        kind: "bank_statement",
        date: "2020-09-19",
        number: "KA-2020-09",
        folder: "Bank",
        tag: "privat",
        keyword: "Kontostand",
        iban: "DE68210501700012345678",
        tax_id: "55/666/77780",
    },
    Row {
        sender: "Zalando SE",
        domain: "zalando.de",
        recipient: "Jürgen Köhler",
        address: "Gartenweg 5\n04109 Leipzig",
        kind: "receipt",
        date: "2019-05-17",
        number: "Z-558120",
        folder: "Einkäufe",
        tag: "einkauf",
        keyword: "Bestellung",
        iban: "DE27100777770209299700",
        tax_id: "66/777/88890",
    },
    Row {
        sender: "Praxis Dr. Schäfer",
        domain: "praxis-schaefer.de",
        recipient: "Anna Weiß",
        address: "Lindenallee 7\n80331 München",
        kind: "invoice",
        date: "2025-08-22",
        number: "A-2025-0822",
        folder: "Posteingang/Arzt",
        tag: "steuer",
        keyword: "Behandlung",
        iban: "DE91100000000123456789",
        tax_id: "77/888/99900",
    },
    Row {
        sender: "Stromio GmbH",
        domain: "stromio.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "termination",
        date: "2017-12-29",
        number: "K-91827",
        folder: "Posteingang/Telefon",
        tag: "wichtig",
        keyword: "Vertragsende",
        iban: "DE44500105175407324931",
        tax_id: "88/999/00010",
    },
];

#[derive(Debug, Clone)]
pub struct Fixture {
    pub fields: DocumentFields,
    pub text: String,
    pub path: PathBuf,
    pub folder: &'static str,
    pub domain: &'static str,
    pub iban: &'static str,
    pub tax_id: &'static str,
    pub keyword: &'static str,
    pub tag: &'static str,
}

impl Fixture {
    fn from_row(index: usize, row: &Row) -> Self {
        let mut day_month_year = row.date.split('-').rev();
        let dotted = day_month_year.by_ref().collect::<Vec<_>>().join(".");
        let kinds = Kinds::built_in(KindLanguage::German);
        let text = format!(
            "{sender}\nMusterstr. {index}, 10115 Berlin\nservice@{domain} · Tel. 0800 1234\n\n\
             {recipient}\n{address}\n\n{kind} Nr. {number}\n{COMMON_WORD}: 4711-{index}\n\
             Datum: {dotted}\nBetreff: {keyword}\nZahlbar auf IBAN {iban}\nSteuernummer {tax_id}\n",
            sender = row.sender,
            domain = row.domain,
            recipient = row.recipient,
            address = row.address,
            kind = kinds.kind_word(row.kind),
            number = row.number,
            keyword = row.keyword,
            iban = spaced(row.iban),
            tax_id = row.tax_id,
        );
        Self {
            fields: DocumentFields {
                sender: Some(row.sender.to_owned()),
                recipient: Some(row.recipient.to_owned()),
                kind: Some(row.kind.to_owned()),
                date: parse_date(row.date),
                number: Some(row.number.to_owned()),
                amount: None,
                tags: BTreeSet::from([row.tag.to_owned()]),
            },
            text,
            path: PathBuf::from(format!("{}/scan-{index}.pdf", row.folder)),
            folder: row.folder,
            domain: row.domain,
            iban: row.iban,
            tax_id: row.tax_id,
            keyword: row.keyword,
            tag: row.tag,
        }
    }
}

fn spaced(iban: &str) -> String {
    iban.as_bytes()
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn fixtures() -> Vec<Fixture> {
    ROWS.iter()
        .enumerate()
        .map(|(index, row)| Fixture::from_row(index + 1, row))
        .collect()
}

/// The language the archive of the fixtures is shown in.
pub fn archive_kinds() -> Kinds {
    Kinds::built_in(KindLanguage::German)
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
    for kind in ConditionKind::ALL {
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
    for (first_position, first) in ConditionKind::ALL.into_iter().enumerate() {
        for second in ConditionKind::ALL.into_iter().skip(first_position + 1) {
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

fn recognize_cases(patterns: Vec<(String, Vec<&'static str>)>) -> Vec<Case> {
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
                axis: Axis::Recognize,
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
    cases.extend(recognize_cases(patterns));
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
        let kinds = ConditionKind::ALL.len();
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
        let generate = all_cases()
            .iter()
            .filter(|case| case.axis != Axis::Recognize)
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
