//! Where a file goes: one pure function over the text, the fields and the
//! rules of the targets of a source. Nothing here reads a disk or the index;
//! what the disk says (place taken, target not mounted) is asked afterwards.

use std::path::{Path, PathBuf};

use ncrs_index::{
    DocumentFields, Field, FieldOrigin, FieldOrigins, Kinds, Rule, RuleId, SenderAliases, TargetId,
};

use super::model::{SortSource, SortTarget};
use crate::conditions::SearchText;
use crate::filing_plan::{conditions_hold, filled_fields, render_strict, PlanInput, Unrenderable};

/// What is known about the text of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextState<'a> {
    /// Not read yet: wait, never read this as "no text".
    NotRead,
    /// Read, and there is none.
    Empty,
    Text(&'a str),
}

/// The words the conditions and the pattern use.
#[derive(Debug, Clone, Copy)]
pub struct Words<'a> {
    pub kinds: &'a Kinds,
    pub sender_aliases: &'a SenderAliases,
}

#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// Below the root of the source, as a folder condition sees it.
    pub relative: &'a Path,
    /// Without the dot, as the file has it.
    pub extension: Option<&'a str>,
    pub text: TextState<'a>,
    pub fields: &'a DocumentFields,
    pub origins: &'a FieldOrigins,
    /// Fields the user set by hand; a rule does not overwrite them.
    pub manual: &'a [Field],
    /// Every place the same content is known at.
    pub known_at: &'a [PathBuf],
}

/// The enabled, confirmed rules of one target, in the order they apply.
#[derive(Debug, Clone, Copy)]
pub struct TargetRules<'a> {
    pub target: &'a SortTarget,
    pub rules: &'a [Rule],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub target: TargetId,
    pub rule: RuleId,
    /// Below the target; the file name has the extension of the file.
    pub relative: PathBuf,
    pub fields: DocumentFields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    File(Place),
    Several(Vec<Place>),
    NoRule,
    NoText,
    NotRead,
    /// The rule fits, but a value its pattern needs is missing, or is one a
    /// model gave and the pattern may not use there.
    MissingValue {
        target: TargetId,
        rule: RuleId,
        field: Field,
    },
    /// The rule fits, but the path it gives is not a name every system keeps
    /// (device name, over 255 bytes, on Windows a dot or space at the end).
    BadName {
        target: TargetId,
        rule: RuleId,
    },
    AlreadyThere {
        target: TargetId,
        path: PathBuf,
    },
}

enum PerTarget {
    NoRule,
    There(TargetId, PathBuf),
    Place(Place),
    Failed(Decision),
}

/// Order: not read, no text, then per target of the source the first rule that
/// fits. A fitting rule with the file already under its target is "already
/// there"; otherwise the pattern is filled completely. Any rule that fits but
/// cannot be filled decides the whole file, so it never goes quietly to the
/// other target.
pub fn decide(
    subject: &Subject<'_>,
    source: &SortSource,
    targets: &[TargetRules<'_>],
    words: &Words<'_>,
) -> Decision {
    let text = match subject.text {
        TextState::NotRead => return Decision::NotRead,
        TextState::Empty => return Decision::NoText,
        TextState::Text(text) => text,
    };
    let search = SearchText::new(text);
    let seen_by_conditions = without_model_values(subject.fields, subject.origins);
    let condition_input = PlanInput {
        text,
        fields: &seen_by_conditions,
        path: Some(subject.relative),
        location: Some(&source.path),
        sender_aliases: words.sender_aliases,
        manual_fields: subject.manual,
        kinds: words.kinds,
    };
    let fill_input = PlanInput {
        fields: subject.fields,
        ..condition_input
    };
    let mut places = Vec::new();
    let mut there = Vec::new();
    for entry in targets
        .iter()
        .filter(|entry| source.targets.contains(&entry.target.id))
    {
        match decide_target(subject, entry, &search, &condition_input, &fill_input) {
            PerTarget::NoRule => {}
            PerTarget::There(target, path) => there.push((target, path)),
            PerTarget::Place(place) => places.push(place),
            PerTarget::Failed(decision) => return decision,
        }
    }
    match places.len() {
        0 => match there.into_iter().next() {
            Some((target, path)) => Decision::AlreadyThere { target, path },
            None => Decision::NoRule,
        },
        1 => places.pop().map_or(Decision::NoRule, Decision::File),
        _ => Decision::Several(places),
    }
}

fn decide_target(
    subject: &Subject<'_>,
    entry: &TargetRules<'_>,
    search: &SearchText,
    condition_input: &PlanInput<'_>,
    fill_input: &PlanInput<'_>,
) -> PerTarget {
    let Some(rule) = entry.rules.iter().find(|rule| {
        rule.spec.enabled
            && rule.spec.confirmed
            && conditions_hold(&rule.spec.conditions, condition_input, search)
    }) else {
        return PerTarget::NoRule;
    };
    let target = &entry.target.id;
    if let Some(path) = subject
        .known_at
        .iter()
        .find(|path| path.starts_with(&entry.target.path))
    {
        return PerTarget::There(target.clone(), path.clone());
    }
    let fields = filled_fields(&rule.spec, fill_input);
    let model_fields = model_fields(subject, &fields);
    let mut usable = fields.clone();
    clear_fields(&mut usable, |field| {
        model_fields.contains(&field) && is_unsure(subject.origins, field)
    });
    match render_strict(
        &rule.spec.filing_pattern,
        &usable,
        &model_fields,
        fill_input.kinds,
        subject.extension,
    ) {
        Ok(relative) => PerTarget::Place(Place {
            target: target.clone(),
            rule: rule.id,
            relative,
            fields,
        }),
        Err(Unrenderable::MissingValue(field)) => PerTarget::Failed(Decision::MissingValue {
            target: target.clone(),
            rule: rule.id,
            field,
        }),
        Err(Unrenderable::BadName) => PerTarget::Failed(Decision::BadName {
            target: target.clone(),
            rule: rule.id,
        }),
    }
}

fn is_model_value(origins: &FieldOrigins, field: Field) -> bool {
    matches!(origins.of(field), FieldOrigin::Ai { .. })
}

/// A model value below this is not even put into a file name.
const SURE_PERCENT: u8 = 80;

fn is_unsure(origins: &FieldOrigins, field: Field) -> bool {
    matches!(origins.of(field), FieldOrigin::Ai { confidence_percent } if confidence_percent < SURE_PERCENT)
}

/// The fields as a condition may see them: what a model read is cleared, so
/// text in a document can never choose the target through a model.
fn without_model_values(fields: &DocumentFields, origins: &FieldOrigins) -> DocumentFields {
    let mut visible = fields.clone();
    clear_fields(&mut visible, |field| is_model_value(origins, field));
    visible
}

fn clear_fields(fields: &mut DocumentFields, cleared: impl Fn(Field) -> bool) {
    for field in Field::ALL.into_iter().filter(|&field| cleared(field)) {
        match field {
            Field::Sender => fields.sender = None,
            Field::Recipient => fields.recipient = None,
            Field::Kind => fields.kind = None,
            Field::Date => fields.date = None,
            Field::Number => fields.number = None,
            Field::Amount => fields.amount = None,
        }
    }
}

/// The fields of `filled` that still hold what the model read; a value the
/// rule set or extracted is the rule's.
fn model_fields(subject: &Subject<'_>, filled: &DocumentFields) -> Vec<Field> {
    let stored = subject.fields;
    Field::ALL
        .into_iter()
        .filter(|&field| is_model_value(subject.origins, field))
        .filter(|&field| match field {
            Field::Sender => filled.sender == stored.sender,
            Field::Recipient => filled.recipient == stored.recipient,
            Field::Kind => filled.kind == stored.kind,
            Field::Date => filled.date == stored.date,
            Field::Number => filled.number == stored.number,
            Field::Amount => filled.amount == stored.amount,
        })
        .collect()
}

#[cfg(test)]
mod tests;
