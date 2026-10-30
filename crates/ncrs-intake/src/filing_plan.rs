//! Decides, from the rules and the text of a document, where the document
//! would be filed. Pure: nothing here touches a file or the index, and the
//! text is only ever searched, never acted on.
//!
//! A filing pattern is a relative path with these placeholders:
//! `{year}`, `{month}`, `{day}`, `{date}` (`YYYY-MM-DD`), `{sender}`,
//! `{number}`, `{kind}` and `{tax_context}`. Patterns learned from an archive
//! add `{folder}` (the sender) and `{name}` (the kind).

use std::path::{Path, PathBuf};

use chrono::Datelike as _;
use ncrs_index::{
    Combinator, Condition, ConditionKind, DocumentFields, Field, ReviewReason, Rule, RuleId,
    RuleSpec, SenderAliases,
};

use crate::conditions::{
    folder_holds, iban_holds, recipient_holds, sender_address_holds, tax_id_holds, word_holds,
    SearchText,
};
use crate::extract::{compile, extract_amount, extract_date, extract_number};
use crate::sanitise::{is_plain_literal, sanitise_value, stays_below};

/// What is known about a document before the rules run.
#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub text: &'a str,
    /// For example a sender found earlier; the rules fill in the rest.
    pub fields: &'a DocumentFields,
    /// Where the document lies now, in the inbox or the archive; `None` when
    /// the caller has no path, which makes a folder condition unusable.
    pub path: Option<&'a Path>,
    /// Names the sender condition treats as one sender.
    pub sender_aliases: &'a SenderAliases,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingPlan {
    /// The first matching rule.
    pub rule: RuleId,
    /// Every enabled confirmed rule that matches, in rule order; all of them
    /// plan the same.
    pub matching_rules: Vec<RuleId>,
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
        /// The first matching rule.
        rule: Option<RuleId>,
        /// Every enabled rule that matched; empty when none did.
        matching_rules: Vec<RuleId>,
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

/// Plans with the enabled rules that match, in the order of `rules`. When
/// several confirmed rules match they must agree: the same result for the
/// document, whichever rule it came from. Otherwise the document needs review
/// with `ReviewReason::RuleConflict` and the contested fields stay as given.
pub fn plan(rules: &[Rule], input: &PlanInput<'_>) -> Planning {
    let search = SearchText::new(input.text);
    let (confirmed, unconfirmed): (Vec<&Rule>, Vec<&Rule>) = rules
        .iter()
        .filter(|rule| rule.spec.enabled && matches(&rule.spec, input, &search))
        .partition(|rule| rule.spec.confirmed);
    let ids_of = |matching: &[&Rule]| matching.iter().map(|rule| rule.id).collect::<Vec<_>>();
    let Some(first) = confirmed.first() else {
        return match unconfirmed.first() {
            Some(rule) => Planning::Review {
                reason: ReviewReason::UnconfirmedRule,
                rule: Some(rule.id),
                matching_rules: ids_of(&unconfirmed),
                fields: filled_fields(&rule.spec, input),
            },
            None => Planning::Review {
                reason: ReviewReason::NoMatchingRule,
                rule: None,
                matching_rules: Vec::new(),
                fields: input.fields.clone(),
            },
        };
    };
    let matching_rules = ids_of(&confirmed);
    let planning = plan_with(first, input);
    let agree = confirmed.iter().skip(1).all(|rule| {
        blanked(plan_with(rule, input), first.id) == blanked(planning.clone(), first.id)
    });
    if !agree {
        return Planning::Review {
            reason: ReviewReason::RuleConflict,
            rule: Some(first.id),
            matching_rules,
            fields: input.fields.clone(),
        };
    }
    match planning {
        Planning::Plan(filing) => Planning::Plan(FilingPlan {
            matching_rules,
            ..filing
        }),
        Planning::Review {
            reason,
            rule,
            fields,
            ..
        } => Planning::Review {
            reason,
            rule,
            matching_rules,
            fields,
        },
    }
}

/// The result with every rule id replaced by `canonical` or dropped, to tell whether two rules came to the
/// same conclusion.
fn blanked(planning: Planning, canonical: RuleId) -> Planning {
    match planning {
        Planning::Plan(filing) => Planning::Plan(FilingPlan {
            rule: canonical,
            matching_rules: Vec::new(),
            ..filing
        }),
        Planning::Review { reason, fields, .. } => Planning::Review {
            reason,
            rule: None,
            matching_rules: Vec::new(),
            fields,
        },
    }
}

/// The conditions of the rule hold, all of them or any of them as the rule
/// says. A rule without a condition would match every document, so it matches
/// none. A condition that cannot be evaluated (empty value, invalid regex,
/// regex over the size limit) makes the rule not match, also under `not` and
/// under any-of, where it would otherwise decide the outcome by accident.
fn matches(spec: &RuleSpec, input: &PlanInput<'_>, search: &SearchText) -> bool {
    let conditions = &spec.conditions;
    if conditions.list.is_empty() {
        return false;
    }
    let Some(outcomes) = conditions
        .list
        .iter()
        .map(|condition| condition_outcome(condition, input, search))
        .collect::<Option<Vec<bool>>>()
    else {
        return false;
    };
    match conditions.combinator {
        Combinator::AllOf => outcomes.iter().all(|holds| *holds),
        Combinator::AnyOf => outcomes.iter().any(|holds| *holds),
    }
}

/// `None` when the condition cannot be evaluated.
fn condition_outcome(
    condition: &Condition,
    input: &PlanInput<'_>,
    search: &SearchText,
) -> Option<bool> {
    if condition.value.trim().is_empty() {
        return None;
    }
    let holds = match condition.kind {
        ConditionKind::Substring => contains_condition_holds(&search.lowered, &condition.value),
        ConditionKind::Word => word_holds(search, &condition.value),
        ConditionKind::SenderAddress => sender_address_holds(&search.lowered, &condition.value),
        ConditionKind::Iban => iban_holds(input.text, &condition.value),
        ConditionKind::TaxId => tax_id_holds(input.text, &condition.value),
        ConditionKind::Recipient => recipient_holds(input.text, &condition.value),
        ConditionKind::Folder => folder_holds(input.path?, &condition.value)?,
        ConditionKind::Regex => {
            compile(&condition.value).map(|regex| regex.is_match(input.text))?
        }
        ConditionKind::Sender => input.fields.sender.as_deref().is_some_and(|sender| {
            let aliases = input.sender_aliases;
            aliases.canonical_sender(sender).to_lowercase()
                == aliases.canonical_sender(&condition.value).to_lowercase()
        }),
    };
    Some(holds != condition.negated)
}

/// The known fields, completed from the rule and from the text. Kind, tax
/// context and tags are the rule's; number, date and amount are read from the
/// text where the rule says how.
fn filled_fields(spec: &RuleSpec, input: &PlanInput<'_>) -> DocumentFields {
    let mut fields = input.fields.clone();
    if fields.sender.is_none() {
        fields.sender = spec.conditions.required_sender().map(str::to_owned);
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
        matching_rules: vec![rule.id],
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
            matching_rules: vec![rule.id],
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
    use ncrs_index::{Amount, Currency, RuleConditions, TaxContext};

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
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                "company a ltd",
            )]),
            extract_number: Some(r"Invoice\s+(\d+)".to_owned()),
            extract_date: Some(r"Date:\s*(\d{2}\.\d{2}\.\d{4})".to_owned()),
            extract_amount: Some(r"Total:\s*([\d.,]+\s*€)".to_owned()),
            kind: "invoice".to_owned(),
            tax_context: TaxContext::Company,
            tags: BTreeSet::from(["office".to_owned()]),
            filing_pattern: pattern.to_owned(),
            confirmed: true,
            enabled: true,
        }
    }

    /// Rule ids are only handed out by the index.
    /// The conditions are put in after the index has stored a placeholder, so
    /// a test can plan with a regex the index would refuse.
    fn rules_from(specs: &[RuleSpec]) -> Vec<Rule> {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = ncrs_index::Index::create_plain(&dir.path().join("index.db")).unwrap();
        for spec in specs {
            index
                .create_rule(&RuleSpec {
                    conditions: RuleConditions::all_of(vec![Condition::new(
                        ConditionKind::Word,
                        "placeholder",
                    )]),
                    ..spec.clone()
                })
                .unwrap();
        }
        let mut rules = index.rules().unwrap();
        for (rule, spec) in rules.iter_mut().zip(specs) {
            rule.spec = spec.clone();
        }
        rules
    }

    const TEXT: &str =
        "Company A Ltd\nInvoice 4711\nDate: 01.03.2026\nTotal: 1.234,56 €\nthank you";

    fn plan_for(spec: RuleSpec, text: &str, fields: &DocumentFields) -> Planning {
        plan(
            &rules_from(&[spec]),
            &PlanInput {
                text,
                fields,
                path: None,
                sender_aliases: &SenderAliases::default(),
            },
        )
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
        regex_spec.conditions = RuleConditions::all_of(vec![
            Condition::new(ConditionKind::Regex, r"(?i)company\s+a\s+ltd"),
            Condition::new(ConditionKind::Sender, "COMPANY A"),
        ]);
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
            broken.conditions =
                RuleConditions::all_of(vec![Condition::new(ConditionKind::Regex, source)]);
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

    fn with_conditions(combinator: Combinator, list: Vec<Condition>) -> RuleSpec {
        RuleSpec {
            conditions: RuleConditions { combinator, list },
            ..spec("{number}")
        }
    }

    fn matched(spec: RuleSpec) -> bool {
        let known = DocumentFields {
            sender: Some("Company A".to_owned()),
            ..DocumentFields::default()
        };
        matches!(plan_for(spec, TEXT, &known), Planning::Plan(_))
    }

    fn substring(text: &str) -> Condition {
        Condition::new(ConditionKind::Substring, text)
    }

    #[test]
    fn all_of_needs_every_condition() {
        let both = vec![substring("invoice 4711"), substring("thank you")];
        let one_missing = vec![substring("invoice 4711"), substring("reminder")];

        assert!(matched(with_conditions(Combinator::AllOf, both)));
        assert!(!matched(with_conditions(Combinator::AllOf, one_missing)));
    }

    #[test]
    fn any_of_needs_one_condition() {
        let one = vec![substring("reminder"), substring("invoice 4711")];
        let none = vec![substring("reminder"), substring("credit note")];

        assert!(matched(with_conditions(Combinator::AnyOf, one)));
        assert!(!matched(with_conditions(Combinator::AnyOf, none)));
    }

    #[test]
    fn a_negated_condition_excludes() {
        let absent = vec![
            substring("invoice"),
            Condition::not(ConditionKind::Substring, "reminder"),
        ];
        let present = vec![
            substring("invoice"),
            Condition::not(ConditionKind::Substring, "thank you"),
        ];
        let wrong_sender = vec![
            substring("invoice"),
            Condition::not(ConditionKind::Sender, "company a"),
        ];

        assert!(matched(with_conditions(Combinator::AllOf, absent)));
        assert!(!matched(with_conditions(Combinator::AllOf, present)));
        assert!(!matched(with_conditions(Combinator::AllOf, wrong_sender)));
    }

    #[test]
    fn a_condition_that_cannot_be_evaluated_blocks_the_rule_even_under_not_and_any_of() {
        let unusable = [
            Condition::not(ConditionKind::Regex, "(unclosed"),
            Condition::new(ConditionKind::Regex, "(unclosed"),
            Condition::not(ConditionKind::Substring, ""),
            Condition::new(ConditionKind::Substring, ""),
        ];
        for condition in unusable {
            let any = vec![substring("invoice"), condition.clone()];
            let all = vec![substring("invoice"), condition.clone()];
            assert!(
                !matched(with_conditions(Combinator::AnyOf, any)),
                "{condition:?}"
            );
            assert!(
                !matched(with_conditions(Combinator::AllOf, all)),
                "{condition:?}"
            );
        }
    }

    #[test]
    fn a_sender_condition_follows_a_confirmed_name_alias_only() {
        let rules = rules_from(&[with_conditions(
            Combinator::AllOf,
            vec![Condition::new(ConditionKind::Sender, "Telekom")],
        )]);
        let fields = DocumentFields {
            sender: Some("Telekom Deutschland GmbH".to_owned()),
            ..DocumentFields::default()
        };
        let planned_with = |aliases: &SenderAliases| {
            !matches!(
                plan(
                    &rules,
                    &PlanInput {
                        text: "",
                        fields: &fields,
                        path: None,
                        sender_aliases: aliases,
                    },
                ),
                Planning::Review {
                    reason: ReviewReason::NoMatchingRule,
                    ..
                }
            )
        };
        let confirmed = SenderAliases::from_confirmed([(
            ncrs_index::SenderAliasKind::Name,
            "Telekom Deutschland GmbH",
            "Telekom",
        )]);

        assert!(planned_with(&confirmed));
        assert!(!planned_with(&SenderAliases::default()));
    }

    /// The rule found the document; fields it cannot fill do not matter here.
    fn matched_at(condition: Condition, text: &str, path: Option<&Path>) -> bool {
        let rules = rules_from(&[with_conditions(Combinator::AllOf, vec![condition])]);
        let fields = DocumentFields::default();
        let planning = plan(
            &rules,
            &PlanInput {
                text,
                fields: &fields,
                path,
                sender_aliases: &SenderAliases::default(),
            },
        );
        !matches!(
            planning,
            Planning::Review {
                reason: ReviewReason::NoMatchingRule,
                ..
            }
        )
    }

    #[test]
    fn a_word_condition_does_not_match_inside_another_word() {
        let word = |text| Condition::new(ConditionKind::Word, text);

        assert!(matched_at(word("invoice"), TEXT, None));
        assert!(!matched_at(word("invoic"), TEXT, None));
        assert!(!matched_at(word("bahn"), "Autobahn Bahnhof", None));
        assert!(matched_at(word("company a ltd"), "Company A\n  Ltd", None));
    }

    #[test]
    fn the_new_condition_kinds_are_evaluated_from_the_text() {
        let text = "Nordlicht GmbH\n\nErika Mustermann\n20457 Hamburg\n\n\
            rechnung@billing.nordlicht.de IBAN DE00 0000 0000 0000 0000 00 USt DE 123 456 789";
        for (condition, expected) in [
            (
                Condition::new(ConditionKind::SenderAddress, "nordlicht.de"),
                true,
            ),
            (
                Condition::new(ConditionKind::SenderAddress, "other.de"),
                false,
            ),
            (
                Condition::new(ConditionKind::Iban, "DE00000000000000000000"),
                true,
            ),
            (
                Condition::new(ConditionKind::Iban, "DE00000000000000000001"),
                false,
            ),
            (Condition::new(ConditionKind::TaxId, "DE123456789"), true),
            (
                Condition::new(ConditionKind::Recipient, "Erika Mustermann"),
                true,
            ),
            (Condition::new(ConditionKind::Recipient, "Nordlicht"), false),
        ] {
            assert_eq!(
                matched_at(condition.clone(), text, None),
                expected,
                "{condition:?}"
            );
        }
    }

    #[test]
    fn a_folder_condition_needs_a_path_even_when_negated() {
        let path = Path::new("/archive/Steuer/2025/bill.pdf");
        let folder = Condition::new(ConditionKind::Folder, "steuer/2025");
        let not_folder = Condition::not(ConditionKind::Folder, "steuer/2025");
        let elsewhere = Condition::not(ConditionKind::Folder, "privat");

        assert!(matched_at(folder.clone(), TEXT, Some(path)));
        assert!(!matched_at(not_folder.clone(), TEXT, Some(path)));
        assert!(matched_at(elsewhere.clone(), TEXT, Some(path)));
        assert!(!matched_at(folder, TEXT, None));
        assert!(!matched_at(not_folder, TEXT, None));
        assert!(!matched_at(elsewhere, TEXT, None));
    }

    #[test]
    fn the_sender_is_only_filled_from_a_plain_sender_condition_under_all_of() {
        let unconfirmed_sender = |combinator, condition| {
            let mut spec = with_conditions(combinator, vec![substring("invoice 4711"), condition]);
            spec.confirmed = false;
            spec
        };
        let sender_of = |spec: RuleSpec| match plan_for(spec, TEXT, &DocumentFields::default()) {
            Planning::Review { fields, .. } => fields.sender,
            Planning::Plan(_) => panic!("expected a review"),
        };

        let sender = Condition::new(ConditionKind::Sender, "Company A");
        let negated = Condition::not(ConditionKind::Sender, "Agency B");

        assert_eq!(
            sender_of(unconfirmed_sender(Combinator::AnyOf, sender)),
            None
        );
        assert_eq!(
            sender_of(unconfirmed_sender(Combinator::AllOf, negated)),
            None
        );
    }

    #[test]
    fn a_rule_without_a_condition_matches_nothing() {
        // The index refuses such a rule; the planner does not rely on that.
        let mut rules = rules_from(&[spec("{number}")]);
        rules[0].spec.conditions = RuleConditions::default();
        let fields = DocumentFields::default();

        let planning = plan(
            &rules,
            &PlanInput {
                text: TEXT,
                fields: &fields,
                path: None,
                sender_aliases: &SenderAliases::default(),
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
                path: None,
                sender_aliases: &SenderAliases::default(),
            },
        );

        assert_eq!(planned(planning).rule, rules[1].id);
    }

    fn plan_all(rules: &[Rule]) -> Planning {
        plan(
            rules,
            &PlanInput {
                text: TEXT,
                fields: &DocumentFields::default(),
                path: None,
                sender_aliases: &SenderAliases::default(),
            },
        )
    }

    #[test]
    fn a_single_matching_rule_lists_itself() {
        let rules = rules_from(&[spec("{number}")]);

        let filing = planned(plan_all(&rules));

        assert_eq!(filing.rule, rules[0].id);
        assert_eq!(filing.matching_rules, [rules[0].id]);
    }

    #[test]
    fn two_matching_rules_with_the_same_result_plan_with_both() {
        let rules = rules_from(&[spec("{number}"), spec("{number}")]);

        let filing = planned(plan_all(&rules));

        assert_eq!(filing.rule, rules[0].id);
        assert_eq!(filing.matching_rules, [rules[0].id, rules[1].id]);
        assert_eq!(filing.relative_path, Path::new("4711"));
    }

    #[test]
    fn two_matching_rules_with_different_results_are_a_conflict() {
        let rules = rules_from(&[spec("{number}"), spec("{year}/{number}")]);

        let planning = plan_all(&rules);

        assert_eq!(
            planning,
            Planning::Review {
                reason: ReviewReason::RuleConflict,
                rule: Some(rules[0].id),
                matching_rules: vec![rules[0].id, rules[1].id],
                fields: DocumentFields::default(),
            }
        );
    }

    #[test]
    fn a_plan_and_a_missing_field_are_different_results() {
        let rules = rules_from(&[spec("{number}"), spec("{sender}/{number}")]);

        assert_eq!(reason(&plan_all(&rules)), ReviewReason::RuleConflict);
    }

    #[test]
    fn a_disabled_rule_is_skipped() {
        let mut disabled = spec("{year}/{number}");
        disabled.enabled = false;
        let rules = rules_from(&[spec("{number}"), disabled]);

        let filing = planned(plan_all(&rules));

        assert_eq!(filing.matching_rules, [rules[0].id]);
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
            ..
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
