//! The little JSON this crate writes: the schema and the request body.
//! jsonc-parser reads but cannot write, and serde_json is not in the lock.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<Json>),
    /// Keys keep the order they were given in.
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn text(value: &str) -> Self {
        Self::Text(value.to_owned())
    }

    pub fn object<const N: usize>(entries: [(&str, Json); N]) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Int(value) => {
                let _ = write!(out, "{value}");
            }
            Self::Text(value) => write_string(out, value),
            Self::List(items) if items.is_empty() => out.push_str("[]"),
            Self::List(items) => {
                out.push_str("[\n");
                for (position, item) in items.iter().enumerate() {
                    separate(out, position, depth + 1);
                    item.write(out, depth + 1);
                }
                close(out, ']', depth);
            }
            Self::Object(entries) if entries.is_empty() => out.push_str("{}"),
            Self::Object(entries) => {
                out.push_str("{\n");
                for (position, (key, value)) in entries.iter().enumerate() {
                    separate(out, position, depth + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, depth + 1);
                }
                close(out, '}', depth);
            }
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _level in 0..depth {
        out.push_str("  ");
    }
}

/// The comma and line break before every item but the first, then the indent.
fn separate(out: &mut String, position: usize, depth: usize) {
    if position > 0 {
        out.push_str(",\n");
    }
    indent(out, depth);
}

fn close(out: &mut String, bracket: char, depth: usize) {
    out.push('\n');
    indent(out, depth);
    out.push(bracket);
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(other));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

    use super::*;

    #[test]
    fn pretty_output_is_indented_and_escaped() {
        let json = Json::object([
            ("name", Json::text("a \"quoted\" line\nwith\\slash\u{1}")),
            ("list", Json::List(vec![Json::Int(1), Json::Bool(false)])),
            ("empty", Json::List(Vec::new())),
        ]);

        assert_eq!(
            json.pretty(),
            "{\n  \"name\": \"a \\\"quoted\\\" line\\nwith\\\\slash\\u0001\",\n  \"list\": [\n    1,\n    false\n  ],\n  \"empty\": []\n}"
        );
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let original = "tab\t, quote \", umlaut ä, control \u{7}";
        let written = Json::object([("value", Json::text(original))]).pretty();

        let Some(JsonValue::Object(root)) =
            parse_to_value(&written, &ParseOptions::default()).unwrap()
        else {
            panic!("not an object");
        };
        assert_eq!(root.get_string("value").unwrap().as_ref(), original);
    }
}
