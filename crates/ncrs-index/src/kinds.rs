//! The document kinds: one stable ID each, shown as a word of the language in
//! use. The built-in kinds can be extended by the config. Any other kind
//! stays free text.

use std::borrow::Cow;
use std::sync::Arc;

use crate::model::tag_name;

/// The language a kind is shown in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KindLanguage {
    #[default]
    English,
    German,
}

struct BuiltIn {
    id: &'static str,
    english: &'static str,
    german: &'static str,
    synonyms: &'static [&'static str],
}

const BUILT_IN: [BuiltIn; 11] = [
    BuiltIn {
        id: "invoice",
        english: "Invoice",
        german: "Rechnung",
        synonyms: &[],
    },
    BuiltIn {
        id: "credit_note",
        english: "Credit note",
        german: "Gutschrift",
        synonyms: &[],
    },
    BuiltIn {
        id: "reminder",
        english: "Reminder",
        german: "Mahnung",
        synonyms: &["Zahlungserinnerung"],
    },
    BuiltIn {
        id: "contract",
        english: "Contract",
        german: "Vertrag",
        synonyms: &[],
    },
    BuiltIn {
        id: "notice",
        english: "Notice",
        german: "Bescheid",
        synonyms: &[],
    },
    BuiltIn {
        id: "receipt",
        english: "Receipt",
        german: "Beleg",
        synonyms: &["Quittung"],
    },
    BuiltIn {
        id: "letter",
        english: "Letter",
        german: "Brief",
        synonyms: &[],
    },
    BuiltIn {
        id: "insurance",
        english: "Insurance",
        german: "Versicherung",
        synonyms: &[],
    },
    BuiltIn {
        id: "quote",
        english: "Quote",
        german: "Angebot",
        synonyms: &[],
    },
    BuiltIn {
        id: "delivery_note",
        english: "Delivery note",
        german: "Lieferschein",
        synonyms: &[],
    },
    BuiltIn {
        id: "termination",
        english: "Termination",
        german: "Kündigung",
        synonyms: &[],
    },
];

/// Words a heading starts with, and the kind they name. Other words of a
/// kind ("Beleg", "Brief") are too common at the start of a line.
const BUILT_IN_HEADINGS: [(&str, &str); 12] = [
    ("rechnung", "invoice"),
    ("gutschrift", "credit_note"),
    ("mahnung", "reminder"),
    ("vertrag", "contract"),
    ("kündigung", "termination"),
    ("angebot", "quote"),
    ("lieferschein", "delivery_note"),
    ("invoice", "invoice"),
    ("reminder", "reminder"),
    ("contract", "contract"),
    ("quote", "quote"),
    ("credit note", "credit_note"),
];

/// The most words of a heading that name a kind together.
const HEADING_WORD_LIMIT: usize = 3;

const NOTICE_ID: &str = "notice";
const NOTICE_SUFFIX: &str = "bescheid";

/// One kind: its ID, a word per language and further words that name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindEntry {
    id: String,
    english: String,
    german: String,
    synonyms: Vec<String>,
}

impl KindEntry {
    /// The stable ID that is stored for the kind.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn word(&self, language: KindLanguage) -> &str {
        match language {
            KindLanguage::English => &self.english,
            KindLanguage::German => &self.german,
        }
    }

    fn spellings(&self) -> impl Iterator<Item = &str> {
        [&self.id, &self.english, &self.german]
            .into_iter()
            .chain(&self.synonyms)
            .map(String::as_str)
    }

    fn names(&self) -> impl Iterator<Item = String> + '_ {
        self.spellings().map(tag_name)
    }

    fn is_named(&self, name: &str) -> bool {
        self.names().any(|spelling| spelling == name)
    }

    /// The tag the kind makes (`credit-note`).
    pub fn tag(&self) -> String {
        tag_name(&self.id)
    }

    /// The tag of every spelling of the kind, in either language.
    pub fn tag_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.names().filter(|name| !name.is_empty()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The lower-cased names the kind can be stored under in older rows.
    pub fn name_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for spelling in self.spellings() {
            let tag = tag_name(spelling);
            keys.push(spelling.to_lowercase());
            keys.push(tag.replace('-', "_"));
            keys.push(tag);
        }
        keys.sort();
        keys.dedup();
        keys
    }
}

/// One `[[kinds]]` entry of the config, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomKind {
    pub id: String,
    pub german: Option<String>,
    pub english: Option<String>,
    pub words: Vec<String>,
}

/// What `KindCatalog::extended` left out of the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindSkip {
    /// An entry whose ID is empty once normalised.
    EmptyId,
    /// A word (or an ID) that already names another kind.
    Word { id: String, word: String },
}

/// Every known kind: the built-in ones plus those of the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindCatalog {
    entries: Vec<KindEntry>,
    /// Tag form of a heading start and the ID of the kind it names.
    headings: Vec<(String, String)>,
}

impl Default for KindCatalog {
    fn default() -> Self {
        Self::built_in()
    }
}

impl KindCatalog {
    pub fn built_in() -> Self {
        let entries = BUILT_IN
            .iter()
            .map(|kind| KindEntry {
                id: kind.id.to_owned(),
                english: kind.english.to_owned(),
                german: kind.german.to_owned(),
                synonyms: kind
                    .synonyms
                    .iter()
                    .map(|word| (*word).to_owned())
                    .collect(),
            })
            .collect();
        let headings = BUILT_IN_HEADINGS
            .iter()
            .map(|(heading, id)| (tag_name(heading), (*id).to_owned()))
            .collect();
        Self { entries, headings }
    }

    pub fn entries(&self) -> &[KindEntry] {
        &self.entries
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(KindEntry::id)
    }

    /// This catalog with the entries of the config applied. An entry with a
    /// known ID adds its words (and replaces the shown words when given);
    /// any other ID makes a new kind. Entries that would name one word for
    /// two kinds are left out and returned.
    pub fn extended(&self, custom: &[CustomKind]) -> (Self, Vec<KindSkip>) {
        let mut entries = self.entries.clone();
        let mut headings = self.headings.clone();
        let mut skips = Vec::new();
        for item in custom {
            let name = tag_name(&item.id);
            if name.is_empty() {
                skips.push(KindSkip::EmptyId);
                continue;
            }
            let position = match entries.iter().position(|entry| tag_name(&entry.id) == name) {
                Some(position) => position,
                None if entries.iter().any(|entry| entry.is_named(&name)) => {
                    skips.push(KindSkip::Word {
                        id: name.clone(),
                        word: item.id.trim().to_owned(),
                    });
                    continue;
                }
                None => {
                    entries.push(KindEntry {
                        id: name.clone(),
                        english: String::new(),
                        german: String::new(),
                        synonyms: Vec::new(),
                    });
                    entries.len() - 1
                }
            };
            let mut accept = |word: &str, known: &[KindEntry]| -> bool {
                let word_name = tag_name(word);
                if word_name.is_empty() {
                    return false;
                }
                let taken = known
                    .iter()
                    .enumerate()
                    .any(|(other, entry)| other != position && entry.is_named(&word_name));
                if taken {
                    skips.push(KindSkip::Word {
                        id: name.clone(),
                        word: word.trim().to_owned(),
                    });
                }
                !taken
            };
            let english = item
                .english
                .as_deref()
                .filter(|word| accept(word, &entries))
                .map(|word| word.trim().to_owned());
            let german = item
                .german
                .as_deref()
                .filter(|word| accept(word, &entries))
                .map(|word| word.trim().to_owned());
            let words: Vec<String> = item
                .words
                .iter()
                .filter(|word| accept(word, &entries))
                .map(|word| word.trim().to_owned())
                .collect();
            let Some(entry) = entries.get_mut(position) else {
                continue;
            };
            let id = entry.id.clone();
            let is_new = entry.english.is_empty();
            headings.extend(
                english
                    .iter()
                    .chain(&german)
                    .chain(&words)
                    .chain(is_new.then_some(&id))
                    .map(|word| (tag_name(word), id.clone())),
            );
            entry.apply(english, german, words);
        }
        (Self { entries, headings }, skips)
    }

    /// The kind a text names, whatever its case, spaces and `_`/`-`.
    pub fn resolve(&self, text: &str) -> Option<&KindEntry> {
        let name = tag_name(text);
        if name.is_empty() {
            return None;
        }
        self.entries.iter().find(|entry| entry.is_named(&name))
    }

    /// The kind a heading line names: it starts with a heading word of a
    /// kind. Running text never counts, only the start of a line. A German
    /// first word ending in "bescheid" is a notice; only writing uses that.
    pub fn heading_kind(&self, line: &str) -> Option<String> {
        let words: Vec<String> = line
            .split_whitespace()
            .map(|word| {
                word.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .collect();
        for count in (1..=HEADING_WORD_LIMIT).rev() {
            let start: Vec<&str> = words.iter().take(count).map(String::as_str).collect();
            let name = tag_name(&start.join(" "));
            if let Some((_, id)) = self.headings.iter().find(|(heading, _)| *heading == name) {
                return Some(id.clone());
            }
        }
        let first = words.first()?;
        first.ends_with(NOTICE_SUFFIX).then(|| NOTICE_ID.to_owned())
    }

    /// What is stored for a kind: the ID of a known kind, otherwise the text.
    pub fn stored_kind(&self, text: &str) -> String {
        self.resolve(text)
            .map_or_else(|| text.trim().to_owned(), |entry| entry.id.clone())
    }

    /// What is written for a kind that was detected or answered: like
    /// `stored_kind`, and a German word ending in "bescheid" is a notice.
    /// Only writing uses the suffix; matching knows the exact words alone.
    pub fn detected_kind(&self, text: &str) -> String {
        let stored = self.stored_kind(text);
        if self.resolve(&stored).is_none() && tag_name(&stored).ends_with(NOTICE_SUFFIX) {
            return NOTICE_ID.to_owned();
        }
        stored
    }

    /// What two kinds are compared by: the ID of a known kind, otherwise the
    /// trimmed lower-case text.
    pub fn kind_key(&self, text: &str) -> String {
        self.resolve(text)
            .map_or_else(|| text.trim().to_lowercase(), |entry| entry.id.clone())
    }

    /// The lower-cased names a kind can be stored under in rows: every
    /// spelling of a known kind, otherwise the trimmed text.
    pub fn kind_name_keys(&self, text: &str) -> Vec<String> {
        self.resolve(text)
            .map_or_else(|| vec![text.trim().to_lowercase()], KindEntry::name_keys)
    }

    /// The tag of every spelling of the kind a tag names; none for any
    /// other tag.
    pub fn tag_spellings(&self, tag: &str) -> Vec<String> {
        self.resolve(tag)
            .map_or_else(Vec::new, KindEntry::tag_names)
    }

    /// A kind as shown: the word of the language for a known kind, otherwise
    /// the text as typed.
    pub fn kind_word<'a>(&'a self, text: &'a str, language: KindLanguage) -> Cow<'a, str> {
        self.resolve(text).map_or(Cow::Borrowed(text), |entry| {
            Cow::Borrowed(entry.word(language))
        })
    }

    /// A tag as shown: a known kind in the word of the language, in tag form.
    pub fn kind_tag_word(&self, tag: &str, language: KindLanguage) -> String {
        self.resolve(tag)
            .map_or_else(|| tag.to_owned(), |entry| tag_name(entry.word(language)))
    }

    /// The tag a kind makes: the tag of its ID, otherwise of the text.
    pub fn kind_tag(&self, text: &str) -> String {
        self.resolve(text)
            .map_or_else(|| tag_name(text), KindEntry::tag)
    }

    /// Whether `tag` is `wanted` or a more specific tag below it
    /// (`firma-a-gmbh` matches `firma-a`, `firma-ab` does not). Two spellings
    /// of one known kind match each other.
    pub fn tag_matches(&self, tag: &str, wanted: &str) -> bool {
        let wanted_name = tag_name(wanted);
        if wanted_name.is_empty() {
            return false;
        }
        if let Some(kind) = self.resolve(wanted) {
            if self.resolve(tag) == Some(kind) {
                return true;
            }
        }
        let tag_key = tag_name(tag);
        tag_key == wanted_name || tag_key.starts_with(&format!("{wanted_name}-"))
    }
}

impl KindEntry {
    /// A new kind shows the word it has for one language in the other too,
    /// and its ID when it has none. A known kind keeps what it shows unless
    /// a word is given; the replaced word stays a synonym, so rows stored
    /// under it still resolve.
    fn apply(&mut self, english: Option<String>, german: Option<String>, words: Vec<String>) {
        if self.english.is_empty() {
            self.english = english
                .clone()
                .or_else(|| german.clone())
                .unwrap_or_else(|| self.id.clone());
            self.german = german.or(english).unwrap_or_else(|| self.id.clone());
        } else {
            if let Some(word) = english {
                let replaced = std::mem::replace(&mut self.english, word);
                self.add_synonym(replaced);
            }
            if let Some(word) = german {
                let replaced = std::mem::replace(&mut self.german, word);
                self.add_synonym(replaced);
            }
        }
        for word in words {
            self.add_synonym(word);
        }
    }

    fn add_synonym(&mut self, word: String) {
        if !self.is_named(&tag_name(&word)) {
            self.synonyms.push(word);
        }
    }
}

/// The catalog with the language to show kinds in; what the screens carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kinds {
    pub catalog: Arc<KindCatalog>,
    pub language: KindLanguage,
}

impl Kinds {
    pub fn new(catalog: Arc<KindCatalog>, language: KindLanguage) -> Self {
        Self { catalog, language }
    }

    /// The built-in kinds only; for tests.
    pub fn built_in(language: KindLanguage) -> Self {
        Self::new(Arc::new(KindCatalog::built_in()), language)
    }

    pub fn in_language(&self, language: KindLanguage) -> Self {
        Self::new(Arc::clone(&self.catalog), language)
    }

    /// A kind as shown: the word of the language for a known kind, otherwise
    /// the text as typed.
    pub fn kind_word<'a>(&'a self, text: &'a str) -> Cow<'a, str> {
        self.catalog.kind_word(text, self.language)
    }

    /// A tag as shown: a known kind in the word of the language, in tag form.
    pub fn kind_tag_word(&self, tag: &str) -> String {
        self.catalog.kind_tag_word(tag, self.language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(id: &str, german: Option<&str>, english: Option<&str>, words: &[&str]) -> CustomKind {
        CustomKind {
            id: id.to_owned(),
            german: german.map(str::to_owned),
            english: english.map(str::to_owned),
            words: words.iter().map(|word| (*word).to_owned()).collect(),
        }
    }

    fn id_of(catalog: &KindCatalog, text: &str) -> Option<String> {
        catalog.resolve(text).map(|entry| entry.id().to_owned())
    }

    #[test]
    fn words_synonyms_and_ids_of_both_languages_name_one_kind() {
        let catalog = KindCatalog::built_in();
        for text in [
            "credit_note",
            "Credit note",
            "credit-note",
            "CREDIT  NOTE",
            "Gutschrift",
            "gutschrift",
        ] {
            assert_eq!(
                id_of(&catalog, text).as_deref(),
                Some("credit_note"),
                "{text}"
            );
        }
        for (text, id) in [
            ("Quittung", "receipt"),
            ("Beleg", "receipt"),
            ("Rechnung", "invoice"),
            ("invoice", "invoice"),
            ("Angebot", "quote"),
            ("Lieferschein", "delivery_note"),
            ("delivery-note", "delivery_note"),
            ("Kündigung", "termination"),
        ] {
            assert_eq!(id_of(&catalog, text).as_deref(), Some(id), "{text}");
        }
    }

    #[test]
    fn an_unknown_kind_stays_as_typed() {
        let kinds = Kinds::built_in(KindLanguage::German);
        assert_eq!(kinds.catalog.resolve("Utility bill"), None);
        assert_eq!(kinds.catalog.resolve(""), None);
        assert_eq!(kinds.catalog.stored_kind("  Utility bill "), "Utility bill");
        assert_eq!(kinds.kind_word("Utility bill"), "Utility bill");
    }

    #[test]
    fn a_known_kind_is_stored_as_its_id_and_shown_as_its_word() {
        let german = Kinds::built_in(KindLanguage::German);
        let english = german.in_language(KindLanguage::English);
        assert_eq!(german.catalog.stored_kind("Rechnung"), "invoice");
        assert_eq!(german.catalog.stored_kind("Credit note"), "credit_note");
        assert_eq!(german.kind_word("invoice"), "Rechnung");
        assert_eq!(english.kind_word("Rechnung"), "Invoice");
        assert_eq!(german.kind_tag_word("credit-note"), "gutschrift");
        assert_eq!(german.kind_tag_word("firma-a"), "firma-a");
        assert_eq!(
            german.catalog.kind_key("Rechnung"),
            german.catalog.kind_key("INVOICE")
        );
    }

    #[test]
    fn every_kind_has_distinct_words_and_resolves_to_itself() {
        let catalog = KindCatalog::built_in();
        for entry in catalog.entries() {
            for language in [KindLanguage::English, KindLanguage::German] {
                assert_eq!(catalog.resolve(entry.word(language)), Some(entry));
            }
            assert_eq!(catalog.resolve(entry.id()), Some(entry));
            assert!(entry.tag_names().contains(&entry.tag()));
        }
    }

    #[test]
    fn a_custom_kind_is_new_and_a_known_id_gets_more_words() {
        let (catalog, skips) = KindCatalog::built_in().extended(&[
            custom(
                "payslip",
                Some("Lohnabrechnung"),
                Some("Payslip"),
                &["Gehaltsabrechnung"],
            ),
            custom("invoice", None, None, &["Honorarnote"]),
        ]);
        assert_eq!(skips, Vec::new());
        assert_eq!(
            id_of(&catalog, "Gehaltsabrechnung").as_deref(),
            Some("payslip")
        );
        assert_eq!(id_of(&catalog, "payslip").as_deref(), Some("payslip"));
        assert_eq!(id_of(&catalog, "Honorarnote").as_deref(), Some("invoice"));
        assert_eq!(id_of(&catalog, "Rechnung").as_deref(), Some("invoice"));
        let german = Kinds::new(Arc::new(catalog), KindLanguage::German);
        assert_eq!(german.kind_word("payslip"), "Lohnabrechnung");
        assert_eq!(german.kind_word("Honorarnote"), "Rechnung");
        assert!(german.catalog.tag_matches("honorarnote", "rechnung"));
    }

    #[test]
    fn a_custom_kind_falls_back_to_the_other_language_then_the_id() {
        let (catalog, _) = KindCatalog::built_in().extended(&[
            custom("payslip", Some("Lohnabrechnung"), None, &[]),
            custom("memo", None, None, &[]),
        ]);
        let english = Kinds::new(Arc::new(catalog), KindLanguage::English);
        assert_eq!(english.kind_word("payslip"), "Lohnabrechnung");
        assert_eq!(english.kind_word("memo"), "memo");
    }

    #[test]
    fn a_given_word_replaces_the_shown_one_but_the_old_one_still_resolves() {
        let (catalog, _) =
            KindCatalog::built_in().extended(&[custom("invoice", Some("Faktura"), None, &[])]);
        let german = Kinds::new(Arc::new(catalog), KindLanguage::German);
        assert_eq!(german.kind_word("invoice"), "Faktura");
        assert_eq!(german.catalog.stored_kind("Rechnung"), "invoice");
    }

    #[test]
    fn an_empty_id_and_a_word_of_two_kinds_are_skipped() {
        let (catalog, skips) = KindCatalog::built_in().extended(&[
            custom("  ", Some("Nichts"), None, &[]),
            custom(
                "payslip",
                Some("Lohnabrechnung"),
                None,
                &["Rechnung", "Entgelt"],
            ),
        ]);
        assert_eq!(
            skips,
            vec![
                KindSkip::EmptyId,
                KindSkip::Word {
                    id: "payslip".to_owned(),
                    word: "Rechnung".to_owned()
                }
            ]
        );
        assert_eq!(id_of(&catalog, "Rechnung").as_deref(), Some("invoice"));
        assert_eq!(id_of(&catalog, "Entgelt").as_deref(), Some("payslip"));
        assert_eq!(id_of(&catalog, "Nichts"), None);
    }

    #[test]
    fn the_bescheid_suffix_is_written_but_never_matched() {
        let (catalog, _) = KindCatalog::built_in().extended(&[]);
        assert_eq!(catalog.detected_kind("Steuerbescheid"), "notice");
        assert_eq!(catalog.stored_kind("Steuerbescheid"), "Steuerbescheid");
        assert!(!catalog.tag_matches("steuerbescheid", "bescheid"));
    }
}
