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
        ConditionKind::Kind => input.fields.kind.as_deref().is_some_and(|kind| {
            input.kinds.catalog.kind_key(kind) == input.kinds.catalog.kind_key(&condition.value)
        }),
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

/// [`render_pattern_in`] with `extension` appended to the file name, and only
/// when every placeholder of the pattern has a value: no partial names.
pub fn render_complete_pattern(
    source: &str,
    fields: &DocumentFields,
    kinds: &Kinds,
    extension: &str,
) -> Option<PathBuf> {
    let pattern = Pattern::parse(source)?;
    let every_value_known = pattern
        .components
        .iter()
        .flatten()
        .all(|token| match token {
            Token::Placeholder(placeholder) => placeholder.value(fields, Some(kinds)).is_some(),
            Token::Literal(_) => true,
        });
    if !every_value_known {
        return None;
    }
    let rendered = pattern
        .render(fields, Some(kinds))
        .filter(|path| stays_below(path))?;
    let name = rendered.file_name()?.to_str()?;
    Some(rendered.with_file_name(format!("{name}.{extension}")))
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
mod tests;
