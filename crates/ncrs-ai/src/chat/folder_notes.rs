//! The question that has the model say one short sentence about each folder
//! of the Platz card. It sees the path, the size and the date, never a
//! file; what may be deleted is decided by code, not by this sentence.

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

use super::intent::FolderFacts;
use crate::contract::clean_value;
use crate::error::AiError;

const MAX_NOTE_CHARS: usize = 160;
const MAX_FOLDERS: usize = 12;
const BYTES_PER_MEGABYTE: u64 = 1_000_000;

pub const FOLDER_NOTES_PROMPT: &str = "You explain folders on a disk in one short German sentence each, for a person who wants to free space. You get one line per folder: path | size | last change. Say what the folder most likely is and what a person should weigh before touching it, from the path, size and date only. You never see the content, so do not claim to know it. Never say that a folder is safe to delete and never tell the person to delete it. Reply with exactly one JSON object {\"notes\":[\"...\"]} holding one sentence per folder, in the order given, as many as there are folders.";

/// The lines the model is asked about: the folders beyond the cap are left out.
pub fn folder_notes_message(folders: &[FolderFacts]) -> String {
    folders
        .iter()
        .take(MAX_FOLDERS)
        .map(|folder| {
            let changed = folder.modified.as_deref().unwrap_or("unknown");
            format!(
                "{} | {} MB | last change {}",
                folder.path,
                folder.size_bytes.div_euclid(BYTES_PER_MEGABYTE),
                changed
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The sentences for `asked` folders, or the reason the reply is no use: the
/// count must match, because a sentence under the wrong folder is a lie.
pub fn parse_folder_notes(raw: &str, asked: usize) -> Result<Vec<String>, AiError> {
    let parsed = parse_to_value(raw, &ParseOptions::default())
        .map_err(|_syntax| AiError::BadReply("not valid JSON"))?;
    let Some(JsonValue::Object(reply)) = parsed else {
        return Err(AiError::BadReply("not a JSON object"));
    };
    let notes: Vec<String> = reply
        .get_array("notes")
        .ok_or(AiError::BadReply("no notes"))?
        .iter()
        .map(|value| match value {
            JsonValue::String(note) => clean_value(note)
                .filter(|cleaned| cleaned.chars().count() <= MAX_NOTE_CHARS)
                .ok_or(AiError::BadReply("a note is empty or too long")),
            _ => Err(AiError::BadReply("a note is not text")),
        })
        .collect::<Result<_, _>>()?;
    if notes.len() == asked.min(MAX_FOLDERS) {
        Ok(notes)
    } else {
        Err(AiError::BadReply("not one note for each folder"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, megabytes: u64) -> FolderFacts {
        FolderFacts {
            path: path.to_owned(),
            size_bytes: megabytes * BYTES_PER_MEGABYTE,
            modified: Some("2025-03-14".to_owned()),
        }
    }

    #[test]
    fn the_model_is_shown_path_size_and_date_only() {
        let message = folder_notes_message(&[folder("~/Backups", 46_000)]);
        assert_eq!(message, "~/Backups | 46000 MB | last change 2025-03-14");
    }

    #[test]
    fn one_note_for_each_folder_is_taken() {
        let notes = parse_folder_notes(r#"{"notes":["Alte Sicherungen.","Modelle."]}"#, 2);
        assert_eq!(
            notes,
            Ok(vec!["Alte Sicherungen.".to_owned(), "Modelle.".to_owned()])
        );
    }

    #[test]
    fn a_wrong_count_or_a_bad_note_is_no_use() {
        parse_folder_notes(r#"{"notes":["Eins."]}"#, 2).unwrap_err();
        parse_folder_notes(r#"{"notes":[""]}"#, 1).unwrap_err();
        parse_folder_notes(r#"{"notes":[3]}"#, 1).unwrap_err();
        parse_folder_notes("nonsense", 1).unwrap_err();
    }
}
