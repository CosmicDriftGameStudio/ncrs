//! What the bridge reports, validated once at the border.

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Production,
    Sandbox,
    Xcode,
}

impl Environment {
    fn name(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Sandbox => "sandbox",
            Self::Xcode => "xcode",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "production" => Some(Self::Production),
            "sandbox" => Some(Self::Sandbox),
            "xcode" => Some(Self::Xcode),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreStatus {
    pub environment: Option<Environment>,
    pub original_app_version: Option<String>,
    /// Unix seconds of the purchase of the trial product.
    pub trial_started_at: Option<i64>,
    pub full_purchased: bool,
    /// The system has no StoreKit 2 app receipt (macOS before 13).
    pub unsupported: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The store or the bridge reported a problem.
    Store(String),
    /// The answer was not the JSON the bridge promises.
    Malformed(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(message) => write!(f, "store: {message}"),
            Self::Malformed(message) => write!(f, "malformed store answer: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl StoreStatus {
    /// Reads the status JSON. An `error` field becomes `Err`: a status without
    /// the app transaction says nothing about the purchase.
    pub fn from_json(text: &str) -> Result<Self, StoreError> {
        let Ok(Some(JsonValue::Object(object))) = parse_to_value(text, &ParseOptions::default())
        else {
            return Err(StoreError::Malformed(text.chars().take(120).collect()));
        };
        if let Some(message) = object.get_string("error") {
            return Err(StoreError::Store(message.to_string()));
        }
        Ok(Self {
            environment: object
                .get_string("environment")
                .and_then(|name| Environment::from_name(name)),
            original_app_version: object
                .get_string("original_app_version")
                .map(ToString::to_string),
            trial_started_at: object
                .get_number("trial_started_at")
                .and_then(|number| number.parse().ok()),
            full_purchased: object.get_boolean("full_purchased").unwrap_or(false),
            unsupported: object.get_boolean("unsupported").unwrap_or(false),
        })
    }

    pub fn to_json(&self) -> String {
        let environment = self.environment.map_or_else(
            || "null".to_owned(),
            |value| format!("\"{}\"", value.name()),
        );
        let version = self
            .original_app_version
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_string);
        let started = self
            .trial_started_at
            .map_or_else(|| "null".to_owned(), |value| value.to_string());
        format!(
            "{{\"environment\":{environment},\"original_app_version\":{version},\"trial_started_at\":{started},\"full_purchased\":{},\"unsupported\":{}}}",
            self.full_purchased, self.unsupported
        )
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if control.is_control() => {
                out.push_str(&format!("\\u{:04x}", u32::from(control)))
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_bridge_answer() {
        let status = StoreStatus::from_json(
            r#"{"environment":"production","original_app_version":"1.6.0","trial_started_at":1700000000,"full_purchased":false,"unsupported":false,"error":null}"#,
        )
        .unwrap_or_default();
        assert_eq!(status.environment, Some(Environment::Production));
        assert_eq!(status.original_app_version.as_deref(), Some("1.6.0"));
        assert_eq!(status.trial_started_at, Some(1_700_000_000));
    }

    #[test]
    fn an_error_field_is_an_error() {
        let result = StoreStatus::from_json(r#"{"error":"app transaction: nope"}"#);
        assert_eq!(
            result,
            Err(StoreError::Store("app transaction: nope".to_owned()))
        );
    }

    #[test]
    fn garbage_is_malformed() {
        assert!(matches!(
            StoreStatus::from_json("not json"),
            Err(StoreError::Malformed(_))
        ));
    }

    #[test]
    fn unsupported_answer_has_no_other_fields() {
        let status = StoreStatus::from_json(r#"{"unsupported":true}"#).unwrap_or_default();
        assert!(status.unsupported);
    }

    #[test]
    fn json_round_trips() {
        let status = StoreStatus {
            environment: Some(Environment::Sandbox),
            original_app_version: Some("1.\"0".to_owned()),
            trial_started_at: Some(-5),
            full_purchased: true,
            unsupported: false,
        };
        assert_eq!(StoreStatus::from_json(&status.to_json()), Ok(status));
    }
}
