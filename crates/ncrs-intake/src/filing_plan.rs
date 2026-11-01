//! Decides, from the rules and the text of a document, where the document
//! would be filed. Pure: nothing here touches a file or the index, and the
//! text is only ever searched, never acted on.
//!
//! A filing pattern is a relative path with these placeholders:
//! `{year}`, `{month}`, `{day}`, `{date}` (`YYYY-MM-DD`), `{sender}`,
//! `{recipient}`, `{number}` and `{kind}`. Patterns learned from an archive
//! add `{folder}` (the sender) and `{name}` (the kind).
//!
//! A pattern that ends in `/` names folders only; the file name is then made
//! from the date, the sender and the number, as far as they are known.
//!
//! An explicit file name, the last part of a pattern that does not end in `/`,
//! is read as pieces joined by `_`: a piece whose value is unknown is left out
//! with its separator, and the name fails only when no piece has a value.

use std::path::{Path, PathBuf};

use chrono::Datelike as _;
use ncrs_index::{
    Combinator, Condition, ConditionKind, DocumentFields, Field, Kinds, ReviewReason, Rule,
    RuleConditions, RuleId, RuleSpec, SenderAliases,
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
    /// Fields the user set by hand: a rule does not overwrite them, they keep
    /// the value of `fields`.
    pub manual_fields: &'a [Field],
    /// The language a `{kind}` folder is written in.
    pub kinds: &'a Kinds,
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

/// The blocks a pattern editor offers; `folder` and `name` stay valid but are
/// not offered.
pub const FILING_PLACEHOLDERS: [&str; 8] = [
    "year",
    "month",
    "day",
    "date",
    "sender",
    "recipient",
    "number",
    "kind",
];

/// The name of the first `{name}` of `source` that is not a block a pattern
/// knows; an unclosed brace is no block.
pub fn unknown_placeholder(source: &str) -> Option<String> {
    let mut rest = source;
    while let Some((_, from)) = rest.split_once('{') {
        let (name, after) = from.split_once('}')?;
        if Placeholder::parse(name).is_none() {
            return Some(name.to_owned());
        }
        rest = after;
    }
    None
}

/// Plans with the first enabled confirmed rule that matches, in the order of
/// `rules`. Rules after it that match too change nothing; they are only
/// listed in `matching_rules`.
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
    match plan_with(first, input) {
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

/// The conditions of the rule hold, all of them or any of them as the rule
/// says. A rule without a condition would match every document, so it matches
/// none. A condition that cannot be evaluated (empty value, invalid regex,
/// regex over the size limit) makes the rule not match, also under `not` and
/// under any-of, where it would otherwise decide the outcome by accident.
fn matches(spec: &RuleSpec, input: &PlanInput<'_>, search: &SearchText) -> bool {
    conditions_hold(&spec.conditions, input, search)
}

fn conditions_hold(
    conditions: &RuleConditions,
    input: &PlanInput<'_>,
    search: &SearchText,
) -> bool {
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
        ConditionKind::Tag => input
            .fields
            .all_tags(&input.kinds.catalog)
            .iter()
            .any(|tag| input.kinds.catalog.tag_matches(tag, &condition.value)),
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

/// The known fields, completed from the rule and from the text. Kind and tags
/// are the rule's; number, date and amount are read from the text where the
/// rule says how.
fn filled_fields(spec: &RuleSpec, input: &PlanInput<'_>) -> DocumentFields {
    let mut fields = input.fields.clone();
    if fields.sender.is_none() {
        fields.sender = spec.conditions.required_sender().map(str::to_owned);
    }
    if let Some(kind) = spec.kind.as_ref().filter(|kind| !kind.is_empty()) {
        fields.kind = Some(input.kinds.catalog.stored_kind(kind));
    }
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
    for field in input.manual_fields {
        restore_manual(&mut fields, input.fields, *field);
    }
    fields
}

fn restore_manual(fields: &mut DocumentFields, kept: &DocumentFields, field: Field) {
    match field {
        Field::Sender => fields.sender.clone_from(&kept.sender),
        Field::Recipient => fields.recipient.clone_from(&kept.recipient),
        Field::Kind => fields.kind.clone_from(&kept.kind),
        Field::Date => fields.date = kept.date,
        Field::Number => fields.number.clone_from(&kept.number),
        Field::Amount => fields.amount.clone_from(&kept.amount),
    }
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
    match pattern.render(&fields, Some(input.kinds)) {
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
        .render(fields, None)
        .filter(|path| stays_below(path))
}

/// [`render_pattern`], with the kind written as the word of the language of `kinds`.
pub fn render_pattern_in(source: &str, fields: &DocumentFields, kinds: &Kinds) -> Option<PathBuf> {
    Pattern::parse(source)?
        .render(fields, Some(kinds))
        .filter(|path| stays_below(path))
}

/// A piece of a rendered filing path: the folder separators and the literal
/// text of the pattern have no field, a value filled in has the field it is
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternPart {
    pub text: String,
    pub field: Option<Field>,
}

/// [`render_pattern`], in the pieces the path is made of; their texts joined
/// are the path.
pub fn render_pattern_parts(
    source: &str,
    fields: &DocumentFields,
    kinds: &Kinds,
) -> Option<Vec<PatternPart>> {
    let components = Pattern::parse(source)?.render_components(fields, Some(kinds))?;
    if !stays_below(&path_of(&components)) {
        return None;
    }
    let mut parts = Vec::new();
    for (position, component) in components.into_iter().enumerate() {
        if position > 0 {
            parts.push(PatternPart {
                text: std::path::MAIN_SEPARATOR_STR.to_owned(),
                field: None,
            });
        }
        parts.extend(component);
    }
    Some(parts)
}

fn path_of(components: &[Vec<PatternPart>]) -> PathBuf {
    components
        .iter()
        .map(|component| {
            component
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>()
        })
        .collect()
}

/// The order fields are asked for and reported in.
pub const FIELD_ORDER: [Field; 5] = [
    Field::Sender,
    Field::Kind,
    Field::Date,
    Field::Number,
    Field::Amount,
];

/// The order fields are asked of an AI: the recipient is asked too, though no
/// pattern uses it.
pub const AI_FIELD_ORDER: [Field; 6] = Field::ALL;

/// A field is needed when the pattern uses it or the rule says how to read it
/// from the text; a value that is empty after sanitising is missing.
fn first_missing(fields: &DocumentFields, spec: &RuleSpec, pattern: &Pattern) -> Option<Field> {
    let needed = |field: Field| {
        pattern.uses_in_folders(field)
            || match field {
                Field::Date => spec.extract_date.is_some(),
                Field::Number => spec.extract_number.is_some(),
                Field::Amount => spec.extract_amount.is_some(),
                Field::Sender | Field::Recipient | Field::Kind => false,
            }
    };
    AI_FIELD_ORDER
        .into_iter()
        .find(|&field| needed(field) && !field_is_present(fields, field))
        .or_else(|| pattern.file_name_missing(fields))
}

fn field_is_present(fields: &DocumentFields, field: Field) -> bool {
    match field {
        Field::Sender => fields.sender.as_deref().and_then(sanitise_value).is_some(),
        Field::Recipient => fields
            .recipient
            .as_deref()
            .and_then(sanitise_value)
            .is_some(),
        Field::Kind => fields.kind.as_deref().and_then(sanitise_value).is_some(),
        Field::Date => fields.date.is_some(),
        Field::Number => fields.number.as_deref().and_then(sanitise_value).is_some(),
        Field::Amount => fields.amount.is_some(),
    }
}

/// The first field the pattern uses that is empty in `fields`; `None` when
/// none is, or the pattern does not read.
pub fn missing_field(source: &str, fields: &DocumentFields) -> Option<Field> {
    let pattern = Pattern::parse(source)?;
    AI_FIELD_ORDER
        .into_iter()
        .find(|&field| pattern.uses_in_folders(field) && !field_is_present(fields, field))
        .or_else(|| pattern.file_name_missing(fields))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    Year,
    Month,
    Day,
    Date,
    Sender,
    Recipient,
    Number,
    Kind,
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
            "recipient" => Self::Recipient,
            "number" => Self::Number,
            "kind" => Self::Kind,
            "folder" => Self::Folder,
            "name" => Self::Name,
            _ => return None,
        })
    }

    fn field(self) -> Field {
        match self {
            Self::Year | Self::Month | Self::Day | Self::Date => Field::Date,
            Self::Sender | Self::Folder => Field::Sender,
            Self::Recipient => Field::Recipient,
            Self::Number => Field::Number,
            Self::Kind | Self::Name => Field::Kind,
        }
    }

    /// Text values pass through `sanitise_value` here, whatever their origin.
    fn value(self, fields: &DocumentFields, kinds: Option<&Kinds>) -> Option<String> {
        match self {
            Self::Year => fields.date.map(|date| format!("{:04}", date.year())),
            Self::Month => fields.date.map(|date| format!("{:02}", date.month())),
            Self::Day => fields.date.map(|date| format!("{:02}", date.day())),
            Self::Date => fields.date.map(|date| date.to_string()),
            Self::Sender | Self::Folder => fields.sender.as_deref().and_then(sanitise_value),
            Self::Recipient => fields.recipient.as_deref().and_then(sanitise_value),
            Self::Number => fields.number.as_deref().and_then(sanitise_value),
            Self::Kind | Self::Name => {
                let kind = fields.kind.as_deref()?;
                match kinds {
                    Some(language_kinds) => sanitise_value(&language_kinds.kind_word(kind)),
                    None => sanitise_value(kind),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Placeholder(Placeholder),
}

/// The file names a pattern that ends in `/` takes, the first one all of whose
/// values are known.
const AUTOMATIC_FILE_NAMES: [&str; 4] = [
    "{date}_{sender}_{number}",
    "{date}_{sender}",
    "{date}_{number}",
    "{date}",
];

/// A parsed filing pattern: folders and a file name, each a list of tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    components: Vec<Vec<Token>>,
    /// The pattern ends in `/`: its components are all folders and the file
    /// name is made from the fields.
    automatic_file_name: bool,
}

impl Pattern {
    /// `None` for a pattern that cannot be rendered on its own: an unknown
    /// placeholder, a stray brace, an empty component (which includes an
    /// absolute path), a component of only dots, or a literal with a control
    /// or Windows-reserved character (which includes a drive prefix).
    /// Both `/` and `\` separate components.
    fn parse(source: &str) -> Option<Self> {
        let unified = source.replace('\\', "/");
        let (folders, automatic_file_name) = match unified.strip_suffix('/') {
            Some(folders) => (folders, true),
            None => (unified.as_str(), false),
        };
        let components = folders
            .split('/')
            .map(Self::parse_component)
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            components,
            automatic_file_name,
        })
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

    /// The components that are folders: all of them when the file name is made
    /// automatically, otherwise all but the last.
    fn folders(&self) -> &[Vec<Token>] {
        if self.automatic_file_name {
            &self.components
        } else {
            self.components
                .split_last()
                .map_or(&[], |(_, folders)| folders)
        }
    }

    /// The explicit file name: the last component of a pattern that does not
    /// end in `/`.
    fn explicit_file_name(&self) -> Option<&[Token]> {
        if self.automatic_file_name {
            return None;
        }
        self.components.last().map(Vec::as_slice)
    }

    /// Whether a folder, or the automatic file name, needs the field.
    fn uses_in_folders(&self, field: Field) -> bool {
        (self.automatic_file_name && field == Field::Date)
            || self.folders().iter().flatten().any(|token| match token {
                Token::Placeholder(placeholder) => placeholder.field() == field,
                Token::Literal(_) => false,
            })
    }

    /// The field of the first piece of an explicit file name, when it has
    /// pieces to fill in and none of them has a value.
    fn file_name_missing(&self, fields: &DocumentFields) -> Option<Field> {
        let placeholders: Vec<Placeholder> = self
            .explicit_file_name()?
            .iter()
            .filter_map(|token| match token {
                Token::Placeholder(placeholder) => Some(*placeholder),
                Token::Literal(_) => None,
            })
            .collect();
        let none_known = placeholders
            .iter()
            .all(|placeholder| placeholder.value(fields, None).is_none());
        placeholders
            .first()
            .filter(|_| none_known)
            .map(|placeholder| placeholder.field())
    }

    /// `None` when a value is missing.
    fn render(&self, fields: &DocumentFields, kinds: Option<&Kinds>) -> Option<PathBuf> {
        self.render_components(fields, kinds)
            .map(|components| path_of(&components))
    }

    fn render_components(
        &self,
        fields: &DocumentFields,
        kinds: Option<&Kinds>,
    ) -> Option<Vec<Vec<PatternPart>>> {
        let mut rendered = self
            .folders()
            .iter()
            .map(|component| render_tokens(component, fields, false, kinds))
            .collect::<Option<Vec<_>>>()?;
        match self.explicit_file_name() {
            Some(tokens) => rendered.push(render_file_name(tokens, fields, kinds)?),
            None => rendered.push(automatic_file_name(fields)?),
        }
        Some(rendered)
    }
}

/// The values of the tokens; `dashed` writes the spaces in a value as `-`, as
/// a file name made automatically does.
fn render_tokens(
    tokens: &[Token],
    fields: &DocumentFields,
    dashed: bool,
    kinds: Option<&Kinds>,
) -> Option<Vec<PatternPart>> {
    tokens
        .iter()
        .map(|token| match token {
            Token::Literal(literal) => Some(PatternPart {
                text: literal.clone(),
                field: None,
            }),
            Token::Placeholder(placeholder) => {
                let value = placeholder.value(fields, kinds)?;
                Some(PatternPart {
                    text: if dashed {
                        value.replace(' ', "-")
                    } else {
                        value
                    },
                    field: Some(placeholder.field()),
                })
            }
        })
        .collect()
}

/// The tokens of an explicit file name cut at the `_` of its literals.
fn file_name_pieces(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut pieces = Vec::new();
    let mut current = Vec::new();
    for token in tokens {
        match token {
            Token::Literal(literal) => {
                let mut parts = literal.split('_');
                if let Some(first) = parts.next().filter(|first| !first.is_empty()) {
                    current.push(Token::Literal(first.to_owned()));
                }
                for part in parts {
                    pieces.push(std::mem::take(&mut current));
                    if !part.is_empty() {
                        current.push(Token::Literal(part.to_owned()));
                    }
                }
            }
            Token::Placeholder(_) => current.push(token.clone()),
        }
    }
    pieces.push(current);
    pieces
}

/// An explicit file name: a piece with an unknown value is left out together
/// with its `_`. `None` when it has pieces to fill in and none has a value.
fn render_file_name(
    tokens: &[Token],
    fields: &DocumentFields,
    kinds: Option<&Kinds>,
) -> Option<Vec<PatternPart>> {
    let has_placeholder = |piece: &[Token]| {
        piece
            .iter()
            .any(|token| matches!(token, Token::Placeholder(_)))
    };
    let mut kept: Vec<Vec<PatternPart>> = Vec::new();
    let mut any_value = false;
    let mut any_placeholder = false;
    for piece in file_name_pieces(tokens) {
        let is_filled_in = has_placeholder(&piece);
        any_placeholder |= is_filled_in;
        if let Some(rendered) = render_tokens(&piece, fields, false, kinds) {
            any_value |= is_filled_in;
            kept.push(rendered);
        }
    }
    if any_placeholder && !any_value {
        return None;
    }
    let mut parts = Vec::new();
    for (position, piece) in kept.into_iter().enumerate() {
        if position > 0 {
            parts.push(PatternPart {
                text: "_".to_owned(),
                field: None,
            });
        }
        parts.extend(piece);
    }
    Some(parts)
}

fn automatic_file_name(fields: &DocumentFields) -> Option<Vec<PatternPart>> {
    AUTOMATIC_FILE_NAMES.iter().find_map(|source| {
        let tokens = Pattern::parse_component(source)?;
        render_tokens(&tokens, fields, true, None)
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use ncrs_index::KindLanguage;
    #[test]
    fn every_offered_block_is_a_placeholder() {
        for name in FILING_PLACEHOLDERS {
            assert!(Placeholder::parse(name).is_some(), "{name}");
        }
    }

    #[test]
    fn the_first_unknown_block_is_named() {
        assert_eq!(
            unknown_placeholder("{year}/{foo}/{bar}").as_deref(),
            Some("foo")
        );
        assert_eq!(unknown_placeholder("{year}/{sender}"), None);
        assert_eq!(unknown_placeholder("{year"), None);
    }

    use std::collections::BTreeSet;
    use std::path::Path;

    use chrono::NaiveDate;
    use ncrs_index::{Amount, Currency, RuleConditions};

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
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                "company a ltd",
            )]),
            extract_number: Some(r"Invoice\s+(\d+)".to_owned()),
            extract_date: Some(r"Date:\s*(\d{2}\.\d{2}\.\d{4})".to_owned()),
            extract_amount: Some(r"Total:\s*([\d.,]+\s*€)".to_owned()),
            kind: Some("invoice".to_owned()),
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
                manual_fields: &[],
                kinds: &Kinds::built_in(KindLanguage::English),
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

    fn plan_keeping(fields: &DocumentFields, manual_fields: &[Field]) -> FilingPlan {
        planned(plan(
            &rules_from(&[spec("{kind}/{date}")]),
            &PlanInput {
                text: TEXT,
                fields,
                path: None,
                sender_aliases: &SenderAliases::default(),
                manual_fields,
                kinds: &Kinds::built_in(KindLanguage::English),
            },
        ))
    }

    fn kind_tag_rule(tag: &str, negated: bool) -> RuleSpec {
        let condition = if negated {
            Condition::not(ConditionKind::Tag, tag)
        } else {
            Condition::new(ConditionKind::Tag, tag)
        };
        RuleSpec {
            conditions: RuleConditions::all_of(vec![condition]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            ..spec("{kind}/{sender}")
        }
    }

    fn fields_of_kind(kind: &str) -> DocumentFields {
        DocumentFields {
            sender: Some("Company A".to_owned()),
            kind: Some(kind.to_owned()),
            ..DocumentFields::default()
        }
    }

    #[test]
    fn a_kind_the_ai_wrote_in_english_is_filed_by_a_german_tag_rule() {
        let planning = plan_for(
            kind_tag_rule("rechnung", false),
            "",
            &fields_of_kind("invoice"),
        );

        assert_eq!(
            planned(planning).relative_path,
            Path::new("Invoice").join("Company A")
        );
    }

    #[test]
    fn a_kind_from_a_german_heading_is_filed_by_an_english_tag_rule() {
        let planning = plan_for(
            kind_tag_rule("invoice", false),
            "",
            &fields_of_kind("Rechnung"),
        );

        assert!(matches!(planning, Planning::Plan(_)));
    }

    #[test]
    fn a_rule_that_excludes_a_kind_excludes_its_other_spelling() {
        let planning = plan_for(
            kind_tag_rule("rechnung", true),
            "",
            &fields_of_kind("invoice"),
        );

        assert_eq!(reason(&planning), ReviewReason::NoMatchingRule);
    }

    #[test]
    fn a_tag_rule_on_one_notice_does_not_match_every_notice() {
        let planning = plan_for(
            kind_tag_rule("steuerbescheid", false),
            "",
            &fields_of_kind("Rentenbescheid"),
        );

        assert_eq!(reason(&planning), ReviewReason::NoMatchingRule);
    }

    #[test]
    fn the_kind_folder_is_written_in_the_language_of_the_plan() {
        let rule = rules_from(&[kind_tag_rule("invoice", false)]);
        let fields = fields_of_kind("invoice");
        let planned_in = |language| {
            let kinds = Kinds::built_in(language);
            planned(plan(
                &rule,
                &PlanInput {
                    text: "",
                    fields: &fields,
                    path: None,
                    sender_aliases: &SenderAliases::default(),
                    manual_fields: &[],
                    kinds: &kinds,
                },
            ))
            .relative_path
        };

        assert_eq!(
            planned_in(KindLanguage::German),
            Path::new("Rechnung").join("Company A")
        );
        assert_eq!(
            planned_in(KindLanguage::English),
            Path::new("Invoice").join("Company A")
        );
    }

    #[test]
    fn a_manual_kind_and_date_survive_a_matching_rule() {
        let by_hand = DocumentFields {
            kind: Some("offer".to_owned()),
            date: NaiveDate::from_ymd_opt(2020, 5, 6),
            ..DocumentFields::default()
        };

        let kept = plan_keeping(&by_hand, &[Field::Kind, Field::Date]);
        assert_eq!(kept.relative_path, Path::new("offer").join("2020-05-06"));
        assert_eq!(kept.fields.kind.as_deref(), Some("offer"));

        let overwritten = plan_keeping(&by_hand, &[]);
        assert_eq!(
            overwritten.relative_path,
            Path::new("Invoice").join("2026-03-01")
        );
    }

    #[test]
    fn only_the_listed_fields_are_kept() {
        let by_hand = DocumentFields {
            kind: Some("offer".to_owned()),
            date: NaiveDate::from_ymd_opt(2020, 5, 6),
            ..DocumentFields::default()
        };

        let kept = plan_keeping(&by_hand, &[Field::Kind]);

        assert_eq!(kept.relative_path, Path::new("offer").join("2026-03-01"));
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
                        manual_fields: &[],
                        kinds: &Kinds::built_in(KindLanguage::English),
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
                manual_fields: &[],
                kinds: &Kinds::built_in(KindLanguage::English),
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
                manual_fields: &[],
                kinds: &Kinds::built_in(KindLanguage::English),
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
                manual_fields: &[],
                kinds: &Kinds::built_in(KindLanguage::English),
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
                manual_fields: &[],
                kinds: &Kinds::built_in(KindLanguage::English),
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
    fn the_first_matching_rule_files_even_if_a_later_one_gives_another_result() {
        let rules = rules_from(&[spec("{number}"), spec("{year}/{number}")]);

        let filing = planned(plan_all(&rules));

        assert_eq!(filing.rule, rules[0].id);
        assert_eq!(filing.relative_path, Path::new("4711"));
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

    fn planned_path(pattern: &str, fields: &DocumentFields) -> Option<PathBuf> {
        let mut plain = spec(pattern);
        plain.extract_number = None;
        plain.extract_date = None;
        plain.extract_amount = None;
        match plan_for(plain, TEXT, fields) {
            Planning::Plan(filing) => Some(filing.relative_path),
            Planning::Review { .. } => None,
        }
    }

    fn known(sender: Option<&str>, number: Option<&str>, date: bool) -> DocumentFields {
        DocumentFields {
            sender: sender.map(str::to_owned),
            number: number.map(str::to_owned),
            date: date.then(|| NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()),
            ..DocumentFields::default()
        }
    }

    #[test]
    fn an_explicit_file_name_leaves_out_the_pieces_it_does_not_know() {
        let pattern = "{year}/{date}_{sender}_{number}";

        assert_eq!(
            planned_path(pattern, &known(Some("Telekom"), Some("42"), true)),
            Some(PathBuf::from("2026/2026-03-09_Telekom_42"))
        );
        assert_eq!(
            planned_path(pattern, &known(Some("Telekom"), None, true)),
            Some(PathBuf::from("2026/2026-03-09_Telekom"))
        );
        assert_eq!(
            planned_path("{sender}_{number}", &known(None, Some("42"), true)),
            Some(PathBuf::from("42"))
        );
        assert_eq!(
            planned_path("{date}_Miete_{number}", &known(None, None, true)),
            Some(PathBuf::from("2026-03-09_Miete"))
        );
    }

    #[test]
    fn an_explicit_file_name_with_no_known_piece_is_a_missing_field() {
        let mut plain = spec("{sender}_{number}");
        plain.extract_number = None;
        let planning = plan_for(plain, TEXT, &DocumentFields::default());

        assert_eq!(reason(&planning), ReviewReason::MissingField(Field::Sender));
        assert_eq!(
            missing_field("{year}/{sender}_{number}", &known(None, None, true)),
            Some(Field::Sender)
        );
    }

    #[test]
    fn a_folder_still_needs_its_field_and_a_slash_pattern_its_date() {
        assert_eq!(
            planned_path("{sender}/{number}", &known(None, Some("42"), true)),
            None
        );
        assert_eq!(planned_path("{year}/", &known(None, None, false)), None);
        assert_eq!(
            planned_path("{year}/", &known(None, None, true)),
            Some(PathBuf::from("2026/2026-03-09"))
        );
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
                recipient: Some(value.to_owned()),
                number: Some(value.to_owned()),
                ..DocumentFields::default()
            };
            let mut hostile_rule = spec("{recipient}/{sender}/{kind}/{number}.pdf");
            hostile_rule.kind = Some(value.to_owned());
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

    #[test]
    fn the_parts_of_a_path_mark_the_fields_it_is_made_of() {
        let fields = DocumentFields {
            sender: Some("Company A".to_owned()),
            kind: Some("invoice".to_owned()),
            date: NaiveDate::from_ymd_opt(2024, 3, 5),
            ..DocumentFields::default()
        };
        let parts = render_pattern_parts(
            "{sender}/{date}_{kind}",
            &fields,
            &Kinds::built_in(KindLanguage::English),
        )
        .unwrap();
        let separator = std::path::MAIN_SEPARATOR_STR;
        let shown: Vec<(&str, Option<Field>)> = parts
            .iter()
            .map(|part| (part.text.as_str(), part.field))
            .collect();
        assert_eq!(
            shown,
            [
                ("Company A", Some(Field::Sender)),
                (separator, None),
                ("2024-03-05", Some(Field::Date)),
                ("_", None),
                ("Invoice", Some(Field::Kind)),
            ]
        );
        let joined: String = parts.iter().map(|part| part.text.as_str()).collect();
        assert_eq!(
            Some(PathBuf::from(joined)),
            render_pattern_in(
                "{sender}/{date}_{kind}",
                &fields,
                &Kinds::built_in(KindLanguage::English)
            )
        );
    }

    #[test]
    fn parts_are_refused_where_the_path_is() {
        assert_eq!(
            render_pattern_parts(
                "{sender}",
                &DocumentFields::default(),
                &Kinds::built_in(KindLanguage::English)
            ),
            None
        );
        assert_eq!(
            render_pattern_parts(
                "{nonsense}",
                &DocumentFields::default(),
                &Kinds::built_in(KindLanguage::English)
            ),
            None
        );
    }
}
