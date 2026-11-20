//! The chats on disk: one small JSON file per chat in `<data dir>/hermes/chats`.
//! A file holds what is needed to show the chat again, not the state of the
//! conversation: that one holds running work and handles that cannot be saved.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};
use ncrs_ai::Json;

const EXTENSION: &str = "json";
const TEMP_EXTENSION: &str = "json.tmp";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChatId(String);

impl ChatId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// An id that is a file name: letters, digits and dashes only.
    fn parse(text: &str) -> Option<Self> {
        let clean = !text.is_empty()
            && text
                .chars()
                .all(|letter| letter.is_ascii_alphanumeric() || letter == '-');
        clean.then(|| Self(text.to_owned()))
    }

    /// The id for a chat made at `seconds`; `taken` says which ids exist, so
    /// two chats made in the same second still differ.
    pub fn from_time(seconds: i64, taken: impl Fn(&Self) -> bool) -> Self {
        let mut counter: u32 = 0;
        loop {
            let candidate = Self(format!("{seconds:012}-{counter:03}"));
            if !taken(&candidate) {
                return candidate;
            }
            counter += 1;
        }
    }
}

/// Where a chat stands, as the list words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatStatus {
    PlanOpen,
    Done,
    Question,
    Running,
}

impl ChatStatus {
    fn key(self) -> &'static str {
        match self {
            Self::PlanOpen => "plan_open",
            Self::Done => "done",
            Self::Question => "question",
            Self::Running => "running",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [Self::PlanOpen, Self::Done, Self::Question, Self::Running]
            .into_iter()
            .find(|status| status.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    Hermes,
}

impl Speaker {
    fn key(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Hermes => "hermes",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [Self::User, Self::Hermes]
            .into_iter()
            .find(|speaker| speaker.key() == key)
    }
}

/// One bubble of the conversation as text; cards and buttons are not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub speaker: Speaker,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredChat {
    pub id: ChatId,
    pub title: String,
    /// Unix seconds.
    pub created: i64,
    /// Unix seconds of the last change of the conversation or the title.
    pub updated: i64,
    pub status: ChatStatus,
    pub lines: Vec<Line>,
}

impl StoredChat {
    pub fn has_user_line(&self) -> bool {
        self.lines.iter().any(|line| line.speaker == Speaker::User)
    }
}

pub fn to_json(chat: &StoredChat) -> String {
    let lines = chat
        .lines
        .iter()
        .map(|line| {
            Json::object([
                ("from", Json::text(line.speaker.key())),
                ("text", Json::text(&line.text)),
            ])
        })
        .collect();
    let mut text = Json::object([
        ("id", Json::text(chat.id.as_str())),
        ("title", Json::text(&chat.title)),
        ("created", Json::Int(chat.created)),
        ("updated", Json::Int(chat.updated)),
        ("status", Json::text(chat.status.key())),
        ("lines", Json::List(lines)),
    ])
    .pretty();
    text.push('\n');
    text
}

pub fn from_json(text: &str) -> Option<StoredChat> {
    let Some(JsonValue::Object(mut chat)) = parse_to_value(text, &ParseOptions::default()).ok()?
    else {
        return None;
    };
    let id = ChatId::parse(&chat.take_string("id")?)?;
    let title = chat.take_string("title")?.into_owned();
    let created = chat.get_number("created")?.parse().ok()?;
    let updated = chat.get_number("updated")?.parse().ok()?;
    let status = ChatStatus::from_key(&chat.take_string("status")?)?;
    let lines = chat
        .take_array("lines")?
        .into_iter()
        .filter_map(|value| {
            let JsonValue::Object(mut line) = value else {
                return None;
            };
            Some(Line {
                speaker: Speaker::from_key(&line.take_string("from")?)?,
                text: line.take_string("text")?.into_owned(),
            })
        })
        .collect();
    Some(StoredChat {
        id,
        title,
        created,
        updated,
        status,
        lines,
    })
}

fn file_of(dir: &Path, id: &ChatId) -> PathBuf {
    dir.join(format!("{}.{EXTENSION}", id.as_str()))
}

/// Writes the chat to a temporary file and renames it over the real one, so a
/// crash leaves the old file or the new one, never half of it.
pub fn write(dir: &Path, chat: &StoredChat) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let target = file_of(dir, &chat.id);
    let temporary = dir.join(format!("{}.{TEMP_EXTENSION}", chat.id.as_str()));
    fs::write(&temporary, to_json(chat))?;
    fs::rename(&temporary, &target)
}

pub fn remove(dir: &Path, id: &ChatId) {
    // A file that is already gone is what was wanted.
    let _ = fs::remove_file(file_of(dir, id));
}

/// Every chat in the folder; files that are not a chat are left alone.
pub fn load_all(dir: &Path) -> Vec<StoredChat> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == EXTENSION))
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|text| from_json(&text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(id: &str, title: &str) -> StoredChat {
        StoredChat {
            id: ChatId::parse(id).expect("a valid id"),
            title: title.to_owned(),
            created: 1_700_000_000,
            updated: 1_700_000_100,
            status: ChatStatus::PlanOpen,
            lines: vec![
                Line {
                    speaker: Speaker::Hermes,
                    text: "Hello \"you\"\nsecond line".to_owned(),
                },
                Line {
                    speaker: Speaker::User,
                    text: "tidy up \u{e4}\\".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn a_chat_survives_the_trip_through_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("hermes").join("chats");
        let first = chat("a-1", "Rechnungen");
        let second = chat("b-2", "Downloads");
        write(&nested, &first).expect("write");
        write(&nested, &second).expect("write");
        let mut loaded = load_all(&nested);
        loaded.sort_by(|left, right| left.id.cmp(&right.id));
        assert_eq!(loaded, vec![first, second]);
    }

    #[test]
    fn a_rewrite_replaces_the_file_and_leaves_no_temporary_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut stored = chat("a-1", "Old");
        write(dir.path(), &stored).expect("write");
        stored.title = "New".to_owned();
        write(dir.path(), &stored).expect("write");
        assert_eq!(load_all(dir.path()), vec![stored]);
        let files = fs::read_dir(dir.path()).expect("dir").count();
        assert_eq!(files, 1);
    }

    #[test]
    fn a_broken_file_is_skipped_and_a_removed_chat_is_gone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let stored = chat("a-1", "Keep");
        write(dir.path(), &stored).expect("write");
        fs::write(dir.path().join("broken.json"), "{ not json").expect("write");
        assert_eq!(load_all(dir.path()), vec![stored.clone()]);
        remove(dir.path(), &stored.id);
        assert!(load_all(dir.path()).is_empty());
    }

    #[test]
    fn ids_made_in_the_same_second_differ() {
        let first = ChatId::from_time(5, |_| false);
        let second = ChatId::from_time(5, |candidate| *candidate == first);
        assert_ne!(first, second);
    }

    #[test]
    fn an_id_with_a_path_in_it_is_refused() {
        assert!(ChatId::parse("../x").is_none());
        assert!(ChatId::parse("").is_none());
    }
}
