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
    /// The content hash of the file as hex. The id alone names another file
    /// once the index was started over.
    pub hash: Option<String>,
}

/// The header and the text after it. A text without a complete header of a
/// note on a file is all text. A leading byte order mark and Windows line ends
/// in the header are accepted, as an editor may have saved the file that way.
pub fn split(text: &str) -> (Option<FrontMatter>, &str) {
    let unmarked = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = unmarked
        .strip_prefix("---\n")
        .or_else(|| unmarked.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut offset = 0;
    let mut closing = None;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == FENCE {
            closing = Some((offset, offset + line.len()));
            break;
        }
        offset += line.len();
    }
    let Some((header_end, body_start)) = closing else {
        return (None, text);
    };
    let (Some(header), Some(body)) = (rest.get(..header_end), rest.get(body_start..)) else {
        return (None, text);
    };
    let mut file = None;
    let mut document = None;
    let mut hash = None;
    for line in header.lines() {
        let Some((key, raw_value)) = line.split_once(':') else {
            continue;
        };
        let value = raw_value.trim();
        match key.trim() {
            "file" if !value.is_empty() => file = Some(PathBuf::from(unquoted(value))),
            "document" => document = value.parse().ok(),
            "hash" if !value.is_empty() => hash = Some(value.to_owned()),
            _ => {}
        }
    }
    match file {
        Some(path) => (
            Some(FrontMatter {
                file: path,
                document,
                hash,
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
    if let Some(hash) = &front.hash {
        text.push_str(&format!("hash: {hash}\n"));
    }
    text.push_str(FENCE);
    text.push('\n');
    text.push_str(body);
    text
}
