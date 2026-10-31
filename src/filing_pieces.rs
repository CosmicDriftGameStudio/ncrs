//! A filing target as the folders and the file name it is made of: the model
//! behind the "Move to" widget. A pattern is its stored form; no widgets here,
//! the view is `ui::filing_pieces`.

use chrono::NaiveDate;
use ncrs_index::DocumentFields;
use ncrs_intake::{is_plain_literal, render_pattern};

use crate::i18n::{Language, Msg};

/// The fields a folder piece can stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderField {
    Year,
    Month,
    Sender,
    Recipient,
    Kind,
}

impl FolderField {
    pub const ALL: [Self; 5] = [
        Self::Year,
        Self::Month,
        Self::Sender,
        Self::Recipient,
        Self::Kind,
    ];

    fn placeholder(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Sender => "sender",
            Self::Recipient => "recipient",
            Self::Kind => "kind",
        }
    }

    fn of_placeholder(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|field| field.placeholder() == name)
    }

    pub fn name(self, lang: Language) -> &'static str {
        lang.text(match self {
            Self::Year => Msg::FilingPiecesFieldYear,
            Self::Month => Msg::FilingPiecesFieldMonth,
            Self::Sender => Msg::FilingPiecesFieldSender,
            Self::Recipient => Msg::FilingPiecesFieldRecipient,
            Self::Kind => Msg::FilingPiecesFieldKind,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPiece {
    /// A folder with a name of its own.
    Named(String),
    Field(FolderField),
    /// A folder made of several parts, as old patterns have them; the
    /// segment of the pattern, kept as it is.
    Mixed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileName {
    /// Made from the date, sender and number, whichever are known.
    Automatic,
    /// The last segment of an old pattern, kept as it is.
    Own(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingTarget {
    pub folders: Vec<FolderPiece>,
    pub file_name: FileName,
}

/// Why a name typed for a folder is not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderNameProblem {
    Empty,
    /// "." and ".." name no folder of their own.
    Dots,
    /// A separator, a brace, a reserved or a control character.
    Character,
}

impl FolderNameProblem {
    pub fn text(self, lang: Language) -> &'static str {
        lang.text(match self {
            Self::Empty => Msg::FilingPiecesNameEmpty,
            Self::Dots => Msg::FilingPiecesNameDots,
            Self::Character => Msg::FilingPiecesNameCharacter,
        })
    }
}

/// The folder name typed by the user, trimmed, or what is wrong with it.
pub fn own_folder_name(input: &str) -> Result<String, FolderNameProblem> {
    let name = input.trim();
    if name.is_empty() {
        Err(FolderNameProblem::Empty)
    } else if name.trim_matches(['.', ' ']).is_empty() {
        Err(FolderNameProblem::Dots)
    } else if name.contains(['{', '}']) || name.ends_with('.') || !is_plain_literal(name) {
        Err(FolderNameProblem::Character)
    } else {
        Ok(name.to_owned())
    }
}

impl FilingTarget {
    pub fn parse(pattern: &str) -> Self {
        let mut segments: Vec<&str> = pattern.split('/').collect();
        let last = segments.pop().unwrap_or_default();
        let file_name = if last.is_empty() {
            FileName::Automatic
        } else {
            FileName::Own(last.to_owned())
        };
        if matches!(pattern, "" | "/") {
            segments.clear();
        }
        Self {
            folders: segments.into_iter().map(parse_folder).collect(),
            file_name,
        }
    }

    pub fn to_pattern(&self) -> String {
        let mut pattern = String::new();
        for folder in &self.folders {
            pattern.push_str(&match folder {
                FolderPiece::Named(name) | FolderPiece::Mixed(name) => name.clone(),
                FolderPiece::Field(field) => format!("{{{}}}", field.placeholder()),
            });
            pattern.push('/');
        }
        match &self.file_name {
            FileName::Automatic if self.folders.is_empty() => "/".to_owned(),
            FileName::Automatic => pattern,
            FileName::Own(segment) => pattern + segment,
        }
    }

    /// The target in words, for example "Firma A / Year / Sender"; a file
    /// name of its own follows after a dot.
    pub fn in_words(&self, lang: Language) -> String {
        let mut words = self
            .folders
            .iter()
            .map(|folder| match folder {
                FolderPiece::Named(name) => name.clone(),
                FolderPiece::Field(field) => field.name(lang).to_owned(),
                FolderPiece::Mixed(segment) => segment_in_words(segment, lang),
            })
            .collect::<Vec<_>>()
            .join(" / ");
        if let FileName::Own(segment) = &self.file_name {
            let name = segment_in_words(segment, lang);
            if words.is_empty() {
                words = name;
            } else {
                words.push_str(" · ");
                words.push_str(lang.text(Msg::FilingPiecesFileNamePrefix));
                words.push_str(&name);
            }
        }
        words
    }

    /// The file name part in words: "Date_Sender_Number" for the automatic one.
    pub fn file_name_in_words(&self, lang: Language) -> String {
        match &self.file_name {
            FileName::Automatic => {
                let [date, sender, number] =
                    [Word::Date, Word::Sender, Word::Number].map(|word| word.text(lang));
                format!("{date}_{sender}_{number}")
            }
            FileName::Own(segment) => segment_in_words(segment, lang),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.file_name == FileName::Automatic
    }

    /// The path of the example document, for example
    /// "Firma A GmbH/2026/Telekom/2026-03-09_Telekom_RE-42.pdf".
    pub fn example(&self, lang: Language) -> Option<String> {
        let fields = DocumentFields {
            sender: Some("Telekom".to_owned()),
            recipient: Some(lang.text(Msg::FilingPiecesExampleRecipient).to_owned()),
            kind: Some(lang.text(Msg::FilingPiecesExampleKind).to_owned()),
            date: NaiveDate::from_ymd_opt(2026, 3, 9),
            number: Some("RE-42".to_owned()),
            ..DocumentFields::default()
        };
        let path = render_pattern(&self.to_pattern(), &fields)?;
        let parts: Vec<_> = path
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect();
        Some(format!("{}.pdf", parts.join("/")))
    }
}

/// What the user does with the target in the widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetEdit {
    Remove(usize),
    AddField(FolderField),
    /// Opens the field for the name of a folder of its own.
    StartOwnName,
    OwnNameTyped(String),
    OwnNameSubmit,
    /// Closes the field without adding the folder.
    OwnNameCancel,
}

/// A target being edited, with the folder name being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetEditor {
    pub target: FilingTarget,
    /// The text of the field for an own folder name while it is open.
    pub own_name: Option<String>,
    pub name_problem: Option<FolderNameProblem>,
}

impl TargetEditor {
    pub fn new(pattern: &str) -> Self {
        Self {
            target: FilingTarget::parse(pattern),
            own_name: None,
            name_problem: None,
        }
    }

    pub fn pattern(&self) -> String {
        self.target.to_pattern()
    }

    /// Whether the target changed.
    pub fn apply(&mut self, edit: TargetEdit) -> bool {
        match edit {
            TargetEdit::Remove(position) => {
                if position < self.target.folders.len() {
                    self.target.folders.remove(position);
                    return true;
                }
            }
            TargetEdit::AddField(field) => {
                self.target.folders.push(FolderPiece::Field(field));
                return true;
            }
            TargetEdit::StartOwnName => {
                self.own_name.get_or_insert_with(String::new);
                self.name_problem = None;
            }
            TargetEdit::OwnNameTyped(typed) => {
                self.own_name = Some(typed);
                self.name_problem = None;
            }
            TargetEdit::OwnNameCancel => {
                self.own_name = None;
                self.name_problem = None;
            }
            TargetEdit::OwnNameSubmit => {
                let Some(typed) = self.own_name.as_deref() else {
                    return false;
                };
                match own_folder_name(typed) {
                    Ok(name) => {
                        self.target.folders.push(FolderPiece::Named(name));
                        self.own_name = None;
                        self.name_problem = None;
                        return true;
                    }
                    Err(problem) => self.name_problem = Some(problem),
                }
            }
        }
        false
    }

    /// The target is something a rule can file by.
    pub fn is_usable(&self) -> bool {
        !self.target.is_empty() && ncrs_intake::pattern_is_valid(&self.pattern())
    }
}

fn parse_folder(segment: &str) -> FolderPiece {
    let single_placeholder = segment
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .filter(|name| !name.contains(['{', '}']));
    match single_placeholder {
        Some(name) => FolderField::of_placeholder(name).map_or_else(
            || FolderPiece::Mixed(segment.to_owned()),
            FolderPiece::Field,
        ),
        None if segment.contains('{') => FolderPiece::Mixed(segment.to_owned()),
        None => FolderPiece::Named(segment.to_owned()),
    }
}

#[derive(Debug, Clone, Copy)]
enum Word {
    Date,
    Sender,
    Number,
    Field(FolderField),
    Day,
}

impl Word {
    fn of_placeholder(name: &str) -> Option<Self> {
        Some(match name {
            "date" => Self::Date,
            "day" => Self::Day,
            "number" => Self::Number,
            "folder" => Self::Sender,
            "name" => Self::Field(FolderField::Kind),
            other => match FolderField::of_placeholder(other)? {
                FolderField::Sender => Self::Sender,
                field => Self::Field(field),
            },
        })
    }

    fn text(self, lang: Language) -> &'static str {
        match self {
            Self::Date => lang.text(Msg::FilingPiecesFieldDate),
            Self::Sender => FolderField::Sender.name(lang),
            Self::Number => lang.text(Msg::FilingPiecesFieldNumber),
            Self::Day => lang.text(Msg::FilingPiecesFieldDay),
            Self::Field(field) => field.name(lang),
        }
    }
}

/// A segment of a pattern with the blocks replaced by their field names; the
/// run `{year}{month}{day}` reads as the date. No braces are left.
fn segment_in_words(segment: &str, lang: Language) -> String {
    let with_date = segment.replace("{year}{month}{day}", "{date}");
    let mut words = String::new();
    let mut rest = with_date.as_str();
    while let Some((before, from)) = rest.split_once('{') {
        words.push_str(before);
        match from.split_once('}') {
            Some((name, after)) => {
                if let Some(word) = Word::of_placeholder(name) {
                    words.push_str(word.text(lang));
                }
                rest = after;
            }
            None => {
                rest = from;
            }
        }
    }
    words.push_str(rest);
    words.replace('}', "")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use ncrs_intake::{pattern_is_valid, DEFAULT_PATTERNS, FILING_PLACEHOLDERS};

    use super::*;

    const OLD_PATTERNS: [&str; 6] = [
        "{year}/{year}{month}{day}_{sender}_{number}",
        "{year}/{sender}/",
        "{kind}/{year}-{month}/{date}_{number}",
        "Post/{year}/{sender}",
        "{sender}_{recipient}/{date}",
        "{year}/{year}{month}{day}_{sender}",
    ];

    #[test]
    fn a_pattern_comes_back_exactly_as_it_was() {
        for pattern in DEFAULT_PATTERNS.iter().chain(OLD_PATTERNS.iter()) {
            assert_eq!(FilingTarget::parse(pattern).to_pattern(), *pattern);
        }
    }

    #[test]
    fn a_trailing_slash_makes_the_file_name_automatic() {
        let target = FilingTarget::parse("Firma A/{year}/{sender}/");

        assert_eq!(
            target.folders,
            [
                FolderPiece::Named("Firma A".to_owned()),
                FolderPiece::Field(FolderField::Year),
                FolderPiece::Field(FolderField::Sender),
            ]
        );
        assert_eq!(target.file_name, FileName::Automatic);
    }

    #[test]
    fn a_folder_of_several_parts_and_a_file_name_of_its_own_stay_whole() {
        let target = FilingTarget::parse("{year}-{month}/{date}_{number}");

        assert_eq!(
            target.folders,
            [FolderPiece::Mixed("{year}-{month}".to_owned())]
        );
        assert_eq!(
            target.file_name,
            FileName::Own("{date}_{number}".to_owned())
        );
    }

    #[test]
    fn the_target_reads_in_words_without_braces() {
        assert_eq!(
            FilingTarget::parse("Firma A/{year}/{sender}/").in_words(Language::German),
            "Firma A / Jahr / Absender"
        );
        assert_eq!(
            FilingTarget::parse("{year}/{sender}/").in_words(Language::English),
            "Year / Sender"
        );
        assert_eq!(
            FilingTarget::parse("{year}/{year}{month}{day}_{sender}_{number}")
                .in_words(Language::German),
            "Jahr · Dateiname: Datum_Absender_Nummer"
        );
    }

    #[test]
    fn no_block_leaves_a_brace_in_the_words() {
        for lang in [Language::English, Language::German] {
            for name in FILING_PLACEHOLDERS {
                for pattern in [
                    format!("{{{name}}}/"),
                    format!("{{{name}}}"),
                    format!("x{{{name}}}y/{{{name}}}z"),
                ] {
                    let target = FilingTarget::parse(&pattern);
                    for words in [target.in_words(lang), target.file_name_in_words(lang)] {
                        assert!(!words.contains(['{', '}']), "{pattern}: {words}");
                    }
                }
            }
            assert!(!FilingTarget::parse("{nope}/{year")
                .in_words(lang)
                .contains(['{', '}']));
        }
    }

    #[test]
    fn the_example_is_the_rendered_path() {
        let target = FilingTarget::parse("Firma A/{year}/{sender}/");

        assert_eq!(
            target.example(Language::German).as_deref(),
            Some("Firma A/2026/Telekom/2026-03-09_Telekom_RE-42.pdf")
        );
        assert_eq!(
            FilingTarget::parse("{recipient}/{year}/")
                .example(Language::English)
                .as_deref(),
            Some("Company A Ltd/2026/2026-03-09_Telekom_RE-42.pdf")
        );
    }

    #[test]
    fn every_built_in_target_gives_a_valid_pattern() {
        for pattern in DEFAULT_PATTERNS {
            let target = FilingTarget::parse(pattern);
            assert!(pattern_is_valid(&target.to_pattern()), "{pattern}");
            assert!(target.example(Language::English).is_some(), "{pattern}");
        }
    }

    #[test]
    fn an_own_folder_name_is_trimmed_and_checked() {
        assert_eq!(own_folder_name("  Firma A "), Ok("Firma A".to_owned()));
        assert_eq!(own_folder_name("   "), Err(FolderNameProblem::Empty));
        assert_eq!(own_folder_name(".."), Err(FolderNameProblem::Dots));
        assert_eq!(own_folder_name("."), Err(FolderNameProblem::Dots));
        assert_eq!(own_folder_name("..."), Err(FolderNameProblem::Dots));
        assert_eq!(own_folder_name(" . . "), Err(FolderNameProblem::Dots));
        for bad in [
            "a/b",
            "a\\b",
            "{year}",
            "a}",
            "a\u{7}b",
            "a\nb",
            "a<b",
            "a>b",
            "a:b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "x\u{202E}gpj",
            "name.",
        ] {
            assert_eq!(
                own_folder_name(bad),
                Err(FolderNameProblem::Character),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn folders_are_added_and_removed_and_an_own_name_is_checked_first() {
        let mut editor = TargetEditor::new("{year}/");

        assert!(editor.apply(TargetEdit::AddField(FolderField::Sender)));
        assert_eq!(editor.pattern(), "{year}/{sender}/");
        assert!(!editor.apply(TargetEdit::StartOwnName));
        assert!(!editor.apply(TargetEdit::OwnNameTyped("a/b".to_owned())));
        assert!(!editor.apply(TargetEdit::OwnNameSubmit));
        assert_eq!(editor.name_problem, Some(FolderNameProblem::Character));
        assert_eq!(editor.pattern(), "{year}/{sender}/");
        assert!(!editor.apply(TargetEdit::OwnNameTyped(" Firma A ".to_owned())));
        assert_eq!(editor.name_problem, None);
        assert!(editor.apply(TargetEdit::OwnNameSubmit));
        assert_eq!(editor.pattern(), "{year}/{sender}/Firma A/");
        assert_eq!(editor.own_name, None);
        assert!(editor.apply(TargetEdit::Remove(0)));
        assert!(!editor.apply(TargetEdit::Remove(9)));
        assert_eq!(editor.pattern(), "{sender}/Firma A/");
        assert!(editor.is_usable());
        editor.apply(TargetEdit::Remove(0));
        editor.apply(TargetEdit::Remove(0));
        assert!(!editor.is_usable());
    }

    #[test]
    fn no_folder_and_no_file_name_is_no_target() {
        let empty = FilingTarget::parse("");

        assert!(empty.is_empty());
        assert!(!pattern_is_valid(&empty.to_pattern()));
        assert!(!pattern_is_valid(&FilingTarget::parse("/").to_pattern()));
    }
}
