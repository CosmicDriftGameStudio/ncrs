use super::*;

use crate::config::{KeyEntry, KeySection};

/// The tabs that read a `[keys.<tab>]` table. A second tab is one more name.
pub const TAB_KEY_TABLES: [&str; 1] = ["git"];

/// One line of a `[keys.<tab>]` table, read for its keys. Whether the name
/// is an action of the tab is for the tab to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabKeyEntry {
    pub action: String,
    /// The line of the entry in the file.
    pub line: Option<usize>,
    pub bindings: Vec<Binding>,
}

/// The lines of one `[keys.<tab>]` table, as the file wrote them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabKeys {
    pub entries: Vec<TabKeyEntry>,
}

/// Why a `[keys.<tab>]` table is not usable, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabKeysError {
    pub span: Range<usize>,
    pub reason: String,
}

impl TabKeys {
    /// Reads the keys of the table `name` of `[keys]`; no table, no entries.
    /// `line_of` turns a position in the file into its line.
    pub fn from_section(
        section: &KeySection,
        name: &str,
        line_of: impl Fn(usize) -> Option<usize>,
    ) -> Result<Self, TabKeysError> {
        let Some(table) = section
            .tables
            .iter()
            .find(|table| table.name.as_ref() == name)
        else {
            return Ok(Self::default());
        };
        let mut entries: Vec<TabKeyEntry> = Vec::new();
        for entry in &table.entries {
            let action = entry.action.as_ref().clone();
            if entries.iter().any(|read| read.action == action) {
                return Err(TabKeysError {
                    span: entry.action.span(),
                    reason: format!("{action} is listed twice in [keys.{name}]"),
                });
            }
            entries.push(TabKeyEntry {
                line: line_of(entry.action.span().start),
                bindings: bindings_of_entry(entry)?,
                action,
            });
        }
        Ok(Self { entries })
    }
}

fn bindings_of_entry(entry: &KeyEntry) -> Result<Vec<Binding>, TabKeysError> {
    let mut bindings: Vec<Binding> = Vec::new();
    for (index, chord) in entry.chords.iter().enumerate() {
        if index >= MAX_KEYS_PER_ACTION {
            return Err(TabKeysError {
                span: chord.span(),
                reason: format!("at most {MAX_KEYS_PER_ACTION} keys per action"),
            });
        }
        let text = chord.as_ref().trim();
        let binding = parse_chord(text).map_err(|reason| TabKeysError {
            span: chord.span(),
            reason,
        })?;
        if bindings.contains(&binding) {
            return Err(TabKeysError {
                span: chord.span(),
                reason: format!("{text:?} is listed twice for {}", entry.action.as_ref()),
            });
        }
        bindings.push(binding);
    }
    Ok(bindings)
}
