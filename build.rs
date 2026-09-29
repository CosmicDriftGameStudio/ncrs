//! Generates `src/i18n/lang/{en,de}.rs` from `strings.json`.
//!
//! The JSON file is the source of truth: it holds the value, the stable key and
//! the translator context together, so they cannot drift apart. Run
//! `cargo build` after editing it; `cargo run --bin dump-strings` prints the
//! same data for translators.

// A build script must stop on bad input. A strings.json with a missing
// translator context would otherwise produce modules whose translations are
// wrong, and the failure would only show up in the running app. So the
// restriction lints about panicking are off here by design, and on nowhere else.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::str_to_string,
    clippy::doc_markdown,
    clippy::min_ident_chars,
    clippy::single_char_lifetime_names,
    clippy::std_instead_of_core
)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

const OUT_DIR: &str = "OUT_DIR";
const INPUT: &str = "strings.json";

struct Entry {
    key: String,
    context: String,
    en: String,
    de: String,
}

/// Minimal reader for the fixed shape of `strings.json`. Written by hand so the
/// build does not depend on `serde` before the config work lands (T10).
fn parse(json: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut current: BTreeMap<&str, String> = BTreeMap::new();
    let mut in_comment = false;

    for raw in json.lines() {
        let line = raw.trim();

        if in_comment {
            if !line.contains(']') {
                continue;
            }
            in_comment = false;
        }

        // Skip comment blocks and the version header.
        if line.starts_with("\"_comment\"") || line.starts_with("\"version\"") {
            if line.ends_with('[') || line.ends_with(',') {
                in_comment = line.contains('[') && !line.contains(']');
            }
            continue;
        }

        if line == "{" {
            current.clear();
            continue;
        }
        if line.starts_with("}") {
            push_entry(&mut entries, &mut current);
            continue;
        }
        if line == "}," || line == "}" {
            continue;
        }

        let Some((raw_key, raw_value)) = split_pair(line) else {
            continue;
        };
        let value = unescape(&raw_value);

        match raw_key.as_str() {
            "key" => {
                current.insert("key", value);
            }
            "context" | "en" | "de" => {
                let slot: &'static str = match raw_key.as_str() {
                    "context" => "context",
                    "en" => "en",
                    _ => "de",
                };
                current.insert(slot, value);
            }
            _ => {}
        }
    }

    entries
}

/// Splits `"key": "value",` into its two parts, tolerating escaped quotes.
///
/// The slices below are byte-indexed. That is safe here because
/// `find_outside_quotes` and `find_closing_quote` both walk `char_indices`, so
/// every index they return is on a character boundary — checked by running the
/// build with a context full of umlauts and an emoji, which strings.json has
/// several of. Clippy flags the pattern as risky anyway; the allowance in
/// Cargo.toml says why it does not apply to this file.
fn split_pair(line: &str) -> Option<(String, String)> {
    let colon = find_outside_quotes(line, ':')?;
    let key = line[..colon].trim().trim_matches('"').to_string();
    let after_colon = line[colon + 1..].trim();
    let rest = after_colon.strip_prefix('"')?;
    let end = find_closing_quote(rest)?;
    Some((key, rest[..end].to_string()))
}

fn find_outside_quotes(line: &str, needle: char) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (i, found) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match found {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            // A guard, not a new binding: naming it `c` shadowed the loop
            // variable of the same name and made the arm unreadable.
            candidate if candidate == needle && !in_quotes => return Some(i),
            _ => {}
        }
    }
    None
}

fn find_closing_quote(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' => return Some(i),
            _ => {}
        }
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("\\\"", "\"").replace("\\\\", "\\")
}

fn push_entry(entries: &mut Vec<Entry>, current: &mut BTreeMap<&str, String>) {
    let Some(key) = current.remove("key") else {
        return;
    };
    let context = current.remove("context").unwrap_or_default();
    let en = current.remove("en").unwrap_or_default();
    let de = current.remove("de").unwrap_or_default();

    // A key without a context is the failure mode that makes translation
    // guesswork, so refuse to build.
    if context.trim().len() < 20 {
        panic!(
            "strings.json: key {key:?} needs a context of at least 20 characters \
             describing where it appears. Translators work from this file alone."
        );
    }
    if en.is_empty() || de.is_empty() {
        panic!("strings.json: key {key:?} is missing its English or German text");
    }

    entries.push(Entry {
        key,
        context,
        en,
        de,
    });
}

fn rust_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn const_name(key: &str) -> String {
    key.to_uppercase().replace('.', "_")
}

/// CamelCase variant name for a key, so the enum follows Rust conventions.
fn variant_name(key: &str) -> String {
    key.split(['.', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn generate_with(entries: &[Entry], lang: &str, value: fn(&Entry) -> &str) -> String {
    let mut out = String::new();
    // Plain comments, not doc comments: these files are pulled in with
    // `include!`, where `//!` is not allowed.
    out.push_str(&format!(
        "// {lang}. Generated from `strings.json` by `build.rs` — do not edit.\n\
         // Context for each string lives in `Msg::note()`, from the same file.\n\n"
    ));
    for entry in entries {
        out.push_str(&format!(
            "pub const {}: &str = {};\n",
            const_name(&entry.key),
            rust_str(value(entry))
        ));
    }
    out
}

fn generate_lookup(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("// Lookup table. Generated from `strings.json` by `build.rs` — do not edit.\n\n");
    out.push_str("/// German text for `msg`. Every key is generated, so this is total.\n");
    out.push_str("pub fn de_text(msg: Msg) -> &'static str {\n    match msg {\n");
    for entry in entries {
        out.push_str(&format!(
            "        Msg::{} => de::{},\n",
            variant_name(&entry.key),
            const_name(&entry.key)
        ));
    }
    out.push_str("    }\n}\n");
    out
}

fn generate_notes(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("pub fn note(msg: Msg) -> &'static str {\n    match msg {\n");
    for entry in entries {
        out.push_str(&format!(
            "        Msg::{} => {},\n",
            variant_name(&entry.key),
            rust_str(&entry.context)
        ));
    }
    out.push_str("    }\n}\n");
    out
}

/// The enum itself, so a key added to `strings.json` produces a compile error
/// in the `match` arms rather than a silently missing variant.
fn generate_enum(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    out.push_str("pub enum Msg {\n");
    for entry in entries {
        out.push_str(&format!("    {},\n", variant_name(&entry.key)));
    }
    out.push_str("}\n\n");
    out.push_str("/// Every string, for the tests and the translation dump.\n");
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("pub const ALL: &[Msg] = &[\n");
    for entry in entries {
        out.push_str(&format!("    Msg::{},\n", variant_name(&entry.key)));
    }
    out.push_str("];\n");
    out
}

/// The English `text()` match.
fn generate_text(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("pub fn text(msg: Msg) -> &'static str {\n    match msg {\n");
    for entry in entries {
        out.push_str(&format!(
            "        Msg::{} => en::{},\n",
            variant_name(&entry.key),
            const_name(&entry.key)
        ));
    }
    out.push_str("    }\n}\n");
    out
}

/// The stable key per message, so a log or the translation sheet can name a
/// string without printing its localised text.
fn generate_keys(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("pub fn key(msg: Msg) -> &'static str {\n    match msg {\n");
    for entry in entries {
        out.push_str(&format!(
            "            Msg::{} => {},\n",
            variant_name(&entry.key),
            rust_str(&entry.key)
        ));
    }
    out.push_str("    }\n}\n");
    out
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = env::var(OUT_DIR).expect(OUT_DIR);
    let input = Path::new(&manifest).join(INPUT);

    let json = fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", input.display()));
    let entries = parse(&json);

    if entries.is_empty() {
        panic!("{} contained no strings", input.display());
    }

    let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != keys.len() {
        panic!("{} contains duplicate keys", input.display());
    }

    let write = |name: &str, content: String| {
        let path = Path::new(&out_dir).join(name);
        fs::write(&path, content)
            .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()));
    };

    write(
        "en.rs",
        generate_with(&entries, "English (source language)", |e| &e.en),
    );
    write("de.rs", generate_with(&entries, "German", |e| &e.de));
    write("lookup.rs", generate_lookup(&entries));
    write("notes.rs", generate_notes(&entries));
    write("enum.rs", generate_enum(&entries));
    write("text.rs", generate_text(&entries));
    write("key.rs", generate_keys(&entries));

    println!("cargo:rerun-if-changed={}", input.display());
    println!("cargo:rerun-if-changed=build.rs");
}
