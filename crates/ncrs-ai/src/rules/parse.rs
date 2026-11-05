//! The answer of the model becomes a rule or a pattern. The grammar already
//! keeps the shape; this checks it again and leaves whether the rule is good
//! to `rule_problem` and `plan`.

use jsonc_parser::{parse_to_value, JsonObject, JsonValue, ParseOptions};
use ncrs_index::{Condition, RuleConditions, RuleSpec};

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

pub fn parse_pattern(raw: &str) -> Result<String, AiError> {
    object_of(raw)?
        .get_string("pattern")
        .map(ToString::to_string)
        .ok_or(AiError::BadReply("no pattern"))
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
    let filing_pattern = object
        .get_string("pattern")
        .ok_or(AiError::BadReply("no pattern"))?
        .to_string();
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
            r#"{"match":"any","conditions":[{"kind":"tax_id","value":"12/345","not":true},{"kind":"folder","value":"Scans","not":false}],"pattern":"{year}/{sender}/"}"#,
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
            parse_pattern(r#"{"pattern":"{date}_{sender}"}"#).unwrap(),
            "{date}_{sender}"
        );
    }

    #[test]
    fn nonsense_is_refused() {
        for raw in [
            "",
            "[]",
            r#"{"match":"all","conditions":[],"pattern":1}"#,
            r#"{"match":"some","conditions":[],"pattern":"a"}"#,
            r#"{"match":"all","conditions":[{"kind":"nope","value":"x","not":false}],"pattern":"a"}"#,
            r#"{"match":"all","conditions":[{"kind":"word","not":false}],"pattern":"a"}"#,
        ] {
            assert!(parse_generate(raw).is_err(), "{raw}");
        }
        parse_pattern(r#"{"other":"x"}"#).unwrap_err();
    }
}
