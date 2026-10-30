//! Measures how well the rules (and, where the caller adds it, the AI) read
//! sender, kind, tax context, date, number and amount of a synthetic corpus of
//! fictitious documents. The corpus lives in `tests/fixtures/recognition`: a
//! header with the expected values, a `---` line, then the document text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fmt;
use std::fs;

use chrono::NaiveDate;
use ncrs_index::{
    Amount, Condition, ConditionKind, Currency, DocumentFields, Field, Index, ReviewReason, Rule,
    RuleConditions, RuleId, RuleSpec, TaxContext,
};
use tempfile::TempDir;

use crate::filing_plan::{plan, PlanInput, Planning, FIELD_ORDER};

const CORPUS_FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/recognition");
const HEADER_END: &str = "\n---\n";
const CORPUS_EXTENSION: &str = "txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusDocument {
    /// The file name without the folder.
    pub name: String,
    pub text: String,
    pub expected: DocumentFields,
}

/// Every document of the corpus, sorted by file name. A file that does not
/// parse panics with its name: the corpus is part of the repository.
pub fn load_corpus() -> Vec<CorpusDocument> {
    let mut paths: Vec<_> = fs::read_dir(CORPUS_FOLDER)
        .unwrap_or_else(|err| panic!("{CORPUS_FOLDER}: {err}"))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == CORPUS_EXTENSION))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let content = fs::read_to_string(path).unwrap_or_else(|err| panic!("{name}: {err}"));
            parse_document(&name, &content).unwrap_or_else(|problem| panic!("{name}: {problem}"))
        })
        .collect()
}

fn parse_document(name: &str, content: &str) -> Result<CorpusDocument, String> {
    let (header, text) = content
        .split_once(HEADER_END)
        .ok_or("no `---` line after the header")?;
    let mut expected = DocumentFields::default();
    let mut seen = BTreeSet::new();
    for line in header.lines() {
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("`{line}` is not `key: value`"))?;
        let field = FIELD_ORDER
            .into_iter()
            .find(|field| field.as_str() == key)
            .ok_or_else(|| format!("unknown key `{key}`"))?;
        if !seen.insert(field.as_str()) {
            return Err(format!("`{key}` twice"));
        }
        set_expected(&mut expected, field, value.trim())
            .map_err(|problem| format!("{key}: {problem}"))?;
    }
    if let Some(absent) = FIELD_ORDER
        .into_iter()
        .find(|field| !seen.contains(field.as_str()))
    {
        return Err(format!("key `{}` is missing", absent.as_str()));
    }
    Ok(CorpusDocument {
        name: name.to_owned(),
        text: text.to_owned(),
        expected,
    })
}

fn set_expected(fields: &mut DocumentFields, field: Field, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    match field {
        Field::Sender => fields.sender = Some(value.to_owned()),
        Field::Kind => fields.kind = Some(value.to_owned()),
        Field::Number => fields.number = Some(value.to_owned()),
        Field::TaxContext => {
            let context = [TaxContext::Company, TaxContext::Private, TaxContext::None]
                .into_iter()
                .find(|context| context.as_str() == value)
                .ok_or_else(|| format!("`{value}` is not a tax context"))?;
            fields.tax_context = Some(context);
        }
        Field::Date => {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|err| format!("`{value}`: {err}"))?;
            fields.date = Some(date);
        }
        Field::Amount => {
            let (minor_units, code) = value
                .split_once(' ')
                .ok_or_else(|| format!("`{value}` is not `<minor units> <CURRENCY>`"))?;
            fields.amount = Some(Amount {
                minor_units: minor_units
                    .parse()
                    .map_err(|_bad| format!("`{minor_units}` is not a whole number"))?,
                currency: Currency::new(code)
                    .ok_or_else(|| format!("`{code}` is not a currency code"))?,
            });
        }
    }
    Ok(())
}

const DATE_DOTTED: &str = r"Datum:?\s*(\d{2}\.\d{2}\.\d{4})";
const DATE_ISO: &str = r"Date:?\s*(\d{4}-\d{2}-\d{2})";

fn contains_rule(
    phrase: &str,
    kind: &str,
    tax_context: TaxContext,
    filing_pattern: &str,
) -> RuleSpec {
    RuleSpec {
        conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, phrase)]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: kind.to_owned(),
        tax_context,
        tags: BTreeSet::new(),
        filing_pattern: filing_pattern.to_owned(),
        confirmed: true,
    }
}

fn german_invoice(match_sender: Option<&str>, filing_pattern: &str) -> RuleSpec {
    let mut spec = contains_rule("rechnung", "Rechnung", TaxContext::Company, filing_pattern);
    spec.conditions
        .list
        .extend(match_sender.map(|sender| Condition::new(ConditionKind::Sender, sender)));
    RuleSpec {
        extract_number: Some(r"Rechnungsnummer:?\s*([A-Z0-9-]+)".to_owned()),
        extract_date: Some(r"(?:Rechnungsdatum|Datum):?\s*(\d{2}\.\d{2}\.\d{4})".to_owned()),
        extract_amount: Some(
            r"(?:Gesamtbetrag|Rechnungsbetrag):?\s*([\d.,]+)\s*(?P<currency>EUR)".to_owned(),
        ),
        ..spec
    }
}

/// The rules the measurement plans against, with the ids 1..n in this order.
/// The first one needs a sender, which only the AI sets; a rule that mentions
/// a word in the middle of another matches there too.
pub fn fixed_rules() -> Vec<Rule> {
    let specs = [
        german_invoice(Some("Nordlicht Energie GmbH"), "{year}/Energie/{number}"),
        german_invoice(None, "{year}/Rechnungen/{number}"),
        RuleSpec {
            extract_number: Some(r"Invoice (?:No\.?|number):?\s*([A-Z0-9-]+)".to_owned()),
            extract_date: Some(DATE_ISO.to_owned()),
            extract_amount: Some(r"Total:?\s*([\d.,]+)\s*(?P<currency>[A-Z]{3})".to_owned()),
            ..contains_rule(
                "invoice",
                "Invoice",
                TaxContext::Company,
                "{year}/Invoices/{number}",
            )
        },
        RuleSpec {
            extract_number: Some(r"Aktenzeichen:?\s*([0-9/]+)".to_owned()),
            extract_date: Some(r"Bescheid vom (\d{2}\.\d{2}\.\d{4})".to_owned()),
            extract_amount: Some(r"Nachzahlung:?\s*([\d.,]+)\s*(?P<currency>EUR)".to_owned()),
            ..contains_rule(
                "Steuerbescheid",
                "Steuerbescheid",
                TaxContext::Private,
                "{year}/Steuern/{number}",
            )
        },
        RuleSpec {
            extract_number: Some(r"Vertragsnummer:?\s*([A-Z0-9-]+)".to_owned()),
            extract_date: Some(r"Vertragsdatum:?\s*(\d{2}\.\d{2}\.\d{4})".to_owned()),
            extract_amount: Some(r"Monatsbeitrag:?\s*([\d.,]+)\s*(?P<currency>EUR)".to_owned()),
            ..contains_rule(
                "Vertrag",
                "Vertrag",
                TaxContext::None,
                "{year}/Verträge/{number}",
            )
        },
        RuleSpec {
            extract_number: Some(r"Angebotsnummer:?\s*([A-Z0-9-]+)".to_owned()),
            extract_date: Some(DATE_DOTTED.to_owned()),
            extract_amount: Some(r"Angebotssumme:?\s*([\d.,]+)\s*(?P<currency>EUR)".to_owned()),
            confirmed: false,
            ..contains_rule(
                "Angebot",
                "Angebot",
                TaxContext::Company,
                "{year}/Angebote/{number}",
            )
        },
        RuleSpec {
            extract_date: Some(DATE_DOTTED.to_owned()),
            ..contains_rule(
                "Kündigung",
                "Kündigung",
                TaxContext::None,
                "{year}/Kündigungen/{date}",
            )
        },
        RuleSpec {
            extract_date: Some(DATE_DOTTED.to_owned()),
            ..contains_rule(
                "Sehr geehrte",
                "Brief",
                TaxContext::Private,
                "{year}/Briefe/{date}",
            )
        },
        RuleSpec {
            extract_date: Some(DATE_ISO.to_owned()),
            ..contains_rule(
                "Dear",
                "Letter",
                TaxContext::Private,
                "{year}/Letters/{date}",
            )
        },
    ];
    // Rule ids are only handed out by the index; the folder is declared before the
    // index so the database file is closed before it is removed.
    let folder = TempDir::new().unwrap();
    let mut index = Index::create_plain(&folder.path().join("index.db")).unwrap();
    for spec in &specs {
        index.create_rule(spec).unwrap();
    }
    index.rules().unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldScore {
    Hit,
    /// A value that differs from the expected one, or a value where none is
    /// expected.
    Wrong,
    /// Expected but absent.
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningOutcome {
    Planned,
    NoRule,
    UnconfirmedRule,
    MissingField(Field),
}

fn score_values<T: PartialEq>(expected: Option<T>, actual: Option<T>) -> FieldScore {
    match (expected, actual) {
        (None, None) => FieldScore::Hit,
        (Some(_), None) => FieldScore::Missing,
        (Some(wanted), Some(found)) if wanted == found => FieldScore::Hit,
        _ => FieldScore::Wrong,
    }
}

fn trimmed_lowercase(value: Option<&str>) -> Option<String> {
    value.map(|text| text.trim().to_lowercase())
}

/// Sender and kind are compared trimmed and case-insensitive, the rest exactly.
pub fn score_field(field: Field, expected: &DocumentFields, actual: &DocumentFields) -> FieldScore {
    match field {
        Field::Sender => score_values(
            trimmed_lowercase(expected.sender.as_deref()),
            trimmed_lowercase(actual.sender.as_deref()),
        ),
        Field::Kind => score_values(
            trimmed_lowercase(expected.kind.as_deref()),
            trimmed_lowercase(actual.kind.as_deref()),
        ),
        Field::TaxContext => score_values(expected.tax_context, actual.tax_context),
        Field::Date => score_values(expected.date, actual.date),
        Field::Number => score_values(expected.number.as_deref(), actual.number.as_deref()),
        Field::Amount => score_values(expected.amount.as_ref(), actual.amount.as_ref()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentResult {
    pub name: String,
    pub outcome: PlanningOutcome,
    /// The rule that was chosen, planned or sent to review.
    pub rule: Option<RuleId>,
    pub scores: Vec<(Field, FieldScore)>,
    /// A rule was chosen, but kind or tax context is not the expected one.
    pub wrong_assignment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecognitionReport {
    pub results: Vec<DocumentResult>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldTally {
    pub hits: usize,
    pub wrong: usize,
    pub missing: usize,
}

impl RecognitionReport {
    pub fn tally(&self, field: Field) -> FieldTally {
        let mut tally = FieldTally::default();
        for (_, score) in self
            .results
            .iter()
            .flat_map(|result| &result.scores)
            .filter(|(scored, _)| *scored == field)
        {
            match score {
                FieldScore::Hit => tally.hits += 1,
                FieldScore::Wrong => tally.wrong += 1,
                FieldScore::Missing => tally.missing += 1,
            }
        }
        tally
    }

    pub fn outcome_count(&self, matching: impl Fn(PlanningOutcome) -> bool) -> usize {
        self.results
            .iter()
            .filter(|result| matching(result.outcome))
            .count()
    }

    pub fn wrong_assignments(&self) -> impl Iterator<Item = &DocumentResult> {
        self.results.iter().filter(|result| result.wrong_assignment)
    }
}

impl fmt::Display for RecognitionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let documents = self.results.len();
        writeln!(f, "documents: {documents}")?;
        writeln!(
            f,
            "{:<12} {:>5} {:>6} {:>8} {:>9}",
            "field", "hits", "wrong", "missing", "hit rate"
        )?;
        for field in FIELD_ORDER {
            let tally = self.tally(field);
            let percent = (tally.hits * 100).checked_div(documents).unwrap_or(0);
            writeln!(
                f,
                "{:<12} {:>5} {:>6} {:>8} {:>8}%",
                field.as_str(),
                tally.hits,
                tally.wrong,
                tally.missing,
                percent
            )?;
        }
        let missing_per_field: Vec<String> = FIELD_ORDER
            .into_iter()
            .filter_map(|field| {
                let count =
                    self.outcome_count(|outcome| outcome == PlanningOutcome::MissingField(field));
                (count > 0).then(|| format!("{} {count}", field.as_str()))
            })
            .collect();
        writeln!(
            f,
            "planned: {}, no rule: {}, unconfirmed rule: {}, missing field: {} ({})",
            self.outcome_count(|outcome| outcome == PlanningOutcome::Planned),
            self.outcome_count(|outcome| outcome == PlanningOutcome::NoRule),
            self.outcome_count(|outcome| outcome == PlanningOutcome::UnconfirmedRule),
            self.outcome_count(|outcome| matches!(outcome, PlanningOutcome::MissingField(_))),
            missing_per_field.join(", ")
        )?;
        let wrong: Vec<&DocumentResult> = self.wrong_assignments().collect();
        writeln!(f, "wrong assignments: {}", wrong.len())?;
        for result in wrong {
            if let Some(rule) = result.rule {
                writeln!(f, "  {} (rule {rule})", result.name)?;
            }
        }
        Ok(())
    }
}

fn outcome_of(planning: &Planning) -> PlanningOutcome {
    match planning {
        Planning::Plan(_) => PlanningOutcome::Planned,
        Planning::Review { reason, .. } => match reason {
            ReviewReason::NoMatchingRule => PlanningOutcome::NoRule,
            ReviewReason::MissingField(field) => PlanningOutcome::MissingField(*field),
            // `plan` gives no other reason; the later steps set them.
            ReviewReason::UnconfirmedRule
            | ReviewReason::TargetExists
            | ReviewReason::LowConfidence(_) => PlanningOutcome::UnconfirmedRule,
        },
    }
}

fn chosen_rule_and_fields(planning: &Planning) -> (Option<RuleId>, &DocumentFields) {
    match planning {
        Planning::Plan(filing) => (Some(filing.rule), &filing.fields),
        Planning::Review { rule, fields, .. } => (*rule, fields),
    }
}

/// Plans every document the way the rules step does, with no fields known.
/// Where the pipeline would ask the AI (no rule, or a field missing),
/// `fill_open_fields` gets the document and the fields known so far and
/// returns them completed, or `None` for no change. After a missing field the
/// document is planned again with them, like the AI step does. The rules-only
/// run passes `|_, _| None`.
pub fn measure(
    corpus: &[CorpusDocument],
    rules: &[Rule],
    mut fill_open_fields: impl FnMut(&CorpusDocument, &DocumentFields) -> Option<DocumentFields>,
) -> RecognitionReport {
    let results = corpus
        .iter()
        .map(|document| {
            let rules_planning = plan(
                rules,
                &PlanInput {
                    text: &document.text,
                    fields: &DocumentFields::default(),
                },
            );
            let planning = match rules_planning {
                Planning::Review {
                    reason: reason @ (ReviewReason::NoMatchingRule | ReviewReason::MissingField(_)),
                    rule,
                    fields,
                } => match fill_open_fields(document, &fields) {
                    Some(merged) if matches!(reason, ReviewReason::MissingField(_)) => plan(
                        rules,
                        &PlanInput {
                            text: &document.text,
                            fields: &merged,
                        },
                    ),
                    Some(merged) => Planning::Review {
                        reason,
                        rule,
                        fields: merged,
                    },
                    None => Planning::Review {
                        reason,
                        rule,
                        fields,
                    },
                },
                other => other,
            };
            result_of(document, &planning)
        })
        .collect();
    RecognitionReport { results }
}

fn result_of(document: &CorpusDocument, planning: &Planning) -> DocumentResult {
    let (rule, actual) = chosen_rule_and_fields(planning);
    let scores: Vec<(Field, FieldScore)> = FIELD_ORDER
        .into_iter()
        .map(|field| (field, score_field(field, &document.expected, actual)))
        .collect();
    let assigned_wrongly = [Field::Kind, Field::TaxContext]
        .into_iter()
        .any(|field| score_field(field, &document.expected, actual) != FieldScore::Hit);
    DocumentResult {
        name: document.name.clone(),
        outcome: outcome_of(planning),
        rule,
        scores,
        wrong_assignment: rule.is_some() && assigned_wrongly,
    }
}

/// The corpus against the fixed rules, nothing filled in by an AI.
pub fn measure_rules_only() -> RecognitionReport {
    measure(&load_corpus(), &fixed_rules(), |_, _| None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(sender: &str, kind: &str) -> DocumentFields {
        DocumentFields {
            sender: Some(sender.to_owned()),
            kind: Some(kind.to_owned()),
            ..DocumentFields::default()
        }
    }

    fn document(name: &str, text: &str, expected: DocumentFields) -> CorpusDocument {
        CorpusDocument {
            name: name.to_owned(),
            text: text.to_owned(),
            expected,
        }
    }

    fn only_result(corpus: &[CorpusDocument]) -> DocumentResult {
        measure(corpus, &fixed_rules(), |_, _| None)
            .results
            .remove(0)
    }

    #[test]
    fn the_corpus_loads_and_every_document_has_sender_and_kind() {
        let corpus = load_corpus();

        assert!(corpus.len() >= 24, "{} documents", corpus.len());
        for document in &corpus {
            assert!(document.expected.sender.is_some(), "{}", document.name);
            assert!(document.expected.kind.is_some(), "{}", document.name);
            assert!(!document.text.trim().is_empty(), "{}", document.name);
        }
        let names: Vec<&str> = corpus.iter().map(|found| found.name.as_str()).collect();
        assert!(names.is_sorted());
    }

    #[test]
    fn a_header_that_does_not_parse_is_an_error() {
        let complete = "sender: A\nkind: B\ntax_context: none\ndate: 2026-01-01\nnumber:\namount:\n---\ntext\n";
        parse_document("ok", complete).unwrap();
        for broken in [
            complete.replace("\n---\n", "\n"),
            complete.replace("number:", "colour:"),
            complete.replace("none", "business"),
            complete.replace("2026-01-01", "01.01.2026"),
            complete.replace("amount:", "amount: 119 euro"),
            complete.replace("number:\n", ""),
            complete.replace("number:\n", "number:\nnumber:\n"),
        ] {
            assert!(parse_document("broken", &broken).is_err(), "{broken}");
        }
    }

    #[test]
    fn equal_values_are_a_hit_and_so_are_two_absent_ones() {
        let expected = DocumentFields {
            number: Some("RE-1".to_owned()),
            ..DocumentFields::default()
        };

        assert_eq!(
            score_field(Field::Number, &expected, &expected),
            FieldScore::Hit
        );
        assert_eq!(
            score_field(
                Field::Amount,
                &DocumentFields::default(),
                &DocumentFields::default()
            ),
            FieldScore::Hit
        );
    }

    #[test]
    fn sender_and_kind_ignore_case_and_outer_spaces_but_other_fields_do_not() {
        let expected = fields("Nordlicht Energie GmbH", "Rechnung");
        let actual = fields("  nordlicht energie gmbh ", "RECHNUNG");
        assert_eq!(
            score_field(Field::Sender, &expected, &actual),
            FieldScore::Hit
        );
        assert_eq!(
            score_field(Field::Kind, &expected, &actual),
            FieldScore::Hit
        );

        let number = |text: &str| DocumentFields {
            number: Some(text.to_owned()),
            ..DocumentFields::default()
        };
        assert_eq!(
            score_field(Field::Number, &number("RE-1"), &number("re-1")),
            FieldScore::Wrong
        );
    }

    #[test]
    fn a_different_value_or_a_value_where_none_is_expected_is_wrong() {
        let expected = fields("Nordlicht Energie GmbH", "Rechnung");
        assert_eq!(
            score_field(
                Field::Sender,
                &expected,
                &fields("Nordlicht Energie", "Rechnung")
            ),
            FieldScore::Wrong
        );
        assert_eq!(
            score_field(Field::Sender, &DocumentFields::default(), &expected),
            FieldScore::Wrong
        );
    }

    #[test]
    fn an_expected_value_that_is_absent_is_missing() {
        let expected = fields("Nordlicht Energie GmbH", "Rechnung");

        assert_eq!(
            score_field(Field::Sender, &expected, &DocumentFields::default()),
            FieldScore::Missing
        );
    }

    #[test]
    fn a_document_no_rule_covers_is_no_rule_and_has_no_wrong_assignment() {
        let expected = fields("Tannenhof Catering UG", "Postkarte");
        let result = only_result(&[document("card", "Liebe Grüße von der Ostsee", expected)]);

        assert_eq!(result.outcome, PlanningOutcome::NoRule);
        assert!(!result.wrong_assignment);
    }

    #[test]
    fn a_rule_that_matches_the_wrong_kind_is_a_wrong_assignment() {
        let expected = DocumentFields {
            kind: Some("Brief".to_owned()),
            tax_context: Some(TaxContext::Private),
            ..DocumentFields::default()
        };
        let result = only_result(&[document(
            "letter",
            "Ihre Rechnungsanschrift bitte",
            expected,
        )]);

        assert!(result.rule.is_some());
        assert!(result.wrong_assignment);
    }

    #[test]
    fn a_confirmed_rule_that_cannot_read_a_field_is_a_missing_field() {
        let result = only_result(&[document(
            "invoice",
            "Rechnung\nRechnungsnummer: RE-1",
            fields("A", "Rechnung"),
        )]);

        assert_eq!(result.outcome, PlanningOutcome::MissingField(Field::Date));
    }

    #[test]
    fn a_complete_invoice_is_planned() {
        let text =
            "Rechnung\nRechnungsnummer: RE-1\nRechnungsdatum: 01.02.2026\nGesamtbetrag: 1,00 EUR";
        let result = only_result(&[document("invoice", text, fields("A", "Rechnung"))]);

        assert_eq!(result.outcome, PlanningOutcome::Planned);
    }

    #[test]
    fn only_an_unconfirmed_rule_is_an_unconfirmed_rule() {
        let result = only_result(&[document("offer", "Angebot", fields("A", "Angebot"))]);

        assert_eq!(result.outcome, PlanningOutcome::UnconfirmedRule);
    }

    #[test]
    fn the_rules_only_report_is_the_same_on_two_runs() {
        assert_eq!(measure_rules_only(), measure_rules_only());
    }

    #[test]
    #[ignore = "measurement, run with --ignored --nocapture"]
    fn recognition_measured_with_the_rules_only() {
        println!("rules only\n{}", measure_rules_only());
    }
}
