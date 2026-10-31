use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use jsonc_parser::{parse_to_value, JsonObject, JsonValue, ParseOptions};
use ncrs_index::{DocumentFields, Field, KindCatalog};
use ncrs_intake::{parse_amount, parse_date, AI_FIELD_ORDER};

use crate::error::AiError;
use crate::json::Json;

/// The longest value kept from a reply, in characters.
pub const MAX_VALUE_CHARS: usize = 200;

/// A field the provider is at least this sure about is written. Below it the
/// value is not stored and a person decides: a wrong sender would file the
/// document in the wrong folder.
pub const CONFIDENT: f32 = 0.8;

/// What the provider is asked. Only the fields the rules left open are
/// requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionRequest {
    pub text: String,
    pub open_fields: Vec<Field>,
    pub known: DocumentFields,
    /// The inbox location the document came from, which consent is given for.
    pub source: PathBuf,
    /// The kinds the document may be named as.
    pub kinds: Arc<KindCatalog>,
}

/// A value the provider found, checked: it reads as the type of its field.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedField {
    pub field: Field,
    pub value: String,
    /// 0.0 to 1.0.
    pub confidence: f32,
}

impl ExtractedField {
    /// `None` when the value is empty after cleaning, the confidence is
    /// outside 0.0..=1.0, or the value does not read as the type of the field.
    pub fn new(field: Field, value: &str, confidence: f32) -> Option<Self> {
        if !(0.0..=1.0).contains(&confidence) {
            return None;
        }
        let cleaned = clean_value(value)?;
        let reads_as_field = match field {
            Field::Date => parse_date(&cleaned).is_some(),
            Field::Amount => parse_amount(&cleaned, None).is_some(),
            Field::Sender | Field::Recipient | Field::Kind | Field::Number => true,
        };
        reads_as_field.then_some(Self {
            field,
            value: cleaned,
            confidence,
        })
    }

    /// Sets the field if it is still empty; a field that is filled stays as
    /// it is. Whether it was set. A kind is written as the ID of a known kind.
    pub fn fill(&self, fields: &mut DocumentFields, kinds: &KindCatalog) -> bool {
        match self.field {
            Field::Sender => set_if_empty(&mut fields.sender, Some(self.value.clone())),
            Field::Recipient => set_if_empty(&mut fields.recipient, Some(self.value.clone())),
            Field::Kind => set_if_empty(&mut fields.kind, Some(kinds.detected_kind(&self.value))),
            Field::Number => set_if_empty(&mut fields.number, Some(self.value.clone())),
            Field::Date => set_if_empty(&mut fields.date, parse_date(&self.value)),
            Field::Amount => set_if_empty(&mut fields.amount, parse_amount(&self.value, None)),
        }
    }
}

fn set_if_empty<T>(slot: &mut Option<T>, value: Option<T>) -> bool {
    if slot.is_some() {
        return false;
    }
    let found = value.is_some();
    *slot = value;
    found
}

/// Control characters out, ends trimmed, at most `MAX_VALUE_CHARS`
/// characters; `None` when nothing is left.
pub fn clean_value(value: &str) -> Option<String> {
    let printable: String = value
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let capped: String = printable.trim().chars().take(MAX_VALUE_CHARS).collect();
    let cleaned = capped.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extraction {
    /// In the order of `AI_FIELD_ORDER`, each field at most once.
    pub fields: Vec<ExtractedField>,
}

/// The only thing a provider can do: name values for fields. It cannot
/// express an action, so nothing it says can move, delete or run anything.
pub trait Provider: Send + Sync + fmt::Debug {
    fn extract(&self, request: &ExtractionRequest) -> Result<Extraction, AiError>;
}

/// The JSON schema of the reply: an object with one optional property per
/// open field, each `{ "value": string, "confidence": number }`, and nothing
/// else. A provider that enforces schemas then cannot answer with more; one
/// that does not is handled by `parse_reply`, which drops what is not asked.
pub fn output_schema(open_fields: &[Field], kinds: &KindCatalog) -> Json {
    let properties = AI_FIELD_ORDER
        .into_iter()
        .filter(|field| open_fields.contains(field))
        .map(|field| (field.as_str().to_owned(), field_schema(field, kinds)))
        .collect();
    Json::object([
        ("type", Json::text("object")),
        ("properties", Json::Object(properties)),
        ("additionalProperties", Json::Bool(false)),
    ])
}

fn field_schema(field: Field, kinds: &KindCatalog) -> Json {
    let hint: &str = match field {
        Field::Sender => "who issued the document: the letterhead, company or sender line; never the addressee or recipient, never the name after Mr/Mrs/Herr/Frau in the address block",
        Field::Recipient => "to whom the document is addressed: the person or company in the address block, never the sender",
        Field::Kind => &kind_hint(kinds),
        Field::Date => "the document date as YYYY-MM-DD",
        Field::Number => "the document or invoice number",
        Field::Amount => "the total with its currency code, for example 12.50 EUR",
    };
    Json::object([
        ("type", Json::text("object")),
        ("description", Json::text(hint)),
        (
            "properties",
            Json::object([
                ("value", Json::object([("type", Json::text("string"))])),
                (
                    "confidence",
                    Json::object([
                        ("type", Json::text("number")),
                        ("minimum", Json::Int(0)),
                        ("maximum", Json::Int(1)),
                    ]),
                ),
            ]),
        ),
        (
            "required",
            Json::List(vec![Json::text("value"), Json::text("confidence")]),
        ),
        ("additionalProperties", Json::Bool(false)),
    ])
}

fn kind_hint(kinds: &KindCatalog) -> String {
    let ids: Vec<&str> = kinds.ids().collect();
    format!(
        "what kind of document it is, one of: {}, or another short word",
        ids.join(", ")
    )
}

/// Lenient in shape, strict in effect: unknown keys, fields that were not
/// asked for and fields with a bad value or confidence are dropped, not an
/// error. Only a reply that is not a JSON object is.
pub fn parse_reply(raw: &str, open_fields: &[Field]) -> Result<Extraction, AiError> {
    let parsed = parse_to_value(raw, &ParseOptions::default())
        .map_err(|_syntax| AiError::BadReply("not valid JSON"))?;
    let Some(JsonValue::Object(reply)) = parsed else {
        return Err(AiError::BadReply("not a JSON object"));
    };
    let fields = AI_FIELD_ORDER
        .into_iter()
        .filter(|field| open_fields.contains(field))
        .filter_map(|field| field_of(&reply, field))
        .collect();
    Ok(Extraction { fields })
}

fn field_of(reply: &JsonObject<'_>, field: Field) -> Option<ExtractedField> {
    let entry = reply.get_object(field.as_str())?;
    let value = entry.get_string("value")?;
    let confidence = entry.get_number("confidence")?.parse::<f32>().ok()?;
    ExtractedField::new(field, value, confidence)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use ncrs_index::Currency;

    use super::*;

    const ALL: [Field; 6] = AI_FIELD_ORDER;

    fn parse(raw: &str, open: &[Field]) -> Extraction {
        parse_reply(raw, open).unwrap()
    }

    #[test]
    fn unknown_keys_and_fields_that_were_not_asked_for_are_dropped() {
        let raw = r#"{
            "sender": {"value": "Agency B", "confidence": 0.9, "action": "delete"},
            "number": {"value": "4711", "confidence": 0.9},
            "action": "delete",
            "path": "/etc"
        }"#;

        let extraction = parse(raw, &[Field::Sender]);

        assert_eq!(
            extraction.fields,
            vec![ExtractedField {
                field: Field::Sender,
                value: "Agency B".to_owned(),
                confidence: 0.9,
            }]
        );
    }

    #[test]
    fn a_bad_confidence_or_a_bad_shape_drops_only_that_field() {
        let raw = r#"{
            "sender": {"value": "Agency B", "confidence": 1.5},
            "kind": {"value": "invoice", "confidence": "high"},
            "number": {"value": "4711"},
            "date": {"value": 20240101, "confidence": 0.9},
            "amount": {"value": "12.50 EUR", "confidence": -0.1}
        }"#;

        assert!(parse(raw, &ALL).fields.is_empty());
    }

    #[test]
    fn control_characters_are_stripped_and_a_long_value_is_capped() {
        let long = "ä".repeat(10_000);
        let raw = format!(
            r#"{{"sender": {{"value": "  Agency\u0007 B\u0000 ", "confidence": 1}},
                "number": {{"value": "{long}", "confidence": 0.5}},
                "kind": {{"value": "\u0001\u0002", "confidence": 0.9}}}}"#
        );

        let extraction = parse(&raw, &ALL);

        assert_eq!(extraction.fields.len(), 2);
        assert_eq!(extraction.fields[0].value, "Agency B");
        assert_eq!(extraction.fields[1].value.chars().count(), MAX_VALUE_CHARS);
    }

    #[test]
    fn dates_and_amounts_that_do_not_read_are_dropped() {
        let raw = r#"{
            "date": {"value": "next Tuesday", "confidence": 0.9},
            "amount": {"value": "about twelve", "confidence": 0.9}
        }"#;

        assert!(parse(raw, &ALL).fields.is_empty());
    }

    #[test]
    fn typed_fields_are_read_by_the_rules_parsers() {
        let raw = r#"{
            "date": {"value": "05.03.2024", "confidence": 0.9},
            "amount": {"value": "1.234,50 EUR", "confidence": 0.9}
        }"#;
        let mut fields = DocumentFields::default();

        for found in &parse(raw, &ALL).fields {
            assert!(found.fill(&mut fields, &KindCatalog::built_in()));
        }

        assert_eq!(fields.date.unwrap().to_string(), "2024-03-05");
        let amount = fields.amount.unwrap();
        assert_eq!(amount.minor_units, 123_450);
        assert_eq!(amount.currency, Currency::new("EUR").unwrap());
    }

    #[test]
    fn a_filled_field_is_not_overwritten() {
        let found = ExtractedField::new(Field::Sender, "Agency B", 0.9).unwrap();
        let mut fields = DocumentFields {
            sender: Some("Company A".to_owned()),
            ..DocumentFields::default()
        };

        assert!(!found.fill(&mut fields, &KindCatalog::built_in()));
        assert_eq!(fields.sender.as_deref(), Some("Company A"));
    }

    #[test]
    fn a_reply_that_is_not_an_object_is_an_error() {
        assert!(matches!(
            parse_reply("[1, 2]", &ALL),
            Err(AiError::BadReply(_))
        ));
        assert!(matches!(
            parse_reply("sure, here you go", &ALL),
            Err(AiError::BadReply(_))
        ));
    }

    #[test]
    fn the_recipient_is_asked_and_filled_only_while_empty() {
        let schema = output_schema(&[Field::Recipient], &KindCatalog::built_in()).pretty();
        let found = ExtractedField::new(Field::Recipient, "Erika Beispiel", 0.9).unwrap();
        let mut fields = DocumentFields::default();

        assert!(schema.contains("\"recipient\""));
        assert!(found.fill(&mut fields, &KindCatalog::built_in()));
        assert_eq!(fields.recipient.as_deref(), Some("Erika Beispiel"));
        assert!(!found.fill(&mut fields, &KindCatalog::built_in()));
    }

    #[test]
    fn the_schema_asks_for_the_open_fields_only() {
        let schema =
            output_schema(&[Field::Date, Field::Sender], &KindCatalog::built_in()).pretty();

        assert!(schema.contains("\"sender\""));
        assert!(schema.contains("\"date\""));
        assert!(!schema.contains("\"amount\""));
        assert!(schema.contains("\"additionalProperties\": false"));
    }

    #[test]
    fn the_kind_hint_names_the_known_and_custom_ids() {
        let (kinds, _) = KindCatalog::built_in().extended(&[ncrs_index::CustomKind {
            id: "payslip".to_owned(),
            ..ncrs_index::CustomKind::default()
        }]);

        let schema = output_schema(&[Field::Kind], &kinds).pretty();

        assert!(schema.contains(
            "one of: invoice, credit_note, reminder, contract, notice, receipt, letter, insurance, quote, delivery_note, termination, payslip, or another short word"
        ));
    }

    #[test]
    fn a_kind_the_provider_names_is_written_as_the_id_of_a_known_kind() {
        let (kinds, _) = KindCatalog::built_in().extended(&[ncrs_index::CustomKind {
            id: "payslip".to_owned(),
            words: vec!["Lohnabrechnung".to_owned()],
            ..ncrs_index::CustomKind::default()
        }]);
        let mut fields = DocumentFields::default();

        let found = ExtractedField::new(Field::Kind, "Lohnabrechnung", 0.9).unwrap();

        assert!(found.fill(&mut fields, &kinds));
        assert_eq!(fields.kind.as_deref(), Some("payslip"));
    }
}
