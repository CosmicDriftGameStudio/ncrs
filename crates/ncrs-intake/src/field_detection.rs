//! The fields the look of a letter gives: sender, recipient, kind and letter
//! date. Pure: the caller decides what to store and what to ask the user.

use chrono::NaiveDate;
use ncrs_index::{DocumentFields, KindCatalog, SenderAliases};

use crate::date_detection::detect_letter_date;
use crate::recipient_detection::{detect_recipient, kind_from_heading};
use crate::sender_detection::{detect_sender, AliasOffer, OwnIdentifiers};

/// Bump when the detection learns something new: archive documents are read
/// again.
pub const FIELDS_READ_VERSION: u32 = 2;

/// What belongs to the user and never to a sender.
#[derive(Debug, Clone, Copy)]
pub struct OwnNames<'a> {
    pub names: &'a [String],
    pub ibans: &'a [String],
    pub tax_ids: &'a [String],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedFields {
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub kind: Option<String>,
    pub date: Option<NaiveDate>,
    pub offers: Vec<AliasOffer>,
}

impl DetectedFields {
    /// `fields` with every empty field filled from what was detected.
    pub fn fill(&self, fields: &DocumentFields) -> DocumentFields {
        let mut filled = fields.clone();
        if filled.sender.is_none() {
            filled.sender.clone_from(&self.sender);
        }
        if filled.recipient.is_none() {
            filled.recipient.clone_from(&self.recipient);
        }
        if filled.kind.is_none() {
            filled.kind.clone_from(&self.kind);
        }
        if filled.date.is_none() {
            filled.date = self.date;
        }
        filled
    }
}

/// Detects only the fields that are empty in `current`.
pub fn detect_fields(
    text: &str,
    current: &DocumentFields,
    aliases: &SenderAliases,
    known_senders: &[String],
    own: &OwnNames<'_>,
    kinds: &KindCatalog,
) -> DetectedFields {
    let recipient = current
        .recipient
        .is_none()
        .then(|| detect_recipient(text))
        .flatten();
    let kind = current
        .kind
        .is_none()
        .then(|| kind_from_heading(text, kinds))
        .flatten();
    let date = current
        .date
        .is_none()
        .then(|| detect_letter_date(text))
        .flatten();
    let detection = current.sender.is_none().then(|| {
        let identifiers = OwnIdentifiers::new(
            own.names
                .iter()
                .map(String::as_str)
                .chain(recipient.as_deref().or(current.recipient.as_deref())),
            own.ibans.iter().map(String::as_str),
            own.tax_ids.iter().map(String::as_str),
        );
        detect_sender(text, aliases, known_senders, &identifiers)
    });
    let (sender, offers) = detection.map_or((None, Vec::new()), |found| {
        (found.sender.map(|sender| sender.name), found.offers)
    });
    DetectedFields {
        sender,
        recipient,
        kind,
        date,
        offers,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const LETTER: &str = "Muster GmbH\nHauptstr. 1, 09111 Chemnitz\n\nMarc Beispiel\nAndrestr. 12\n09112 Chemnitz\n\nChemnitz, 28.01.2019\n\nRechnung\n\nBetrag 10 EUR\n\nMit freundlichen Grüßen\nMuster GmbH\n";

    fn own() -> OwnNames<'static> {
        OwnNames {
            names: &[],
            ibans: &[],
            tax_ids: &[],
        }
    }

    fn detect(current: &DocumentFields) -> DetectedFields {
        detect_fields(
            LETTER,
            current,
            &SenderAliases::default(),
            &[],
            &own(),
            &KindCatalog::built_in(),
        )
    }

    #[test]
    fn empty_fields_are_detected_and_filled() {
        let found = detect(&DocumentFields::default());
        assert_eq!(found.sender.as_deref(), Some("Muster GmbH"));
        assert_eq!(found.recipient.as_deref(), Some("Marc Beispiel"));
        assert_eq!(found.date, NaiveDate::from_ymd_opt(2019, 1, 28));
        let filled = found.fill(&DocumentFields::default());
        assert_eq!(filled.sender, found.sender);
        assert_eq!(filled.date, found.date);
    }

    #[test]
    fn a_field_that_is_set_is_neither_detected_nor_overwritten() {
        let current = DocumentFields {
            sender: Some("Von Hand".to_owned()),
            date: NaiveDate::from_ymd_opt(2020, 2, 2),
            ..DocumentFields::default()
        };
        let found = detect(&current);
        assert_eq!(found.sender, None);
        assert_eq!(found.date, None);
        let stray = DetectedFields {
            sender: Some("Muster GmbH".to_owned()),
            date: NaiveDate::from_ymd_opt(2019, 1, 28),
            ..DetectedFields::default()
        };
        let filled = stray.fill(&current);
        assert_eq!(filled.sender.as_deref(), Some("Von Hand"));
        assert_eq!(filled.date, current.date);
    }
}
