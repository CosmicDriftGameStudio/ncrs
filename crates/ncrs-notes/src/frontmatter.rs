//! The header of a note on a file: which file it is about.

use std::path::PathBuf;

const FENCE: &str = "---";

/// What a note on a file keeps about its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    /// The path the file had when the note last looked.
    pub file: PathBuf,
    /// The id of the file in the index; set only when the file is indexed.
    pub document: Option<i64>,
}

/// The header and the text after it. A text without a complete header of a
/// note on a file is all text.
pub fn split(text: &str) -> (Option<FrontMatter>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (None, text);
    };
    let (header, body) = match rest.split_once("\n---\n") {
        Some(parts) => parts,
        None => match rest.strip_suffix("\n---") {
            Some(header) => (header, ""),
            None => return (None, text),
        },
    };
    let mut file = None;
    let mut document = None;
    for line in header.lines() {
        let Some((key, raw_value)) = line.split_once(':') else {
            continue;
        };
        let value = raw_value.trim();
        match key.trim() {
            "file" if !value.is_empty() => file = Some(PathBuf::from(unquoted(value))),
            "document" => document = value.parse().ok(),
            _ => {}
        }
    }
    match file {
        Some(path) => (
            Some(FrontMatter {
                file: path,
                document,
            }),
            body,
        ),
        None => (None, text),
    }
}

/// A path that would not read back as written (line breaks, edge spaces, a
/// leading quote) is kept in quotes with escapes; any other path is plain.
fn quoted_if_needed(path: &str) -> String {
    let plain = path.trim() == path && !path.starts_with('"') && !path.contains(['\n', '\r']);
    if plain {
        return path.to_owned();
    }
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn unquoted(value: &str) -> String {
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return value.to_owned();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// The text of the file on disk: the header, then `body`.
pub fn compose(front: &FrontMatter, body: &str) -> String {
    let file = quoted_if_needed(&front.file.to_string_lossy());
    let mut text = format!("{FENCE}\nfile: {file}\n");
    if let Some(document) = front.document {
        text.push_str(&format!("document: {document}\n"));
    }
    text.push_str(FENCE);
    text.push('\n');
    text.push_str(body);
    text
}
