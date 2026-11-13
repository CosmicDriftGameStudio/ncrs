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
        // A comment after the value, as in the spec's example.
        let value = raw_value.split(" #").next().unwrap_or(raw_value).trim();
        match key.trim() {
            "file" if !value.is_empty() => file = Some(PathBuf::from(value)),
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

/// The text of the file on disk: the header, then `body`.
pub fn compose(front: &FrontMatter, body: &str) -> String {
    let mut text = format!("{FENCE}\nfile: {}\n", front.file.display());
    if let Some(document) = front.document {
        text.push_str(&format!("document: {document}\n"));
    }
    text.push_str(FENCE);
    text.push('\n');
    text.push_str(body);
    text
}
