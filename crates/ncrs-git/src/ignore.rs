use std::fs;
use std::io::Write as _;

use crate::cli::Git;
use crate::undo::{refuse_symlinks, UndoAction, UndoRecord, UndoSummary};
use crate::ActionError;

const IGNORE_FILE: &str = ".gitignore";

/// A path (or, with a trailing slash, a folder) as one literal ignore line,
/// anchored at the repository root. Characters that mean something in a
/// pattern are escaped; separators are always `/`, also on Windows. Elsewhere a
/// backslash is a letter of the name.
pub fn literal_pattern(relative_path: &str) -> String {
    let slashed = if cfg!(windows) {
        relative_path.replace('\\', "/")
    } else {
        relative_path.to_string()
    };
    let name = slashed.trim_start_matches('/');
    let trailing_spaces_from = name.trim_end_matches(' ').len();
    let mut pattern = String::from("/");
    for (index, letter) in name.char_indices() {
        let is_outer_space = letter == ' ' && (index == 0 || index >= trailing_spaces_from);
        let needs_escape =
            matches!(letter, '*' | '?' | '[' | '!' | '#' | '\\') || is_outer_space;
        if needs_escape {
            pattern.push('\\');
        }
        pattern.push(letter);
    }
    pattern
}

/// The line without the white space git ignores: an escaped space at the end counts.
fn without_trailing_space(line: &str) -> &str {
    let trimmed = line.trim_end();
    let backslashes = trimmed.bytes().rev().take_while(|b| *b == b'\\').count();
    line.get(..=trimmed.len())
        .filter(|longer| backslashes % 2 == 1 && longer.ends_with(' '))
        .unwrap_or(trimmed)
}

impl Git {
    /// Adds `pattern` as a new line of the root `.gitignore`, creating the file
    /// when it is missing. `None` when the line is already there. Undo puts the
    /// old content back, or deletes a file this call made.
    pub fn ignore_pattern(&self, pattern: &str) -> Result<Option<UndoRecord>, ActionError> {
        let line = without_trailing_space(pattern);
        if line.is_empty() || line.contains(['\n', '\r']) {
            return Err(ActionError::Other {
                git_output: "an ignore pattern is one non-empty line".to_string(),
            });
        }
        refuse_symlinks(self.root(), IGNORE_FILE)?;
        let target = self.root().join(IGNORE_FILE);
        let existing = match fs::read(&target) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(found) if found.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(found) => return Err(ActionError::Io(found.to_string())),
        };
        if existing
            .lines()
            .any(|known| without_trailing_space(known) == line)
        {
            return Ok(None);
        }
        let newline = if existing.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut saved = self.save_files(&[IGNORE_FILE.to_string()])?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .map_err(|found| ActionError::Io(found.to_string()))?;
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            newline
        };
        file.write_all(format!("{separator}{line}{newline}").as_bytes())
            .map_err(|found| ActionError::Io(found.to_string()))?;
        drop(file);
        self.mark_discarded(&mut saved);
        Ok(Some(UndoRecord {
            summary: UndoSummary::Ignore {
                pattern: line.to_string(),
            },
            description: format!("Ignore {line}"),
            action: UndoAction::RestoreFiles { saved },
        }))
    }
}
