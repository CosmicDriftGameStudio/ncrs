//! Decides, from the rules and the text of a document, where the document
//! would be filed. Pure: nothing here touches a file or the index, and the
//! text is only ever searched, never acted on.
//!
//! A filing pattern is a relative path with these placeholders:
//! `{year}`, `{month}`, `{day}`, `{date}` (`YYYY-MM-DD`), `{sender}`,
//! `{number}`, `{kind}` and `{tax_context}`. Patterns learned from an archive
//! add `{folder}` (the sender) and `{name}` (the kind).

use std::path::PathBuf;

use chrono::Datelike as _;
use ncrs_index::{DocumentFields, Field, ReviewReason, Rule, RuleId, RuleSpec, TextMatch};

use crate::extract::{compile, extract_amount, extract_date, extract_number};
use crate::sanitise::{is_plain_literal, sanitise_value, stays_below};

/// What is known about a document before the rules run.
#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub text: &'a str,
    /// For example a sender found earlier; the rules fill in the rest.
    pub fields: &'a DocumentFields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingPlan {
    pub rule: RuleId,
    pub fields: DocumentFields,
    /// Below the root of the archive; `stays_below` holds for it.
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planning {
    Plan(FilingPlan),
    /// The user has to look. `fields` holds whatever could be filled, so a
    /// later step can fill the rest and plan again.
    Review {
        reason: ReviewReason,
        rule: Option<RuleId>,
        fields: DocumentFields,
    },
}

/// Whether a "text contains" condition holds for a text that is already
/// lowercase: case-insensitive, and an empty condition never holds.
pub fn contains_condition_holds(lowered_text: &str, needle: &str) -> bool {
    !needle.is_empty() && lowered_text.contains(&needle.to_lowercase())
}

/// Whether `source` is a filing pattern `plan` can render.
pub fn pattern_is_valid(source: &str) -> bool {
    Pattern::parse(source).is_some()
}

/// The first confirmed rule that matches wins, in the order of `rules`.
pub fn plan(rules: &[Rule], input: &PlanInput<'_>) -> Planning {
    let lowered_text = input.text.to_lowercase();
    let mut unconfirmed = None;
    for rule in rules {
        if !matches(&rule.spec, input, &lowered_text) {
            continue;
        }
        if rule.spec.confirmed {
            return plan_with(rule, input);
        }
        unconfirmed.get_or_insert(rule);
    }
    match unconfirmed {
        Some(rule) => Planning::Review {
            reason: ReviewReason::UnconfirmedRule,
            rule: Some(rule.id),
            fields: filled_fields(&rule.spec, input),
        },
        None => Planning::Review {
            reason: ReviewReason::NoMatchingRule,
            rule: None,
            fields: input.fields.clone(),
        },
    }
}

/// All conditions of the rule hold. A rule without a condition would match
/// every document, so it matches none; an empty or invalid condition and a
/// regex over the size limit make the rule not match.
fn matches(spec: &RuleSpec, input: &PlanInput<'_>, lowered_text: &str) -> bool {
    let has_condition = spec.match_text.is_some() || spec.match_sender.is_some();
    let text_matches = match &spec.match_text {
        None => true,
        Some(TextMatch::Contains(needle)) => contains_condition_holds(lowered_text, needle),
        Some(TextMatch::Regex(source)) => {
            compile(source).is_some_and(|regex| regex.is_match(input.text))
        }
    };
    let sender_matches = match &spec.match_sender {
        None => true,
        Some(wanted) => input
            .fields
            .sender
            .as_ref()
            .is_some_and(|sender| sender.to_lowercase() == wanted.to_lowercase()),
    };
    has_condition && text_matches && sender_matches
}

/// The known fields, completed from the rule and from the text. Kind, tax
/// context and tags are the rule's; number, date and amount are read from the
/// text where the rule says how.
fn filled_fields(spec: &RuleSpec, input: &PlanInput<'_>) -> DocumentFields {
    let mut fields = input.fields.clone();
    if fields.sender.is_none() {
        fields.sender = spec.match_sender.clone();
    }
    if !spec.kind.is_empty() {
        fields.kind = Some(spec.kind.clone());
    }
    fields.tax_context = Some(spec.tax_context);
    fields.tags.extend(spec.tags.iter().cloned());
    if let Some(number) = spec
        .extract_number
        .as_deref()
        .and_then(|source| extract_number(source, input.text))
    {
        fields.number = Some(number);
    }
    if let Some(date) = spec
        .extract_date
        .as_deref()
        .and_then(|source| extract_date(source, input.text))
    {
        fields.date = Some(date);
    }
    if let Some(amount) = spec
        .extract_amount
        .as_deref()
        .and_then(|source| extract_amount(source, input.text))
    {
        fields.amount = Some(amount);
    }
    fields
}

fn plan_with(rule: &Rule, input: &PlanInput<'_>) -> Planning {
    let fields = filled_fields(&rule.spec, input);
    let review = |reason, filled| Planning::Review {
        reason,
        rule: Some(rule.id),
        fields: filled,
    };
    // A rule whose pattern cannot be rendered needs the user like an
    // unconfirmed one: there is nothing the text could add.
    let Some(pattern) = Pattern::parse(&rule.spec.filing_pattern) else {
        return review(ReviewReason::UnconfirmedRule, fields);
    };
    if let Some(missing) = first_missing(&fields, &rule.spec, &pattern) {
        return review(ReviewReason::MissingField(missing), fields);
    }
    match pattern.render(&fields) {
        Some(relative_path) if stays_below(&relative_path) => Planning::Plan(FilingPlan {
            rule: rule.id,
            fields,
            relative_path,
        }),
        _ => review(ReviewReason::UnconfirmedRule, fields),
    }
}

/// The path a pattern gives for these fields, below the root of the archive.
/// `None` when the pattern cannot be rendered, a value it needs is missing, or
/// the result would not stay below the root.
pub fn render_pattern(source: &str, fields: &DocumentFields) -> Option<PathBuf> {
    Pattern::parse(source)?
        .render(fields)
        .filter(|path| stays_below(path))
}

/// The order fields are asked for and reported in.
pub const FIELD_ORDER: [Field; 6] = Field::ALL;

/// A field is needed when the pattern uses it or the rule says how to read it
/// from the text; a value that is empty after sanitising is missing.
fn first_missing(fields: &DocumentFields, spec: &RuleSpec, pattern: &Pattern) -> Option<Field> {
    let needed = |field: Field| {
        pattern.uses(field)
            || match field {
                Field::Date => spec.extract_date.is_some(),
                Field::Number => spec.extract_number.is_some(),
                Field::Amount => spec.extract_amount.is_some(),
                Field::Sender | Field::Kind | Field::TaxContext => false,
            }
    };
    FIELD_ORDER
        .into_iter()
        .find(|&field| needed(field) && !field_is_present(fields, field))
}

fn field_is_present(fields: &DocumentFields, field: Field) -> bool {
    match field {
        Field::Sender => fields.sender.as_deref().and_then(sanitise_value).is_some(),
        Field::Kind => fields.kind.as_deref().and_then(sanitise_value).is_some(),
        Field::TaxContext => fields.tax_context.is_some(),
        Field::Date => fields.date.is_some(),
        Field::Number => fields.number.as_deref().and_then(sanitise_value).is_some(),
        Field::Amount => fields.amount.is_some(),
    }
}

/// The first field the pattern uses that is empty in `fields`; `None` when
/// none is, or the pattern does not read.
pub fn missing_field(source: &str, fields: &DocumentFields) -> Option<Field> {
    let pattern = Pattern::parse(source)?;
    FIELD_ORDER
        .into_iter()
        .find(|&field| pattern.uses(field) && !field_is_present(fields, field))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    Year,
    Month,
    Day,
    Date,
    Sender,
    Number,
    Kind,
    TaxContext,
    Folder,
    Name,
}

impl Placeholder {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "year" => Self::Year,
            "month" => Self::Month,
            "day" => Self::Day,
            "date" => Self::Date,
            "sender" => Self::Sender,
            "number" => Self::Number,
            "kind" => Self::Kind,
            "tax_context" => Self::TaxContext,
            "folder" => Self::Folder,
            "name" => Self::Name,
            _ => return None,
        })
    }

    fn field(self) -> Field {
        match self {
            Self::Year | Self::Month | Self::Day | Self::Date => Field::Date,
            Self::Sender | Self::Folder => Field::Sender,
            Self::Number => Field::Number,
            Self::Kind | Self::Name => Field::Kind,
            Self::TaxContext => Field::TaxContext,
        }
    }

    /// Text values pass through `sanitise_value` here, whatever their origin.
    fn value(self, fields: &DocumentFields) -> Option<String> {
        match self {
            Self::Year => fields.date.map(|date| format!("{:04}", date.year())),
            Self::Month => fields.date.map(|date| format!("{:02}", date.month())),
            Self::Day => fields.date.map(|date| format!("{:02}", date.day())),
            Self::Date => fields.date.map(|date| date.to_string()),
            Self::Sender | Self::Folder => fields.sender.as_deref().and_then(sanitise_value),
            Self::Number => fields.number.as_deref().and_then(sanitise_value),
            Self::Kind | Self::Name => fields.kind.as_deref().and_then(sanitise_value),
            Self::TaxContext => fields
                .tax_context
                .map(|context| context.as_str().to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Placeholder(Placeholder),
}

/// A parsed filing pattern: folders and a file name, each a list of tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    components: Vec<Vec<Token>>,
}

impl Pattern {
    /// `None` for a pattern that cannot be rendered on its own: an unknown
    /// placeholder, a stray brace, an empty component (which includes an
    /// absolute path), a component of only dots, or a literal with a control
    /// or Windows-reserved character (which includes a drive prefix).
    /// Both `/` and `\` separate components.
    fn parse(source: &str) -> Option<Self> {
        let unified = source.replace('\\', "/");
        let components = unified
            .split('/')
            .map(Self::parse_component)
            .collect::<Option<Vec<_>>>()?;
        Some(Self { components })
    }

    fn parse_component(text: &str) -> Option<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let Some(at) = rest.find(['{', '}']) else {
                tokens.push(Token::Literal(rest.to_owned()));
                break;
            };
            let (before, from) = rest.split_at_checked(at)?;
            if !before.is_empty() {
                tokens.push(Token::Literal(before.to_owned()));
            }
            let (name, after) = from.strip_prefix('{')?.split_once('}')?;
            tokens.push(Token::Placeholder(Placeholder::parse(name)?));
            rest = after;
        }
        let only_dots = tokens.iter().all(|token| match token {
            Token::Literal(literal) => literal.trim_matches(['.', ' ']).is_empty(),
            Token::Placeholder(_) => false,
        });
        let plain = tokens.iter().all(|token| match token {
            Token::Literal(literal) => is_plain_literal(literal),
            Token::Placeholder(_) => true,
        });
        (!only_dots && plain).then_some(tokens)
    }

    fn uses(&self, field: Field) -> bool {
        self.components.iter().flatten().any(|token| match token {
            Token::Placeholder(placeholder) => placeholder.field() == field,
            Token::Literal(_) => false,
        })
    }

    /// `None` when a value is missing.
    fn render(&self, fields: &DocumentFields) -> Option<PathBuf> {
        let mut path = PathBuf::new();
        for component in &self.components {
            let mut name = String::new();
            for token in component {
                match token {
                    Token::Literal(literal) => name.push_str(literal),
                    Token::Placeholder(placeholder) => name.push_str(&placeholder.value(fields)?),
                }
            }
            path.push(name);
        }
        Some(path)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use chrono::NaiveDate;
    use ncrs_index::{Amount, Currency, TaxContext};

    use super::*;

    #[test]
    fn a_pattern_is_valid_when_it_can_be_parsed() {
        assert!(pattern_is_valid("{year}/Invoices/{sender}"));
        assert!(!pattern_is_valid("{year}/{nonsense}"));
        assert!(!pattern_is_valid("{year"));
        assert!(!pattern_is_valid("/absolute"));
    }

    #[test]
    fn a_contains_condition_ignores_case_and_an_empty_one_never_holds() {
        assert!(contains_condition_holds(
            "invoice from company a ltd",
            "Company A"
        ));
        assert!(!contains_condition_holds(
            "invoice from agency b",
            "company a"
        ));
        assert!(!contains_condition_holds("anything", ""));
    }

    fn spec(pattern: &str) -> RuleSpec {
        RuleSpec {
            match_text: Some(TextMatch::Contains("company a ltd".to_owned())),
            match_sender: None,
            extract_number: Some(r"Invoice\s+(\d+)".to_owned()),
            extract_date: Some(r"Date:\s*(\d{2}\.\d{2}\.\d{4})".to_owned()),
            extract_amount: Some(r"Total:\s*([\d.,]+\s*€)".to_owned()),
            kind: "invoice".to_owned(),
            tax_context: TaxContext::Company,
            tags: BTreeSet::from(["office".to_owned()]),
            filing_pattern: pattern.to_owned(),
            confirmed: true,
        }
    }

    /// Rule ids are only handed out by the index.
    fn rules_from(specs: &[RuleSpec]) -> Vec<Rule> {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = ncrs_index::Index::create_plain(&dir.path().join("index.db")).unwrap();
        for spec in specs {
            index.create_rule(spec).unwrap();
        }
        index.rules().unwrap()
    }

    const TEXT: &str =
        "Company A Ltd\nInvoice 4711\nDate: 01.03.2026\nTotal: 1.234,56 €\nthank you";

    fn plan_for(spec: RuleSpec, text: &str, fields: &DocumentFields) -> Planning {
        plan(&rules_from(&[spec]), &PlanInput { text, fields })
    }

    fn planned(planning: Planning) -> FilingPlan {
        match planning {
            Planning::Plan(plan) => plan,
            Planning::Review { reason, .. } => panic!("review: {reason:?}"),
        }
    }

    fn reason(planning: &Planning) -> ReviewReason {
        match planning {
            Planning::Review { reason, .. } => *reason,
            Planning::Plan(_) => panic!("expected a review"),
        }
    }

    #[test]
    fn a_text_rule_matches_and_reads_number_date_and_amount() {
        let known = DocumentFields {
            sender: Some("Company A".to_owned()),
            ..DocumentFields::default()
        };

        let filed = planned(plan_for(
            spec("{year}/{date}_{sender}_{number}.pdf"),
            TEXT,
            &known,
        ));

        assert_eq!(
            filed.relative_path,
            Path::new("2026").join("2026-03-01_Company A_4711.pdf")
        );
        assert_eq!(filed.fields.number.as_deref(), Some("4711"));
        assert_eq!(filed.fields.date, NaiveDate::from_ymd_opt(2026, 3, 1));
        assert_eq!(
            filed.fields.amount,
            Some(Amount {
                minor_units: 123_456,
                currency: Currency::new("EUR").unwrap()
            })
        );
        assert_eq!(filed.fields.kind.as_deref(), Some("invoice"));
        assert_eq!(filed.fields.tax_context, Some(TaxContext::Company));
        assert!(filed.fields.tags.contains("office"));
    }

    #[test]
    fn a_regex_rule_matches_and_sender_is_compared_without_case() {
        let mut regex_spec = spec("{year}/{number}");
        regex_spec.match_text = Some(TextMatch::Regex(r"(?i)company\s+a\s+ltd".to_owned()));
        regex_spec.match_sender = Some("COMPANY A".to_owned());
        let known = DocumentFields {
            sender: Some("company a".to_owned()),
            ..DocumentFields::default()
        };

        assert!(matches!(
            plan_for(regex_spec.clone(), TEXT, &known),
            Planning::Plan(_)
        ));
        assert_eq!(
            reason(&plan_for(regex_spec, TEXT, &DocumentFields::default())),
            ReviewReason::NoMatchingRule
        );
    }

    #[test]
    fn an_invalid_or_too_large_regex_does_not_match_and_does_not_panic() {
        let known = DocumentFields::default();
        for source in ["(unclosed", "(a{1000}){1000}{1000}", r"\p{L}{1000}{1000}"] {
            let mut broken = spec("{number}");
            broken.match_text = Some(TextMatch::Regex(source.to_owned()));
            assert_eq!(
                reason(&plan_for(broken, TEXT, &known)),
                ReviewReason::NoMatchingRule,
                "{source}"
            );
        }
        let mut broken_extractor = spec("{number}");
        broken_extractor.extract_number = Some("(unclosed".to_owned());
        assert_eq!(
            reason(&plan_for(broken_extractor, TEXT, &known)),
            ReviewReason::MissingField(Field::Number)
        );
    }

    #[test]
    fn a_rule_without_a_condition_matches_nothing() {
        // The index refuses such a rule; the planner does not rely on that.
        let mut rules = rules_from(&[spec("{number}")]);
        rules[0].spec.match_text = None;
        let fields = DocumentFields::default();

        let planning = plan(
            &rules,
            &PlanInput {
                text: TEXT,
                fields: &fields,
            },
        );

        assert_eq!(reason(&planning), ReviewReason::NoMatchingRule);
    }

    #[test]
    fn the_first_confirmed_rule_wins_over_an_earlier_unconfirmed_one() {
        let mut unconfirmed = spec("a");
        unconfirmed.confirmed = false;
        let rules = rules_from(&[unconfirmed, spec("{number}")]);
        let fields = DocumentFields::default();

        let planning = plan(
            &rules,
            &PlanInput {
                text: TEXT,
                fields: &fields,
            },
        );

        assert_eq!(planned(planning).rule, rules[1].id);
    }

    #[test]
    fn an_unconfirmed_matching_rule_needs_review() {
        let mut unconfirmed = spec("{number}");
        unconfirmed.confirmed = false;

        let planning = plan_for(unconfirmed, TEXT, &DocumentFields::default());

        let Planning::Review {
            reason,
            rule,
            fields,
        } = planning
        else {
            panic!("expected a review");
        };
        assert_eq!(reason, ReviewReason::UnconfirmedRule);
        assert!(rule.is_some());
        assert_eq!(fields.number.as_deref(), Some("4711"));
    }

    #[test]
    fn a_missing_field_is_named() {
        let planning = plan_for(spec("{sender}/{number}"), TEXT, &DocumentFields::default());
        assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Sender));

        let no_date = plan_for(
            spec("{year}"),
            "Company A Ltd Invoice 4711 Total: 5 €",
            &DocumentFields::default(),
        );
        assert_eq!(reason(&no_date), ReviewReason::MissingField(Field::Date));
    }

    #[test]
    fn an_amount_without_currency_counts_as_missing() {
        let mut bare = spec("{number}");
        bare.extract_amount = Some(r"Total:\s*([\d.,]+)".to_owned());

        let planning = plan_for(
            bare,
            "Company A Ltd Invoice 4711 Date: 01.03.2026 Total: 12,50",
            &DocumentFields::default(),
        );

        assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Amount));
    }

    #[test]
    fn a_pattern_that_cannot_be_rendered_needs_review() {
        for pattern in [
            "{unknown}",
            "{year",
            "year}",
            "/{number}",
            "../{number}",
            "{year}/../{number}",
            "{year}//{number}",
            "C:/{number}",
            "{year}/.",
            "...",
            "",
            "{year}/a\u{0}b",
        ] {
            let planning = plan_for(spec(pattern), TEXT, &DocumentFields::default());
            assert_eq!(
                reason(&planning),
                ReviewReason::UnconfirmedRule,
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn hostile_values_cannot_leave_the_target_folder() {
        let hostile = [
            "../../etc/passwd",
            "..\\..\\windows",
            "/absolute",
            "C:\\temp",
            "a/../b",
            "..",
            "...",
            " . . ",
            "\u{0}\u{7}\t\n",
            "x\u{202E}gpj.exe",
            "<>:\"|?*",
        ];
        for value in hostile {
            let fields = DocumentFields {
                sender: Some(value.to_owned()),
                number: Some(value.to_owned()),
                ..DocumentFields::default()
            };
            let mut hostile_rule = spec("{sender}/{kind}/{number}.pdf");
            hostile_rule.kind = value.to_owned();
            hostile_rule.extract_number = None;
            hostile_rule.extract_date = None;
            hostile_rule.extract_amount = None;
            let planning = plan_for(hostile_rule, "Company A Ltd", &fields);
            match planning {
                Planning::Plan(filed) => {
                    assert!(stays_below(&filed.relative_path), "{value:?}");
                    for component in filed.relative_path.components() {
                        let text = component.as_os_str().to_string_lossy();
                        assert!(
                            !text.chars().any(|c| matches!(
                                c,
                                '/' | '\\' | ':' | '<' | '>' | '|' | '?' | '*' | '"'
                            ) || c.is_control()),
                            "{value:?} -> {text:?}"
                        );
                        assert!(!text.starts_with(['.', ' ']) && !text.ends_with(['.', ' ']));
                    }
                }
                Planning::Review { reason, .. } => assert!(
                    matches!(reason, ReviewReason::MissingField(_)),
                    "{value:?}: {reason:?}"
                ),
            }
        }
    }

    #[test]
    fn a_number_read_from_text_is_sanitised_before_it_is_stored() {
        let mut dotty = spec("{number}.pdf");
        dotty.extract_number = Some(r"Ref:(\S+)".to_owned());
        dotty.extract_date = None;
        dotty.extract_amount = None;

        let filed = planned(plan_for(
            dotty,
            "Company A Ltd Ref:../../x/y",
            &DocumentFields::default(),
        ));

        assert_eq!(filed.fields.number.as_deref(), Some("xy"));
        assert_eq!(filed.relative_path, Path::new("xy.pdf"));
    }

    #[test]
    fn a_number_of_only_dots_is_missing() {
        let mut dotty = spec("{number}");
        dotty.extract_number = Some(r"Ref:(\S+)".to_owned());
        dotty.extract_date = None;
        dotty.extract_amount = None;

        let planning = plan_for(dotty, "Company A Ltd Ref:....", &DocumentFields::default());

        assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Number));
    }

    #[test]
    fn stays_below_checks_components() {
        assert!(stays_below(Path::new("a/b..c/d.pdf")));
        assert!(!stays_below(Path::new("")));
        assert!(!stays_below(Path::new("../a")));
        assert!(!stays_below(Path::new("a/../../b")));
        assert!(!stays_below(Path::new("/a")));
        assert!(!stays_below(Path::new("./a")));
    }
}
