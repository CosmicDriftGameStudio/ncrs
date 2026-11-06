//! The answer of the model becomes a rule or a pattern. The grammar already
//! keeps the shape; this checks it again and leaves whether the rule is good
//! to `rule_problem` and `plan`.

use jsonc_parser::{parse_to_value, JsonObject, JsonValue, ParseOptions};
use ncrs_index::{Condition, RuleConditions, RuleSpec};

use super::levels::{placeholder_named, separator_named, Level, Levels, Part};
use super::vocabulary::{combinator_from_word, condition_from_word};
use crate::error::AiError;

fn object_of(raw: &str) -> Result<JsonObject<'_>, AiError> {
    let parsed = parse_to_value(raw, &ParseOptions::default())
        .map_err(|_syntax| AiError::BadReply("not valid JSON"))?;
    match parsed {
        Some(JsonValue::Object(object)) => Ok(object),
        _ => Err(AiError::BadReply("not a JSON object")),
    }
}

fn condition_of(value: &JsonValue<'_>) -> Result<Condition, AiError> {
    let JsonValue::Object(object) = value else {
        return Err(AiError::BadReply("a condition is not an object"));
    };
    let kind = object
        .get_string("kind")
        .and_then(|word| condition_from_word(word))
        .ok_or(AiError::BadReply("unknown condition kind"))?;
    let text = object
        .get_string("value")
        .ok_or(AiError::BadReply("a condition has no value"))?;
    Ok(Condition {
        kind,
        value: text.to_string(),
        negated: object.get_boolean("not").unwrap_or(false),
    })
}

fn part_of(value: &JsonValue<'_>) -> Result<Part, AiError> {
    match value {
        JsonValue::String(name) => placeholder_named(name)
            .map(Part::Placeholder)
            .ok_or(AiError::BadReply("unknown placeholder")),
        // The model often writes a placeholder as fixed text ({"fest":"date"});
        // no file name holds the bare English placeholder word on purpose.
        JsonValue::Object(object) => object
            .get_string("fest")
            .map(|text| {
                let bare = text.trim_start_matches('{').trim_end_matches('}');
                placeholder_named(bare)
                    .map_or_else(|| Part::Fixed(text.to_string()), Part::Placeholder)
            })
            .ok_or(AiError::BadReply(
                "a part is neither fixed nor a placeholder",
            )),
        _ => Err(AiError::BadReply(
            "a part is neither fixed nor a placeholder",
        )),
    }
}

fn level_of(value: &JsonValue<'_>) -> Result<Level, AiError> {
    let JsonValue::Object(object) = value else {
        return Err(AiError::BadReply("a level is not an object"));
    };
    let parts = object
        .get_array("teile")
        .ok_or(AiError::BadReply("a level has no parts"))?
        .iter()
        .map(part_of)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err(AiError::BadReply("a level has no parts"));
    }
    let separator = object
        .get_string("trenner")
        .and_then(|text| separator_named(text))
        .ok_or(AiError::BadReply("unknown separator"))?;
    Ok(Level { parts, separator })
}

fn levels_of(object: &JsonObject<'_>) -> Result<Levels, AiError> {
    let folders = object
        .get_array("ordner")
        .ok_or(AiError::BadReply("no folders"))?
        .iter()
        .map(level_of)
        .collect::<Result<Vec<_>, _>>()?;
    let file = match object.get("datei") {
        Some(JsonValue::Null) => None,
        Some(value) => Some(level_of(value)?),
        None => return Err(AiError::BadReply("no file name")),
    };
    Ok(Levels { folders, file })
}

pub fn parse_levels(raw: &str) -> Result<Levels, AiError> {
    levels_of(&object_of(raw)?)
}

#[cfg(test)]
pub fn parse_pattern(raw: &str) -> Result<String, AiError> {
    parse_levels(raw).map(|levels| levels.pattern())
}

/// The rule is confirmed and enabled: it is what the user asked for, and the
/// planner skips anything else.
pub fn parse_generate(raw: &str) -> Result<RuleSpec, AiError> {
    let object = object_of(raw)?;
    let combinator = object
        .get_string("match")
        .and_then(|word| combinator_from_word(word))
        .ok_or(AiError::BadReply("no match"))?;
    let list = object
        .get_array("conditions")
        .ok_or(AiError::BadReply("no conditions"))?
        .iter()
        .map(condition_of)
        .collect::<Result<Vec<_>, _>>()?;
    let filing_pattern = levels_of(&object)?.pattern();
    Ok(RuleSpec {
        name: None,
        conditions: RuleConditions { combinator, list },
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: std::collections::BTreeSet::new(),
        filing_pattern,
        confirmed: true,
        enabled: true,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use ncrs_index::{Combinator, ConditionKind};

    use super::*;

    #[test]
    fn a_rule_reads_back() {
        let rule = parse_generate(
            r#"{"match":"any","conditions":[{"kind":"tax_id","value":"12/345","not":true},{"kind":"folder","value":"Scans","not":false}],"ordner":[{"teile":["year"],"trenner":""},{"teile":["sender"],"trenner":""}],"datei":null}"#,
        )
        .unwrap();
        assert_eq!(rule.conditions.combinator, Combinator::AnyOf);
        assert_eq!(
            rule.conditions.list,
            vec![
                Condition::not(ConditionKind::TaxId, "12/345"),
                Condition::new(ConditionKind::Folder, "Scans"),
            ]
        );
        assert_eq!(rule.filing_pattern, "{year}/{sender}/");
        assert!(rule.confirmed && rule.enabled);
    }

    #[test]
    fn a_pattern_reads_back() {
        assert_eq!(
            parse_pattern(
                r#"{"ordner":[{"teile":[{"fest":"Post"}],"trenner":""}],"datei":{"teile":["date","sender"],"trenner":"_"}}"#
            )
            .unwrap(),
            "Post/{date}_{sender}"
        );
        assert_eq!(
            parse_pattern(
                r#"{"ordner":[],"datei":{"teile":[{"fest":"Beleg"},{"fest":"date"},{"fest":"{sender}"}],"trenner":"_"}}"#
            )
            .unwrap(),
            "Beleg_{date}_{sender}"
        );
        let levels = parse_levels(r#"{"ordner":[],"datei":null}"#).unwrap();
        assert_eq!(levels.count(), 0);
    }

    #[test]
    fn nonsense_is_refused() {
        for raw in [
            "",
            "[]",
            r#"{"match":"all","conditions":[],"ordner":1,"datei":null}"#,
            r#"{"match":"some","conditions":[],"ordner":[],"datei":null}"#,
            r#"{"match":"all","conditions":[{"kind":"nope","value":"x","not":false}],"ordner":[],"datei":null}"#,
            r#"{"match":"all","conditions":[{"kind":"word","not":false}],"ordner":[],"datei":null}"#,
            r#"{"match":"all","conditions":[],"ordner":[{"teile":["nope"],"trenner":""}],"datei":null}"#,
            r#"{"match":"all","conditions":[],"ordner":[{"teile":["year"],"trenner":"x"}],"datei":null}"#,
            r#"{"match":"all","conditions":[],"ordner":[]}"#,
        ] {
            assert!(parse_generate(raw).is_err(), "{raw}");
        }
        parse_pattern(r#"{"other":"x"}"#).unwrap_err();
    }
}
