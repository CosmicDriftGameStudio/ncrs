//! The known document kinds: one stable ID each, shown as a word of the
//! language in use. Any other kind stays free text.

use std::borrow::Cow;

use crate::model::tag_name;

/// The language a kind is shown in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KindLanguage {
    #[default]
    English,
    German,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Invoice,
    CreditNote,
    Reminder,
    Contract,
    Notice,
    Receipt,
    Letter,
    Insurance,
    Quote,
    DeliveryNote,
    Termination,
}

struct Words {
    kind: Kind,
    id: &'static str,
    english: &'static str,
    german: &'static str,
    synonyms: &'static [&'static str],
}

const WORDS: [Words; 11] = [
    Words {
        kind: Kind::Invoice,
        id: "invoice",
        english: "Invoice",
        german: "Rechnung",
        synonyms: &[],
    },
    Words {
        kind: Kind::CreditNote,
        id: "credit_note",
        english: "Credit note",
        german: "Gutschrift",
        synonyms: &[],
    },
    Words {
        kind: Kind::Reminder,
        id: "reminder",
        english: "Reminder",
        german: "Mahnung",
        synonyms: &["Zahlungserinnerung"],
    },
    Words {
        kind: Kind::Contract,
        id: "contract",
        english: "Contract",
        german: "Vertrag",
        synonyms: &[],
    },
    Words {
        kind: Kind::Notice,
        id: "notice",
        english: "Notice",
        german: "Bescheid",
        synonyms: &[],
    },
    Words {
        kind: Kind::Receipt,
        id: "receipt",
        english: "Receipt",
        german: "Beleg",
        synonyms: &["Quittung"],
    },
    Words {
        kind: Kind::Letter,
        id: "letter",
        english: "Letter",
        german: "Brief",
        synonyms: &[],
    },
    Words {
        kind: Kind::Insurance,
        id: "insurance",
        english: "Insurance",
        german: "Versicherung",
        synonyms: &[],
    },
    Words {
        kind: Kind::Quote,
        id: "quote",
        english: "Quote",
        german: "Angebot",
        synonyms: &[],
    },
    Words {
        kind: Kind::DeliveryNote,
        id: "delivery_note",
        english: "Delivery note",
        german: "Lieferschein",
        synonyms: &[],
    },
    Words {
        kind: Kind::Termination,
        id: "termination",
        english: "Termination",
        german: "Kündigung",
        synonyms: &[],
    },
];

impl Kind {
    pub const ALL: [Kind; 11] = [
        Kind::Invoice,
        Kind::CreditNote,
        Kind::Reminder,
        Kind::Contract,
        Kind::Notice,
        Kind::Receipt,
        Kind::Letter,
        Kind::Insurance,
        Kind::Quote,
        Kind::DeliveryNote,
        Kind::Termination,
    ];

    fn words(self) -> &'static Words {
        let [invoice, credit_note, reminder, contract, notice, receipt, letter, insurance, quote, delivery_note, termination] =
            &WORDS;
        match self {
            Kind::Invoice => invoice,
            Kind::CreditNote => credit_note,
            Kind::Reminder => reminder,
            Kind::Contract => contract,
            Kind::Notice => notice,
            Kind::Receipt => receipt,
            Kind::Letter => letter,
            Kind::Insurance => insurance,
            Kind::Quote => quote,
            Kind::DeliveryNote => delivery_note,
            Kind::Termination => termination,
        }
    }

    /// The stable ID that is stored for the kind.
    pub fn id(self) -> &'static str {
        self.words().id
    }

    pub fn word(self, language: KindLanguage) -> &'static str {
        match language {
            KindLanguage::English => self.words().english,
            KindLanguage::German => self.words().german,
        }
    }

    /// The known kind a text names, whatever its case, spaces and `_`/`-`.
    pub fn resolve(text: &str) -> Option<Kind> {
        let name = tag_name(text);
        if name.is_empty() {
            return None;
        }
        WORDS
            .iter()
            .find(|words| words.spellings().any(|spelling| tag_name(spelling) == name))
            .map(|words| words.kind)
    }

    /// The tag the kind makes (`credit-note`).
    pub fn tag(self) -> String {
        tag_name(self.id())
    }

    /// The tag of every spelling of the kind, in either language.
    pub fn tag_names(self) -> Vec<String> {
        let mut names: Vec<String> = self.words().spellings().map(tag_name).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The lower-cased names the kind can be stored under in older rows.
    pub fn name_keys(self) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for spelling in self.words().spellings() {
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

impl Words {
    fn spellings(&self) -> impl Iterator<Item = &'static str> {
        [self.id, self.english, self.german]
            .into_iter()
            .chain(self.synonyms.iter().copied())
    }
}

/// What is stored for a kind: the ID of a known kind, otherwise the text.
pub fn stored_kind(text: &str) -> String {
    Kind::resolve(text).map_or_else(|| text.trim().to_owned(), |kind| kind.id().to_owned())
}

/// What is written for a kind that was detected or answered: like
/// `stored_kind`, and a German word ending in "bescheid" is a notice. Only
/// writing uses the suffix; matching knows the exact words alone.
pub fn detected_kind(text: &str) -> String {
    let stored = stored_kind(text);
    if Kind::resolve(&stored).is_none() && tag_name(&stored).ends_with(NOTICE_SUFFIX) {
        return Kind::Notice.id().to_owned();
    }
    stored
}

const NOTICE_SUFFIX: &str = "bescheid";

/// A kind as shown: the word of the language for a known kind, otherwise
/// the text as typed.
pub fn kind_word(text: &str, language: KindLanguage) -> Cow<'_, str> {
    Kind::resolve(text).map_or(Cow::Borrowed(text), |kind| {
        Cow::Borrowed(kind.word(language))
    })
}

/// A tag as shown: a known kind in the word of the language, in tag form.
pub fn kind_tag_word(tag: &str, language: KindLanguage) -> String {
    Kind::resolve(tag).map_or_else(|| tag.to_owned(), |kind| tag_name(kind.word(language)))
}

/// What two kinds are compared by: the ID of a known kind, otherwise the
/// trimmed lower-case text.
pub fn kind_key(text: &str) -> String {
    Kind::resolve(text).map_or_else(|| text.trim().to_lowercase(), |kind| kind.id().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_synonyms_and_ids_of_both_languages_name_one_kind() {
        for text in [
            "credit_note",
            "Credit note",
            "credit-note",
            "CREDIT  NOTE",
            "Gutschrift",
            "gutschrift",
        ] {
            assert_eq!(Kind::resolve(text), Some(Kind::CreditNote), "{text}");
        }
        for (text, kind) in [
            ("Quittung", Kind::Receipt),
            ("Beleg", Kind::Receipt),
            ("Rechnung", Kind::Invoice),
            ("invoice", Kind::Invoice),
            ("Angebot", Kind::Quote),
            ("Lieferschein", Kind::DeliveryNote),
            ("delivery-note", Kind::DeliveryNote),
            ("Kündigung", Kind::Termination),
        ] {
            assert_eq!(Kind::resolve(text), Some(kind), "{text}");
        }
    }

    #[test]
    fn an_unknown_kind_stays_as_typed() {
        assert_eq!(Kind::resolve("Utility bill"), None);
        assert_eq!(Kind::resolve(""), None);
        assert_eq!(stored_kind("  Utility bill "), "Utility bill");
        assert_eq!(
            kind_word("Utility bill", KindLanguage::German),
            "Utility bill"
        );
    }

    #[test]
    fn a_known_kind_is_stored_as_its_id_and_shown_as_its_word() {
        assert_eq!(stored_kind("Rechnung"), "invoice");
        assert_eq!(stored_kind("Credit note"), "credit_note");
        assert_eq!(kind_word("invoice", KindLanguage::German), "Rechnung");
        assert_eq!(kind_word("Rechnung", KindLanguage::English), "Invoice");
        assert_eq!(
            kind_tag_word("credit-note", KindLanguage::German),
            "gutschrift"
        );
        assert_eq!(kind_tag_word("firma-a", KindLanguage::German), "firma-a");
        assert_eq!(kind_key("Rechnung"), kind_key("INVOICE"));
    }

    #[test]
    fn every_kind_has_distinct_words_and_resolves_to_itself() {
        for kind in Kind::ALL {
            assert_eq!(Kind::resolve(kind.id()), Some(kind));
            assert_eq!(Kind::resolve(kind.word(KindLanguage::English)), Some(kind));
            assert_eq!(Kind::resolve(kind.word(KindLanguage::German)), Some(kind));
            assert!(kind.tag_names().contains(&kind.tag()));
        }
    }
}
