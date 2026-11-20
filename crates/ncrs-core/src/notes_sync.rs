//! How the notes are kept in a second folder: shared by the config, the
//! settings page and the notes tab.

/// Which way the notes travel to the sync folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotesSyncDirection {
    /// New and changed notes go to the sync folder; nothing is deleted there.
    #[default]
    OneWay,
    /// The newer note wins; when both changed, both are kept.
    BothWays,
}

impl NotesSyncDirection {
    /// The words of the config file.
    pub const fn key(self) -> &'static str {
        match self {
            Self::OneWay => "one_way",
            Self::BothWays => "both_ways",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "one_way" => Some(Self::OneWay),
            "both_ways" => Some(Self::BothWays),
            _ => None,
        }
    }
}

/// When the sync runs by itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotesSyncWhen {
    /// After every saved change.
    #[default]
    AfterSave,
    /// Once, when ncrs starts.
    AtStart,
    /// Every this many minutes.
    EveryMinutes(u32),
}

impl NotesSyncWhen {
    /// The intervals the settings page offers.
    pub const INTERVALS: [u32; 3] = [5, 15, 60];

    /// The words of the config file.
    pub fn key(self) -> String {
        match self {
            Self::AfterSave => "after_save".to_owned(),
            Self::AtStart => "at_start".to_owned(),
            Self::EveryMinutes(minutes) => format!("every_{minutes}"),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "after_save" => Some(Self::AfterSave),
            "at_start" => Some(Self::AtStart),
            other => other
                .strip_prefix("every_")
                .and_then(|minutes| minutes.parse().ok())
                .filter(|minutes| *minutes > 0)
                .map(Self::EveryMinutes),
        }
    }
}

/// The options of the lock timeout, in minutes; 0 never locks by itself.
pub const NOTES_LOCK_MINUTES: [u32; 5] = [5, 15, 30, 60, 0];

/// The timeout of the lock when the config says nothing.
pub const NOTES_LOCK_DEFAULT_MINUTES: u32 = 15;

/// What the settings say about the notes besides their folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesSettings {
    pub sync_folder: Option<std::path::PathBuf>,
    pub direction: NotesSyncDirection,
    pub when: NotesSyncWhen,
    /// Minutes without input until the key leaves memory; 0 never.
    pub lock_minutes: u32,
    /// New notes are written encrypted.
    pub encrypt_all: bool,
    /// Hermes may read and write notes.
    pub hermes_access: bool,
}

impl Default for NotesSettings {
    fn default() -> Self {
        Self {
            sync_folder: None,
            direction: NotesSyncDirection::default(),
            when: NotesSyncWhen::default(),
            lock_minutes: NOTES_LOCK_DEFAULT_MINUTES,
            encrypt_all: false,
            hermes_access: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_words_read_back() {
        for when in [
            NotesSyncWhen::AfterSave,
            NotesSyncWhen::AtStart,
            NotesSyncWhen::EveryMinutes(15),
        ] {
            assert_eq!(NotesSyncWhen::from_key(&when.key()), Some(when));
        }
        assert_eq!(NotesSyncWhen::from_key("every_0"), None);
        assert_eq!(
            NotesSyncDirection::from_key(NotesSyncDirection::BothWays.key()),
            Some(NotesSyncDirection::BothWays)
        );
    }
}
