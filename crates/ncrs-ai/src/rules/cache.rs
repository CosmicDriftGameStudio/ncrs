//! The answers of the model, kept between runs. Greedy decoding gives the
//! same answer to the same text, so what is asked is the whole key: the model
//! file, the prompt as the model reads it, the grammar and the length. Change
//! any of them and an old answer is never reused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};
use sha2::{Digest as _, Sha256};

use super::prompt::END_OF_TURN;
use super::session::RuleRequest;
use crate::model_file::ModelPin;

/// Each part with its length in front, so that moving text from one part to
/// the next cannot give the same key.
pub fn cache_key(pin: &ModelPin, request: &RuleRequest) -> String {
    let max_new_tokens = request.max_new_tokens.to_string();
    let mut hasher = Sha256::new();
    for part in [
        pin.name,
        pin.sha256,
        request.prefix.as_str(),
        request.task.as_str(),
        END_OF_TURN,
        request.grammar.as_str(),
        max_new_tokens.as_str(),
    ] {
        hasher.update(format!("{}:", part.len()));
        hasher.update(part.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out
}

#[derive(Debug)]
pub struct ResultCache {
    path: PathBuf,
    answers: HashMap<String, String>,
}

impl ResultCache {
    /// Reads the file if there is one; a line that is not an answer is left
    /// out.
    pub fn open(path: &Path) -> Self {
        let answers = fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let Ok(Some(JsonValue::Object(entry))) =
                    parse_to_value(line, &ParseOptions::default())
                else {
                    return None;
                };
                let key = entry.get_string("key")?.to_string();
                let raw = entry.get_string("raw")?.to_string();
                Some((key, raw))
            })
            .collect();
        Self {
            path: path.to_path_buf(),
            answers,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.answers.get(key).map(String::as_str)
    }

    pub fn put(&mut self, key: &str, raw: &str) -> io::Result<()> {
        if let Some(folder) = self.path.parent() {
            fs::create_dir_all(folder)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{{\"key\":\"{key}\",\"raw\":\"{}\"}}", escaped(raw))?;
        self.answers.insert(key.to_owned(), raw.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::model_file::LOCAL_MODEL;

    fn request() -> RuleRequest {
        RuleRequest::generate("PREFIX|", "Alles nach Jahr")
    }

    #[test]
    fn the_key_changes_with_every_part() {
        let base = cache_key(&LOCAL_MODEL, &request());
        assert_eq!(base, cache_key(&LOCAL_MODEL, &request()));
        let other_name = ModelPin {
            name: "other.gguf",
            ..LOCAL_MODEL
        };
        let other_hash = ModelPin {
            sha256: "0000",
            ..LOCAL_MODEL
        };
        let mut other_prefix = request();
        other_prefix.prefix.push('x');
        let mut other_task = request();
        other_task.task.push('x');
        let mut other_grammar = request();
        other_grammar.grammar.push('x');
        let mut other_length = request();
        other_length.max_new_tokens += 1;
        let keys = [
            cache_key(&other_name, &request()),
            cache_key(&other_hash, &request()),
            cache_key(&LOCAL_MODEL, &other_prefix),
            cache_key(&LOCAL_MODEL, &other_task),
            cache_key(&LOCAL_MODEL, &other_grammar),
            cache_key(&LOCAL_MODEL, &other_length),
        ];
        for key in &keys {
            assert_ne!(*key, base);
        }
        let mut distinct: Vec<&String> = keys.iter().collect();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), keys.len());
    }

    #[test]
    fn moving_text_between_prefix_and_task_changes_the_key() {
        let mut moved = request();
        moved.prefix.push_str("TASK");
        moved.task = moved.task.replacen("TASK", "", 1);
        assert_eq!(moved.full_prompt(), request().full_prompt());
        assert_ne!(
            cache_key(&LOCAL_MODEL, &moved),
            cache_key(&LOCAL_MODEL, &request())
        );
    }

    #[test]
    fn an_answer_survives_a_reopen() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("cache.jsonl");
        let raw = "{\"pattern\":\"{year}/\"}\n\twith \\ and \" and \u{1}";
        let mut cache = ResultCache::open(&path);
        assert_eq!(cache.get("k1"), None);
        cache.put("k1", raw).unwrap();
        cache.put("k2", "second").unwrap();
        assert_eq!(cache.get("k1"), Some(raw));
        let reopened = ResultCache::open(&path);
        assert_eq!(reopened.get("k1"), Some(raw));
        assert_eq!(reopened.get("k2"), Some("second"));
    }
}
